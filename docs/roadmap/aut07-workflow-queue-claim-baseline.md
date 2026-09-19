# AUT-07 workflow queue claim baseline (partial)

AUT-07 adds `WorkflowQueueClaimContract` as a shared WorkPacket/queue admission shape. It binds
packet, parent/item scope and budget digests, path lock, dependency resolution, parallel limit,
duplicate claim and bounded expiry; missing parent scope/budget, cycles, unresolved dependencies,
scope/budget widening, duplicate claims and expired claims fail closed.

This is a pure domain contract. AUT-08 still owns durable QueueStore leases, heartbeat, fencing,
reclaim and recovery; no queue or scheduler was started by this slice.
