# DEP-36 container adapter baseline (partial)

DEP-36 extends the existing CAP-33 EnvironmentPort adapter with immutable image identity,
volume/root identity labels bound to owner and scope, an operation-level environment allowlist,
explicit SIGTERM stop semantics, and separate startup, readiness and liveness probe requests.
Startup is inspect-only; readiness and liveness require explicit argv and remain observations,
not permit or rollout authority.

`ContainerLifecycleEvidence` now binds the pinned image, root/owner/scope/plan identity,
environment allowlist, runtime identity, startup/readiness/liveness/quiesce/stop/dispose phase
receipts, SIGTERM, no-host-fallback, inventory/fence/health/cleanup references and
`result_unknown`. Target-only and fallback paths fail closed; non-verified observations retain
limitations.

The adapter preserves fail-closed behavior for identity drift, unpinned images, unapproved env
names, unknown launch/exec fields, unconfirmed cancellation and probe timeouts. A fake/container
harness is CI-only in this step. Runtime inventory and restart recovery are still process-local, and no real container,
gVisor target, traffic drain, durable fence, health receipt or cross-process cleanup was executed.
DEP-36 remains partial with source/static evidence only; result_unknown cannot be promoted to
success.
