# UI-33 reconnect/replay/gap/crash recovery 基线

> 快照日期：2026-09-25。Rust 验收由 GitHub Actions 执行；本步骤不在本地运行测试、build、
> check、clippy 或 smoke，且不等待 CI 结果。

## Recovery plan 与 fence

`kiana-client/src/ui_recovery.rs` 定义 bounded `RecoveryInput`、`RecoveryPlan` 和 `RecoveryFence`。
恢复决策永远是 query/replay/hydrate/reconcile 的 read plan，保留原 command ID、instance/epoch/
sequence 与 limitation；每个 plan 都把 `new_effect_allowed` 固定为 false。

映射规则固定为：Accepted disconnect/kill → QueryOriginalCommand；feed gap/old epoch/worker kill
→ HydrateSnapshot；artifact fetch → QueryArtifact；duplicate terminal → ReplayTerminal；cancel/terminal
不确定 → ReconcileUnknown。Fence 按 instance epoch 和连续 sequence 接受 feed，重复返回 Duplicate，
跳号返回 Gap，旧 epoch 返回 OldEpoch，terminal 后迟到 frame 返回 LateAfterTerminal。

## CI-only fixture 与限制

`kiana-client/tests/fixtures/ui33-recovery.json` 与 `ui33_recovery.rs` 覆盖 original command query、
hydrate/replay/Unknown、duplicate/gap/old epoch/late terminal 和 no-new-effect assertions。
`.github/workflows/ui33-recovery-contract.yml` 运行 Rust format、聚焦 recovery fixture 与 workspace
test-target compile。

`feature_status=implemented`; `proof_level=source`。未证明真实 browser/PTY/Electron/ACP reconnect、
daemon kill/restart、EventLog durable projector、跨进程 command query/effect counter、provider/Broker/
external effect 或 live/physical recovery；CI 结果保持 pending/unobserved。
