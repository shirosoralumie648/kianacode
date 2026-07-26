# Phase 04 特性账本 — 状态权威与投影恢复

**Created:** 2026-07-26 | **父需求：** CORE-02, CORE-08 | **主旅程：** core.session-lifecycle, core.memory-provenance
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。`[M0]` 标记 walking-skeleton 最薄子集。

### FEAT-04-01 — [M0] Session 持久化与重启一致性
- 父需求: CORE-02
- 领域旅程: core.session-lifecycle
- 描述: 一次请求的 session（父子 turn、stop reason、tool lifecycle）在进程重启后 resume 得到一致状态。
- 验收:
  - [local_behavior] create → 执行 → 重启 → resume：turn 树、stop reason、tool lifecycle 逐字段一致
- Verifier: 重启一致性测试
- 当前基线: partial — SDK JSONL session tree/resume/replay 已实现；一致性矩阵未验收
- 设计引用: DESIGN-INDEX Phase 4 行（workflow-runtime-design、project_os 02）
- 依赖: FEAT-03-01
- 状态: pending

### FEAT-04-02 — Session 全生命周期操作
- 父需求: CORE-02
- 领域旅程: core.session-lifecycle
- 描述: create/list/resume/fork/compact/import/export/delete 全部可用，附件与 pack 选择随 session 保持。
- 验收:
  - [local_behavior] 八操作全覆盖测试；compact 后语义恢复；附件/pack 选择重启保持
- Verifier: session 命令套件
- 当前基线: partial — create/list/resume/fork/compact/import/export 已实现；delete、附件与 pack 选择持久化缺失
- 设计引用: project_os 02
- 依赖: FEAT-04-01
- 状态: pending

### FEAT-04-03 — 投影损坏后从 EventLog 重建
- 父需求: CORE-02
- 领域旅程: core.session-lifecycle
- 描述: 删除或损坏 rebuildable projection 后，从 authoritative EventLog 重建相同状态；日志完整性无法验证时任务 blocked。
- 验收:
  - [local_behavior] 删除/篡改投影 fixture：重建结果与原状态等价；EventLog 损坏 → blocked 且诊断可操作
- Verifier: 投影重建测试
- 当前基线: partial — workflow crash-window 前缀修复与 HMAC 链已实现；session/memory 投影域的通用重建未实现
- 设计引用: workflow-runtime-design；project_os 02/23
- 依赖: FEAT-04-01
- 状态: pending

### FEAT-04-04 — 旧 session 迁移与 portable export
- 父需求: CORE-02
- 领域旅程: core.session-lifecycle
- 描述: 旧 session 有可验证的迁移、回滚与可移植导出路径，升级不静默丢弃。
- 验收:
  - [local_behavior] legacy JSON→JSONL 迁移可回滚；export 产物可在干净环境 import 复原
- Verifier: 迁移回滚测试
- 当前基线: partial — legacy 兼容读取与 import 已有；显式迁移/回滚合同缺失
- 设计引用: project_os 23
- 依赖: FEAT-04-02
- 状态: pending

### FEAT-04-05 — Memory 类型化与 provenance
- 父需求: CORE-08
- 领域旅程: core.memory-provenance
- 描述: memory 按 Observation/Reasoning/Decision/Git/Research evidence 类型持久化，带 source/confidence/scope/retention/stale。
- 验收:
  - [local_behavior] 五类型记录读写；字段全存；检索按类型/scope 过滤
- Verifier: memory 类型测试
- 当前基线: partial — `kiana.memory-record.v1` JSONL/status/search 与脱敏已实现；类型分级与 provenance 字段缺失
- 设计引用: project_os 28（记忆形成）/16
- 依赖: None
- 状态: pending

### FEAT-04-06 — Memory 用户操作与 live 优先
- 父需求: CORE-08
- 领域旅程: core.memory-provenance
- 描述: 用户可搜索、编辑、导出、删除 memory；live evidence 始终优先于 stale/inferred 内容（AF-11）。
- 验收:
  - [local_behavior] 编辑/导出/删除操作可用；stale 标记降权；live file/git/test 冲突时 memory 不覆盖事实
- Verifier: live-priority 冲突测试
- 当前基线: partial — search/status 已有；编辑/删除/stale 失效缺失
- 设计引用: project_os 28；AF-11
- 依赖: FEAT-04-05
- 状态: pending
