# NM-03 notification event registry baseline

> 快照日期：2026-09-17。本页记录通知来源事件的 server-owned 分类与 source guard；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`NM-03`](../roadmap.md#step-nm-03) |
| feature_status | `implemented`（notification event classification/source guard） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | committed RuntimeEvent → NotificationEventSpecs/source validation → future notification projector |
| this step does | registered approval/run terminal/unknown/handoff/status/evidence/incident/reminder mapping, owner/source/critical rules and required event-kind rejection |
| this step does not | 不实现 NotificationProjector、cursor/checkpoint、recipient resolution、delivery/outbox 或改变旧事件事实；NM-04+ 负责 |

## 2. Registry rules

`NOTIFICATION_EVENT_SPECS` 是 domain-owned allow-list：critical approval、run terminal/unknown、handoff、incident 只能来自 ControlPlane/EventLog，必须有非空 owner (`owner_id`/`actor_id`/`sender_id`/`request_id`)；model/UI source 即使自报 approval/completion 也 fail-closed。Status/Evidence/Reminder 也必须先在 registry 中登记才能 materialize；required family 的未知 kind 一律拒绝，普通 opaque event 不产生通知。

`validate_notification_runtime_event` 在同一 source validation 函数上服务 replay/projector；若 payload 显式声明 `source`，必须与 server 解析的 source 相同。Core `append_event` 只在输入显式携带 source metadata 时调用该 guard，以保留当前大量 legacy event payload 的兼容性；未来 projector 必须对已提交事件指定可信 source，不得从 transcript/UI/model 自述推断。

现有 RuntimeEvent `EVENT_KIND_SPECS` 继续拥有 wire/aggregate/field allow-list；NM-02 新增的 Handoff ACK/Incident escalation kinds 已登记。两层 registry 都不授予权限，只有 committed EventLog 可作为通知输入。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `notification_event_registry_classifies_only_registered_committed_kinds` | approval/run terminal/unknown/handoff/status/evidence/incident/reminder 映射固定，unknown required kind 拒绝 |
| `model_ui_self_report_and_ownerless_critical_events_are_rejected` | model/UI 自报、ownerless critical、source mismatch/unknown source 均 fail-closed |
| `runtime_event_classification_requires_a_registered_server_source` | RuntimeEvent helper 只接受登记 kind 与可信 source；future communication kind 拒绝 |
| `notification_registry_is_server_owned_and_checked_at_event_boundary` | domain registry 与 core append boundary/source guard 互相接线，无模型/UI authority path |

`.github/workflows/nm03-event-registry.yml` 在 GitHub runner 执行 domain registry fixtures、core source guard、fmt 和 domain/ports/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- Source label 是 server-side classification input，不是 OS peer credential；真实 authenticated process identity、project trust 和 durable owner 仍由 CI/SC/PD/CP 负责。
- Core legacy events without explicit `source` 暂不自动 materialize；未完成 committed-only projector/cursor/checkpoint，任何 notification UI 仍是派生视图。
- Registry presence 不证明现实 approval/completion 或 recipient delivery；NM-04/05/06/07/08 继续处理 projector、scope、OCC、outbox、Unknown 与 recovery。
