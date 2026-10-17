# P4-J7-26 · Model events, redaction and correlation baseline

This source slice defines the versioned model/provider fact boundary. A model fact carries
server-owned Session/Turn/Run/step/ModelCall/attempt identity, route and prompt hashes, actual
model/configuration revision, bounded provider references and correlation metadata. Provider
request/response headers, query values, error bodies, response bodies, replay bytes and prompt
text are represented only by redacted digest references.

## Source contract

- `ModelEvent` registers prepared, denied, attempt-started, retry-scheduled, finished and usage
  correction facts. Identity and correlation fields are checked together and unknown provider
  request/response IDs stay explicitly `unknown`. `ModelUsageCorrection::new` binds its
  correction digest to the late-usage payload and validation rejects post-seal mutation.
- `ProviderTraceMetadata` and `ProviderDeltaLedger` use the existing `RedactionProfile` and
  `StreamingRedactor`. Secret markers split across chunks are redacted, payloads are bounded and
  the durable event contains no raw provider material. The delta ledger counts all observations
  while retaining only a bounded number of references.
- `ModelFactCommitment` is a fail-closed append guard. A pending or failed fact cannot dispatch a
  tool or mark a run completed; only a committed durable sequence can pass either check.
- `ModelAttemptInvocationReceiptLink` binds model attempt, Invocation/Execution and receipt
  digests. `ModelEvent::into_runtime_event` emits through the existing RuntimeEvent envelope and
  redaction metadata; it does not perform an append or grant execution authority.
- The legacy `run.model_turn` payload remains readable. Runner provenance now includes the
  configuration revision and explicit known/unknown provider reference status while the existing
  DaemonHost → ControlPlane path remains the only execution spine.

## 2026-10-17 completion slice (CI-only)

The card's exit conditions are now covered at the source boundary by named, deny-first fixtures.
No runtime producer, EventStore writer, provider transport or UI projector was added.

- `provider_secrets_split_across_deltas_are_redacted`: markers split at arbitrary delta
  boundaries (`Authorization: Bear…er`, `api_key=`, `password=`, `x-api-key:`) are closed by the
  shared `StreamingRedactor`; the ledger records all five observations but retains at most three
  digest-only references, and no retained digest covers a raw secret.
- `provider_error_body_cannot_leak_authorization`: a provider authentication error body with
  `Authorization: Bearer`/`token=` material plus header/query sentinels becomes digest references;
  the `ModelEvent` payload and the emitted `RuntimeEvent` contain none of the sentinels,
  `payload_recoverable=false`, and the error digest equals the digest of the redacted text rather
  than the raw body.
- `model_fact_persistence_failure_prevents_tool_dispatch`: a failed append leaves the commitment
  `Failed`, blocks both `require_tool_dispatch()` and `require_mark_completed()`, refuses a later
  `mark_appended`, and binds the stable failure digest.
- `model_attempt_to_invocation_receipt_chain_is_complete`: run/turn/step/model-call/attempt/
  invocation/execution/tool identity, known provider request plus explicitly unknown response,
  independent UI/upstream/durable channels and the source event cursor all survive into the
  registered `RuntimeEvent`; the Invocation→Receipt link re-binds the model event digest.
- `provider_trace_records_actual_model_and_config_version`: two facts record distinct reported
  models/configuration revisions and different digests; the payload has no prompt/request/body/
  credential field, and missing provider wire IDs stay `unknown` while fabricated
  known-without-id / unknown-with-id references are rejected.
- `provider_delta_ledger_granularity_remains_bounded`: 512 deltas keep four retained references,
  508 dropped observations and one bounded durable row (<4096 bytes), so token-level display does
  not become token-level durable writes.
- `every_registered_model_kind_round_trips_through_the_event_registry`: prepared, denied,
  attempt-started, retry-scheduled, finished and usage-correction facts each validate, survive
  JSON round-trip and pass `validate_runtime_event` with kind-specific fields.

## CI wiring

`.github/workflows/p4-j7-26-model-events.yml` is a manual `workflow_dispatch` lane with
`permissions: contents: read`; it runs `cargo fetch --locked`, `cargo fmt --all --check`, the
domain fixture and the Core source-boundary guard with `--test-threads=1`, then
`cargo check -p kiana-domain -p kiana-core --tests --locked`. The automatic lane is the unified
`ci.yml`: `kiana-domain-s3/4` lists `p4_j7_26_model_events` and `kiana-core-s5/6` lists
`p4_j7_26_model_events_guard`, so the crate shards pick both targets up without new manifests.

Local tests, builds, checks, clippy and smoke commands are intentionally not run; GitHub Actions
is the test authority and its receipt is not awaited. `scripts/ci/test-shards.json` and
`Cargo.toml`/`Cargo.lock`/`package.json` are unchanged.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` plus GitHub CI wiring. It does
not claim a durable EventStore writer integration, cross-process recovery, provider invoice
truth, live model effects, or physical tool/receipt execution. Runtime producers still need to
append each model fact through the existing EventLog boundary and use the commitment guard before
dispatch or completion; P4-J7-27 and later steps own recovery and surface projections.
