# Official Cloud 黄金旅程账本

**Created:** 2026-07-26 | **覆盖需求：** CLOUD-01..08 | **消费方：** features/MILESTONES/verifier。
**纪律：** 云只增加同步、远程与团队规模；不得削弱 Local Personal（AF-05）；计费失败不删除本地数据或锁死导出。

### cloud.account-identity — 可选账户与身份
- 覆盖需求: CLOUD-01
- 必备能力点: 注册/OAuth 登录/MFA/session-device 管理/recovery/注销/账号删除；未登录 Local Personal 完整可用
- Required proof: target_environment | 归属阶段: 20
- 当前 gap: 无云端实现；OAuth token 生命周期本地面可复用

### cloud.encrypted-sync — 显式加密同步
- 覆盖需求: CLOUD-02
- 必备能力点: session/workflow/artifact/memory 按范围显式启用；方向/冲突/last sync/设备/恢复状态；暂停/export/删除云副本
- Required proof: target_environment | 归属阶段: 20
- 当前 gap: 无实现；EventLog 权威 + 投影重建是同步语义基础

### cloud.tenant-isolation — 租户隔离
- 覆盖需求: CLOUD-05
- 必备能力点: storage/cache/queue/logs/search index/worker/connector credentials/support tooling 全隔离；跨租户自动化负面测试
- Required proof: target_environment | 归属阶段: 20
- 当前 gap: 无实现

### cloud.data-lifecycle — 云数据生命周期
- 覆盖需求: CLOUD-07
- 必备能力点: retention/residency 设置；portable export；workspace/account 删除与 deletion status；备份恢复/tombstone/密钥轮换/同步冲突可验证
- Required proof: target_environment | 归属阶段: 20
- 当前 gap: 无实现

### cloud.remote-worker — 远程执行
- 覆盖需求: CLOUD-03
- 必备能力点: 签名限权 WorkPacket；隔离 workspace；stream typed events；reconnect/cancel/timeout/budget；verifier 回传；unknown-side-effect 安全
- Required proof: target_environment | 归属阶段: 21
- 当前 gap: 本地 WorkPacket/swarm/remote session 合同可复用；hosted worker 全部缺失

### cloud.team-workspace — 团队空间
- 覆盖需求: CLOUD-04
- 必备能力点: 成员/邀请；共享 project/workflow/artifact；comments/mentions/handoff/approval/activity；个人与团队数据边界
- Required proof: target_environment | 归属阶段: 21
- 当前 gap: 无实现；daily.team-sharing 的本地边界模型是前置

### cloud.billing-entitlement — 订阅与计费
- 覆盖需求: CLOUD-06
- 必备能力点: subscription/plan/quota/usage/invoice/payment failure/trial-cancel/entitlement ledger 可审计；计费失败不破坏本地数据与导出
- Required proof: target_environment | 归属阶段: 21
- 当前 gap: entitlement proof 合同与 license status 本地面已有；计费系统全部缺失

### cloud.operations-slo — 云运维与降级
- 覆盖需求: CLOUD-08
- 必备能力点: status/health/SLO/rate limit/queue visibility/incident-audit/backup restore/abuse control/脱敏 support diagnostics；云降级本地继续
- Required proof: target_environment | 归属阶段: 21
- 当前 gap: 无实现
