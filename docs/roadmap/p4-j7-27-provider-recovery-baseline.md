# P4-J7-27 · Complete provider-round recovery and in-flight reconciliation baseline

This source slice adds the bounded provider recovery contract consumed by the existing
ControlPlane/EventLog resume path. It does not create a second runner loop, provider client, or
recovery authority.

## Source contract

- `ProviderResumeBinding` binds the same Run/Turn/Step/ModelCall/ModelAttempt to the exact route,
  profile, protocol dialect, prompt/schema/tool/data digests, authority/data epochs and expiry.
  `validate_for_resume` rechecks those values before an explicit resume. A valid provider
  continuation is only an optimization; a stale or invalid remote ID can use a complete local
  `ProtectedReplayMaterial`, and missing or expired material fails closed.
- `ProviderHistoryFact` and `ProviderHistoryProjection` rebuild model-visible history from
  committed source cursors. They run the existing `validate_model_history` invariant, reject
  uncommitted/duplicate/out-of-order facts, keep partial assistant output out of history, and
  expose completed invocation IDs so a restored runner never re-executes a completed tool.
- `ProviderInFlightObservation` classifies prepared-before-send, sent/unknown, model-completed
  before commit, capability-completed before delivery and terminal facts. `ProviderReconciliationCase`
  routes model uncertainty and capability uncertainty to separate reconciliation dispositions,
  keeps unknown usage explicit, and forbids automatic retry or effect re-execution.
- The contract is reference-only and synchronous. It does not read EventLog/artifacts, append
  facts, claim a permit, dispatch a provider/capability, or promote a UI transcript to authority.
  Existing `kiana-core::resume_run`/`drive_run` and `kiana-provider` continuation validation
  remain the only execution paths.

## CI-only fixtures

`.github/workflows/p4-j7-27-provider-recovery.yml` runs the domain recovery fixture, Core source
boundary guard, formatting and workspace test-target compilation on GitHub Actions. Its path filter
includes the current CM-36 `kiana-domain/src/memory_workbench.rs` module so a fresh remote run can
clear the repository-wide fmt dependency; that result is pending and unobserved. Local tests,
builds, checks, clippy and smoke commands are intentionally not run.

The fixtures cover missing replay material, stale route/authority, invalid remote continuation
with complete local fallback, ordered committed model/tool history, uncommitted partial facts,
model in-flight no-resubmit, capability unknown reconciliation, schema registration and reuse of
the existing EventLog/history/resume/drive_run spine.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` plus GitHub CI wiring. It does
not claim a durable cross-process EventStore projector, artifact-store reopen, process crash or
power-loss evidence, provider-side continuation validity, invoice truth, live provider effects or
physical capability execution. `ProtectedReplayStore` remains process-local; a production resume
adapter must provide complete authorized material and an explicit ControlPlane re-admission before
calling the existing `drive_run`.
