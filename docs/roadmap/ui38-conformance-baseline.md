# UI-38 协议/入口 conformance 集成门基线

> 历史快照：2026-09-25 创建时，测试、workspace check 和 conformance gate 计划由 GitHub Actions
> 执行。该历史约定不是当前限制；当前 head 的受限本地验证见下文。

## Deliverables

- `kiana-client/src/conformance.rs`：消费四 surface trace 的只读 schema/cursor/action/retry/digest/
  receipt/Unknown comparator，复用 UI-31 parity 规则，并 fail-closed 校验可选 artifact/receipt digest
  的 SHA-256 形状。
- `kiana-client/tests/fixtures/ui38-conformance.json` 与 `ui38_conformance.rs`：同一 Unknown trace 的
  四入口一致性、隐藏 Unknown、敏感字段和 capability schema drift deny-first fixture。
- `.github/workflows/ui38-conformance.yml`：GitHub-only `cargo fmt`、聚焦 conformance test 和 workspace
check gate。
其 push/pull_request path filter 现在同时包含当前 CM-36 `kiana-domain/src/memory_workbench.rs`，
fresh remote run 会覆盖 repository-wide fmt dependency；该远程结果 pending/unobserved。

source slice 只验证可比较 trace metadata；CLI、Workbench、Web、Desktop 和 ACP fake peer 的真实
transport/host/effect 仍不是本次切片的直接执行对象。comparator 不提交命令、不 retry/cancel/resume/
approve、不写 EventLog、不启动 provider/Broker 或第二执行循环。

`feature_status=implemented`; `proof_level=source`。远程 CI 结果 pending/unobserved，因此不能提升为
`local_behavior`、`durable`、`live` 或 `physical`。

已知限制：未证明真实四入口 trace capture、browser/PTY/Electron/ACP runtime、跨进程 cursor/replay、
真实 artifact/receipt 投影、provider/connector effect、差异 cassette 持久化和 live host 兼容；CI 结果
不等待。

## Current-head local fixture validation (2026-10-06)

`cargo test -p kiana-client --test ui38_conformance --locked --offline -- --test-threads=1` passed
3/3 tests (0.52s, max RSS 228,100 KiB, no swap). Unsupported capability schema versions are asserted
as schema-invalid; a separate valid-but-different artifact digest exercises the cross-surface
mismatch path.

This verifies the focused comparator locally. It does not capture traces from running CLI,
Workbench, Web, Desktop or ACP surfaces, nor prove transport, receipt projection or live effects.
