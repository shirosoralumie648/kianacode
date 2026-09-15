# CAP-01 capability descriptor and handler binding baseline

> 快照日期：2026-09-16。本文记录 CAP-01 的单一 capability authority source、model-visible tool
> mapping、descriptor/schema/policy metadata 和 Broker binding seal；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CAP-01`](capability.md#step-cap-01) |
| source snapshot | `f41c636`（ER-04 CommandReceipt 后的干净基线） |
| feature_status | `implemented`（catalog/binding source and composition-root seal） |
| proof_level | `source`；静态编译不提升为 local_behavior/durable/live/physical |
| canonical path | Runner model tool schema/mapping → domain action catalog → DaemonHost registrations → Broker exact binding/version → ControlPlane permit/policy |
| this step does | 为每个 registered operation 校验 descriptor/schema/binding，拒绝 duplicate alias/operation、kind mismatch、binding version drift 和 unregistered fallback；区分五个 model-visible tools 与 operator-only operations |
| this step does not | 不把 descriptor/handler 注册当授权，不把 model schema 当 Grant，不扩展五工具，不让 Broker fallback shell，不声明外部 adapter/live effect |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain catalog/action/capability | `kiana-domain/src/actions.rs`, `kiana-domain/src/tool_catalog.rs`, `kiana-domain/src/capabilities.rs`, `kiana-domain/src/contracts.rs` | `cd170e01e581015c917fdc0ac2c419149349a5de88164c9f7369c07eecd0a020`, `9c46d8713d6ce55a8eca707179e566036111b2da285539693aac11426dd480ac`, `1e8c6cb45c2875fcc35a9c876cc780bc2011913d15db7d4376a8ced1c117b522`, `dbbda14e5df8db53d5abbd095aa651143e57d3937164ef8639f8c8602f963cba` |
| Broker/daemon bindings | `kiana-capability-broker/src/lib.rs`, `kiana-daemon/src/lib.rs`, `kiana-daemon/src/harness_capabilities.rs`, `kiana-daemon/src/harness_memory.rs`, `kiana-daemon/src/harness_mcp.rs` | `afd4556a453bc6462fb6a66f59f981b67db2ed3041d74869a0f90d145d7d6b28`, `fabcc0eefb41852dc7dfd4ad7cf511bcfae02c13f2ec92ceed8f95fdfae04839`, `342dcbca27709f41821872c60ab199595f51ebc6fa9dccabbaf0b040ae5c46ce`, `6379bfb054a745f07f9977cf63122107f97e3753b5e15bcfc90217d8518b953c`, `9e7a27090750a3524af01f2e036fe3ed3d30e58aedb706a7d5837133957fa5d4` |
| Runner/fixtures/guard/workflow | `kiana-runner/src/tools.rs`, `kiana-domain/tests/cap01_registry.rs`, `kiana-capability-broker/tests/cap01_registry.rs`, `kiana-core/tests/cap01_authority_guard.rs`, `.github/workflows/cap01-authority.yml` | `2a757b5ba06622622d949cccacbf50f4caf6d38b04c8806611853e6f4498d213`, `443798e08f8039925c632d7ab2fe2697863728d5f7304a39dd70cd99010136f7`, `b5b4e9e77c6364e39bba4e08292f0c031c89231096b77c29f595a7215c3eea56`, `32d762ae086f04b51a792dce6808b85e1dba002acf3d80910ed4d9479008b2da`, `19aef036dc44f9143a340342f4047e998b23d65c195156450ee586f68f24ea6c` |

hash 仅用于 CAP-01 源码漂移复核；静态 catalog seal 不等于授权、permit、durability 或外部效果。

## 2. 单一权威矩阵

| 层 | canonical owner | 规则 |
|---|---|---|
| Model schema | `kiana-domain::tool_schemas` consumed by `kiana-runner::capability_for_tool` | 只有 `shell`、`apply_patch`、`mcp`、`memory.search`、`memory.write`；别名先规范化，未知不回退 |
| Action descriptor | `ACTION_OPERATIONS` + `capability_action_descriptor` | operation、CapabilityKind、minimum risk、argument/result schema、resource/effect/cancel/reconcile/idempotency/binding version 同源 |
| Policy metadata | `capability_action_contract` + `kiana-policy::hard_policy_denial` | kind/risk/role/path/scope 必须 server-owned；metadata 不能 mint authorization |
| Handler binding | `CapabilityBroker::register_static/register` + DaemonHost composition root | exact `(CapabilityKind, operation)` key；duplicate、unknown operation、kind mismatch、binding version mismatch fail-closed |
| Catalog seal | `validate_catalog_bindings` once after all daemon registrations | every `ACTION_OPERATIONS` has exactly one descriptor/binding，handler count 与 catalog 对齐；seal 后不可注册 |
| Execution | Broker `validate_action` → permit verifier → handler | normalized PreparedAction/action digest 与 permit 一致；未注册 operation 不尝试相近名称或 shell |

`validate_action_catalog()` 在 catalog seal 和 PreparedAction preparation 前检查 descriptor 完整性、schema 可解析性、resource 字段唯一/有界和 `additionalProperties` 显式值。Catalog 是行为合同，不是授权事实；Grant/Approval/Authority/Permit 仍由 ControlPlane/EventLog 生成和验证。

## 3. Model-visible 与 operator-only

| Model-visible | Operator-only（不会进入模型 schema） |
|---|---|
| `shell` → `shell.exec` | `mcp.discover`、`workspace.transaction`、`local.package` |
| `apply_patch` → `apply_patch` | `execution.output.read`、`process.start/poll/stdin/resize/stop` |
| `mcp` → `mcp.call` | `environment.inspect`、`tool.search`、management/context direct commands |
| `memory.search` → `memory.search` | Company/Memory review/Data governance/Extension/Connector management commands |
| `memory.write` → `memory.write` | 只读或治理命令可有 catalog descriptor，但由 explicit Command/role/approval scope 触发 |

Operator-only 不等于无需授权；它只表示模型不能直接提出该 operation。Direct Company/context actions 仍经过 ControlPlane action/policy/approval/permit 路径，且不虚构 Harness Run。

## 4. Binding and failure matrix

| 输入/状态 | 处理 |
|---|---|
| duplicate alias or exact operation | registry 不覆盖旧 handler，返回 `capability_handler_already_registered` |
| unknown operation or alias not canonical | `capability_operation_unknown`/`action_operation_unknown`，不 fallback shell |
| descriptor capability differs from handler key | `capability_binding_catalog_mismatch` |
| handler `binding_version` differs | `capability_binding_version_mismatch`，装配失败，不 dispatch |
| catalog incomplete/schema invalid | `validate_action_catalog`/`validate_catalog_bindings` fail before model request |
| catalog sealed then register | `capability_catalog_sealed` |
| Broker request not normalized | `capability_action_not_prepared` |
| permit missing/invalid | `execution_permit_verifier_required`/structured unavailable，handler 不调用 |
| model tool unknown or malformed | Runner schema/mapping returns `tool_unsupported`/`invalid_arguments`，zero Broker effect |

## 5. CI-only fixture catalog

| Fixture | Purpose |
|---|---|
| `tool_authority_covers_every_model_visible_tool` | 五工具 schema→mapping→descriptor 一致 |
| `operator_only_capabilities_never_enter_model_schema` | operator-only 不泄漏给模型 |
| `duplicate_alias_or_operation_is_rejected` | duplicate exact key/非 canonical alias fail-closed |
| `descriptor_binding_version_mismatch_never_dispatches` | handler binding version drift 在装配期拒绝 |
| `unregistered_operation_does_not_fallback_to_shell` | Broker 未注册 operation 不 fallback |
| `capability_catalog_binding_is_single_source_and_sealed_at_composition_root` | Broker/Daemon/Runner/handler source guard |

`.github/workflows/cap01-authority.yml` 在 GitHub runner 串行执行 domain/Broker/core fixtures、fmt/fetch；本地不运行测试，不连接 provider/connector。

## 6. 限制与交接

- Catalog/descriptor/binding 仍是进程内静态 source；未实现持久 ToolSnapshot、签名 manifest、动态扩展 revoke 或跨进程 binding provenance。
- `additionalProperties` 兼容字段和 bounded JSON Schema 仍允许后续收紧；收紧必须升级 catalog/binding 版本并重新审批，不能静默改变旧 action digest。
- Broker exact binding/version 不等于 policy Grant/Approval/authority epoch/permit 已被提交，也不等于 handler effect、TOCTOU、网络或外部业务结果安全。
- CAP-02/03/04/05、CP-06+、ER/PD/SC 继续负责参数 typed input、ExecutionScope、state/outcome、permit/CAS、recovery/reconcile 和 durable/live/physical 证据。
- 本地只做格式、workspace test-target 静态编译和 diff 检查；GitHub CI 结果不等待，不提升 local_behavior/durable/live/physical。
