# CAP-23 concurrency, conflict and fair-admission baseline

CAP-23 closes the missing admission boundary between an authorized request and the existing
Broker. `CapabilityAdmissionScheduler` is bounded (eight active admissions and 256 queued),
FIFO-fair for conflicting work, and process-local. It never invokes a handler or starts another
model loop.

Read footprints may run together. Writes use the server-derived `ExecutionScope.write_roots`, so
shell/process calls lock their authorized write scope instead of guessing a file list from command
syntax. Overlapping read/write or write/write scopes conflict; unknown or external effects use a
wildcard conservative footprint. Existing CellRegistry admission still owns budget reservations,
Cell concurrency, parent cancellation and Cell lifecycle; existing descriptor-relative OS locks
cover cross-process workspace conflicts.

Approval waiting happens before this gate, so an approval request does not hold an execution slot.
Cancellation removes a queued admission and returns `cancelled:before_dispatch`/`not_executed`;
the Runner's pending queue and existing cancellation facts prevent a later spawn. A completed or
known failed invocation releases its footprint. `Unknown` moves the footprint to an explicit local
quarantine, and the packet Cell remains non-retirable until reconciliation/retirement can prove a
safe release.

GitHub Actions runs the existing Cell lifecycle and quota guards, the CAP-23 source guard and
workspace compilation. No local runtime tests or smoke commands were run.
