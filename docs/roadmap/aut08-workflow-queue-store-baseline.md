# AUT-08 workflow queue store baseline

AUT-08 provides the shared queue lease contract, `WorkflowQueueStore` port, ControlPlane dispatch
guard, and a CI-only in-process adapter. Claim, heartbeat, effect recording, fence and reclaim
are serialized under one adapter lock; wrong owner/fence, authority rollback, active leases,
in-flight effects, overlong TTLs and unknown effects fail closed. A safe expired item requires a
new owner and a strictly larger fence token.

`MemoryWorkflowQueueStore` is a semantic fixture only. It does not prove fsync, EventLog replay,
cross-process locking, worker death detection, or physical external-effect reconciliation. The
AUT-08 source/CI slice is complete; durable queue history, scheduler integration and physical
recovery remain later AUT steps, and an unknown effect is retained as recovery-required and is
never directly reclaimed.
