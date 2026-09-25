# UI-38 协议/入口 conformance 集成门基线

> 快照日期：2026-09-25。测试、workspace check 和 conformance gate 只由 GitHub Actions 执行；本步骤
> 不在本地运行测试，并且不等待 CI 结果。

## Deliverables

- `kiana-client/src/conformance.rs`：消费四 surface trace 的只读 schema/cursor/action/retry/digest/
  receipt/Unknown comparator，复用 UI-31 parity 规则。
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
