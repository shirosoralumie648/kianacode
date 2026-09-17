# SC-03 稳定安全 reason code 基线

> 快照日期：2026-09-17。本页是 domain/protocol source contract 与 CI-only fixture 记录，不是
> 安全认证，也不提升现有运行时安全能力的 proof level。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-03`](security-compliance.md#step-sc-03) |
| feature_status | `implemented`（stable code、policy、safe reason DTO 和 protocol export） |
| proof_level | `source`；本地不运行测试，测试由 GitHub Actions 执行 |
| authority | `SecurityReasonCode`/`SecurityReason` 是错误解释合同；ControlPlane 仍是授权事实源 |
| this step does | AUTH/POLICY/DATA/SECRET/EXT/FS/NET/RESOURCE/FACT/UNKNOWN code families，retryability/remediation policy，legacy reason classification，digest-only evidence refs |
| this step does not | 不改变旧 `CapabilityErrorCode` 兼容映射，不实现 SecurityContext、policy evaluator、SecretStore、redaction pipeline 或 effect/retry |

## 1. Contract

`kiana-domain/src/security_reasons.rs` 固定不随 provider/入口文字变化的
`SecurityReasonCode`。代码覆盖 `AUTH_*`、`POLICY_*`、`DATA_*`、`SECRET_*`、`EXT_*`、
`FS_*`、`NET_*`、`RESOURCE_*`、`FACT_*` 和 `UNKNOWN_*`，每个代码绑定
`SecurityReasonClass`、`SecurityRetryability` 和 `SecurityRemediation`。未知输入只进入
`UNKNOWN_UNCLASSIFIED`，未知结果永远进入 reconciliation/quarantine 语义，绝不转成 allow 或
自动重试。

`classify_security_reason` 只匹配有限 legacy reason 名称和已知安全代码，不保留原错误、provider
响应、命令行、prompt 或 secret。`SecurityReason` 采用 `kiana.security-reason.v1`、strict
serde、typed `OperationId`/`EvidenceRefId` 关联和可选 `sha256:` detail digest；它没有 raw
message/error 字段，unknown field、nil/duplicate evidence、错误 digest 或版本均拒绝。

`kiana-domain/src/contracts.rs` 登记该 schema；`kiana-protocol` 只重导出 code/policy/DTO 与
schema 常量，不执行 retry、审批或 capability。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `security_reason_codes_have_stable_families_and_policies` | code 字符串、family、retryability/remediation 及 ALL round-trip 固定 |
| `legacy_reason_mapping_is_stable_and_does_not_echo_text` | legacy reason/unknown 只映射到稳定 code，不回显原文 |
| `security_reason_round_trip_is_strict_and_digest_bound` | strict serde、typed refs、detail digest 和 reason digest 校验 |
| `unknown_reason_is_conservative_and_has_no_raw_error_field` | UNKNOWN 具备 reconciliation/quarantine 语义且 JSON 没有 raw error/message |
| `security_reason_contract_is_stable_and_does_not_authorize_or_echo_errors` | source guard 固定 domain-only、协议导出和无执行/原文依赖 |

## 3. Proof ceiling and handoff

SC-03 只证明分类合同和安全错误边界，proof ceiling 为 `source`。错误代码不等于拒绝已经在
每个入口强制；后续 SC-04/05 负责把 server-owned context 和 policy decision 绑定到 reason，
SC-18/20 负责 SecretRef/redaction，SC-31 负责 AuditRecord。旧 capability 适配仍可保留详细
诊断在受控内部路径使用，但对外安全 DTO 只能传 code、policy、digest 和 opaque refs。
