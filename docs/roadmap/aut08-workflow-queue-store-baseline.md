# AUT-08 workflow queue store baseline (partial)

AUT-08 adds the shared queue lease contract, `WorkflowQueueStore` port, ControlPlane dispatch
guard, and a CI-only in-process adapter. Claim, heartbeat, effect recording, fence and reclaim
are serialized under one adapter lock; wrong owner/fence, rollback, active leases, in-flight
effects and unknown effects fail closed. A safe expired item requires a new owner and a strictly
larger fence token.

`MemoryWorkflowQueueStore` is a semantic fixture only. It does not prove fsync, EventLog replay,
cross-process locking, worker death detection, or physical external-effect reconciliation. A
durable queue implementation and scheduler remain later AUT steps; an unknown effect is retained
as recovery-required and is never directly reclaimed.
