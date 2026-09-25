# NM-13 run stream/notification bridge baseline

> 快照日期：2026-09-25。测试只由 GitHub Actions 执行；本步骤不在本地运行测试、build、check、
> clippy 或 smoke，且不等待 CI。

`NotificationStreamBridge` consumes the existing daemon-owned `RunStreamFeedSubscription` and
classifies versioned `UiFeedFrameV1` frames. It is snapshot-first: a delta before a boundary,
old epoch/instance, replay expiry, lag/backpressure or sequence gap yields an explicit
`SnapshotRequired`; heartbeat, terminal and disposed states remain visible. It preserves feed and
source cursor identity and never creates another broadcast bus or delivery worker.

`feature_status=implemented`; `proof_level=source`. The bridge is a daemon adapter over the existing
RunStreamBus, not a durable notification fact source. Browser/SSE/CLI/Desktop wiring, cross-process
subscriber recovery, notification action reconciliation and physical/live delivery remain later
steps.
