# Phase 20 特性账本 — Official Cloud 身份、同步与租户数据基础

**Created:** 2026-07-26 | **父需求：** CLOUD-01, CLOUD-02, CLOUD-05, CLOUD-07 | **主旅程：** cloud.account-identity, cloud.encrypted-sync, cloud.tenant-isolation, cloud.data-lifecycle
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。未登录仍可完整使用 Local Personal（AF-05）。

### FEAT-20-01 — 可选账户与身份管理
- 父需求: CLOUD-01
- 领域旅程: cloud.account-identity
- 描述: 选择注册或 OAuth 登录；MFA/session-device 管理/recovery/注销/账号删除；未登录 Local Personal 完整可用。
- 验收:
  - [target_environment] 注册→MFA→device 管理→注销→账号删除全链；断网 Local Personal 不受影响
- Verifier: 账户旅程测试
- 当前基线: none — OAuth token 生命周期本地面可复用
- 设计引用: DESIGN-INDEX Phase 20-21 行（project_os 17；CLOUD 需求组）
- 依赖: FEAT-05-03
- 状态: pending

### FEAT-20-02 — 显式加密同步
- 父需求: CLOUD-02
- 领域旅程: cloud.encrypted-sync
- 描述: session/workflow/artifact/memory 按范围显式启用加密同步；同步方向/冲突/last sync/设备/恢复状态可见；暂停/导出/删除云副本。
- 验收:
  - [target_environment] 至少 session 类同步全链（启用→上传→冲突→暂停→删除）；EventLog 不作为同步内容（source of truth 保持本地）
- Verifier: 同步语义测试
- 当前基线: none — EventLog 权威与投影重建是同步语义基础
- 设计引用: 产品总纲 §7.1；FEAT-04-03
- 依赖: FEAT-20-01
- 状态: pending

### FEAT-20-03 — 租户隔离
- 父需求: CLOUD-05
- 领域旅程: cloud.tenant-isolation
- 描述: storage/cache/queue/logs/search index/worker/connector credentials/support tooling 全按 tenant 隔离；跨租户访问有自动化负面测试。
- 验收:
  - [target_environment] 跨租户负面测试（七类资源）全部 deny；隔离边界可审计
- Verifier: 跨租户负面套件
- 当前基线: none
- 设计引用: 产品总纲 §7.5
- 依赖: FEAT-20-01
- 状态: pending

### FEAT-20-04 — 云数据生命周期
- 父需求: CLOUD-07
- 领域旅程: cloud.data-lifecycle
- 描述: retention/residency 设置、portable export、workspace/account 删除与 deletion status；备份恢复/密钥轮换/同步冲突可验证。
- 验收:
  - [target_environment] export→删除→deletion status 全链；备份恢复演练
- Verifier: 数据生命周期测试
- 当前基线: none
- 设计引用: CLOUD-07
- 依赖: FEAT-20-02
- 状态: pending
