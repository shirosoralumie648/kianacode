# Phase 19 特性账本 — 跨入口本地连续性与平台交付

**Created:** 2026-07-26 | **父需求：** CORE-15, SURF-07, SURF-09, DIF-05, DIF-06, NFR-06 | **主旅程：** surfaces.cross-surface-continuity, surfaces.platform-matrix, core.local-first-data
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。parity: cc.install.*×2, cc.surfaces.shared-session-state, cc.voice.*×3（user_value）。

### FEAT-19-01 — 跨入口同一 workflow 连续性（DIF-06）
- 父需求: SURF-07
- 领域旅程: surfaces.cross-surface-continuity（parity: cc.surfaces.shared-session-state）
- 描述: CLI 创建 workflow→IDE 查看 diff→Desktop 审批→Web 观察 remote worker；所有端看到相同 IDs/events/state/evidence；离线冲突有明确解决。
- 验收:
  - [target_environment] 四端旅程端到端；离线冲突 fixture；parity shared-session-state 取证
- Verifier: 跨入口旅程测试
- 当前基线: partial — 事件与 session 合同已统一；四端旅程缺失
- 设计引用: DESIGN-INDEX Phase 19 行（project_os 17；governance parity cc.surfaces 域）
- 依赖: FEAT-17-02, FEAT-18-02
- 状态: pending

### FEAT-19-02 — 无账户 Local Personal 完整运行（DIF-05）
- 父需求: CORE-15
- 领域旅程: core.local-first-data
- 描述: Local Personal 无需账户即可运行三个 pack；数据默认留在设备；备份/迁移/导出/彻底删除；sync/telemetry 明示 opt-in。
- 验收:
  - [target_environment] 断网环境完整三 pack 旅程；备份→迁移→删除旅程；opt-in 以外无上传
- Verifier: 离线旅程测试
- 当前基线: partial — 本地运行与 TELEMETRY 承诺已有；备份/迁移/删除未实现
- 设计引用: 产品总纲 §7.1
- 依赖: FEAT-18-01
- 状态: pending

### FEAT-19-03 — 平台交付矩阵（Linux/macOS/Windows-WSL）
- 父需求: SURF-09
- 领域旅程: surfaces.platform-matrix（parity: cc.install.native-platforms / update-doctor）
- 描述: Linux/macOS 原生与 Windows/WSL 的 path/terminal/sandbox/Keychain/browser-native-host 差异经过端到端验证；不支持能力明示；安装/升级/回滚生命周期验证。
- 验收:
  - [target_environment] 三平台安装/升级/回滚/卸载旅程；两条 parity install 能力取证
- Verifier: 平台矩阵测试
- 当前基线: partial — 平台边界诊断与 Windows/macOS doctor 姿态已有；目标环境矩阵证据缺失
- 设计引用: governance parity cc.install 域；CORE-16 前置
- 依赖: FEAT-02-01
- 状态: pending

### FEAT-19-04 — Voice 跨入口完整性（parity voice 域）
- 父需求: SURF-09
- 领域旅程: surfaces.platform-matrix（parity: cc.voice.dictation-lifecycle / permission-retention / text-fallback-approval）
- 描述: voice 在多平台/多入口的权限/转写/保留策略/文本回退可见且一致（本地行为面在 FEAT-11-08，此处覆盖跨入口 user_value）。
- 验收:
  - [user_value] 至少两个入口（CLI+Desktop）的 voice 旅程；三条 parity voice 能力取证
- Verifier: voice 跨入口测试
- 当前基线: none — parity voice 域全部 missing
- 设计引用: governance parity cc.voice 域（target_phase 19）
- 依赖: FEAT-11-08, FEAT-18-01
- 状态: pending

### FEAT-19-05 — 同一 workflow 跨 pack 组合恢复
- 父需求: DIF-06
- 领域旅程: surfaces.cross-surface-continuity
- 描述: crash/daemon restart/disconnect 或入口切换后，跨 pack workflow 从最后可信节点恢复，不需要导入另一套状态。
- 验收:
  - [target_environment] 跨 pack DAG 故障注入与恢复旅程（结合 core.recovery）
- Verifier: 跨 pack 恢复测试
- 当前基线: partial — WorkflowRun 恢复底座已有；跨 pack 旅程缺失
- 设计引用: 产品总纲 §8；FEAT-08-05
- 依赖: FEAT-08-05, FEAT-19-01
- 状态: pending

### FEAT-19-06 — 打磨质量门收尾（NFR-06）
- 父需求: NFR-06
- 领域旅程: surfaces.state-consistency
- 描述: 全部产品入口对 loading/empty/error/offline/degraded/permission-denied/result-unknown 的一致性矩阵最终验收；各 pack 收尾验收项。
- 验收:
  - [user_value] 七态 × 七入口矩阵全绿；错误信息可操作性用户评审通过
- Verifier: 质量门矩阵
- 当前基线: partial — FEAT-18-03 建立矩阵；此处收尾为 user_value 级验收
- 设计引用: NFR-06
- 依赖: FEAT-18-03, FEAT-19-01
- 状态: pending
