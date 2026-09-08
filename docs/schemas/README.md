# Kiana Schema 目录说明

> `docs/schemas/` 保存可机器校验的 wire/schema 合同。它不是当前能力清单，也不替代 `kiana-domain` 的业务不变量、`kiana-protocol` 的 Rust 类型或安全宪法。

## 1. Schema 分层

```text
Canonical domain schema
  organization / objective / project / work-packet / acceptance / outcome

Runtime event schema
  session / turn / run / invocation / approval / event / receipt

Platform schema
  memory / context-plan / capability / tool-snapshot / workflow / swarm

Application wire schema
  app-server commands / responses / events / settings / status
```

Canonical domain schema 定义业务对象的字段、版本和兼容边界；Runtime event schema 定义执行事实；Platform schema 定义平台能力交换；Application wire schema 只服务于入口和客户端协议。

## 2. 当前文件

当前目录包含 21 个 `kiana-app-server-*.v1.schema.json`。这些文件是 app-server envelope 或 endpoint 形状（见 §2.1 索引），必须结合源码和测试理解，不能仅凭 envelope 的 `schema` 字段推导完整业务能力。

### 2.1 最低限度元数据索引

21 个文件的 canonical owner 均为 `kiana-entrypoints`（direct-connect app contract；运行时数据投影来自 `kiana-daemon`），unknown-field policy 一律为宽松（`additionalProperties: true`，仅 `schema` 常量必填），当前证明等级一律为 `source`（envelope 壳，不是完整业务 schema）。依据见 §4 现状说明和 [`CURRENT_STATUS.md`](../../CURRENT_STATUS.md) 的「App-server schema 与 approval actor 修复证据（2026-08-29）」。

| schema 文件 | canonical owner | unknown-field policy | 当前证明等级 |
|---|---|---|---|
| `kiana-app-server-approval-decision.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-command-run.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-commands.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-config-resolved.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-contract.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-conversation-files.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-conversations.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-distribution-review.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-doctor.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-events.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-git-status.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-live-provider-smoke.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-model-current.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-permissions-status.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-plugins.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-prompt-history.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-sandbox.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-secrets.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-settings.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-team-status.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |
| `kiana-app-server-trust-status.v1.schema.json` | `kiana-entrypoints` | `additionalProperties: true`；仅 `schema` 必填 | `source`（壳） |

## 3. 目标目录

本目录规划为 schema registry 的唯一目标位置；[`company-os-domain-contracts.md`](../company-os-domain-contracts.md) §9 的 registry 引用本节（其早期 `docs/schemas/company-os/` 写法以本节为准）。

后续建议按以下结构扩展：

```text
schemas/
├── README.md
├── app-server/
│   └── kiana-app-server-*.v1.schema.json
├── domain/
│   ├── organization.v1.schema.json
│   ├── objective.v1.schema.json
│   ├── initiative.v1.schema.json
│   ├── project.v1.schema.json
│   ├── milestone.v1.schema.json
│   ├── work-packet.v1.schema.json
│   ├── acceptance.v1.schema.json
│   ├── delivery.v1.schema.json
│   ├── outcome.v1.schema.json
│   ├── risk.v1.schema.json
│   ├── change-request.v1.schema.json
│   └── incident.v1.schema.json
├── runtime/
│   ├── session.v1.schema.json
│   ├── turn.v1.schema.json
│   ├── run.v1.schema.json
│   ├── invocation.v1.schema.json
│   ├── approval.v1.schema.json
│   ├── runtime-event.v1.schema.json
│   └── receipt.v1.schema.json
└── platform/
    ├── memory-record.v1.schema.json
    ├── context-plan.v1.schema.json
    ├── capability-descriptor.v1.schema.json
    ├── mcp-server.v1.schema.json
    ├── workflow-definition.v1.schema.json
    ├── workflow-instance.v1.schema.json
    ├── swarm-plan.v1.schema.json
    ├── normalized-event.v1.schema.json
    ├── eval-case.v1.schema.json
    └── extension-manifest.v1.schema.json
```

## 4. 目标契约：每个 Schema 必须说明（当前 app-server 壳未满足）

**现状**：现有 21 个 `kiana-app-server-*.v1.schema.json` 只校验 envelope 的 `schema` 常量标签（`required: ["schema"]`、`additionalProperties: true`），不校验任何业务字段；下面 13 项是目标契约，不是当前完成度。标识一致性是健康的：每个文件的 `$id` 与文件名一一对应，并与 `kiana-entrypoints` 中 advertised endpoint 的 schema 字符串一致，由 `direct_connect_app_contract_schemas_match_packaged_schema_files` 守住。证明等级不高于 [`CURRENT_STATUS.md`](../../CURRENT_STATUS.md) 的 2026-08-29 证据块：该证据只到 schema parity 的 `local_behavior`，并明确 schema files 是 minimal envelope contracts，不是完整 endpoint payload schemas。

```text
schema id and version
canonical owner
runtime owner
required fields
unknown-field policy
identity and ownership
state representation
commands and events
security classification
redaction rules
idempotency key
compatibility and migration
test fixture
proof level
```

Schema 不是完整状态机。状态转移、权限守卫、事件顺序和副作用规则必须在对应规范和代码中定义，并在 contract/integration tests 中验证。

## 5. 版本规则

- 兼容性字段新增可以提升 minor；
- 删除、改类型、改变 required 语义或改变状态含义必须提升 major；
- 旧版本必须有 upcaster、迁移窗口或明确拒绝策略；
- 未知 major 必须 fail-closed；
- Event schema 的历史版本不能因为当前代码升级而被静默改写；
- schema hash、Policy epoch、Tool Catalog version 和 Prompt version 必须在相关 Receipt 中可追溯。

## 6. 校验边界

```text
Deserialize
  → schema validation
  → canonicalization
  → identity/ownership check
  → policy/gate/approval
  → state transition
  → event append
  → projection
```

“JSON 可以解析”不代表：

- actor 已认证；
- role 有效；
- scope 合法；
- payload 获得批准；
- 状态转移合法；
- 结果可以写入 Receipt。

## 7. 关联文档

- [`../company-os-spec-index.md`](../company-os-spec-index.md)：canonical owner 和规范索引；
- [`../company-os-domain-contracts.md`](../company-os-domain-contracts.md)：Company 业务对象和生命周期；
- [`../company-os-platform-architecture.md`](../company-os-platform-architecture.md)：Runtime、Memory、Capability、Workflow 和 Swarm；
- [`../company-os-operations-governance.md`](../company-os-operations-governance.md)：身份、Artifact、成本、恢复和数据治理；
- [`../company-os-quality-ecosystem.md`](../company-os-quality-ecosystem.md)：Eval、版本治理和扩展生态；
- [`../company-os-security-constitution.md`](../company-os-security-constitution.md)：安全宪法和负向验收；
- [`../../CURRENT_STATUS.md`](../../CURRENT_STATUS.md)：当前状态和证据上限。
