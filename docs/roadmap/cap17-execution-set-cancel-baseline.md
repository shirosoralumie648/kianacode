# CAP-17 execution-set cancellation and revocation baseline

CAP-17 reconciles the durable RunCancellationFact/`run.cancelling` barrier with every execution
class already present: queued tool work, pending approval, Runner pending tools, shell/process
children and MCP stop. The control plane persists the cancellation command before signaling,
closes not-started work as `not_executed`, and only emits `run.cancelled` after every started
effect has confirmed its StopReport. Any unconfirmed stop or late result remains
`run.result_unknown`/fenced; cancellation never creates a second dispatch path or automatic retry.

The existing CP-15/CP-16 and daemon race fixtures are linked into one CAP-17 remote gate. This
step does not claim cross-process recovery, external effect reconciliation or physical stop proof.
No local runtime tests or smoke commands were run.
