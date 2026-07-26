# Phase 05 特性账本 — 策略、信任、凭据与本地数据边界

**Created:** 2026-07-26 | **父需求：** CORE-05, CORE-06, DIF-09 | **主旅程：** core.policy-decision
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。`[M0]` 标记 walking-skeleton 最薄子集。

### FEAT-05-01 — [M0] 单请求 PolicyDecision 链路
- 父需求: CORE-06
- 领域旅程: core.policy-decision
- 描述: 一次工具执行请求产出一条可解释 PolicyDecision（allow/ask/deny + 原因 + 来源层），deny 优先。
- 验收:
  - [local_behavior] 单请求经 trust→profile→managed→rule 链产出 typed decision 并事件化
- Verifier: 决策链测试
- 当前基线: partial — trust root/managed policy/permission profiles 已 fail-closed；决策事件化与解释字段未统一
- 设计引用: DESIGN-INDEX Phase 5 行（project_os 08/33）
- 依赖: FEAT-03-01
- 状态: pending

### FEAT-05-02 — 七层配置确定性优先级
- 父需求: CORE-05
- 领域旅程: core.policy-decision
- 描述: defaults/user/project/local/env/CLI/managed 优先级确定，用户可查某项配置为何生效。
- 验收:
  - [local_behavior] 七层覆盖矩阵测试；`config resolved` 展示层与来源且脱敏
- Verifier: 优先级矩阵测试
- 当前基线: implemented — config resolved v1 与 managed overlay 已实现；project/local/CLI 层与七层完整矩阵待补
- 设计引用: project_os 08
- 依赖: None
- 状态: pending

### FEAT-05-03 — 凭据仅入 Keychain/vault
- 父需求: CORE-05
- 领域旅程: core.policy-decision
- 描述: API key/OAuth/Cookie/SSH key/license key 只进系统 Keychain 或 vault；状态只显示脱敏元数据；安全存储不可用时 fail closed（AF-06）。
- 验收:
  - [target_environment] Linux/macOS Keychain 写读；不可用时拒绝明文回退
  - [local_behavior] 全部状态/诊断面无明文 secret（现有脱敏合同回归）
- Verifier: Keychain 集成测试 + 脱敏扫描
- 当前基线: partial — OAuth token 文件原子写/脱敏状态/secrets 端点已有；系统 Keychain 集成缺失
- 设计引用: 产品总纲 §7.2
- 依赖: None
- 状态: pending

### FEAT-05-04 — 首启自主权档位与项目覆盖
- 父需求: CORE-06
- 领域旅程: core.policy-decision
- 描述: 首次启动选择安全/平衡/自治档位，可按项目覆盖；界面清楚展示实际生效档位与硬策略。
- 验收:
  - [local_behavior] 三档语义（逐项确认/低风险自动/受限持续）落入 PolicyDecision；项目覆盖生效且可见
- Verifier: 档位行为测试
- 当前基线: partial — 首启 onboarding sentinel 与 permission profiles 已有；三档产品语义未实现
- 设计引用: 产品总纲 §7.4；project_os 33
- 依赖: FEAT-05-01
- 状态: pending

### FEAT-05-05 — 不可绕过硬策略（DIF-09）
- 父需求: DIF-09
- 领域旅程: core.policy-decision
- 描述: 删除、发布、付款、凭据、外部消息等硬策略对全部档位生效；hook/plugin/MCP/managed override/remote worker 均不可绕过（AF-07/08）。
- 验收:
  - [local_behavior] 每个绕过向量（档位/hook/plugin/MCP/env/session）负面测试全部 deny
- Verifier: 绕过向量负面套件
- 当前基线: partial — commercial profile 对 mutating 工具 fail-closed 已证；硬策略清单与全向量套件未建
- 设计引用: AF-07/AF-08；project_os 33
- 依赖: FEAT-05-04
- 状态: pending

### FEAT-05-06 — 全入口一致 PolicyDecision
- 父需求: CORE-06
- 领域旅程: core.policy-decision
- 描述: CLI、MCP、remote、Desktop、Web 对同一 path/network/exec/sandbox/副作用请求产出一致可解释决策。
- 验收:
  - [local_behavior] 同一请求 fixture 经五入口得到等价 decision（含原因），差异即失败
- Verifier: 跨入口决策等价测试
- 当前基线: partial — trust 决策已跨 runner/MCP/direct-connect 传播；等价矩阵未验收
- 设计引用: control-plane 架构
- 依赖: FEAT-05-01
- 状态: pending

### FEAT-05-07 — 云供应商认证旅程（parity）
- 父需求: CORE-05
- 领域旅程: core.policy-decision（parity: cc.auth.oauth-api-cloud, target_phase 05）
- 描述: OAuth、API key 与受支持云供应商认证旅程达到 parity 要求的 target_environment proof。
- 验收:
  - [target_environment] OAuth 登录/刷新/过期处理对真实端点通过；`auth status` 就绪面脱敏正确
- Verifier: live auth smoke（opt-in）
- 当前基线: partial — OAuth 刷新/重试/`auth status` 已实现；真实端点 proof 缺失
- 设计引用: governance parity cc.auth.oauth-api-cloud
- 依赖: FEAT-05-03
- 状态: pending
