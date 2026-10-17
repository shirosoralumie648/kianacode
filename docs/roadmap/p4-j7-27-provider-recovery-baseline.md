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

`.github/workflows/p4-j7-27-provider-recovery.yml` runs the domain recovery fixtures, the Core
source boundary guard, formatting and a targeted test compilation on GitHub Actions
(`workflow_dispatch`-only, `permissions: contents: read`, `cargo fetch --locked`,
`cargo fmt --all --check`, both targets with `--test-threads=1`, `cargo check -p kiana-domain -p
kiana-core --tests`). The automatic lane is the unified `ci.yml` shards `kiana-domain-s3/4`
(`p4_j7_27_provider_recovery`) and `kiana-core-s5/6` (`p4_j7_27_provider_recovery_guard`). Local
tests, builds, checks, clippy and smoke commands are intentionally not run in a CI-only session.

The fixtures cover missing replay material, stale route/authority, invalid remote continuation
with complete local fallback, ordered committed model/tool history, uncommitted partial facts,
model in-flight no-resubmit, capability unknown reconciliation, schema registration and reuse of
the existing EventLog/history/resume/drive_run spine.

## 2026-10-17 completion slice (CI-only)

The round is now folded end to end at the source boundary:

- `ProviderRoundPlan` binds the committed facts of exactly one run/turn to every committed
  in-flight observation, rejects foreign scope, duplicate attempts/cursors and consumed attempts,
  and exposes the model-unknown attempts, capability-unknown invocations and prepared attempts as
  separate lists. `authorize_resume` refuses the whole round while any attempt is unknown, and
  only then delegates to `ProviderResumeBinding::validate_for_resume`.
- The "capability completed before delivery" crash point keeps `history = None` with the stable
  `model_history_incomplete_batch` reason instead of promoting a partial round to a complete
  assistant/tool pair. Committed tool facts still populate the never-re-execute invocation set.
- `ProviderResumeCheck` re-binds route, prompt/schema/tool/data digests, authority/data epochs,
  wall clock and the local protected material, so an incomplete live scope is refused before any
  binding is trusted. The plan digest is verified on `validate` and after JSON round trips.
- New rejection codes: `provider_recovery_round_scope_mismatch`,
  `provider_recovery_round_observation_conflict`, `provider_recovery_round_input_missing`,
  `provider_recovery_round_resume_blocked_by_unknown`,
  `provider_recovery_round_attempt_already_consumed`, `provider_resume_check_invalid`, plus the
  recorded `model_history_incomplete_batch` reason. The schema
  `kiana.provider-round-plan.v1` is registered in `contracts.rs`.
- Deny-first fixtures: `expired_resume_binding_and_widened_ttl_are_rejected_before_any_send`,
  `tampered_binding_digest_cannot_be_trusted_even_with_matching_fields`,
  `revoked_deleted_or_route_switched_replay_material_blocks_resume`,
  `history_rejects_partial_duplicate_and_out_of_scope_facts`,
  `every_crash_point_classifies_without_automatic_retry`,
  `round_plan_rejects_missing_foreign_duplicate_and_consumed_rounds`,
  `round_plan_blocks_resume_while_an_inflight_attempt_is_unknown`,
  `capability_completed_before_delivery_never_fabricates_history_or_reexecutes`.
- Success fixtures: `fresh_process_resumes_completed_model_turn_with_tool_history`,
  `resume_never_reexecutes_completed_invocation`,
  `round_plan_authorizes_a_prepared_attempt_after_explicit_scope_recheck` (valid remote
  continuation, local fallback for a stale remote ID) and the existing pre-send/terminal cases.
- The Core guard's `round_plan_recovery_fixtures_stay_wired_to_the_manual_lane_and_auto_shards`
  binds the fixture names, the new rejection codes, the lane commands, the dispatch-only rule and
  the two automatic shard targets.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` plus GitHub CI wiring. It does
not claim a durable cross-process EventStore projector, artifact-store reopen, process crash or
power-loss evidence, provider-side continuation validity, invoice truth, live provider effects or
physical capability execution. `ProtectedReplayStore` remains process-local; a production resume
adapter must provide complete authorized material and an explicit ControlPlane re-admission before
calling the existing `drive_run`.
