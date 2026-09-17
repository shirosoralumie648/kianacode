# P2-M4-01 Run/Artifact 详情基线

> 快照日期：2026-09-18。本文记录 Run timeline、Invocation、Diff、Evidence、Receipt 的共同引用和展示边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Shared detail projection

同一个 owner-scoped run/event stream 是详情的连接键。`kiana-core::receipt_from_events` 从 committed facts 重建 `run_receipt`、typed `execution_receipts`、invocations、files_changed、model/capability/observability、aggregation（含 source event IDs、evidence-ref digests 和 provider receipt refs）；`project_invocations` 和 `project_capability_attempts` 保留 invocation/request/attempt/action/source cursor，artifact/evidence helpers 继续校验引用而不把展示字段升级为事实。

Web `web_thread::items_from_turn` 将同一 response 投影为 user/commandExecution/fileChange/agentMessage/error timeline；`DaemonHost`/Web `/api/receipt` 读取同一 ControlPlane receipt，Workbench 的 `/receipt` 复用相同 envelope，CLI 的产品命令/受控 diff 入口仍只显示服务端结果。run_id、invocation_id、event_id、artifact/evidence refs 和 receipt digest 让时间线、调用、文件 diff、证据与回执可以互相定位；owner/redaction/Unknown/terminal conflict 仍由 core 决定。

## Detail safety

详情视图只读 committed EventLog/Receipt projections，不自行拼装状态、不重新执行 Broker/Provider/Runner。缺失或矛盾 invocation/terminal/source evidence 返回 Unknown/limitation；文件列表是受控回执声明，artifact bytes 仍需 ArtifactStore/hash/permission 校验；Receipt 的 Completed 只证明本机事实链，不证明外部业务 outcome。

## CI-only 验收

`receipt_artifact_and_evidence_cross_locate` core source guard 覆盖统一 receipt aggregation、invocation/execution/artifact/evidence refs、Web/Workbench/CLI timeline and receipt paths、owner/redaction boundary；P2-K4 checkpoint 与 Web thread projection 作为回归来源。

```text
cargo fmt --all --check
cargo test -p kiana-core --test p2_m4_01_run_artifact_detail --locked -- --test-threads=1
cargo test -p kiana-core --test p2_k4_01_checkpoint --locked -- --test-threads=1
cargo test -p kiana-entrypoints --lib web_thread::tests --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- 当前统一连接是 receipt/aggregation 的 JSON projection 与 Web thread items，尚无独立 typed RunDetail endpoint、durable detail index、artifact diff store 或跨进程 UI cache；后续 UI/PD/ER work 负责。
- CLI/TTY/Web 共享 server facts 和 ControlPlane，但呈现层仍不同；完整四入口视觉/交互 parity、artifact bytes diff、large evidence pagination、external human auth、live/physical effect proof 不在本切片。
- source event/evidence/artifact refs 可定位本机事实，但不等于外部 provider receipt、业务 acceptance 或现实世界 rollback。
