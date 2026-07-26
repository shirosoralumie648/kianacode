# Phase 08 特性账本 — 可靠 Workflow、证据与副作用语义

**Created:** 2026-07-26 | **父需求：** CORE-09, CORE-10, CORE-11, DIF-01, DIF-02, DIF-03 | **主旅程：** core.workflow-evidence, core.recovery
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。

### FEAT-08-01 — Intent Router 与路由理由
- 父需求: CORE-09
- 领域旅程: core.workflow-evidence
- 描述: 每个请求输出 pack、运行档位、风险、权限、副作用和验收理由；用户可覆盖；简单请求直通，复杂请求转持久 WorkflowRun。
- 验收:
  - [local_behavior] 路由决策 typed 且事件化；直通/升格判据（跨步/跨会话/多 Agent/外部副作用）有 fixture
- Verifier: 路由决策测试
- 当前基线: none — 无 Intent Router；WorkflowRun 升格入口是手动命令
- 设计引用: DESIGN-INDEX Phase 8 行（functional-design §17、project_os 03）
- 依赖: FEAT-06-01
- 状态: pending

### FEAT-08-02 — 验收前置定义（acceptance-first）
- 父需求: CORE-10
- 领域旅程: core.workflow-evidence
- 描述: 任务启动时定义可观察 acceptance；verifier 只对照启动时定义验收，禁止事后降标。
- 验收:
  - [local_behavior] WorkflowRun 创建时固化 acceptance；执行中修改被拒或强制走显式变更记录
- Verifier: acceptance 固化测试
- 当前基线: partial — VerificationPacket 与五态已实现；acceptance 前置定义缺失
- 设计引用: project_os 05
- 依赖: FEAT-08-01
- 状态: pending

### FEAT-08-03 — Evidence Ledger 全类别覆盖
- 父需求: CORE-10
- 领域旅程: core.workflow-evidence
- 描述: diff、命令、测试、引用、实验、审批、外部回执、产物全部入账并绑定验收条目。
- 验收:
  - [local_behavior] 八类证据的 ledger 记录与 acceptance 条目双向引用；complete 状态可逐条展开证据
- Verifier: ledger 覆盖测试
- 当前基线: partial — Evidence Ledger/immutable artifact/verification 已实现（diff/命令/测试类）；引用/实验/回执/审批类未接入
- 设计引用: project_os 05
- 依赖: FEAT-08-02
- 状态: pending

### FEAT-08-04 — 五态 Verifier 与模型自述禁令（DIF-01）
- 父需求: DIF-01
- 领域旅程: core.workflow-evidence
- 描述: complete/rework/blocked/awaiting_approval/result_unknown 五态；模型自然语言声明不能单独使任务 complete。
- 验收:
  - [local_behavior] 缺证据 fixture 全部无法 complete；五态转移矩阵测试；用户可展开每个 complete 的逐条 evidence
- Verifier: 五态矩阵测试
- 当前基线: partial — workflow complete 已要求 final_status=pass 的 VerificationPacket；五态中 result_unknown/awaiting_approval 语义未落地
- 设计引用: 产品总纲 §4.4
- 依赖: FEAT-08-03
- 状态: pending

### FEAT-08-05 — Crash/中断恢复到最后可信节点（DIF-02）
- 父需求: CORE-11
- 领域旅程: core.recovery
- 描述: crash、interrupt、超时、provider 断流、worker 失败、projection 损坏后恢复到最后可信节点；partial/unknown 不显示为 Done。
- 验收:
  - [local_behavior] 六类故障注入 fixture 各自恢复或 blocked；恢复不通过"重新执行整个回复"猜测状态
- Verifier: 故障注入套件
- 当前基线: partial — crash-window state 修复、recovery journal、swarm 恢复已实现；provider 断流与统一恢复入口缺失
- 设计引用: workflow-runtime-design；project_os 22
- 依赖: FEAT-07-06
- 状态: pending

### FEAT-08-06 — 内部幂等键与原子提交
- 父需求: CORE-11
- 领域旅程: core.recovery
- 描述: 内部状态变更使用幂等键和原子提交；重复应用同一操作无二次副作用。
- 验收:
  - [local_behavior] 重放同一内部操作 fixture：状态不变、事件标记幂等命中
- Verifier: 幂等测试
- 当前基线: partial — 同 packet 重试幂等已示范（workflow complete）；通用幂等键机制缺失
- 设计引用: 产品总纲 §8.3
- 依赖: FEAT-08-05
- 状态: pending

### FEAT-08-07 — result_unknown 外部副作用安全（DIF-03）
- 父需求: DIF-03
- 领域旅程: core.recovery
- 描述: 外部响应在 dispatch 后丢失时进入 result_unknown，先查询目标系统或请求人工核对，绝不盲目重放（AF-08）。
- 验收:
  - [local_behavior] 丢失回执 fixture：状态 result_unknown、重放被拒、核对路径（查询/人工）事件化
- Verifier: result_unknown 语义测试
- 当前基线: none — result_unknown 状态与核对流程未实现
- 设计引用: AF-08；产品总纲 §8.2
- 依赖: FEAT-08-04
- 状态: pending

### FEAT-08-08 — 跨入口一致的任务状态与恢复建议
- 父需求: CORE-09
- 领域旅程: core.workflow-evidence
- 描述: 用户在所有入口看到一致的五态与恢复建议。
- 验收:
  - [local_behavior] CLI/TUI/App 对同一 run 显示等价状态与建议；状态语义无入口私有变体
- Verifier: 状态一致性测试
- 当前基线: partial — workflow 状态在 CLI/App 可见；恢复建议与一致性矩阵缺失
- 设计引用: project_os 35
- 依赖: FEAT-08-04
- 状态: pending

### FEAT-08-09 — 离线 eval 门禁接入 workflow 质量回归
- 父需求: CORE-10
- 领域旅程: core.workflow-evidence
- 描述: `kiana eval run` 的 suite/baseline 机制接入 workflow 验收，作为行为回归证据类别。
- 验收:
  - [local_behavior] workflow 验收可引用 eval 报告为证据；回归失败阻塞 complete
- Verifier: eval-as-evidence 测试
- 当前基线: partial — eval harness 与 baseline 回归语义已实现；与 workflow 验收未打通
- 设计引用: offline-eval-harness 设计
- 依赖: FEAT-08-03
- 状态: pending
