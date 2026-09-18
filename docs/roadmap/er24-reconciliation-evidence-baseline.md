# ER-24 reconciliation commands and evidence baseline

ER-24 keeps runtime outcomes immutable while adding an explicit reconciliation
command. `failure.reconcile` accepts only `observed_succeeded`, `observed_failed` or
`no_effect`; it requires the original source-event reference plus an incident-bound
provider/OS/file/human evidence reference. Evidence must belong to the caller's
authorized scope and the source run; evidence from another incident is rejected.

The command rechecks authority revision and rejects source streams followed by data
revocation or workspace restore facts. It commits `failure.reconciled` through the
existing protected platform stream with source event, authority/data epoch, evidence
scope, revision and idempotency key. Retries are not reconciliation, duplicate keys
replay the original receipt, and the original runtime outcome remains unchanged.

GitHub Actions runs the CP-20 Unknown/reconciliation guard, P2-K6 reliability fixture,
ER-24 source guard and workspace target compilation. Local runtime tests and smoke
commands are intentionally not run.

This is source/static evidence only; external provider/OS evidence stores, cross-process
durability and live/physical reconciliation remain outside this slice.
