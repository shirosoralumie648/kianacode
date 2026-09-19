# DEP-35 managed-local / embedded-local rollout baseline (partial)

The domain now models both managed-local and embedded-local rollout modes through the ordered
phase machine plan → preflight → backup → drain → replace → ready → promote. Every transition
requires an evidence snapshot: preflight, verified backup, zero active runs/writers, old-revision
fence, replacement start, readiness and retained old root. Phase skips and missing gates fail
closed.

DEP-35 remains partial with source/static evidence only. The state machine does not invoke a
supervisor, make a backup, replace a binary/root, start a new revision or promote traffic. Durable
operation leases, rollback receipts, startup/health probes and actual local rollout adapters remain
later work.
