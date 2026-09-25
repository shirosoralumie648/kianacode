# DEP-37 orchestrated rollout baseline (partial)

DEP-37 adds a pure contract for canary, blue-green and rainbow rollout profiles. Every worker
build route carries an `ExecutionRevisionPin`; the routing table requires full bounded traffic
weights, one active writer, a writer fence digest and unique revision/build identities. A bounded
canary observation and progress deadline gate resume/promote decisions, while pause and rollback
remain explicit adapter actions.

`OrchestratedRolloutEvidence` now binds plan/routing/canary/decision digests, active writer and
fence, deadline, phase/action, typed operator approval and health/traffic-drain receipt references.
Target backends, Simulation and `result_unknown` cannot be marked verified; non-verified rows
retain their proof level and limitations.

Kubernetes and generic orchestrator backends are named targets only. The CI fixture is fake and
source-bound; it does not change traffic, stop old workers, fence a process, call a cluster or
claim a live receipt. DEP-37 remains partial until a durable rollout state, real adapter, traffic
drain, health window, cross-process writer fence and operator-approved target environment exist.
