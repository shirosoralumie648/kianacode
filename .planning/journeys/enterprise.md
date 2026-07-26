# Enterprise Self-hosted 黄金旅程账本

**Created:** 2026-07-26 | **覆盖需求：** ENT-01..10 | **消费方：** features/MILESTONES/verifier。
**纪律：** central deny 不能被下级覆盖；许可证问题不破坏数据访问/export/recovery；worker 只获得短期最小凭据。

### enterprise.install-airgap — 部署与离线安装
- 覆盖需求: ENT-01
- 必备能力点: online/air-gapped bundle/preflight/容量检查/install/upgrade/rollback/uninstall/数据迁移；失败恢复到已验证版本
- Required proof: target_environment | 归属阶段: 22
- 当前 gap: enterprise offline manifest 合同与 bundle 校验已实现（本地面）；真实自托管安装未验证

### enterprise.sso-identity — 企业身份
- 覆盖需求: ENT-02
- 必备能力点: OIDC/SAML SSO/MFA policy/break-glass admin/用户停用/组织团队映射；认证故障安全降级
- Required proof: target_environment | 归属阶段: 22
- 当前 gap: 无实现

### enterprise.rbac-policy — 集中 RBAC 与策略
- 覆盖需求: ENT-03
- 必备能力点: user/workspace admin/security admin/auditor/operator 角色；provider/model/tool/connector/plugin/data/worker/高风险动作集中 policy；deny 优先
- Required proof: target_environment | 归属阶段: 22
- 当前 gap: managed policy 单机面已 fail-closed；多角色 RBAC 缺失

### enterprise.secrets-kms — 密钥与 vault
- 覆盖需求: ENT-04
- 必备能力点: vault/HSM/KMS 接入；secret scope/rotation/revocation/audit/zero-secret diagnostics；worker 短期最小凭据
- Required proof: target_environment | 归属阶段: 22
- 当前 gap: 本地 HMAC 信任根与脱敏诊断已有；外部 KMS 适配缺失

### enterprise.restricted-network — 受限网络
- 覆盖需求: ENT-06
- 必备能力点: outbound allowlist/proxy/custom CA/offline provider/private MCP-connectors/artifact quarantine/egress review；受限时 status 解释缺失能力
- Required proof: target_environment | 归属阶段: 22
- 当前 gap: 无实现；offline 模式与本地 provider 是基础

### enterprise.license-offline — 离线许可
- 覆盖需求: ENT-09
- 必备能力点: 离线签发/到期宽限/续期/席位容量核对/可审计状态；许可问题不破坏数据访问/export/recovery
- Required proof: target_environment | 归属阶段: 22
- 当前 gap: license status 本地合同已有；签发与核对后端缺失

### enterprise.audit — 追加式审计
- 覆盖需求: ENT-05
- 必备能力点: login/policy/approval/tool-connector/data access/admin/export/release 全覆盖 append-only audit；检索/导出/retention/legal hold/完整性验证
- Required proof: target_environment | 归属阶段: 23
- 当前 gap: EventLog HMAC 链是完整性底座；企业审计域缺失

### enterprise.data-dr — 数据生命周期与容灾
- 覆盖需求: ENT-07
- 必备能力点: retention/residency/workspace export-delete/backup-restore/DR/schema migration/integrity check 演练；恢复保持 event/evidence 因果序
- Required proof: target_environment（演练证据） | 归属阶段: 23
- 当前 gap: 无实现；投影重建语义是基础

### enterprise.observability-support — 运维观测与支持
- 覆盖需求: ENT-08
- 必备能力点: logs/metrics/traces/health-readiness/queue-worker-provider-connector dashboard/alerts/脱敏 support bundle；接常见 observability stack
- Required proof: target_environment | 归属阶段: 23
- 当前 gap: doctor 与脱敏诊断本地面已有；企业 observability 缺失

### enterprise.docs-acceptance — 企业文档与验收
- 覆盖需求: ENT-10
- 必备能力点: support matrix/security-privacy-licensing 文档/漏洞修复 SLA/管理员-用户-API 手册/迁移与破坏性变更策略；签字 evidence 验收
- Required proof: user_value（签字） | 归属阶段: 23
- 当前 gap: SECURITY/PRIVACY/TELEMETRY 基础文档已有；企业手册与 SLA 缺失
