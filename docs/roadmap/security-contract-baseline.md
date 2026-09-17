# SC-02 安全 ID 与 schema 契约基线

> 快照日期：2026-09-17。本页记录 domain source contract 和 CI-only deny-first fixtures；不把
> 类型、编译或 CI 结果写成认证、durable、live 或 physical 证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-02`](security-compliance.md#step-sc-02) |
| feature_status | `implemented`（domain IDs、canonical registry view、versioned envelope、upcast boundary） |
| proof_level | `source`；本步未执行本地测试，CI workflow 负责运行夹具 |
| authority | `kiana-domain::SCHEMA_CONTRACTS` 是唯一 schema 事实源；`SecuritySchemaRegistry` 只是链式安全投影 |
| execution spine | `entrypoints → protocol → DaemonHost → ControlPlane → policy/gate/approval → Broker/Runner → EventLog → Receipt/projection` |
| this step does | 稳定 security IDs、UUID 生成/解析、严格 registry snapshot、digest/epoch/sequence 链接、secret-safe envelope、显式 upcast 拒绝 |
| this step does not | 不实现 SecurityContext 解析、认证、策略、Grant 交集、SecretStore、Audit projector、执行或第二 schema/执行循环 |

## 1. Source contract

`kiana-domain/src/ids.rs` 新增 `SecurityRegistryId`、`SecurityContextId`、
`SecurityPolicyId`、`SecurityDecisionId`、`SecurityEventId`、`GrantId`、`OperationId`、
`AuditId`、`SecretRefId` 和 `EvidenceRefId`。它们沿用 UUID wire shape，并由 domain 的
ID registry round-trip guard 覆盖；ID 本身不是 grant、permit 或认证凭据。

`kiana-domain/src/security_contracts.rs` 提供：

- `SecuritySchemaRegistry`：从唯一 `SCHEMA_CONTRACTS` 生成排序后的 entry 快照，绑定
  `contracts_digest`、registry revision、authority/data epoch、source sequence 和 parent
  digest；重复 schema/ID、未知或漂移 contract、digest/epoch/sequence rollback、非
  canonical 顺序均 fail-closed。
- `SecurityObjectEnvelope`：固定 schema/version、transport event ID、object kind、epoch、
  sequence、父 digest、payload digest 和 envelope digest；payload 仅允许 bounded JSON object，
  递归拒绝 secret/token/password/bearer/private-key 等原值或字段。
- `from_json`/`to_json`/`parse_security_object` 与 `upcast_security_object`、
  `upcast_security_registry`：当前版本显式解析；未知 major 或未注册 migration 不猜测、不
  丢字段、不降级放行。

`kiana-domain/src/contracts.rs` 继续维护唯一 schema registry，并登记
`kiana.security-schema-registry.v1` 与 `kiana.security-object.v1`；`kiana-protocol` 只重导出
这些 domain DTO/常量，不直接执行或授予权限。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `security_registry_is_generated_sorted_and_round_trips` | genesis registry 可生成、canonical 排序、serde round-trip 和 digest 稳定 |
| `security_registry_rejects_unknown_major_duplicate_ids_and_rollback` | unknown major、重复 ID、authority/data/sequence/revision/digest 链回退全部拒绝 |
| `security_registry_rejects_secret_fields_and_unknown_upcasts` | strict serde 不接受 secret 字段，未注册 major upcast 不放行 |
| `security_object_is_canonical_chain_and_secret_safe` | object envelope 生成、父 digest successor、epoch fencing 和 secret sentinel 拒绝 |
| `security_object_rejects_unknown_major_and_implicit_migration` | unknown major 与隐式 v0 migration fail-closed |
| `security_contracts_stay_domain_only_and_do_not_create_an_execution_path` | source guard 固定 domain-only、唯一 registry 和无 Broker/Daemon/网络/进程依赖 |

测试只在 GitHub Actions 执行；不使用真实 secret、provider、外部连接器或现实 effect。

## 3. Proof ceiling and handoff

SC-02 的证据上限是 source：它证明了 DTO、ID、digest 链和拒绝边界的源码合同，不证明
跨进程持久 registry、authenticated principal、SecretStore 解析、策略 enforcement 或外部
系统安全。SC-03 消费这些稳定 schema/ID 并建立 reason code；SC-04 消费 security context
边界。后续 upcaster 必须显式登记 migration，不能把兼容读路径当作授权。
