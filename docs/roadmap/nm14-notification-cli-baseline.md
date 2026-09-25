# NM-14 CLI/TTY inbox and run-status baseline

> 快照日期：2026-09-25。测试只由 GitHub Actions 执行；本步骤不在本地运行测试、build、check、
> clippy 或 smoke，且不等待 CI。

`kiana-entrypoints::notification_cli` is a display-only adapter for the shared NotificationStore
page and existing `UiFeedFrameV1` run feed. It renders bounded rows, urgency/due/read/ACK/snooze
metadata and action id/command labels while dropping payload/detail authority. Run status keeps
Running/Heartbeat/Terminal/Unknown distinct and maps Unknown/gap to `query_original` plus snapshot
reconciliation.

The adapter does not alter frozen `cli.rs`, call Broker/Runner/processes, submit action commands,
approve HumanTask, retry Unknown or treat presenter output as a receipt. `feature_status=implemented`;
`proof_level=source`.

Known limits: no real interactive TTY/CLI E2E, durable inbox/read state, ControlPlane action routing,
SSE/Web/Desktop parity, PTY/accessibility or live/physical delivery evidence is claimed.
