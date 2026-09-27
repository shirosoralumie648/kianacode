# BQ-25 redaction, DataClass and telemetry separation baseline

> Snapshot date: 2026-09-28. This slice adds one read-only per-channel telemetry safety contract in
> `kiana-domain`. Local Cargo test/build/check/clippy/smoke commands are intentionally not run;
> GitHub Actions owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-25`](../roadmap.md#step-bq-25) |
| source snapshot | master plus this BQ-25 telemetry-separation slice |
| feature_status | `partial` for source-level Event/Log/Metric/Trace/Receipt safety decisions |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | producer candidate → per-channel `evaluate_candidate` → ordered `TelemetrySafetyReport` |

The contract answers one question that the existing pieces could not answer together: **is this
value safe to put in *this* channel?** `redact_text`, `scan_secret_sentinels` and `RedactionProfile`
already decide whether a value has been redacted. `MetricCatalog` and `MetricCardinalityGuard`
already decide whether a label is registered and low-cardinality. Neither of them can say that the
same value is fine in an `Event` and forbidden in a `Metric`, because neither is asked the question
per channel. BQ-25 is that question, stated once, with the existing answers reused as inputs.

`TelemetryChannel::ALL` pins the five channels in a fixed order — `Event`, `Log`, `Trace`, `Metric`,
`Receipt` — and `TelemetryChannelPolicy` holds the per-channel table: a `DataClass` ceiling, a
`DataClass` floor, a label dimension budget, and whether an identifier may be a label at all. That
table is the whole BQ-25 class decision; it is deliberately two numbers per channel rather than a
second classification vocabulary, because `DataClass` (`Public`/`Internal`/`Confidential`/
`Restricted`) and `SecretScanChannel` already exist and already have one owner each.

The trust asymmetry the card names is encoded directly: `Metric` is capped at `Internal` while
`Event` is capped at `Restricted`, and `admits_identifier_label` is `false` for `Metric` and `Trace`
and `true` for `Event`, `Log` and `Receipt`. A `Confidential` tenant name is admitted into the event
log and refused into a metric; a `run_id` label is legal as a log dimension and refused as a trace
attribute. Both are the same value, and both answers come from the same table.

## What the card rejects, and how

| Rejected first | How it is refused |
|---|---|
| **API key** | `unredacted_content`. `redact_text(value) != value` is checked first, then `scan_secret_sentinels` under the channel's existing `SecretScanChannel`. `api_key=…` is refused in the `Event` channel too, so this is not a metric-only rule exercised by luck |
| **prompt** | `payload_shape_forbidden`, on the candidate **key**, in every channel. A prompt has no sentinel shape — `redact_text` rewrites an API key and leaves a prompt alone — so the scanner cannot see it and no redaction makes it safe. The rule is `is_payload_key`, exempting `Label` candidates so a legitimate `prompt_version` dimension still works |
| **raw response** | `payload_shape_forbidden` in all five channels, for the same reason. The refusal is uniform: there is no channel here where the raw body is admissible |
| **invoice secret** | `unredacted_content` in `Event` and `Receipt`. BQ-22's `ProviderInvoiceImport` only ever carries a `ProviderReceiptRef` and an `authentication_ref`; the credential that authenticated an invoice has no field here to live in. The fixture also shows the admissible alternative: an opaque invoice reference is admitted in both channels |
| **path / high-cardinality ID into a metric** | `high_cardinality_label`, on the label **key**, before any content is read. A UUID is not merely un-admitted — `safe_text` refuses it as a channel key or reference handle at all, so the identifier has no representable shape in this contract. Identity belongs in the typed `trace_id`/`span_id` fields and in the EventLog |

Two rules exist because the card's two failure modes are different in kind. A secret is a *content*
problem and the shared scanner is the right tool. A high-cardinality identifier is a *shape*
problem — an unbounded dimension wearing a label's clothes — and it is caught on the key, because
`run_id` as a value looks exactly like any other clean string until the series table has already
grown by a factor of the request rate.

A third rule, `channel_guarantee_unknown`, encodes the reason telemetry is the lower-trust channel
at all: a channel whose adapter never reported whether it redacts at the sink is refused, never
passed. Silence is not proof, and an unreported channel is the common case rather than the exotic
one.

## Decision order

`decide` runs seven rules in a fixed order, and the order is the contract:

1. **shape** — a text value is not representable as a metric dimension; a payload key is not a
   telemetry value; an identifier is not a dimension in a dimension-shaped channel
2. **content** — `redact_text` idempotence, then the shared sentinel scan
3. **class** — the value's `DataClass` against the channel's ceiling
4. **export ceiling** — the caller's declared downstream class against the channel's floor
5. **label budget** — the label's observed value count against the channel's dimension budget
6. **guarantee** — the channel's `RedactedAtSink` / `Unverified` / `Unknown` state

Shape precedes content because both are *positive detections*: a UUID in a metric label is a leak
already visible in the candidate, and reporting `data_class_too_high` for it would bury that.
Guarantee is last because every earlier rule is decidable from the candidate alone, while the
guarantee is the one fact only the adapter can establish. Within a request, candidates are walked in
order and channels in `TelemetryChannel::ALL` order, so the same facts always produce the same
reason string — and therefore the same `report_digest`, which is what makes the reason quotable in an
incident.

`TelemetrySafetyReport::evaluate` seals `report_digest` and calls `validate_against`, which re-derives
the decision and refuses a report that disagrees with it. The same pairing pattern as PD-26
revocation and DEP-20 quiesce: a report that says `Admitted` may not carry a refusal list, and a
report that says `Refused` may not carry an empty one, so a caller reading the status line cannot
skip the list.

## Failure-first fixture matrix

`kiana-domain/tests/bq25_telemetry_separation.rs` — one test per card rejection plus the success
condition and the binding rules.

| Test | Assertion |
|---|---|
| `api_key_into_metric_is_refused` | `api_key=…` refused in `Event` and `Metric` with `unredacted_content`; the sentinel never appears in the report |
| `prompt_text_is_refused_in_telemetry_channels` | `Metric` refused on shape, `Log`/`Trace` refused on the payload key; the same reason in every channel |
| `raw_provider_response_is_refused_in_every_channel` | all five channels refuse; the raw body is admissible nowhere |
| `invoice_account_secret_is_refused` | `client_secret=…` refused in `Event` and `Receipt`; the opaque invoice reference is the admitted alternative |
| `path_and_high_cardinality_ids_are_refused_as_metric_labels` | `run_id` refused in `Metric` and `Trace`, **admitted** in `Event`; `request_id` and `file_path` refused in `Metric` |
| `a_uuid_is_not_representable_as_a_channel_key` | a UUID key is refused by `safe_text`, not merely un-admitted |
| `digest_and_low_cardinality_labels_correlate_across_all_five_channels` | 2 candidates x 5 channels = 10 admitted decisions; the same `value_digest` on both sides of the event/metric boundary |
| `data_class_ceiling_is_per_channel` | `Confidential` admitted in `Event`, refused in `Metric` with `data_class_too_high` |
| `a_label_above_the_channel_budget_is_refused` | one value over the budget refused with `label_cardinality_exceeded`, the value exactly at it admitted |
| `an_unreported_channel_guarantee_is_a_refusal` | a channel absent from the guarantee map refuses; the four that did report are still admitted |
| `an_unverified_channel_guarantee_is_a_refusal` | an explicitly `Unverified` exporter refuses even for a clean digest |
| `the_decision_order_is_fixed` | a payload+secret+restricted candidate reports the shape reason; the same candidate in `Log` falls through to content; re-evaluation yields the same `report_digest` |
| `report_status_and_refusal_list_cannot_disagree` | a forged `Admitted`-with-refusals and a forged `Refused`-without-reason are both refused |
| `candidates_and_reports_are_digest_bound` | an edited cursor breaks the request digest; a forged report fails `validate_against` |
| `the_channel_vocabulary_is_fixed_and_reuses_the_existing_scanner` | 5 channels, fixed order, `rank() == index`, each mapping to a `SecretScanChannel`; a candidate carrying two payloads is refused |

`kiana-domain/tests/bq25_telemetry_separation_guard.rs` — source guard, three tests:

| Test | Assertion |
|---|---|
| `bq25_telemetry_separation_contract_reuses_the_existing_vocabulary` | 41 literal markers present; 13 forbidden authority strings absent (`tokio::spawn`, `CapabilityBroker`, `EventStore`, `ControlPlane`, `ModelCallPermit`, `commit_transition`, `TcpStream`, `hyper::`, …); `redact_text` / `scan_secret_sentinels` / `SecretScanChannel` / `RedactionSignal` / `RedactionProfile` still exist in `redaction.rs`; the label bound is read from `crate::MAX_METRIC_LABEL_VALUES`, not copied |
| `bq25_telemetry_separation_is_not_a_second_redaction_system` | all three `redact_text` call sites keep the exact compare-not-assign shape; no `RedactionProfile` is constructed, no `encode_bounded_*`, no `StreamingRedactor` |
| `bq25_telemetry_separation_does_not_claim_runtime_telemetry_behaviour` | five unproven-claim phrases are absent; the header states the read-only ceiling |

## CI and limitations

`cargo check -p kiana-domain --tests --offline --keep-going` and
`cargo clippy -p kiana-domain --tests --offline` both report zero findings for the three BQ-25
files. The repository's release gate (`bash scripts/release-smoke.sh`) and GitHub Actions own the
fixtures. Local `cargo test` is not run in this slice, so the fixture assertions above are verified
by construction and by `cargo check`, not by a local test execution.

**What this does NOT prove.** Stated plainly, because the card's success line is
"Event/Log/Metric/Trace/Receipt separation proven" and a source contract cannot reach that:

- **No telemetry is produced, routed or exported.** Nothing calls `evaluate_candidate`. There is no
  adapter, exporter, collector, scrape endpoint, or `TelemetryChannelGuarantee` reporter in this
  slice; the guarantee map is supplied by a caller that does not exist yet.
- **No runtime redaction happens at any sink.** The contract *reads* an adapter's claim that a
  channel redacts at the sink. Whether any channel does is unproven. In particular, a raw metric or
  trace exporter is by default `Unverified`, and every value aimed at one is refused — so this
  contract cannot currently be satisfied by the system's actual telemetry paths.
- **The existing `MetricCardinalityGuard` is not wired to this contract.** It has its own forbidden-label list in `kiana-core/src/metrics.rs`. BQ-25 pins the same *shape* of rule at source level and
  reuses the same `MAX_METRIC_LABEL_VALUES` bound, but the two lists are not one list and are not
  checked against each other. A label BQ-25 admits and the runtime guard rejects is possible.
- **No p50/p95, series count, or cardinality measurement.** BQ-28 owns that. This slice bounds a
  declared label's value count and refuses identifiers; it does not measure growth.
- **The class ceilings and floors are a policy statement, not a measured risk assessment.** They are
  stated in `TelemetryChannelPolicy` and are changeable by one edit. Nothing here establishes that
  `Internal` is the right ceiling for a metric backend in this deployment.
- **No data-governance or retention link.** The contract does not consult the `DataPolicy` /
  `ProcessingGrant` / `DataPropagationTarget` chain from `governance.rs`, and does not record a
  propagation receipt. If a BQ-25 refusal ever needs to be reconciled with a revoke or delete
  tombstone, that binding does not exist yet.
- **No billing or receipt interaction beyond shape.** `TelemetryChannel::Receipt` shares its
  vocabulary with the `SecretScanChannel::Receipt` used by BQ-13/BQ-22, but this contract does not
  read a `CostReceipt`, does not touch the ledger, and cannot decide a billing question.
- **Local worktree state.** While this slice was written, `kiana-domain` did not build in this
  worktree for reasons outside these files (`tool_discovery.rs`, concurrent edits by other agents);
  those errors cleared and `cargo check -p kiana-domain --tests` returned zero findings for
  `telemetry_separation.rs` and both test files. A full-workspace `cargo check --workspace --tests`
  was not run to completion, so a workspace-level regression introduced elsewhere would not appear
  in the numbers above.
