# ER-15 Adapter Result 统一边界基线

> 快照日期：2026-09-17。本页记录 Hook/MCP/Memory/Patch（以及兼容 shell）在进入
> ControlPlane result commit 前的 bounded result 元数据；它不把 adapter 自身提交误认为
> EventLog 提交，也不把本地文件状态外推为外部业务成功。

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-15`](event-receipt-recovery.md#step-er-15) |
| feature_status | `implemented`（strict AdapterResult + adapter integration） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | ControlPlane committed result；AdapterResult 只描述 adapter boundary，不签发 permit |
| this step does | 统一 adapter kind、process/effect/stop、bounded output/evidence digest、adapter commit state 与 reconciliation fence；Hook 禁止 updated input；MCP disconnect/stop uncertainty 保留 Unknown；Memory sync、Patch transaction 完成后才标记 Committed |
| this step does not | 不新增第二执行循环、不绕过 Broker、不自动 retry effect、不把 Unknown/partial patch/memory cancellation 伪造成成功、不提供 HTTP MCP 或 live provider 证明 |

## 1. Contract

`kiana-domain::AdapterResult` 是 strict、digest-only DTO。`output_bytes` 受 JSON/tool 上限约束，
`output_digest`/`result_digest`/evidence refs 只保留 SHA-256；`AdapterCommitState::Unknown` 或
`effect=Unknown` 必须带 `reconciliation_required`，`NotStarted` 不能带已启动 process。该 envelope
通过 `attach_adapter_result` 写入 object-shaped `CapabilityResult`，保留旧 payload 兼容形状。

四类 adapter 的边界如下：

| Adapter | 自身 boundary | Unknown/拒绝条件 |
|---|---|---|
| Hook | 受信快照 + read-only child 输出后写 `hook.decision` 事实 | `updatedInput`/越权变更拒绝；child stop/输出无法确认则 Unknown |
| MCP | per-invocation stdio frame、child stop、workspace retain/finish | 已发送无响应、disconnect 或 stop 未确认保留 workspace 并返回 Unknown |
| Memory | locked JSONL append + `sync_data` + mutation receipt | 取消不会丢弃 writer；sync/join 失败是 `result_unknown`，不返回成功 |
| Patch | preflight overlay + project lock + prepared journal + atomic rename/fsync | partial commit 先 rollback；rollback/finish 不确定返回 `result_unknown` |

Shell 也附加同一 envelope，保证固定五工具的旧 cassette 与核心 `CapabilityResultReceipt`
使用同一 process/effect/stop shape。AdapterResult 不是事实源；最终状态仍由 EventLog 的
`execution.result_committed` 与 Receipt projection 重建。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `adapter_result_is_common_and_digest_only` | shell envelope 的 strict schema、digest、bounded fields 与 attach round-trip |
| `adapter_unknown_and_cancelled_boundaries_stay_fenced` | MCP Unknown 必须 reconciliation；cancel-before-start 是 NotStarted；digest tamper 拒绝 |
| `hook_observation_rejects_unfenced_unknown_and_unbounded_output` | Hook observation 状态/evidence、Unknown fence 和输出上限 |
| `er15_adapters_share_bounded_commit_and_stop_boundary` | 四类 adapter 接线、Hook mutation deny、MCP retain、Memory sync、Patch transaction/rollback source guard |

## 3. Proof ceiling and handoff

ER-15 proof ceiling 为 `source`：所有现有本地 adapter 的返回路径附加统一 envelope，负向边界
由 CI fixtures/source guard 固化；未运行本地测试。真实 provider、跨进程 durable adapter
checkpoint、文件系统掉电、外部 Hook/MCP effect、live/physical proof 仍未证明；terminal
唯一性、snapshot/restore 和真实集成由 ER-16+ / INT / PD / DEP 收口。
