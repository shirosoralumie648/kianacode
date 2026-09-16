# P0-A-01b schema registry baseline

> 快照日期：2026-09-16。本文记录 canonical schema registry、domain/wire/runtime layer、unknown field/event 和 major migration 规则；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-A-01b`](../roadmap.md#step-p0-a-01b) |
| source snapshot | `4d48220`（CO-08 parent）加本步 evidence/fixtures；最终 commit 记录在 git history |
| feature_status | `implemented`（existing domain registry/event registry verified and CI-wired） |
| proof_level | `source`；静态编译与远程 fixtures 不提升为 local_behavior/durable/live/physical |
| authority path | schema_contract/SchemaLayer + check_schema_compatibility → strict domain serde / wire compatibility → event_kind registry/migration → existing ControlPlane/EventLog |
| this step does | 为已存在的 `SCHEMA_CONTRACTS`、`SchemaLayer`、`SchemaVersion` 和 `EVENT_KIND_SPECS` 建立专用验收与 source guard；确认 wire 可按规则接收 additive minor，domain/runtime unknown field 拒绝，unknown major/schema/event 不执行，已登记 migration 只能显式使用 |
| this step does not | 不另造第二 schema registry、不把 unknown opaque event 当执行事实、不改变已有业务状态迁移语义；具体 upcaster、全量 legacy event coverage 和 durable migration runner 留给 ER/PD/CO-08+ |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Schema contracts | `kiana-domain/src/contracts.rs` | `aa796d275675e113d0a5fa338664fae0d4234357ee833de19acb384e72370403` |
| Event kind/migration registry | `kiana-domain/src/event_contracts.rs` | `5ba2b23e9999bb07ea39d26fe0672a19f1e519c262ae165c222f9cb6be24f4cb` |
| Protocol/core guard | `kiana-protocol/src/lib.rs`, `kiana-core/tests/p0_a01b_schema_guard.rs` | `a5efad3bbff0cd67f82ad79b4d137dfec3424cc1c6f3969ba9eaacc356cc260c`, `91a134ebb6af366c1d1208695f8ec80222268a539202035fa5f910df155ffd34` |
| Fixtures and CI | `kiana-domain/tests/p0_a01b_schema.rs`, `.github/workflows/p0-a01b-schema.yml` | `a88d52e960890ddb85cc3a691ed5f43a69db4daa5cc03f06476e5cd3715d0c95`, `f21f495ae29e7b9b300dee78d5d0c73b10dd01ebe2835ec0e57ce55ca51caea3` |

## 2. Registry rules

`SchemaContract` 为每个公开 schema 固定 owner、layer、当前版本、compatibility 和 unknown-field policy。`check_schema_compatibility` 对未知 schema 直接错误，major 不匹配 fail-closed，minor 在同一 major 内按 contract 规则兼容；domain/runtime contracts 的 `allow_unknown_fields=false` 与 `serde(deny_unknown_fields)` 配对，wire envelope 保留 additive 兼容边界。

`EventKindSpec`/`EVENT_KIND_SPECS` 登记 required IDs、allowed fields、aggregate/terminal/secret policy 和 migration metadata。未知但可读的 opaque event 只可查询；落在 request/run/capability/approval/invocation/execution/action/session family 的未知 kind 直接 `unknown_required_event_kind`。`event_migration` 只返回登记过的 family/from/to upcaster 名称，不猜测未来 major。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `schema_registry_separates_layers_and_fails_closed_on_unknown_major` | wire/domain/runtime layer 与 unknown field policy 清晰，未知 schema/major 拒绝，minor 兼容 |
| `event_unknown_and_migration_rules_are_explicit` | known/opaque/required unknown event 和 migration registry 行为显式 |
| `schema_and_event_registry_are_the_single_compatibility_boundary` | protocol/core 只复用这两个 registry，不新增旁路判断 |

`.github/workflows/p0-a01b-schema.yml` 在 GitHub runner 执行 domain schema/event fixtures、core source guard、fmt 和 domain/protocol/core test-target compile；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- registry 已覆盖当前列出的 contracts/event kinds，但并不证明所有 legacy payload 都有 upcaster；事件 registry 仍需 ER-02+ 全量接线和 durable projector 验收。
- unknown major 的原始数据保留/隔离由具体 EventStore/PD migration adapter 决定；本步只证明 compatibility decision fail-closed。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
