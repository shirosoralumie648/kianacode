# Phase 12 特性账本 — Research 来源、引用与证据图谱

**Created:** 2026-07-26 | **父需求：** RES-01..RES-06, DIF-08 | **主旅程：** research.question-scope 等（Phase 12 域）
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。纪律：AF-09——不存在的 DOI/引用/数据即学术不端，缺证据必须 blocked。

### FEAT-12-01 — 研究问题与范围定义
- 父需求: RES-01
- 领域旅程: research.question-scope
- 描述: 定义 question、scope、纳排标准、假设、变量、伦理/数据限制、里程碑与 acceptance；变更形成 decision history。
- 验收:
  - [local_behavior] 定义对象全字段持久化；变更历史可回放；acceptance 接入 workflow 验收（FEAT-08-02 机制）
- Verifier: 定义对象测试
- 当前基线: none — 可复用 WorkflowRun acceptance 与 EventLog decision 记录
- 设计引用: DESIGN-INDEX Phase 12-13 行（project_os 34；RES 需求组）
- 依赖: FEAT-08-02
- 状态: pending

### FEAT-12-02 — 文献/数据/代码检索与合法性区分
- 父需求: RES-02
- 领域旅程: research.literature-retrieval
- 描述: 可配置 search/connectors 检索文献、数据集和代码；显示 query、来源、抓取时间、license/access 状态；合法下载与仅记录 DOI/arXiv/URL 明确区分。
- 验收:
  - [target_environment] 至少 2 个真实检索源（如 arXiv/Crossref）带 license/access 状态；下载 vs 仅记录的区分显式
- Verifier: 检索旅程测试
- 当前基线: none — connector 合同依赖 FEAT-09-05 扩展生命周期
- 设计引用: project_os 34
- 依赖: FEAT-09-05
- 状态: pending

### FEAT-12-03 — 解析与出处保留
- 父需求: RES-03
- 领域旅程: research.parse-provenance
- 描述: PDF、网页、supplement、表格和扫描件解析、OCR、去重、版本关联；每个 excerpt 保留页码/section/坐标、content hash 和 parser warning。
- 验收:
  - [local_behavior] 五类输入解析带全 provenance 字段；parser warning 不静默
- Verifier: 解析 provenance 测试
- 当前基线: none — context ingest 的 hash/manifest 机制可复用
- 设计引用: project_os 06/23
- 依赖: None
- 状态: pending

### FEAT-12-04 — 引用库与元数据校验
- 父需求: RES-04
- 领域旅程: research.citation-library
- 描述: DOI/arXiv/ISBN/URL 元数据校验、BibTeX/RIS/CSL import/export、引用样式、重复/撤稿/版本提示；正文引用跳回 source record。
- 验收:
  - [local_behavior] 三格式 import/export 往返；重复/版本检测
  - [target_environment] DOI/撤稿校验对真实注册中心通过
- Verifier: 引用库测试
- 当前基线: none
- 设计引用: RES 需求组
- 依赖: FEAT-12-03
- 状态: pending

### FEAT-12-05 — Evidence graph（DIF-08 载体）
- 父需求: RES-05
- 领域旅程: research.evidence-graph
- 描述: notes、claims、counter-evidence、methods、datasets、experiments、artifacts 形成 evidence graph；边带 extracted/inferred/ambiguous 与 confidence；孤立 claim 被标记。
- 验收:
  - [local_behavior] 七类节点/边全字段；孤立 claim 检测；每边可回溯 source/hash/time（DIF-08）
- Verifier: 图谱完整性测试
- 当前基线: none — context artifact-graph 是结构参考
- 设计引用: project_os 06；DIF-08
- 依赖: FEAT-12-03
- 状态: pending

### FEAT-12-06 — 综述矩阵与冲突并列
- 父需求: RES-06
- 领域旅程: research.synthesis-matrix
- 描述: 可编辑 literature review matrix、研究计划、related-work taxonomy、progress board；每个 synthesized statement 引用具体来源，冲突证据并列显示。
- 验收:
  - [local_behavior] 矩阵/计划/taxonomy 生成可编辑；无来源 statement 被阻塞；冲突并列展示
- Verifier: 综述阻塞测试
- 当前基线: none — board 机制可复用 tasks/board
- 设计引用: project_os 04/31
- 依赖: FEAT-12-05
- 状态: pending
