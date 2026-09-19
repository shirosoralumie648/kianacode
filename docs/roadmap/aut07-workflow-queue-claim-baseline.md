# AUT-07 workflow queue claim baseline

AUT-07 now derives `WorkflowQueueClaimContract` from the same `WorkPacket` and
`ready_packets` predicate used by the packet board. Packet identity, project/data/path scope,
budget lease/deadline, canonical path-lock set, parent-child containment and claim expiry are
server-derived; missing parent scope/budget, cycles, unresolved dependencies, scope/budget
widening, duplicate claims and expired claims fail closed.

`kiana-core` exposes a deterministic queue-ready view with one claim per item and a digest-bound
blocked/expired projection. This remains a pure source/CI fixture slice: AUT-08 still owns durable
QueueStore leases, heartbeat, fencing, reclaim and recovery; no queue or scheduler was started by
this step.
