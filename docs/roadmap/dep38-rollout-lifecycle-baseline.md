# DEP-38 rollout lifecycle baseline (partial)

DEP-38 adds a pure lifecycle state machine for pause/resume/promote/rollback, old revision
drain/retirement, retention windows and post-deploy verification. Promotion requires a pinned
target route, a drained old revision, readiness and liveness evidence, a passing bounded health
window and an independent receipt digest. Retirement cannot make an old root deletion-eligible
until the retention window has closed and the old revision is retired with no active runs or
writers.

`RolloutLifecycleEvidence` now binds state/plan, health-window/verification, retention and drain
digests, phase/action, old-root retention, active run/writer counts, deletion eligibility and
Unknown/target proof boundaries. A target backend or result_unknown cannot be verified, and an
early deletion-eligible claim is rejected.

The CI fixture is fake and source-bound. No traffic was changed, no process was fenced or
deleted, and no external health/receipt was observed. DEP-38 remains partial until durable state,
real pause/resume/rollback effects, cross-process leases, retention cleanup and live post-deploy
verification are wired to an approved adapter.
