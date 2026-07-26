# Daily Work 黄金旅程账本

**Created:** 2026-07-26 | **覆盖需求：** DAY-01..12 | **消费方：** features/MILESTONES/verifier。
**纪律：** 一切外部写入有可识别目标、预览/策略判断、回执与审计；发送/发布/删除/付款/权限变更走硬审批（AF-08）；响应丢失进入 result_unknown，禁止盲目重放（DIF-03）。

### daily.personal-objects — 个人对象管理
- 覆盖需求: DAY-01
- 必备能力点: 文件/notes/todos/reminders/知识对象 CRUD/search/link/archive/export；来源/时间/状态/冲突处理
- Required proof: local_behavior | 归属阶段: 14
- 当前 gap: 无实现；memory JSONL 与 context artifacts 是存储底座候选

### daily.calendar — 日历连接器
- 覆盖需求: DAY-02
- 必备能力点: availability 查询/创建/改期/取消；时区/重复规则/参与者；写入前预览；服务端 event ID 记录
- Required proof: target_environment（真实日历服务） | 归属阶段: 14
- 当前 gap: 无实现

### daily.email-messaging — 邮件与消息
- 覆盖需求: DAY-03
- 必备能力点: 检索/thread/草稿/附件/收件人解析/发送预览/审批/回执/失败与未知处理；默认不自动发送
- Required proof: target_environment | 归属阶段: 14
- 当前 gap: 无实现

### daily.documents — 办公文档
- 覆盖需求: DAY-04
- 必备能力点: Word/Markdown/PDF/spreadsheet/presentation 读取/创建/编辑/批注/导出/diff；结构化 warning；不支持元素明确降级
- Required proof: local_behavior | 归属阶段: 14
- 当前 gap: 无实现

### daily.meetings — 会议工作流
- 覆盖需求: DAY-05
- 必备能力点: agenda/材料包/提醒/经授权录音转写/speaker-time provenance/notes/decision/action item/follow-up draft；隐私策略可见
- Required proof: target_environment | 归属阶段: 14
- 当前 gap: 无实现

### daily.workspace-search — 跨域搜索
- 覆盖需求: DAY-08
- 必备能力点: 本地文件/notes/mail/calendar/meeting/获批 connector 联合搜索；权限裁剪；来源与 freshness；断开后缓存按 policy 处理
- Required proof: local_behavior | 归属阶段: 14
- 当前 gap: context search 本地面已有；跨 connector 联合与权限裁剪缺失

### daily.connector-center — 连接器中心
- 覆盖需求: DAY-09
- 必备能力点: discover/OAuth scopes/health/last sync/data access；revoke/re-auth/least privilege/per-workspace enable/audit；凭据不进模型上下文
- Required proof: target_environment（真实 OAuth 流） | 归属阶段: 14
- 当前 gap: 无实现；OAuth token 文件管理与 secrets 脱敏合同可复用

### daily.approval-inbox — 统一审批收件箱
- 覆盖需求: DAY-10
- 必备能力点: 汇总待发送/发布/删除/付款/权限变更/result_unknown；inspect/edit/approve/deny/escalate；决定回写原 workflow
- Required proof: local_behavior | 归属阶段: 14
- 当前 gap: 无实现；TUI 审批面板与 workflow approval 事件是底座

### daily.automation-rpa — 浏览器与桌面自动化
- 覆盖需求: DAY-06
- 必备能力点: 受限导航/表单/上传下载/clipboard/app control；每步可视状态与目标校验；timeout/cancel/checkpoint/compensation/receipt；目标不匹配即停
- Required proof: target_environment | 归属阶段: 15
- 当前 gap: chrome-mcp/computer-mcp/computer-input crate 基础存在；可恢复 RPA 语义未实现

### daily.project-templates — 目标拆解与混合流程
- 覆盖需求: DAY-07
- 必备能力点: 模板拆解为 DAG/board/task/owner/deadline/approval/report；Coding/Research/Daily 混合 task；bounded multi-agent
- Required proof: local_behavior | 归属阶段: 15
- 当前 gap: tasks/workflow/board 基础已实现；模板与混合 pack 路由缺失

### daily.verifier-receipts — Daily verifier 与回执核对
- 覆盖需求: DAY-11
- 必备能力点: 核对目标应用最终状态/外部 ID/回执/附件 hash/参与者/审批；无法确认保持 result_unknown 且不自动重放
- Required proof: target_environment | 归属阶段: 15
- 当前 gap: 无实现；五态 verifier 底座已有

### daily.team-sharing — 团队共享与交接
- 覆盖需求: DAY-12
- 必备能力点: 共享 project/task/artifact/comment/mention/handoff；role visibility 与 audit；私有对象不自动共享
- Required proof: local_behavior | 归属阶段: 15
- 当前 gap: 本地 team/task 状态合同已有；共享边界与审计缺失
