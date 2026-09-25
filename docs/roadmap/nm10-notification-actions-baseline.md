# NM-10 notification action refs and HumanTask action command baseline

> 快照日期：2026-09-25。测试只由 GitHub Actions 执行；本步骤不在本地运行测试、build、check、
> clippy 或 smoke，且不等待 CI。

`NotificationActionCommand` is a strict, redaction-safe command candidate. It binds one registered
`ActionRef` to an exact notification scope/id, target revision/digest, recipient/decider, authority
epoch, committed source cursor, opaque payload and idempotency key. `NotificationActionGate` compares
the command against the server-authoritative ref and current target before returning a typed
ControlPlane admission.

The gate returns `control_plane_required=true` and `direct_effect=false`; it does not mutate
HumanTask/Approval, append EventLog facts, consume a permit, invoke Broker, retry, or start a worker.
Stale ref/target/epoch/cursor, wrong decider, expiry, scope widening, unknown action and secret
payload fail closed. `feature_status=implemented`; `proof_level=source`.

Known limits: this is a source/CI admission boundary only. Actual registered CommandRequest routing,
single-consume HumanTask decision, wait-key wakeup, receipt settlement, cross-process idempotency and
external/live/physical actions remain later ControlPlane/NM/ER work.
