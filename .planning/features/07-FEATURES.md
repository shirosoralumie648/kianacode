# Phase 07 特性账本 — Provider 能力协商与适配闭环

**Created:** 2026-07-26 | **父需求：** CORE-03, CORE-14, DIF-07 | **主旅程：** core.provider-negotiation
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。

### FEAT-07-01 — 六类 Provider 注册认证与健康检查
- 父需求: CORE-03
- 领域旅程: core.provider-negotiation（parity: cc.models.provider-routing）
- 描述: Anthropic、OpenAI、Gemini、OpenRouter、OpenAI-compatible、本地模型可注册、认证、选择、健康检查。
- 验收:
  - [local_behavior] 六类 provider 走统一 registry 元数据与标准 adapter 测试包
  - [target_environment] 每类 live catalog/text/tool smoke（opt-in）通过或记录显式跳过原因
- Verifier: provider_standard 套件 + provider-live-smoke
- 当前基线: partial — Anthropic/OpenAI-compatible/Ollama/fake 已接入且有标准测试包与 live smoke 脚本；Gemini/OpenRouter 缺失
- 设计引用: DESIGN-INDEX Phase 7 行（functional-design §17-18、project_os 03/21）
- 依赖: None
- 状态: pending

### FEAT-07-02 — 能力探测与 capability profile
- 父需求: CORE-03
- 领域旅程: core.provider-negotiation（parity: cc.models.effort-context）
- 描述: tool use/vision/structured output/reasoning/streaming/context window 逐能力显式建档，capability source 可见。
- 验收:
  - [local_behavior] 每 provider 的 capability profile 完整且带来源；`model list` 展示 streaming_mode 等真实能力
- Verifier: capability profile 测试
- 当前基线: partial — model profile/streaming_mode/native_streaming 已实现；vision/reasoning/context 维度不全
- 设计引用: 同上
- 依赖: FEAT-07-01
- 状态: pending

### FEAT-07-03 — 显式路由、降级、模拟与拒绝
- 父需求: CORE-03
- 领域旅程: core.provider-negotiation（parity: cc.models.selection-fallback）
- 描述: Provider 不支持任务所需能力时，调用前显式 route/degrade/emulate/reject，所有客户端收到同一 capability event（AF-14）。
- 验收:
  - [local_behavior] 四种处置各有 fixture；capability event 入 RuntimeEvent 流；无静默降质路径
- Verifier: 协商决策测试
- 当前基线: partial — 不支持 tool 拒绝已有；路由/降级/模拟决策与事件缺失
- 设计引用: 产品总纲 §4.3；AF-14
- 依赖: FEAT-07-02
- 状态: pending

### FEAT-07-04 — 每次选择的理由与成本影响展示
- 父需求: DIF-07
- 领域旅程: core.provider-negotiation
- 描述: 每次模型选择展示 capability source、支持矩阵、选择理由和质量/成本影响。
- 验收:
  - [local_behavior] 选择事件含理由与影响字段；CLI/TUI/App 三面可见
- Verifier: 选择解释测试
- 当前基线: none — 无选择解释面
- 设计引用: DIF-07
- 依赖: FEAT-07-03
- 状态: pending

### FEAT-07-05 — Usage/成本/延迟/健康账本
- 父需求: CORE-14
- 领域旅程: core.provider-negotiation
- 描述: usage、token、估算成本、延迟、retry、budget、tool/connector 健康、policy denial、audit event 可按 session/workflow/provider/tenant 查询。
- 验收:
  - [local_behavior] 四维查询接口；成本估算与 token 记录一致；本地遥测默认不外发
- Verifier: 账本查询测试
- 当前基线: partial — eval 的 token 计量与 usage 事件已有；聚合查询账本缺失
- 设计引用: project_os 35（dashboard 投影）
- 依赖: FEAT-07-01
- 状态: pending

### FEAT-07-06 — Provider 断流与 retry 预算
- 父需求: CORE-03
- 领域旅程: core.provider-negotiation
- 描述: 断流/限流/超时按预算退避重试并记录每次尝试；超预算进入可恢复暂停而不是无限重试（AF-10）。
- 验收:
  - [local_behavior] 断流 fixture：重试有界、事件记录、恢复语义与 core.recovery 对接
- Verifier: 断流注入测试
- 当前基线: partial — 远端 401/403 重试已有；统一退避预算缺失
- 设计引用: 产品总纲 §8.2
- 依赖: FEAT-07-01
- 状态: pending
