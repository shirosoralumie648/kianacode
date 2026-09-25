# UI-35 旧 Web/CLI 迁移与兼容收口基线

> 快照日期：2026-09-25。Rust 验收由 GitHub Actions 执行；本步骤不在本地运行测试、build、
> check、clippy 或 smoke，且不等待 CI 结果。

## One-way mapping

`LegacyRouteMapping` 使用 `kiana.ui-legacy-migration.v1`，把旧 Web `GET /api/state`、`GET
/api/events` 和旧 CLI `run`/`cancel`/`receipt` 映射到 `ui.snapshot`/`ui.feed`/`turn.prompt`/
`turn.cancel`/`receipt.query`。已知 legacy route 明确标记 Deprecated、`requires_typed_client=true`
和 `writes_facts=false`；unknown、NUL、超长或恶意输入返回稳定拒绝。

Mapping validation also rechecks the canonical allowlist, deprecation text bounds and the expected
read-only bit for each operation; a forged mapping cannot widen a legacy route into a new capability.

兼容窗口只能做单向适配/迁移提示；它不能自读写 EventLog、直接执行 capability、改变 retry/Unknown
语义或生成第二个 runner loop。旧 route 的事实与授权仍由现有 typed client → DaemonHost →
ControlPlane 路径提供。

## CI-only fixture 与限制

`kiana-client/tests/fixtures/ui35-legacy-migration.json` 与 `ui35_legacy_migration.rs` 覆盖 known
route deprecation、canonical mapping、unknown/NUL injection、no-fact-write 和 no-second-loop
metadata。`.github/workflows/ui35-legacy-migration.yml` 运行 Rust format、聚焦 migration fixture
和 workspace test-target compile。
其 push/pull_request path filter 现在同时包含当前 CM-36 `kiana-domain/src/memory_workbench.rs`，
fresh remote run 会覆盖 repository-wide fmt dependency；该远程结果 pending/unobserved。

`feature_status=implemented`; `proof_level=source`。未证明真实旧浏览器/CLI traffic usage、双读
projection capture、feature-flag rollout/deprecation telemetry、实际删除旧 endpoint、跨进程 durable
compatibility、provider/Broker/external effect 或 live/physical proof；CI 结果保持 pending/unobserved。
