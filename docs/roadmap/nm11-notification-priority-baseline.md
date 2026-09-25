# NM-11 notification urgency/read/ACK/snooze/digest baseline

> 快照日期：2026-09-25。测试只由 GitHub Actions 执行；本步骤不在本地运行测试、build、check、
> clippy 或 smoke，且不等待 CI。

NM-11 extends the in-process `NotificationStore` projection with deterministic server-derived
urgency, due time, source cursor and digest-group ordering. `NotificationUrgency` maps only the
server-owned Human Inbox kind; client clocks and presenter text cannot reprioritize items. Page
items expose read/ack/snooze projection metadata without copying HumanTask/Approval authority.

Snooze is bounded projection state and does not remove critical or Unknown items. Read/ACK/snooze
are idempotent, clock-fenced projection mutations; none is approval, retry, cancel, resume, EventLog
append or Broker effect. `feature_status=implemented`; `proof_level=source`.

Known limits: no durable read/snooze state, digest delivery worker, retention/withdraw, action
command, Web/SSE/Desktop surface, cross-process rebuild or live/physical delivery evidence is
claimed. CI fixtures and source guards cover ordering and no-authority boundaries only.
