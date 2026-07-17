# Volume 20: Roadmap Governance 与 Scope Control

## 1. 目标

Kiana 的范围很大，如果没有治理，会变成“什么都想做、什么都做不完”。本卷定义路线、阶段、scope 控制和反膨胀规则。

## 2. 路线原则

- Core protocol before UI。
- Evidence before report。
- State before swarm。
- Policy before plugin marketplace。
- Review before release。
- EDA review before EDA automation。

## 3. P0 范围

P0 只做：

- WorkflowRun。
- Task Board。
- Evidence Ledger。
- Verification Gate。
- Context/Recovery。
- Basic Trust Policy。
- Strict Audit。
- Progress Report。

P0 不做：

- full cloud worker。
- full dashboard。
- auto PCB layout。
- auto marketplace trust。
- team SaaS。

## 4. P1 范围

P1 做：

- bounded swarm。
- repo intelligence。
- memory ingest。
- rules/skills injection。
- MCP provenance。
- `/eda review`。

## 5. P2 范围

P2 做：

- dashboard。
- cloud workspace。
- enterprise offline。
- cost usage。
- plugin marketplace UX。
- EDA advanced automation。

## 6. Scope Creep 信号

危险信号：

- UI 先于协议。
- 自动化先于 approval。
- worker 先于 WorkPacket。
- memory 先于 source/confidence。
- graph 先于 live evidence。
- plugin 先于 policy。

出现时必须回到 P0 milestone。

## 7. Feature Admission

新功能进入 backlog 必须回答：

- 用户场景是什么。
- 属于哪个 mode。
- 数据对象是什么。
- evidence 是什么。
- failure path 是什么。
- policy 风险是什么。
- P0/P1/P2 哪个阶段。

答不出来就不进入 P0。

## 8. Milestone Gate

每个 milestone 开始前：

- scope locked。
- schema identified。
- command identified。
- tests identified。
- failure path identified。

结束时：

- evidence generated。
- docs updated。
- report updated。

## 9. Review Governance

Review 必须区分：

- blocker。
- high。
- medium。
- low。
- note。

所有 blocker 必须：

- fix。
- block。
- user accepted risk。

## 10. Reference Governance

参考仓库进入设计的规则：

- 必须有实际目录或明确来源。
- 必须归因到能力域。
- 必须有不借鉴边界。
- 必须转成 Kiana backlog。

## 11. Documentation Governance

文档分层：

- main spec：产品级决策。
- spec library：深度软件逻辑。
- reference audit：来源归因。
- implementation plan：代码任务。

不要混：

- 文档不能假装代码完成。
- 计划不能假装验证完成。
- reference 不能假装实现完成。

## 12. Versioning

规格版本：

- date-based。
- schema version。
- milestone version。

重大变更：

- 更新 main spec。
- 更新 affected volume。
- 更新 reference audit if source changed。

## 13. Decision Log

记录：

- decision。
- alternatives。
- reason。
- impact。
- date。
- evidence。

必须记录：

- scope cut。
- approval policy。
- P0/P1/P2 migration。
- trust boundary。

## 14. Anti-Patterns

避免：

- 把所有参考都复制。
- 把 UI 做成核心。
- 让 agent 自己判自己完成。
- 无 evidence 的报告。
- 无边界 swarm。
- 无 policy 插件。
- 无恢复状态。
- 旧 memory 当事实。

## 15. Roadmap Review Cadence

建议：

- 每完成一个 P0 milestone，更新规格。
- 每次 strict audit 后更新 blockers。
- 每次 reference 新增后更新 coverage matrix。
- 每次插件/政策变化后更新 trust docs。

## 16. 成功标准

Kiana 成功不是“功能最多”，而是：

- 继续能恢复。
- 做事有证据。
- 并行不乱。
- 风险可控。
- 进度能汇报。
- 长期能演进。
