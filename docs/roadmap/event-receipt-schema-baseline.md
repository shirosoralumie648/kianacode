# ER-01 event schema, kind registry, and migration baseline

> 快照日期：2026-09-16。本文记录 ER-01 的 machine-readable event kind/version/migration source
> boundary；RuntimeEvent 旧 envelope 继续兼容读取，运行时 fixtures 只在 GitHub CI 执行，本地不运行测试。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-01`](event-receipt-recovery.md#step-er-01) |
| source snapshot | `b597dfe`（CP-06 atomic transition contract 后的干净基线） |
| feature_status | `implemented`（EventKindSpec registry、payload/version validator、unknown policy、migration map source） |
| proof_level | `source`；静态编译不能提升为 local_behavior/durable/live/physical |
| canonical path | RuntimeEvent envelope → EventKindSpec/version/payload interpretation → EventLog/projector/Receipt |
| this step does | 固定 owner、aggregate、required IDs、terminal/secret policy、allowed fields、schema version 和 legacy migration；unknown opaque event 只读保留，required family unknown fail-closed |
| this step does not | 不修改旧 RuntimeEvent serde 形状，不把 unknown event 当执行权，不凭 registry 存在宣称所有 legacy payload 已迁移或 durable reader 已完成 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Event registry/RuntimeEvent/journal | `kiana-domain/src/event_contracts.rs`, `kiana-domain/src/contracts.rs`, `kiana-domain/src/states.rs`, `kiana-domain/src/journal.rs`, `kiana-domain/src/lib.rs` | `9af3362951ac13d2faf8abde6a6ea2ba38659c94b48e7a789e51d20b28508745`, `cfd802764d31a331eddfeecdd97270af146c39f868513c75a36cdd7e59212954`, `dca28dc71ef75e5dd92a25bed399349ca286bc7c5c0e514e11de20d6d1491ae3`, `4dbd656d03ec12e7821ffac254307227419423cbaf74ccb99a2b819c5eeedd48`, `080b20570e29fedcd062e53b6391d138b2db4e31c81fc91332638578dcd07c83` |
| Protocol boundary | `kiana-protocol/src/lib.rs` | `8fa329b16172f81eda38f0d4ab49f03d35f2cdb524420ecf020dc872727c1254` |
| Fixtures/guard/workflow | `kiana-domain/tests/er01_event_contract.rs`, `kiana-core/tests/er01_event_contract_guard.rs`, `.github/workflows/er01-event-schema.yml` | `34e924faf84ad1cc9e2ce1c928b165416775591c73d322627d549ac300f8f68c`, `3d78c2ef8538d7b8400e89b8644d5a57fcf3c8b5baee22e3d6eaa8fd837c3645`, `e4b638fb2917a0388cb3824beb0b014d60b61178115f24a23c287826cf4d7bb9` |

hash 仅固定本步 source snapshot 的解释层边界；不代表 legacy 全量迁移或事件事实已被重写。

## 2. Registry contract

`kiana-domain/src/event_contracts.rs` 提供 `EVENT_KIND_SPECS` 和 `EventKindSpec`。每个登记的 kind 显式声明：

| 字段 | 作用 |
|---|---|
| `schema` / `version` | 当前 RuntimeEvent 解释格式；major 不兼容时拒绝 |
| `owner_crate` / `aggregate_type` | 事实 owner 与 CAS stream 归属；不能由 payload 自报替换 |
| `required_ids` | 进入对应 projector 前必须存在的 run/request/approval/invocation 关联 |
| `allowed_fields` | payload 可接受字段；未知字段不静默丢弃 |
| `terminal` | 仅供投影器解释终态，不直接授予执行权 |
| `secret_policy` | 事件写入必须经过 redaction 或拒绝；原始 secret 不在 registry payload 中 |
| `migration` | legacy reader/upcaster 名称；无确定 migration 的记录只能查询 |

当前 registry 覆盖 request/run/capability/approval/invocation/execution/action/session 关键 kind，并注册统一 `kiana.runtime-event.v1` schema。新增公开 kind 必须同时补 owner、aggregate、IDs、fields、迁移/unknown 语义和 CI fixture；不能只在 `RuntimeEvent.kind` 字符串处追加。

## 3. Unknown and version boundary

| 输入 | 处理 |
|---|---|
| 未登记且非 required family（例如 `future.opaque`） | `PreserveOpaqueWithoutExecution`：EventLog 可保留，history/projection 不猜状态，不产生授权 |
| 未登记但属于 request/run/capability/approval/invocation/execution/action/session family | `unknown_required_event_kind`，在 authority/projection 前拒绝 |
| 已登记 kind + unknown major | `event_schema_version_incompatible`，不能按旧字段猜测 |
| 已登记 payload 缺 required ID | `event_required_id_missing:<field>`，不得配对到另一个 run/sequence |
| 已登记 payload 有未知字段 | `event_payload_unknown_field`，不能 serde drop 后继续 |
| legacy v0 有确定 upcaster | 只按 `EVENT_MIGRATIONS`/spec migration 生成 v1 解释；原始事件不覆盖 |
| legacy v0 无法确定 turn/aggregate/owner | 保留 opaque/query-only，不能派发、审批或恢复 |

`validate_runtime_event` 是解释层显式 validator；为兼容旧 JSONL，RuntimeEvent 的基础构造/读取仍保留旧字段和未知 kind。EventStore/各 projector 在逐步接线前，不能把“registry 有条目”误写成“历史全部已验证”。

## 4. Core/EventLog handoff

`TransitionBatch`/JournalFrame 仍负责 command digest、aggregate read-set、CAS、frame integrity 和 all-or-none；ER-01 registry 只定义事件语义，不创建第二事实源。`RuntimeEvent.event_id`、request sequence、aggregate stream version 和 idempotency key 继续由 EventLog 所有；kind validator 不能重写已提交事实。

Receipt、history、invocation/run projector 应按以下顺序消费：

```text
read committed frame
  -> validate header/writer/version
  -> classify known/opaque/required-unknown kind
  -> validate required IDs/allowed fields/redaction boundary
  -> apply deterministic migration (if registered)
  -> fold projection; otherwise preserve Unknown/query-only
