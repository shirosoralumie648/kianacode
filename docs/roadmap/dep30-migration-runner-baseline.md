# DEP-30 migration runner baseline (partial)

The domain now defines a fenced MigrationRunnerState with an expiring lease, monotonically
increasing fence token and digest-bound MigrationCheckpoint. It emits strict Started, Step,
Blocked and Completed event shapes, issues a resume token bound to run/owner/registry/fence/event
sequence/checkpoint, and rejects concurrent owners, stale fences, expired leases, checksum drift,
bad tokens and completion before a verified Contract phase.

Blocked state is a quarantine: it cannot apply another step or be resumed by the old state, and a
new start cannot silently continue it. Event and state digests provide the facts that a later
EventLog adapter must append.

DEP-30 remains partial with source/static evidence only. No durable lock or fence CAS, EventLog
append/replay, recovery collector, or persistent failure quarantine is wired. The current state
machine must not be described as cross-process runner durability.
