# Phase 22 特性账本 — Enterprise 部署、身份与集中治理

**Created:** 2026-07-26 | **父需求：** ENT-01, ENT-02, ENT-03, ENT-04, ENT-06, ENT-09 | **主旅程：** enterprise.install-airgap, enterprise.sso-identity, enterprise.rbac-policy, enterprise.secrets-kms, enterprise.restricted-network, enterprise.license-offline
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。central deny 不可被下级覆盖；worker 只获短期最小凭据。

### FEAT-22-01 — 企业部署与 air-gapped 安装
- 父需求: ENT-01
- 领域旅程: enterprise.install-airgap
- 描述: online/air-gapped bundle、preflight、容量检查、install/upgrade/rollback/uninstall/数据迁移；失败可恢复到已验证版本。
- 验收:
  - [target_environment] air-gapped 环境完整安装/升级/回滚旅程；失败后恢复到已验证版本
- Verifier: air-gapped 安装测试
- 当前基线: partial — enterprise offline manifest 合同与 bundle 校验已实现；真实自托管安装未验证
- 设计引用: DESIGN-INDEX Phase 22-23 行（project_os 15/17/36；ENT 需求组）
- 依赖: FEAT-19-03
- 状态: pending

### FEAT-22-02 — 企业 SSO 与身份
- 父需求: ENT-02
- 领域旅程: enterprise.sso-identity
- 描述: OIDC/SAML SSO、MFA policy、break-glass admin、用户停用、组织/团队映射；认证故障安全降级。
- 验收:
  - [target_environment] OIDC/SAML 登录旅程；break-glass fixture；认证失败安全降级
- Verifier: SSO 旅程测试
- 当前基线: none
- 设计引用: ENT-02
- 依赖: FEAT-22-01
- 状态: pending

### FEAT-22-03 — 集中 RBAC 与策略
- 父需求: ENT-03
- 领域旅程: enterprise.rbac-policy
- 描述: 五角色 RBAC；provider/model/tool/connector/plugin/data/worker/高风险动作集中 policy；central deny 优先。
- 验收:
  - [target_environment] 五角色矩阵测试；central deny 优先于本地允许 fixture
- Verifier: RBAC 矩阵测试
- 当前基线: partial — managed policy 单机面已 fail-closed；多角色 RBAC 缺失
- 设计引用: ENT-03；AF-07
- 依赖: FEAT-22-02
- 状态: pending

### FEAT-22-04 — Vault/KMS 密钥管理
- 父需求: ENT-04
- 领域旅程: enterprise.secrets-kms
- 描述: vault/HSM/KMS 接入；secret scope/rotation/revocation/audit/zero-secret diagnostics；worker 短期最小凭据。
- 验收:
  - [target_environment] 至少一个 KMS 后端（HashiCorp Vault 或云 KMS）的 rotation/revocation 旅程；worker 短期 token fixture
- Verifier: KMS 集成测试
- 当前基线: partial — HMAC 信任根与脱敏诊断已有；外部 KMS 适配缺失
- 设计引用: ENT-04；product_os 08
- 依赖: FEAT-05-03
- 状态: pending

### FEAT-22-05 — 受限网络
- 父需求: ENT-06
- 领域旅程: enterprise.restricted-network
- 描述: outbound allowlist/proxy/custom CA/offline provider/private MCP-connectors/artifact quarantine/egress review；受限时 status 解释缺失能力。
- 验收:
  - [target_environment] 受限网络环境下完整功能旅程；egress 拒绝时能力 missing 显式提示
- Verifier: 受限网络测试
- 当前基线: none — offline 模式与本地 provider 是基础
- 设计引用: ENT-06
- 依赖: FEAT-19-03
- 状态: pending

### FEAT-22-06 — 离线 license 与席位管理
- 父需求: ENT-09
- 领域旅程: enterprise.license-offline
- 描述: 离线签发/宽限/续期/席位容量核对/可审计状态；许可证问题不破坏数据访问/export/recovery。
- 验收:
  - [target_environment] 离线 license 签发/过期/续期旅程；到期后数据访问/export 不受影响 fixture
- Verifier: 离线 license 测试
- 当前基线: partial — license status 本地合同已有；签发与后端缺失
- 设计引用: ENT-09
- 依赖: FEAT-22-01
- 状态: pending