```

旧 adapter 只读兼容不代表支持新 schema 写入；writer format、migration result、projection cursor 和 receipt digest 仍需后续 ER-02..08/PD/CP 验收。

## 5. CI-only fixture catalog

| Fixture | Purpose |
|---|---|
| `event_kind_registry_is_machine_readable_and_bounded` | kind/schema/owner/aggregate/terminal/secret metadata 完整 |
| `unknown_required_kind_and_schema_downgrade_fail_closed` | opaque unknown 可查询；required family unknown 和 major downgrade 拒绝；migration lookup 确定 |
| `payload_unknown_field_is_not_silently_dropped` | required ID/allowed field 检查和 legacy opaque policy |
| `event_contract_registry_and_migration_boundary_are_source_owned` | domain/contracts/states/journal/protocol source guard |

`.github/workflows/er01-event-schema.yml` 在 GitHub runner 执行上述 domain fixtures、core guard、`cargo fmt --all --check` 和 `cargo fetch --locked`；本地不运行测试，不把 CI 结果写成 durable/live。

## 6. 限制与交接

- 当前 `RuntimeEvent` 没有强制内嵌 schema/version 字段；registry 是 additive interpretation layer，完整 EventStore/projector 接线由 ER-02+ 完成。
- `allowed_fields` 是关键 kind 的 bounded contract，不宣称覆盖所有 177+ 历史 event literals；未覆盖 kind 仍按 opaque/required-family policy 处理。
- legacy migration map 只声明确定的 v0→v1 family 名称，不会猜测缺失 TurnId、aggregate、owner 或 secret provenance；歧义只能查询。
- payload validator 不替代 redaction、Artifact 引用、CAS、receipt correctness、external effect/reconcile、backup/retention/delete 或 cross-process recovery。
- 本地只做格式、workspace test-target 静态编译和 diff 检查；GitHub CI 结果按用户要求不等待，后续 ER-02/03 应在新快照刷新 hash。
