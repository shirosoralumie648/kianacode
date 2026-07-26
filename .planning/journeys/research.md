# Research 黄金旅程账本

**Created:** 2026-07-26 | **覆盖需求：** RES-01..14 + DIF-08/10 | **消费方：** features/MILESTONES/verifier。
**纪律：** 一切结论可回溯 source/hash/time/confidence；无证据必须 blocked/rework/unknown（AF-09）。

### research.question-scope — 研究问题与范围定义
- 覆盖需求: RES-01
- 必备能力点: question/scope/纳排标准/假设/变量/伦理数据限制/里程碑/acceptance；decision history
- Required proof: local_behavior | 归属阶段: 12
- 当前 gap: 无实现；依赖 core.workflow-evidence 的验收定义机制

### research.literature-retrieval — 文献与数据检索
- 覆盖需求: RES-02
- 必备能力点: 可配置 search/connectors；query/来源/抓取时间/license/access 状态；合法下载与仅记录 DOI/arXiv/URL 区分
- Required proof: target_environment（真实检索源） | 归属阶段: 12
- 当前 gap: 无实现；connector 合同待 core.extension-lifecycle

### research.parse-provenance — 解析与出处保留
- 覆盖需求: RES-03
- 必备能力点: PDF/网页/supplement/表格/扫描件解析与 OCR；页码/section/坐标/hash/版本关系/parser warning
- Required proof: local_behavior | 归属阶段: 12
- 当前 gap: 无实现；context ingest 的 hash 机制可复用

### research.citation-library — 引用库与校验
- 覆盖需求: RES-04
- 必备能力点: DOI/arXiv/ISBN/URL 校验；BibTeX/RIS/CSL；重复/撤稿/版本；正文引用跳回 source record
- Required proof: local_behavior（DOI 校验 target_environment） | 归属阶段: 12
- 当前 gap: 无实现

### research.evidence-graph — 证据图谱
- 覆盖需求: RES-05 + DIF-08
- 必备能力点: notes/claims/counter-evidence/methods/datasets/experiments/artifacts 图；extracted/inferred/ambiguous + confidence + freshness；孤立 claim 标记
- Required proof: local_behavior | 归属阶段: 12
- 当前 gap: 无实现；context artifact-graph 是结构参考

### research.synthesis-matrix — 综述与冲突并列
- 覆盖需求: RES-06
- 必备能力点: literature review matrix/研究计划/related-work taxonomy/progress board；synthesized statement 逐条引用；冲突证据并列
- Required proof: local_behavior | 归属阶段: 12
- 当前 gap: 无实现

### research.dataset-code-intake — 数据与代码引入
- 覆盖需求: RES-07
- 必备能力点: version/license/checksum/schema/split/预处理/环境/访问限制记录；敏感数据 policy
- Required proof: local_behavior | 归属阶段: 13
- 当前 gap: 无实现；plugin receipt 完整性模式可复用

### research.experiment-run — 可复现实验执行
- 覆盖需求: RES-08
- 必备能力点: 参数/seed/环境/输入 hash/代码 revision/预算/预期指标；local/remote 可暂停恢复；immutable result artifact
- Required proof: local_behavior | 归属阶段: 13
- 当前 gap: 无实现；WorkflowRun + immutable artifact 层可复用

### research.notebook-stats — 隔离 Notebook 与统计
- 覆盖需求: RES-09
- 必备能力点: 隔离 kernel；表格检查/统计检验/effect size/CI/多重比较提示/图表 provenance；失败 cell 不算结果
- Required proof: local_behavior | 归属阶段: 13
- 当前 gap: 无实现

### research.benchmark-ablation — 基准与消融
- 覆盖需求: RES-10
- 必备能力点: benchmark/baseline/ablation/error analysis/robustness/negative result 入账；metric selection 决策记录
- Required proof: local_behavior | 归属阶段: 13
- 当前 gap: 无实现；`kiana eval` 合同是执行底座候选

### research.paper-workspace — 论文工作区
- 覆盖需求: RES-11
- 必备能力点: 大纲/章节/LaTeX/Markdown/Word/figure/citation/claim-support 检查/术语一致/reviewer diff；无来源数字阻塞
- Required proof: local_behavior | 归属阶段: 13
- 当前 gap: 无实现

### research.repro-package — 复现包与投稿
- 覆盖需求: RES-12
- 必备能力点: 环境 lock/code-data manifest/run commands/results/licenses/limitations/checksums；投稿 checklist/supplement/response 工作区
- Required proof: local_behavior | 归属阶段: 13
- 当前 gap: 无实现；SBOM/manifest 工具链模式可复用

### research.domain-packs — 领域扩展包（EDA 先行）
- 覆盖需求: RES-13
- 必备能力点: 领域 objects/tools/rules/eval/views 注册；EDA 1.0：需求/原理图网表/BOM/Gerber/CPL/DFM intake/risk review/bring-up plan；签字下单人工审批
- Required proof: local_behavior（工程签字 user_value） | 归属阶段: 13
- 当前 gap: `kiana eda review` 结构化审查已实现（netlist/BOM/Gerber 规则、bringup-plan）；pack 注册合同与其余领域缺失

### research.verifier — 研究完整性 verifier
- 覆盖需求: RES-14 + DIF-10
- 必备能力点: DOI/引用/数据/实验/统计/图表/投稿状态逐类验证；缺证据 blocked/rework/unknown；模型声明不能完成论文结论
- Required proof: local_behavior（外部状态 target_environment） | 归属阶段: 13
- 当前 gap: 无实现；五态 verifier 底座已在 core.workflow-evidence
