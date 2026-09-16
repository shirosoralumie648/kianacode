# P1-E-01 typed communication and accountability baseline

> 快照日期：2026-09-16。本页记录 Chat、Command、Handoff、Decision、StatusReport、Evidence、Incident 七类通信合同，以及定向 handoff/ACK 与“聊天不授予 authority”边界；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-E-01`](../roadmap.md#step-p1-e-01) |
| feature_status | `implemented`（typed message + ControlPlane event boundary source） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | CommunicationMessage → CommunicationPort/ControlPlane `communication.send` → redacted EventLog fact → Human/UI projection |
| this step does | 七类消息枚举、schema/kind registry、bounded/digest DTO、Chat authority-field denial、server sender check、Handoff recipient/ACK contract 和 formal communication event kinds |
| this step does not | 不把聊天、通知 ACK 或模型文本变成审批/Grant；不实现外部通知 channel、durable inbox/read state、outbox 或 delivery worker |

## 2. Message/accountability rules

- `CommunicationMessageKind` 明确区分 Chat、Command、Handoff、Decision、StatusReport、Evidence、Incident；消息 body 是事实/请求描述，不是执行结果或授权凭证。
- Chat 必须没有 `action_ref` 和 ACK 标志，`grants_authority()` 恒为 false；任何需要改变权限或业务状态的动作必须另发 ControlPlane command，并重新经过 policy/gate/approval。
- `communication.send` 只接受 server context actor 作为 sender，验证 project trust、message digest 和 kind 后通过 `record_event` 追加 formal event；事件 payload 明确 `authority_granted=false`。
- Handoff 必须有 recipient 且要求 ACK；既有 `PacketHandoff::acknowledge` 继续绑定目标 role、不同 session、有效期和 reason，ACK 只确认接收，不等于执行许可。
- `CommunicationPort` 只是事实追加边界，adapter 不得从 message kind/文本推断 Grant；UI/通知只能投影已提交消息和待处理 action。
- `communication.*` event kinds 登记在现有 RuntimeEvent registry，并限制 required `message`/server metadata fields；未知 `communication.*` 变体 fail-closed，不作为 opaque authority fact。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `free_chat_never_grants_authority` | Chat 无 action/ACK、digest 有效且 `grants_authority=false`；添加 action_ref 或 unknown field fail-closed |
| `handoff_is_directed_and_requires_ack` | Handoff 缺 recipient/ACK 被拒，合法 handoff 不携带 authority |
| `communication_layers_are_typed_and_cannot_grant_authority` | domain/ports/core 命令路径有七类消息、sender 校验、formal event 和 authority=false guard |

`.github/workflows/p1-e01-communication.yml` 在 GitHub runner 执行 domain message fixtures、core source guard、fmt 和 domain/ports/core/protocol test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 当前消息事实写入复用现有 EventStore，但没有独立 NotificationStore/outbox/delivery receipt/read-state；NM/ER/PD 后续步骤负责。
- `CommunicationPort` 尚无生产 adapter；消息类型与 ACK 证明的是 source boundary，不证明跨进程送达、外部通道或业务决定执行。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
