# UI-31 CLI/Workbench/Web/Desktop 行为 parity 基线

> 快照日期：2026-09-25。Rust 验收由 GitHub Actions 执行；本步骤不在本地运行测试、build、
> check、clippy 或 smoke，且不等待 CI 结果。

## Shared trace contract

`SurfaceTrace` 是只读的 server-result comparison DTO，绑定 `command_id`、operation、
disposition、retry disposition、cursor epoch/sequence、object revision、Receipt digest 和
stable error code。四个 surface 必须各有且仅有一条 trace：CLI、Workbench、Web、Desktop；文案、
颜色、快捷键、layout 和入口 limitation 不进入 equality key。

`compare_surface_traces` 先做 schema/field/cursor/revision/receipt-digest/error-code/sensitive-field validation，再
检查四 surface 集合与 identity equality。`Unknown` 仍可 parity，但所有 surface 必须保留同一
`query_original`/limitation 语义；任何 drift 返回稳定错误，不自动 retry、resume、cancel 或调用
Broker。surface-specific limitations 仅按集合排序合并进 read-only report。

## CI-only fixture 与限制

`kiana-client/tests/fixtures/ui31-parity.json` 与 `ui31_surface_parity.rs` 覆盖四 surface happy
trace、command/disposition/retry/cursor/revision/receipt drift、missing/duplicate surface、
sensitive field 和 Unknown parity。`.github/workflows/ui31-surface-parity.yml` 运行 Rust format、
聚焦 comparator fixture 与 workspace test-target compile。其 push/pull_request path filter 现在同时
包含当前 CM-36 `kiana-domain/src/memory_workbench.rs`，因此 fresh remote run 会覆盖
repository-wide fmt dependency；该远程结果 pending/unobserved。

`feature_status=implemented`; `proof_level=source`。未接入真实四入口 command harness、CLI/Web/
Desktop runtime trace capture、跨进程/durable parity index、真实 browser/PTY/Electron timing、
provider/Broker/external effect、crash/recovery 或 live/physical proof；CI 结果保持 pending/unobserved。
