# P4-J7-26 · Model events, redaction and correlation baseline

This source slice defines the versioned model/provider fact boundary. A model fact carries
server-owned Session/Turn/Run/step/ModelCall/attempt identity, route and prompt hashes, actual
model/configuration revision, bounded provider references and correlation metadata. Provider
request/response headers, query values, error bodies, response bodies, replay bytes and prompt
text are represented only by redacted digest references.

## Source contract

- `ModelEvent` registers prepared, denied, attempt-started, retry-scheduled, finished and usage
  correction facts. Identity and correlation fields are checked together and unknown provider
  request/response IDs stay explicitly `unknown`.
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

## CI-only fixtures

`.github/workflows/p4-j7-26-model-events.yml` runs the domain lifecycle/redaction fixture, the
Core source-boundary guard, formatting and workspace test-target compilation on GitHub Actions.
Local tests, builds, checks, clippy and smoke commands are intentionally not run.

The fixtures cover redacted provider traces and split markers, bounded delta retention, runtime
event schema/identity, persistence-before-effect denial, receipt-link digest tampering and the
model lifecycle registry. They use synthetic IDs and do not open a provider or network route.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` plus GitHub CI wiring. It does
not claim a durable EventStore writer integration, cross-process recovery, provider invoice
truth, live model effects, or physical tool/receipt execution. Runtime producers still need to
append each model fact through the existing EventLog boundary and use the commitment guard before
dispatch or completion; P4-J7-27 and later steps own recovery and surface projections.
