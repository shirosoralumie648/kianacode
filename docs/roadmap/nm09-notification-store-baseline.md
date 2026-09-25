# NM-09 in-process/in-app NotificationStore query/page baseline

> 快照日期：2026-09-25。测试只由 GitHub Actions 执行；本步骤不在本地运行测试、build、check、
> clippy 或 smoke，且不等待 CI。

`kiana-core::NotificationStore` consumes the committed-only `NotificationMaterializer` and exposes
bounded recipient-scoped `list` pages plus projection-only `mark_read`/`acknowledge` mutations.
Pages bind the materializer source cursor and digest, cap page size at 30, distinguish unavailable
projection from an empty result, and reject stale/unknown cursors or foreign recipients.

Read/ACK state is process-local presentation state. It never copies HumanTask/Approval status,
does not delete source history, does not call EventLog/Broker/Runner, and does not turn an ACK into
approval, retry, cancel or resume. `feature_status=implemented`; `proof_level=source`.

Known limits: no durable NotificationStore/read state, cross-process rebuild, NM-10 action command,
NM-11 urgency/read ordering, NM-12 retention/withdraw, Web/SSE/Desktop channel or live receipt
evidence is claimed. Empty/unavailable, stale/foreign and mutation idempotency boundaries are only
covered by GitHub CI fixtures and source guards.
