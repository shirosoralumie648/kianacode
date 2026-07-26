# Phase 06 特性账本 — RuntimeHost 与运行时抽取

**Created:** 2026-07-26 | **父需求：** CORE-07 | **主旅程：** core.context-pack, core.walking-skeleton
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。`[M0]` 在本阶段收口为完整 walking-skeleton 演示。

### FEAT-06-01 — [M0] Walking-skeleton 贯通演示
- 父需求: CORE-07
- 领域旅程: core.walking-skeleton
- 描述: 一个真实请求走完 typed event → session 持久化 → PolicyDecision → 单工具执行（带最小 context）→ typed result，CLI 演示且全程可重放。
- 验收:
  - [local_behavior] 贯通链路端到端测试 + CLI 演示脚本；EventLog 重放一致（M0 退出门禁）
- Verifier: M0 门禁核对（MILESTONES.md）
- 当前基线: partial — 各环节独立存在；贯通旅程未验收
- 设计引用: MILESTONES M0；control-plane 架构
- 依赖: FEAT-03-01, FEAT-04-01, FEAT-05-01
- 状态: pending

### FEAT-06-02 — 预算受控 ContextPack
- 父需求: CORE-07
- 领域旅程: core.context-pack
- 描述: Context Builder 按任务预算生成 ContextPack，每项带 source/time/hash/permission/truncation reason。
- 验收:
  - [local_behavior] 预算裁剪确定性；每项五元数据齐全；truncation 有原因码
- Verifier: pack 预算测试
- 当前基线: partial — context pack v1/snippet 预算已有；permission 与 truncation reason 字段缺失
- 设计引用: DESIGN-INDEX Phase 6 行（functional-design §8/F04、project_os 06）
- 依赖: None
- 状态: pending

### FEAT-06-03 — 上下文组合面（repo map/symbol/impact/文件集）
- 父需求: CORE-07
- 领域旅程: core.context-pack
- 描述: repo map、symbol/path search、impact/trace、editable/read-only 文件集、artifact graph 可组合，遗漏项与预算消耗可检查。
- 验收:
  - [local_behavior] 五种来源组合入 pack；遗漏项显式列出；预算消耗可查询
- Verifier: 组合矩阵测试
- 当前基线: partial — repo-map/search/vector-search/artifact-graph/files 均已实现；impact/trace 与遗漏报告缺失
- 设计引用: project_os 06
- 依赖: FEAT-06-02
- 状态: pending

### FEAT-06-04 — 快照冻结与可观察刷新
- 父需求: CORE-07
- 领域旅程: core.context-pack
- 描述: session 恢复、入口切换或 retry 不静默改变已冻结上下文；必要刷新生成新 snapshot 与原因。
- 验收:
  - [local_behavior] resume/retry fixture：冻结 pack hash 不变；刷新产生新 snapshot 事件与原因
- Verifier: 快照语义测试
- 当前基线: none — 无冻结语义
- 设计引用: project_os 06
- 依赖: FEAT-06-02
- 状态: pending

### FEAT-06-05 — RuntimeHost 抽取与入口等价
- 父需求: CORE-07
- 领域旅程: core.context-pack
- 描述: 同一请求经 InProcessHost 或其他 RuntimeHost 入口执行时，客户端看到语义一致的 ContextPack。
- 验收:
  - [local_behavior] InProcess 与 daemon 路径产出等价 pack（source/hash 一致）
- Verifier: host 等价测试
- 当前基线: partial — daemon 组合根与 broker 迁移已完成大半（architecture status legacy_edges_remaining=9）；host 抽象未定型
- 设计引用: control-plane 架构；command-query 迁移计划
- 依赖: FEAT-06-01
- 状态: pending
