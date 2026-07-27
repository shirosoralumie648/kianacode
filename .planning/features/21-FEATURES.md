# Phase 21 特性账本 — Official Cloud Worker、团队、计费与运维

**Created:** 2026-07-26 | **父需求：** CLOUD-03, CLOUD-04, CLOUD-06, CLOUD-08 | **主旅程：** cloud.remote-worker, cloud.team-workspace, cloud.billing-entitlement, cloud.operations-slo
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。

### FEAT-21-01 — 云端 remote worker
- 父需求: CLOUD-03
- 领域旅程: cloud.remote-worker
- 描述: remote worker 接收签名限权 WorkPacket，在隔离 workspace 执行，stream typed events，支持 reconnect/cancel/timeout/budget；结果经 verifier 回传；未知副作用不重放。
- 验收:
  - [target_environment] 真实 hosted worker 全链：签名 packet→执行→stream→verifier 回传→result_unknown 安全
- Verifier: remote worker 端到端测试
- 当前基线: none — 本地 WorkPacket/swarm/remote session 合同可复用
- 设计引用: DESIGN-INDEX Phase 20-21 行；FEAT-09-01 packet 合同
- 依赖: FEAT-09-01, FEAT-20-01
- 状态: pending

### FEAT-21-02 — 云端团队空间
- 父需求: CLOUD-04
- 领域旅程: cloud.team-workspace
- 描述: 成员/邀请/共享 project/workflow/artifact/comments/mentions/handoff/approval/activity；个人与团队数据边界显式。
- 验收:
  - [target_environment] 多用户团队旅程（邀请→共享→handoff→审批）；跨用户边界负面测试
- Verifier: 团队旅程测试
- 当前基线: none — daily.team-sharing 本地边界模型（FEAT-15-04）是前置
- 设计引用: CLOUD-04；project_os 17
- 依赖: FEAT-15-04, FEAT-20-01
- 状态: pending

### FEAT-21-03 — 订阅与计费
- 父需求: CLOUD-06
- 领域旅程: cloud.billing-entitlement
- 描述: subscription/plan/quota/usage/invoice/payment failure/trial-cancel/entitlement 有可审计 ledger；计费失败不删除本地数据或锁死导出。
- 验收:
  - [target_environment] 计费 ledger 可审计；payment failure fixture 不影响本地数据与导出
- Verifier: 计费隔离测试
- 当前基线: partial — entitlement proof 合同与 license status 本地面已有；计费系统全部缺失
- 设计引用: CLOUD-06
- 依赖: FEAT-20-01
- 状态: pending

### FEAT-21-04 — 云运维与降级
- 父需求: CLOUD-08
- 领域旅程: cloud.operations-slo
- 描述: status/health/SLO/rate limit/queue visibility/incident-audit/backup restore/abuse control/脱敏 support diagnostics；云降级时本地工作继续。
- 验收:
  - [target_environment] 降级 fixture：云不可用时本地旅程不受影响；backup restore 演练
- Verifier: 降级隔离测试
- 当前基线: none
- 设计引用: CLOUD-08；project_os 15
- 依赖: FEAT-20-03
- 状态: pending
