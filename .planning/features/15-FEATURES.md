# Phase 15 特性账本 — Daily 自动化、回执与团队 Handoff

**Created:** 2026-07-26 | **父需求：** DAY-06, DAY-07, DAY-11, DAY-12 | **主旅程：** daily.automation-rpa, daily.project-templates, daily.verifier-receipts, daily.team-sharing
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。origin 或目标不匹配时停止；无法确认保持 result_unknown 且不自动重放。

### FEAT-15-01 — 可恢复浏览器/桌面自动化
- 父需求: DAY-06
- 领域旅程: daily.automation-rpa
- 描述: browser/desktop automation 完成受限导航、表单、下载/上传、clipboard 和 app control；每一步显示目标身份和可视状态，支持 timeout、cancel、checkpoint、compensation 与 receipt；目标不匹配即停。
- 验收:
  - [target_environment] 真实浏览器/桌面自动化链：每步目标校验、中断后从 checkpoint 恢复、补偿动作与回执记录；origin 不匹配 fixture 停止
- Verifier: RPA 恢复测试
- 当前基线: partial — chrome-mcp/computer-mcp/computer-input crate 基础存在；可恢复语义与目标校验缺失
- 设计引用: DESIGN-INDEX Phase 14-15 行；FEAT-11-02/03 workbench
- 依赖: FEAT-11-02, FEAT-11-03, FEAT-08-05
- 状态: pending

### FEAT-15-02 — 混合项目模板与 DAG 拆解
- 父需求: DAY-07
- 领域旅程: daily.project-templates
- 描述: 项目/流程模板把目标拆成包含 Coding、Research、Daily task 的 DAG、board、owner、deadline、approval、report 与 bounded multi-agent 工作流。
- 验收:
  - [local_behavior] 模板→DAG/board 生成；三 pack 混合 task 路由正确；multi-agent 走 WorkPacket 边界
- Verifier: 模板拆解测试
- 当前基线: partial — tasks/workflow DAG/board 已实现；模板与混合 pack 路由缺失
- 设计引用: project_os 04/31；FEAT-08-01 router
- 依赖: FEAT-08-01, FEAT-09-01
- 状态: pending

### FEAT-15-03 — Daily verifier 与外部回执核对
- 父需求: DAY-11
- 领域旅程: daily.verifier-receipts
- 描述: 核对目标应用最终状态、外部 ID、回执、附件 hash、参与者与审批；无法确认时保持 result_unknown 且不自动重放。
- 验收:
  - [target_environment] 真实外部写入（calendar/email）后 verifier 核对目标状态与回执；核对失败保持 result_unknown、重放被拒
- Verifier: 回执核对套件
- 当前基线: none — 五态 verifier（FEAT-08-04）与 result_unknown（FEAT-08-07）是底座
- 设计引用: DIF-03；AF-08
- 依赖: FEAT-08-07, FEAT-14-02, FEAT-14-03
- 状态: pending

### FEAT-15-04 — 团队共享与 handoff
- 父需求: DAY-12
- 领域旅程: daily.team-sharing
- 描述: 团队成员共享 project、task、artifact、comment、mention 与 handoff，检查 role visibility 和 audit；个人私有对象不因加入团队自动共享。
- 验收:
  - [local_behavior] 共享/评论/提及/handoff 旅程；role visibility 裁剪；私有对象默认不共享 fixture
- Verifier: 共享边界测试
- 当前基线: partial — 本地 team/task 状态合同已有；共享边界与审计缺失
- 设计引用: project_os 31/33
- 依赖: FEAT-14-01, FEAT-09-04
- 状态: pending
