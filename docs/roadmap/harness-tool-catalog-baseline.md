# H09 Tool Catalog 基线

> 快照日期：2026-09-17。本页记录模型工具 schema、wire、Capability、风险和 Broker 绑定的一致性；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H09`](harness.md#step-h09) |
| feature_status | `implemented`（versioned ToolCatalogSnapshot + mapper/provider pin） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | server-owned `ToolCatalogSnapshot`/`ToolSpec` + action catalog；Harness only advertises and maps, ControlPlane/Broker revalidate |
| this step does | catalog version/digest、wire name、argument/result schema、capability/risk、output limit、execution/replay class；prepared request/checkpoint/action digest pin；schema mapper/provider wire resolver reuse |
| this step does not | 不批量接入历史工具，不让模型文本直接执行，不把 aliases 当新 capability，不把静态 descriptor 当授权或 external effect 证明 |

## 1. Contract

`ToolCatalogSnapshot::current()` 从 server-owned `ToolSpec` 与模型 schema 构造严格版本化快照，
并校验 descriptor identity、wire/alias 唯一性、schema、输出上限和 replay class。每个
`PreparedModelCall` 的 `tool_catalog_hash` 同时绑定 catalog digest 与该步可见 schema；目录改变
会改变 action catalog digest、配置 revision 和 checkpoint pin，旧 approval/prepared action 不可复用。

Runner 的 `capability_for_tool` 先校验 snapshot 再按 descriptor 解析能力/operation；Provider wire
name resolver 复用同一 catalog。Harness 在 run/checkpoint 固定 catalog digest，漂移或恢复时不
悄悄换目录。当前五个工具仍是 baseline 数据集，但 catalog contract 不再以 `len == 5` 作为架构上限。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `unadvertised_tool_and_wire_name_collision_are_rejected` | 未声明模型工具与 wire collision fail-closed |
| `catalog_change_cannot_reuse_old_approval` | descriptor 改动导致 snapshot digest 校验失败，旧 approval 不能复用 |
| `schema_mapper_and_broker_resolve_same_tool_version` | mapper 生成的 operation/capability 与 snapshot descriptor 一致 |
| `h09_tool_catalog_is_versioned_and_single_source` | source guard 固定版本、digest、pin 和无 fallback |

## 3. Proof ceiling and handoff

H09 proof ceiling 为 `source`：catalog/mapper/provider pin 与 action/checkpoint 漂移拒绝已接线，
CI-only fixtures/source guard 固化。真实第三方 provider wire catalog、多版本迁移/别名下线、
Broker handler 远程一致性、approval durable recovery 与 external/live/physical proof 仍留待 H10+ / CP/PD/P4/INT。
