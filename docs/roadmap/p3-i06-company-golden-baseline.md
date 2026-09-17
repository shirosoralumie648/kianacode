# P3-I-06 Company fake-model coding 黄金闭环基线

> 快照日期：2026-09-18。运行时验收只由 GitHub Actions 执行；本地不运行测试或 smoke。

## Golden path

`kiana-daemon/tests/p3_i06_company_golden.rs` 通过真实 `DaemonHost::handle` 协议入口注入
`ScriptedModel`。它先用 Sponsor 注册 Charter/Decision、提出并激活 Objective、提出并批准
Project，再由 PM 创建 Milestone、批准 WorkPacket 并规划项目。Builder 的 fake model 只能在
冻结的 packet/path scope 内通过 `spawn_from_packet` 写入 `OUTPUT.txt`；运行完成后，Builder
提交 Acceptance，新的 Reviewer session 记录完整 criterion/evidence，Reviewer/Sponsor 决定
接受，Closer 注册并准备 artifact delivery，Sponsor 批准，Closer 交付并由具名 recipient
确认，最后 `CloseProject` 生成带 run/artifact/evidence/review/delivery linkage 的
`CompanyClosingReceipt`。每个命令都经过同一 revision/CAS/idempotency/authority 入口，断言
实际文件和 EventLog evidence，而不是只检查最后的 status 文本。

## Failure and replay boundary

`CompanyState` 与 `CompanyReplayReducer` 保留拒绝、返工、暂停/恢复、取消、失败关闭、
Delivery `ResultUnknown`/Reconcile 和重复命令的显式状态。Unknown 关联 Incident/原始
evidence，不能自动重试、伪造 Delivered/Closed(success) 或由模型文本改写；旧失败事实在
重放中保留。Core source guard 逐项检查这些边界及 `CompanyCommandReceipt`、artifact/evidence
引用和无第二模型/Capability 执行循环约束。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-daemon --test p3_i06_company_golden --locked -- --test-threads=1
cargo test -p kiana-core --test p3_i06_company_golden --locked -- --test-threads=1
cargo test -p kiana-core --test p3_i05_closeout_outcome --locked -- --test-threads=1
cargo test -p kiana-core --test oa27_company_governance --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、
`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试，也不
等待 GitHub CI。

## 限制与交接

- 此切片的可执行 fixture 是本地 EventLog/Artifact/ScriptedModel 的受控 CI 场景；它不证明
  live provider、外部接收者、真实业务 KPI、跨进程 power-loss recovery 或 physical delivery。
- Company aggregate/replay 与 artifact store 仍由当前 DaemonHost/ControlPlane 组合；独立的
  durable Company query index、完整故障矩阵和四入口用户流程属于后续 CO/EQ/ER/PD/DEP/SC 卡。
- 不等待本次 CI 结果，因此本提交将证明等级保持为 `source`；CI 红灯需要后续单独修复，不能
  通过删弱断言或把 Runtime Completed 当作业务成功来收口。
