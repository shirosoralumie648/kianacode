# Phase 13 特性账本 — Research 实验、论文、复现与领域 Pack

**Created:** 2026-07-26 | **父需求：** RES-07..RES-14, DIF-10 | **主旅程：** research.dataset-code-intake 等（Phase 13 域）
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。工程签字与下单保持人工审批（AF-17）。

### FEAT-13-01 — 数据集与代码 intake
- 父需求: RES-07
- 领域旅程: research.dataset-code-intake
- 描述: intake 记录 version、license、checksum、schema、split、预处理、环境和访问限制；敏感数据有本地/组织 policy。
- 验收:
  - [local_behavior] intake manifest 全字段；敏感数据 policy 拦截 fixture
- Verifier: intake 合同测试
- 当前基线: none — plugin receipt/context ingest 完整性模式可复用
- 设计引用: DESIGN-INDEX Phase 12-13 行；project_os 23
- 依赖: FEAT-12-02
- 状态: pending

### FEAT-13-02 — 实验定义与可复现执行
- 父需求: RES-08
- 领域旅程: research.experiment-run
- 描述: 实验固定参数、seed、环境、输入 hash、代码 revision、资源预算和预期指标；local/remote 可暂停恢复；每次 run 产出 immutable result artifact。
- 验收:
  - [local_behavior] 实验对象全字段；pause/resume；immutable artifact 绑定 WorkflowRun
- Verifier: 实验复现测试（同参数重跑结果一致或差异可解释）
- 当前基线: none — WorkflowRun/immutable artifact/recovery 底座可复用
- 设计引用: project_os 02/05
- 依赖: FEAT-13-01, FEAT-08-05
- 状态: pending

### FEAT-13-03 — 隔离 Notebook 与统计纪律
- 父需求: RES-09
- 领域旅程: research.notebook-stats
- 描述: notebook/data interpreter 在隔离环境运行受支持 kernel；表格检查、统计检验、effect size、置信区间、多重比较提示、图表 provenance；失败 cell 不算结果。
- 验收:
  - [local_behavior] 隔离 Python kernel 执行；统计输出带方法与前提；失败 cell 阻塞下游引用
- Verifier: notebook 纪律测试
- 当前基线: none
- 设计引用: project_os 19
- 依赖: FEAT-13-02
- 状态: pending

### FEAT-13-04 — Benchmark/消融/负结果账本
- 父需求: RES-10
- 领域旅程: research.benchmark-ablation
- 描述: benchmark、baseline、ablation、error analysis、robustness、qualitative case workflow；metric 定义、选择决策和 negative result 均进入 ledger。
- 验收:
  - [local_behavior] 六类 workflow 模板；metric 决策与负结果入账不可删除
- Verifier: 账本追加性测试
- 当前基线: none — `kiana eval` suite/baseline 是执行底座候选
- 设计引用: offline-eval-harness；project_os 19
- 依赖: FEAT-13-02
- 状态: pending

### FEAT-13-05 — 论文工作区与 claim-support 阻塞
- 父需求: RES-11
- 领域旅程: research.paper-workspace
- 描述: 大纲、章节、LaTeX/Markdown/Word、figure/table、citation、claim-support 检查、术语一致性、reviewer-facing diff；无来源数字/引用被阻塞或标记。
- 验收:
  - [local_behavior] 三格式写作面；claim-support 检查逐条链接 evidence graph；无来源数字阻塞导出
- Verifier: claim 阻塞测试
- 当前基线: none
- 设计引用: project_os 29
- 依赖: FEAT-12-05, FEAT-12-06
- 状态: pending

### FEAT-13-06 — 复现包与投稿材料
- 父需求: RES-12
- 领域旅程: research.repro-package
- 描述: 一键生成可审查 reproducibility package：环境 lock、code/data manifest、run commands、results、licenses、limitations、checksums；投稿 checklist、supplement 与 response-to-reviewers 工作区。
- 验收:
  - [local_behavior] 包生成完整且可在干净环境重放；投稿三件套生成
- Verifier: 复现包重放测试
- 当前基线: none — SBOM/manifest/checksum 工具链模式可复用
- 设计引用: project_os 36 模式
- 依赖: FEAT-13-02, FEAT-13-05
- 状态: pending

### FEAT-13-07 — Domain pack 注册与 EDA 迁移
- 父需求: RES-13
- 领域旅程: research.domain-packs
- 描述: EDA、硬件、机器人 pack 注册领域 objects/tools/rules/eval/views；EDA 1.0 覆盖需求、原理图/网表、BOM/Gerber/CPL/DFM intake、risk review 和 bring-up plan；签字/下单人工审批。
- 验收:
  - [local_behavior] EDA 现有能力迁移为首个注册 pack（依赖 FEAT-09-07 合同）；`hardware_order` 类动作保持审批记录
- Verifier: pack 注册回归 + EDA 套件
- 当前基线: partial — `kiana eda review`（netlist/BOM/Gerber 规则、bringup-plan、审批记录）已实现；pack 化与其余领域缺失
- 设计引用: project_os 09/26；EDA T3 specs
- 依赖: FEAT-09-07
- 状态: pending

### FEAT-13-08 — Research verifier（DIF-10 载体）
- 父需求: RES-14
- 领域旅程: research.verifier
- 描述: 对 DOI、引用、数据、实验、统计、图表和外部投稿状态逐类验证；缺证据输出 blocked/rework/unknown；模型声明不能使论文结论自动完成。
- 验收:
  - [local_behavior] 七类 verifier 各有正/负 fixture；缺证据路径全部非 complete
  - [target_environment] DOI/投稿状态对真实外部系统核对
- Verifier: verifier 套件
- 当前基线: none — 五态 verifier 底座在 FEAT-08-04
- 设计引用: DIF-10；AF-09
- 依赖: FEAT-08-04, FEAT-12-04
- 状态: pending
