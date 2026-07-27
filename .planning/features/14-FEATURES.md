# Phase 14 特性账本 — Daily 对象、Connector 与审批控制

**Created:** 2026-07-26 | **父需求：** DAY-01..DAY-05, DAY-08, DAY-09, DAY-10 | **主旅程：** daily.personal-objects 等（Phase 14 域）
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。默认不自动发送；凭据不进模型上下文（AF-06/08）。

### FEAT-14-01 — 个人对象 CRUD 与冲突处理
- 父需求: DAY-01
- 领域旅程: daily.personal-objects
- 描述: 本地文件、notes、todos、reminders 与知识对象可 create/read/update/search/link/archive/export；每项有来源、时间、状态和冲突处理。
- 验收:
  - [local_behavior] 五类对象八操作；冲突（并发修改）fixture 显式解决
- Verifier: 对象生命周期测试
- 当前基线: none — memory JSONL/context artifacts 是存储底座候选
- 设计引用: DESIGN-INDEX Phase 14-15 行（project_os 33/34；DAY 需求组）
- 依赖: FEAT-04-05
- 状态: pending

### FEAT-14-02 — Calendar connector
- 父需求: DAY-02
- 领域旅程: daily.calendar
- 描述: 查询 availability、创建/改期/取消事件、处理时区/重复规则/参与者；写入前展示变化并记录服务端 event ID。
- 验收:
  - [target_environment] 真实日历服务（CalDAV 或主流 API）全操作带预览与服务端 ID 回执
- Verifier: calendar 回执测试
- 当前基线: none
- 设计引用: DAY 需求组
- 依赖: FEAT-14-06
- 状态: pending

### FEAT-14-03 — Email/消息 connector
- 父需求: DAY-03
- 领域旅程: daily.email-messaging
- 描述: 检索、thread/context、草稿、附件、收件人解析、发送预览、审批、发送回执和失败/未知处理；默认不自动发送。
- 验收:
  - [target_environment] 真实邮件服务：草稿→预览→审批→发送→回执全链；未审批不发送；丢失回执进 result_unknown
- Verifier: 发送审批链测试
- 当前基线: none
- 设计引用: AF-08；DIF-03
- 依赖: FEAT-14-06, FEAT-14-08, FEAT-08-07
- 状态: pending

### FEAT-14-04 — 办公文档读写与降级
- 父需求: DAY-04
- 领域旅程: daily.documents
- 描述: Word/Markdown/PDF 文档、spreadsheet 和 presentation 读取、创建、编辑、批注、导出和 diff；公式、图表、引用和版式变化有结构化 warning；不支持元素明确降级。
- 验收:
  - [local_behavior] 五格式读写往返；warning 结构化；不支持元素降级显式
- Verifier: 文档往返测试
- 当前基线: none
- 设计引用: DAY 需求组
- 依赖: None
- 状态: pending

### FEAT-14-05 — 会议工作流
- 父需求: DAY-05
- 领域旅程: daily.meetings
- 描述: agenda、材料包、时间提醒、经授权的录音/转写、speaker/time provenance、notes、decision、action item 和 follow-up draft；参会者隐私策略可见。
- 验收:
  - [target_environment] 授权录音→转写（带 speaker/time provenance）→纪要→action item 全链；隐私策略展示
- Verifier: 会议链测试
- 当前基线: none
- 设计引用: DAY 需求组
- 依赖: FEAT-14-01
- 状态: pending

### FEAT-14-06 — Connector center
- 父需求: DAY-09
- 领域旅程: daily.connector-center
- 描述: discover、OAuth scopes、health、last sync、data access 展示；revoke、re-auth、least privilege、per-workspace enable 和 audit；凭据不进入模型上下文。
- 验收:
  - [target_environment] 真实 OAuth 流（授权→scope 展示→revoke→re-auth）；凭据隔离验证
- Verifier: connector 生命周期测试
- 当前基线: none — OAuth token 管理/secrets 脱敏合同/扩展 receipt 模式可复用
- 设计引用: 产品总纲 §5.3；FEAT-09-05 合同
- 依赖: FEAT-09-05, FEAT-05-03
- 状态: pending

### FEAT-14-07 — Workspace 联合搜索
- 父需求: DAY-08
- 领域旅程: daily.workspace-search
- 描述: 跨本地文件、notes、mail、calendar、meeting 与获批 connectors 搜索；结果按权限裁剪并显示来源/新鲜度；断开 connector 后缓存按 policy 删除或降级。
- 验收:
  - [local_behavior] 联合搜索带权限裁剪与来源/freshness；断开后缓存策略 fixture
- Verifier: 联合搜索测试
- 当前基线: partial — context search 本地面已有；跨域联合与裁剪缺失
- 设计引用: project_os 06
- 依赖: FEAT-14-06, FEAT-14-01
- 状态: pending

### FEAT-14-08 — Approval inbox
- 父需求: DAY-10
- 领域旅程: daily.approval-inbox
- 描述: 汇总待发送、发布、删除、付款、权限变更与 result_unknown 动作；inspect/edit/approve/deny/escalate；决定回写原 workflow。
- 验收:
  - [local_behavior] 六类动作入箱；五种处置回写 workflow 并事件化；result_unknown 项要求核对路径
- Verifier: inbox 回写测试
- 当前基线: none — TUI 审批面板与 workflow approval 事件是底座
- 设计引用: 产品总纲 §5.3；DIF-03
- 依赖: FEAT-08-04, FEAT-08-07
- 状态: pending
