# BQ-27 - Legacy usage/cassette/config upcasters and migration baseline

> Snapshot date: 2026-09-28. This slice owns the *decision* of what an old payload is allowed to
> become when it is read by a current build. Local Cargo test/build/check/clippy/smoke commands are
> intentionally not run; GitHub Actions owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-27`](../roadmap.md#step-bq-27) |
| source snapshot | master plus this BQ-27 source slice |
| feature_status | `partial` for source-level upcast classification, field-level promotion and an append-only migration history |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | declared legacy schema → named upcaster → per-field promotion decision → digest-bound report → append-only history |

The card names four legacy material kinds, and this module names all four:

| Kind | Legacy source | Current destination |
|---|---|---|
| `CostLedger` | `kiana.cost-ledger.v0` | `kiana.normalized-usage.v1` |
| `Quota` | `kiana.quota.v0` | `kiana.quota-window-budget.v1` |
| `Cassette` | `kiana.harness-cassette.v0` | `kiana.provider-cassette.v1` |
| `ProviderConfig` | `kiana.provider-config.v0` | `kiana.provider-config-snapshot.v1` |

The `.v0` names are *declared* legacy schema names for this contract. They are not registered in
`SCHEMA_CONTRACTS`, because that registry is `kiana-domain/src/contracts.rs` and the integration
owner owns it. `LEGACY_UPCAST_SOURCES` is the BQ-27-local source/destination table and is digest
bound into every request through `legacy_upcast_registry_digest()`.

## What the card rejects, and how

The card's rejected-first column has three items. All three are the same failure wearing different
clothes: an old payload being made to say more than it ever said.

| Rejected | How |
|---|---|
| unknown major silently accepted | `classify_legacy_schema` returns only `LegacySchemaClass` or the typed refusal `legacy_upcast_unknown_major`; the admission order re-checks the same name and returns the same code (`legacy_upcast_registry_drift` is checked first, so a drifted registry is reported before the schema verdict). A same-family *different major* (`kiana.cost-ledger.v9`) and a same-major *different name* are both refused, and an unrecognised field in an otherwise-valid payload is refused rather than dropped (`legacy_upcast_non_migratable_field`) |
| old `cost_micros = 0` treated as measured | the legacy field is a bare `u64`, so `Some(0)` is classified `LegacyMeasureOrigin::LegacyUnsetZero`, never `Reported`. `LegacyUpcastField::validate` refuses `promoted == Some(0)` outright (`legacy_upcast_explicit_zero_forbidden`), and refuses *any* promotable cost whatever its value (`legacy_upcast_measured_cost_forbidden`) because an old number has no rate card and no provider receipt behind it. `LegacyUpcastTotals.cost_micros` and `.used_units` are structurally `None` and re-asserted in `validate_against`, so nothing this module emits can be summed, reported or settled as a real cost |
| repeated migration overwriting history | `LegacyUpcastHistory::append` folds over reports. A replayed report for the same source is `AlreadyRecorded` and consumes no new sequence number; a *second, different* report for the same source is `Conflict`, never a rewrite. The history has no `remove` or `clear` path, and a hand-built history that reuses one source for two entries fails `legacy_upcast_history_source_duplicate` |

Two further refusals this slice adds because the card's success condition ("upgrades without lying
about what it was") is not otherwise checkable:

| Rejected | How |
|---|---|
| a partial usage fold reported as a smaller total | `fold_measure` collapses to absent when any contributor is missing or zero (`legacy_usage_partial_report`), because adding over a hole would report a more confident number than the file contains |
| an old profile version presented as a reported one | a legacy config has no `profile_version`, so the field is recorded `Absent` with `legacy_provider_version_assigned_at_upcast`. The `1` a current snapshot would carry was assigned by the upcaster, not written by the old file |

### The decision order is fixed

`admit` runs in one sequence, so the reported reason is the first violated rule and is therefore
deterministic for the same facts:

```text
request shape -> registry drift -> already-current -> named upcaster
```

`LegacyUpcastReport::validate_against` re-derives that whole decision, so an edited status, reason,
promoted value, field list or digest is rejected rather than trusted.

## Failure-first fixture matrix

One named test per item in the card's rejected-first column, plus the success and boundary cases.
All live in `kiana-domain/tests/bq27_legacy_upcast.rs` (12 tests); the source guard is
`kiana-core/tests/bq27_legacy_upcast_guard.rs` (5 tests).

| Fixture | Assertion |
|---|---|
| `rejects_an_unknown_schema_major_instead_of_best_effort_parsing` | six unrecognised schema names all return `legacy_upcast_unknown_major`; a drifted registry is refused before the payload is read; the registered source and destination are the only two names that classify |
| `rejects_a_legacy_zero_cost_being_treated_as_measured` | `cost_micros: 0` promotes to nothing, totals stay `None`, origin is `LegacyUnsetZero`, reason is `legacy_zero_is_absent_measurement`; `Some(0)` is refused for all three origins; a legacy zero *token* count is also refused; a reported sibling still promotes |
| `rejects_a_positive_legacy_cost_measured_without_rate_card_or_receipt` | a positive legacy cost still promotes to nothing; a hand-set `totals.cost_micros` is refused |
| `rejects_repeated_migration_overwriting_recorded_history` | replay is `AlreadyRecorded` with no new sequence; a divergent report is `Conflict`; a second source still appends at sequence 2; a forged duplicate-source history is refused |
| `upgrades_old_ledger_quota_and_cassette_without_claiming_more_than_they_said` | reported usage and limits carry; cost and the three consumption counters do not become numbers; a partial cassette fold collapses; a legacy config manufactures no version; an unknown selection mode or protocol is refused |
| `an_already_current_payload_is_idempotent_and_invents_nothing` | the already-current path carries no field; a forged one that does is refused; the upcasting path refuses already-current material |
| `every_report_is_read_only_and_names_its_rollback_inputs` | every report is read-only and names its backup ref and source digest; a report edited to claim it wrote back is refused |
| `rejects_an_unknown_legacy_field_rather_than_dropping_it` | an unrecognised field and a malformed field type are two different typed refusals |
| `a_tampered_report_is_rejected_by_re_derivation` | four tamper shapes, with and without a recomputed digest, are all caught |
| `legacy_text_is_bounded_redacted_and_secret_scanned` | scope text is bounded, redacted-checked and sentinel-scanned; a cassette body is scanned as a transcript; a request whose digest does not cover its fields is refused |
| `the_upcast_registry_is_named_versioned_and_digest_bound` | all four sources are `.v0`, all four destinations are current, every pair classifies, and the registry digest is stable and bound into every request |

The source guard additionally asserts, against the literal source: the unknown-major refusal is
emitted from both the classifier and the admission order; the classification of `Some(0)` sits on
its own arm; `cost_field` returns `None` on both paths and never `Some(reported)`; the ordered
decision rules appear in sequence; and the module contains none of `std::fs`, `std::process`,
`File::`, `tokio::`, `EventStorePort`, `ArtifactStorePort`, `write_all`, `remove_file`, `fn apply`
or `fn run`.

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate, which includes
`cargo check --workspace --locked`, `cargo clippy --workspace --all-targets --locked` and
`cargo test --workspace --locked`. No separate workflow is added by this slice.

What was actually run locally while writing this slice, and what it showed:

- `cargo check --workspace --tests --offline --keep-going` with the module registered: 0 errors
  attributable to BQ-27. The 12 errors in the log all belong to other slices in flight at the time
  (`bq25_telemetry_separation`, `bq26_fault_injection`, `pd30_storage_fault_matrix` and two other
  agents' temporary `zz_*` probe files).
- `cargo clippy -p kiana-domain --offline`: exit 0, 0 warnings attributable to `legacy_upcast.rs`.
  One `clippy::useless_conversion` on a `u64::from(u64)` was found and fixed during this slice.
- `rustfmt --edition 2021 --check` on the three Rust files this slice adds: clean.
- The five source-guard assertions were executed against the real source (compiled standalone
  outside the repo) and all passed. The twelve fixture tests were executed earlier in the slice;
  nine passed on the first run and three failed, all three on my own incorrect expectations rather
  than on the module, and were corrected. **The final fixture suite has not been re-run**, so treat
  "12 tests pass" as unverified and let CI confirm it.

**This is a source contract. It does not migrate anything.** No file is read, no payload is
written, no lock is taken, no migration is applied and no event is appended. The guard asserts that
boundary literally. Concretely, the following are *not* proven here:

- **No store has been migrated.** `LegacyUpcastReport` is a report about bytes someone else read.
  Nothing in this crate opens a `KIANA_HARNESS_SCRIPT`, a config file or a usage ledger. The
  `Cargo.toml`/ledger/quota/cassette `.v0` shapes are declared here for the first time; whether any
  file on any disk matches them is unverified, and a real adapter may find field names this module
  did not anticipate.
- **No rollback has been performed.** `LegacyUpcastRollback` names what a caller *would* need (a
  verified backup reference, a source digest to re-read) and asserts `read_only: true`. It neither
  creates nor verifies a backup. Its `verified_backup_ref` is carried from the caller's request and
  is a claim the domain does not check.
- **Idempotency is a pure fold, not a concurrency property.** `LegacyUpcastHistory::append` is
  idempotent for a replayed report within one process and one `History` value. Two concurrent
  writers still need the EventLog CAS path to make that hold; nothing here serialises them.
- **The unknown-major rule is restated, not re-decided.** `legacy_upcast_unknown_major` mirrors
  `storage_schema_unknown_major` and `security_schema_unknown_major` in the same family and
  returns a parallel code. The three are intentionally separate strings so a caller can tell which
  boundary refused, and no test asserts the mapping between them.
- **`ModelProtocol::wire_name` is new.** It exists so the legacy config parser derives protocol
  spellings from the current enum rather than from a second hand-written list that could drift. It
  is additive; the provider crate keeps its own provider-name-to-protocol mapping, which is a
  different vocabulary (provider names, not protocol names) and is not touched here.
- **No `SCHEMA_CONTRACTS` registration.** The new `.v0` legacy names and the report schemas are not
  in the workspace schema registry. `validate_schema_registry` therefore does not see them, and
  the `Domain` layer's `allow_unknown_fields: false` rule is not exercised against them. The
  integration owner owns `kiana-domain/src/contracts.rs`.
- **Nothing here is measured billing.** The module can only ever *remove* a claim. It cannot create
  one, and it does not make a single call measured, settled or reconciled. A legacy cost stays
  unknown until a rate card and a provider receipt exist through the ordinary BQ-04/BQ-13 path.

## Reused vocabulary

This slice deliberately reuses rather than reinvents:

- `crate::json_digest`, `crate::redact_text`, `crate::scan_secret_sentinels`,
  `SecretScanChannel` (the cassette body is scanned as `Transcript`, carried text as `Cache`,
  because both are written back into local storage).
- `BillingUnknownReason` for every unknown, and `UsageConfidence`-era `Option<u64>` semantics from
  `kiana-domain/src/billing_usage.rs`: `None` is absent, `Some(0)` is an explicit zero — and here a
  legacy `0` is the *absent* case, which is the entire point of the card.
- `ProviderSelectionMode`, `ProviderConfigSource` (its `LegacyCassette` variant is what an upcasted
  config reports) and the `PROVIDER_*_SCHEMA` constants for the cassette/config destinations.
- `crate::UsageRecord` for the legacy ledger's records, unchanged and un-re-summed.
- `crate::MigrationRegistry` and `crate::MigrationRecord` are *not* extended. This module is the
  payload-level upcaster; the registry stays the step-level authority, and nothing here competes
  with it.
