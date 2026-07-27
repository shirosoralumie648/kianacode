# Phase 18 特性账本 — Desktop 与 Web/App Server

**Created:** 2026-07-26 | **父需求：** SURF-05, SURF-06, SURF-08, NFR-03, NFR-05 | **主旅程：** surfaces.desktop, surfaces.web-appserver, surfaces.state-consistency, surfaces.i18n-a11y
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。Desktop 和 Web 只提交 typed commands，消费 snapshots/events，不保存第二套任务事实（AF-04）。

### FEAT-18-01 — Desktop 无账户本地工作区
- 父需求: SURF-05
- 领域旅程: surfaces.desktop（parity: cc.desktop.workspace-conversations）
- 描述: Desktop 提供无账户 local mode 的 conversation/workspace、files/sources、artifacts、connector center、models/settings、computer-use、approval inbox、history/search 与三 pack views。
- 验收:
  - [user_value] 无账户完整个人旅程；parity cc.desktop.workspace-conversations 取证
- Verifier: Desktop 用户旅程
- 当前基线: none — `/app` 合同广覆盖是底座
- 设计引用: DESIGN-INDEX Phase 18 行（project_os 35；governance parity cc.desktop 域）
- 依赖: FEAT-16-06, FEAT-14-08
- 状态: pending

### FEAT-18-02 — Web/App Server 实时操作面
- 父需求: SURF-06
- 领域旅程: surfaces.web-appserver（parity: cc.web.remote-sessions）
- 描述: Web/App Server 提供 conversations/workflows/events/files/artifacts/team/admin/release operations，支持实时 reconnect、RBAC、pagination 和 bounded history；local 与 hosted 同一 contract。
- 验收:
  - [local_behavior] workflows/team/admin 操作面实现；local/hosted 合同等价测试；parity cc.web.remote-sessions 取证
- Verifier: Web 操作面测试
- 当前基线: partial — conversations/events/release/context 族广泛实现；workflows/team/admin 面缺失
- 设计引用: governance parity cc.web 域
- 依赖: FEAT-16-06
- 状态: pending

### FEAT-18-03 — 全入口一致状态与打磨质量门（NFR-06）
- 父需求: SURF-08
- 领域旅程: surfaces.state-consistency
- 描述: loading/empty/error/offline/degraded/permission-denied/result-unknown 七态在所有入口一致；错误信息含原因/影响/下一步操作；帮助从错误现场可达。
- 验收:
  - [local_behavior] 七态一致性矩阵测试（五入口 × 七态）；错误信息 schema 检查
- Verifier: 状态矩阵测试
- 当前基线: partial — 状态语义分散；一致性矩阵与错误信息标准未定义
- 设计引用: project_os 22
- 依赖: FEAT-08-04
- 状态: pending

### FEAT-18-04 — 国际化（NFR-03）
- 父需求: NFR-03
- 领域旅程: surfaces.i18n-a11y
- 描述: 全部用户可见字符串外化，至少中英双语；CJK/长文本/窄终端布局验证；语言按用户配置切换。
- 验收:
  - [local_behavior] 字符串外化率 100%；中英切换旅程；CJK 终端宽度测试
- Verifier: i18n 覆盖率测试
- 当前基线: none — 当前混合中英硬编码
- 设计引用: NFR-03
- 依赖: None
- 状态: pending

### FEAT-18-05 — 可访问性门禁（NFR-05）
- 父需求: NFR-05
- 领域旅程: surfaces.i18n-a11y
- 描述: CLI/TUI 与图形入口满足键盘完整导航、screen reader 兼容、reduced motion 与对比度要求，有可重复检查清单。
- 验收:
  - [local_behavior] CLI/TUI 键盘导航测试；图形入口 screen reader 冒烟；检查清单入 CI
- Verifier: 可访问性检查清单
- 当前基线: none
- 设计引用: NFR-05
- 依赖: FEAT-18-01
- 状态: pending

### FEAT-18-06 — 全入口键盘/屏幕阅读器/本地化
- 父需求: SURF-08
- 领域旅程: surfaces.i18n-a11y
- 描述: 长内容、键盘导航、screen reader、reduced motion 与本地化在 Desktop/Web/IDE 一致可用。
- 验收:
  - [user_value] 三入口可访问性矩阵通过；reduced motion 尊重系统设置
- Verifier: 可访问性矩阵测试
- 当前基线: none
- 设计引用: SURF-08；NFR-05
- 依赖: FEAT-18-05
- 状态: pending
