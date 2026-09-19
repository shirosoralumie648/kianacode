# DEP-36 container adapter baseline (partial)

DEP-36 extends the existing CAP-33 EnvironmentPort adapter with immutable image identity,
volume/root identity labels bound to owner and scope, an operation-level environment allowlist,
explicit SIGTERM stop semantics, and separate startup, readiness and liveness probe requests.
Startup is inspect-only; readiness and liveness require explicit argv and remain observations,
not permit or rollout authority.

The adapter preserves fail-closed behavior for identity drift, unpinned images, unapproved env
names, unconfirmed cancellation and probe timeouts. A fake/container harness is CI-only in this
step. Runtime inventory and restart recovery are still process-local, and no real container,
gVisor target, traffic drain, durable fence, health receipt or cross-process cleanup was executed.
DEP-36 remains partial with source/static evidence only; result_unknown cannot be promoted to
success.
