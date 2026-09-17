# SC-11 入口与 adapter parity 基线

> 快照日期：2026-09-17。本页记录统一协议/路由 source contract 与 GitHub CI-only fixture；不把
> parity DTO 写成四入口完整 UAT 或真实外部执行证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-11`](security-compliance.md#step-sc-11) |
| feature_status | `implemented`（normalized entrypoint command、parity matrix、DaemonHost shared route） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | DaemonHost→ControlPlane→policy/gate/approval→Broker/Runner→EventLog；UI/adapter 不做 allow |
| this step does | CLI/TTY/Web/Workbench/Desktop/scheduler/swarm/connector labels、共同 command/context/args digest、固定 route、deny zero-effect matrix |
| this step does not | 不实现跨入口完整 transport/session parity、durable projector、Human Inbox UI、external connector/live/physical effect |

## 1. Contract

`kiana-core/src/entrypoint_parity.rs` 新增 strict `EntrypointCommand` 和
`EntrypointParityMatrix`。命令规范化只记录 request ID、command name、arguments/context digest、
固定 `daemonhost.controlplane` route 和 server decision；入口 label 可扩展为 CLI/TTY/Web/
Workbench/Desktop/scheduler/swarm/connector，但不能改变 route 或权限。Matrix 要求所有命令共享
request/command digest、entrypoint 唯一，任一 Denied 时 `handler_calls` 必须为 0；unknown/approval
只是状态，不是本地 allow。

`RequestMetadata` 的可选 entrypoint 字段仅做相关性标记；DaemonHost 仍执行统一的 ingress→
SecurityContext→ControlPlane 路径，legacy metadata 缺失时保持兼容。现有 `EntryPointParitySnapshot`
继续由 ControlPlane 从 EventLog projection 生成，UI/CLI/Web/Workbench/Desktop 只消费该 projection。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `all_entrypoint_labels_share_one_normalized_command_and_route` | 多入口同一 command/context digest 与 route，matrix serde 稳定 |
| `deny_or_unknown_never_claims_handler_effect_and_entrypoint_mismatch_is_rejected` | Denied+handler_calls 非零、digest/entrypoint mismatch fail-closed |
| `unknown_entrypoint_command_fields_and_alternate_route_fail_closed` | alternate broker route/unknown field 拒绝 |
| `entrypoint_parity_has_one_control_plane_route_and_no_local_authority_logic` | source guard 固定 DaemonHost shared route、无入口本地授权/执行 |

## 3. Proof ceiling and handoff

SC-11 proof ceiling 为 `source`：统一 DTO/route/deny matrix 已固化，但各入口仍有历史展示和
transport 适配差异，scheduler/swarm/connector 未全部经过该 metadata 标签，Human Inbox/receipt/
unknown/redaction parity 由 NM/OA/UI/ER 后续接入。SC-12 继续把 command/digest/epoch/fence 与
PendingInvocation/Permit CAS 绑定；任何入口仍不能绕过 ControlPlane 或把 UI/transcript 当事实源。
