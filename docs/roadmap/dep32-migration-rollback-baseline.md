# DEP-32 migration rollback baseline (partial)

The domain now evaluates binary rollback, data restore and external-effect reconciliation as
separate decision paths. Every receipt binds the registry and fact digest, requires retained old
root, verified backup, no active writers, a fenced new revision and zero unknown effects. Binary
rollback additionally requires compatible old revision; data rollback requires verified restore;
effect reconciliation requires an explicit external receipt state.

Without those facts the gate returns Deny with a stable reason and remediation. The decision
receipt is evidence only: it does not restore a root, start an old binary, kill a writer, delete
the old root, retry an effect or claim compensation succeeded.

DEP-32 remains partial with source/static evidence only. Durable backup/restore, writer fencing,
rollback execution, external idempotency lookup and receipt projection are later work.
