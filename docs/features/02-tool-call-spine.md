# 工具调用与授权脊柱

> 一句话：AI 助手每次"想动手干活"（跑命令、改文件、查记忆、调外部工具），都必须先递交一份正式申请，层层过审之后才能被执行；这条"申请—审批—执行—留痕"的流水线就是本篇要讲的东西，它是整个产品最重要的安全链条。
>
> 本文写的是代码现在的真实样子，不是产品愿景；"做到什么程度"以 CURRENT_STATUS.md 为准。

## 这个功能是干什么的

模型说的不算数。它说"我要跑个命令"，Kiana 不会直接执行：这个意图先变成一份正式申请（CapabilityRequest），申请要过控制面（ControlPlane，所有请求必经的授权与派发入口）这一关，批了才由能力经纪人（CapabilityBroker）交给对应的 handler 去执行，干完留下事件和回执（event / receipt），全程可查。

这样设计的根本原因是：模型可能被提示词骗、可能出错、也可能天生就想干超出授权的事。所以"模型想做什么"和"实际允许做什么"必须分成两件事，中间隔一道墙。

## 现在能干什么 / 不能干什么

**能（每条在代码里都有出处）：**

- 模型只看得见五个固定工具：shell（跑命令）、apply_patch（改文件）、mcp（调外部工具）、memory.search（查记忆库）、memory.write（写记忆库）；工具清单是写死的，连测试都断言清单就是这五个（`kiana-runner/src/tools.rs:tool_schemas`）。
- 模型喊出的任何工具名都会被转成 CapabilityRequest，不会直接执行；认不出的名字（包括"shell;rm -rf /"这种注入式假名字）直接报错拒掉，不做模糊匹配、不降级成别的工具（`kiana-runner/src/tools.rs:capability_for_tool`，测试 `unknown_tool_names_fail_closed_without_alias_or_shell_fallback`）。
- 审批按"项目信任 → 角色权限 → 风险等级"三道关顺序判断；越权（比如规划角色想写代码目录）属于"人工批准也救不回来"的硬拒绝（`kiana-policy/src/lib.rs:DefaultPolicyEngine::evaluate`、`role_decision`）。
- MCP 调用被强制标为"对外副作用"风险，任何人都不能把它降级成低风险来绕过审批（`kiana-policy/src/lib.rs:capability_risk_violation`）。
- 四类真正的执行器已经登记在 broker 上：shell、apply_patch（`kiana-daemon/src/harness_capabilities.rs:register`）、mcp（`kiana-daemon/src/harness_mcp.rs`）、memory 查/写（`kiana-daemon/src/harness_memory.rs`）。
- shell 命令在 Linux 上用 bubblewrap（一种沙箱工具）隔离执行，默认 30 秒超时、最多 1MB 输出（`kiana-daemon/src/harness_capabilities.rs:ShellExecHandler::execute`）。
- apply_patch 只在 workspace-write（允许改工作区）档位下可用，只读档位明确拒绝（`kiana-daemon/src/harness_capabilities.rs:ApplyPatchHandler::execute`）。
- 人工审批绑定"申请内容的指纹（request_hash）+ 一次性 nonce"，只能用一次，5 分钟不确认就作废，并且持久化在本地文件里（`kiana-daemon/src/approval_store.rs`，默认有效期见 `DEFAULT_APPROVAL_TTL`）。
- 审批请求缺指纹或缺 nonce 会被网关直接打回（`kiana-daemon/src/lib.rs:handle` 里的 `approval_proof_required` / `approval_context_mismatch`）。
- 每一步都有流水记录（事件）落盘到本地 JSONL 文件，执行结果在写进记录前会先脱敏（把疑似密码、令牌的文字抹掉）（`kiana-core/src/lib.rs:redact_capability_result`、`kiana-eventlog` 的 `JsonlEventLog`）。
- 运行结束后可以查"收据"：调了哪些工具、改了哪些文件（`kiana-core/src/lib.rs:read_receipt`）。
- CLI 的 Safe 档可以设置"本地写类审批自动同意"这个明确的例外开关（`kiana-entrypoints/src/command_dispatch.rs:should_auto_approve_local_write`）。

**不能（CURRENT_STATUS.md 明确记录，或代码里有明确拒绝路径）：**

- 整条链的现状是"代码在、局部测过，还没当真产品验证过"——CURRENT_STATUS.md 把 `shell / apply_patch broker 主路径`、`基础 trust/sandbox/role/path/memory policy` 等都标为 partial + local_behavior（局部行为验证），且验证是在"录好的模型脚本"（cassette）上做的。
- 还没接入真实的在线模型服务（CURRENT_STATUS.md：live provider 为 not_supported），所以这条链跑的是假模型脚本，不是真模型。
- 跨进程完整恢复做不到：审批的"接下来怎么继续执行"这件事的信息只存在内存里，程序一重启，就算审批单还躺在磁盘上，也只能报 `approval_continuation_unavailable` 然后拒绝执行（`kiana-core/src/lib.rs:decide_approval_with_proof`；CURRENT_STATUS.md 把跨进程 resume 标为 deferred）。
- shell / apply_patch 主路径仍留有几处已知待加强：检查与执行之间的时间差（TOCTOU，"看的时候没危险不代表干的时候没危险"）、补丁的原子性、进程树取消、输出脱敏的完备度（CURRENT_STATUS.md §2 能力表原文）。
- MCP 只支持 stdio（本地进程）方式，HTTP 方式的 MCP 明确拒绝。
- 工具执行失败后没有自动重试——失败结果原样喂回模型，让模型自己决定下一步（`kiana-core/src/lib.rs:broker_harness_capability` 的错误分支）。
- 未受信（没有 `kiana trust` 过）的项目连只读申请都直接拒绝，不进入审批（`kiana-policy/src/lib.rs:DefaultPolicyEngine::evaluate` 第一条）。

## 代码怎么跑（走读）

先介绍几个会反复出现的类型：

- **CapabilityRequest（下文有时简称"申请单"）**：一份写着"我要用什么类型的能力（Process/Filesystem/Network/Query/Secret），做什么操作，参数是什么，风险多高"的正式申请。定义在 `kiana-domain`（领域合同层，全仓库统一的表格式样）。
- **AuthorizedCapabilityRequest**：申请通过审批后包上 authorization_id 的形态。没有这个授权号，broker 拒收。
- **ControlPlane（控制面，`kiana-core`）**：所有申请的必经授权与派发入口，也是唯一有权放行的地方。
- **KianaHarness（harness，`kiana-runner`）**：驱动模型循环的运行器，守在模型旁边，把模型的工具调用意图转成 CapabilityRequest。它自己没有任何执行工具的权力。
- **CapabilityBroker（能力经纪人，`kiana-capability-broker`）**：维护一张"（能力类型，操作名）→ handler"的精确注册表。只按完全一致的键派发，查不到就把申请退回（拒绝），绝不找人"顺便干了"。
- **CapabilityHandler（handler）**：真正执行能力的适配器，一个 handler 只处理一种操作。

### 主路径：一次工具调用的完整旅程

1. **你敲下命令。** 你在终端输入 `kiana run "帮我看看这个项目"`。入口层（`kiana-entrypoints`）组装请求：它先查这个项目你是否信任过（`kiana-entrypoints/src/harness_run.rs:client_on_host`），并组装好请求上下文 RequestContext，里面有：本次请求编号、会话编号、项目路径、操作者身份（本地固定主体）、项目是否受信、权限档位（permission profile，分 Safe/Balanced 等，Safe 最严格）、角色（role，决定能用哪些工具）。然后通过进程内的客户通道把请求送到 DaemonHost。

2. **DaemonHost 先做入口校验。** DaemonHost（`kiana-daemon/src/lib.rs:handle`）是 composition root（组装根：控制面、策略、事件账本、执行器都在这里装配起来）。它在转交之前先做几件事：检查请求用的是正确的协议版本；审批类请求必须带指纹和 nonce，否则打回；操作者身份一律以本机登记的本地主体为准（客户端自称是谁不算数）；角色必须是 `RoleSpec` 花名册上真实存在的（RoleSpec 规定每个角色能用什么工具、能碰哪些路径）。它构建 ControlPlane 时已经把 broker 和四类 handler 装配好了（`kiana-daemon/src/lib.rs:with_runner_events_approval_and_authority` 里能看到四行 `register` 登记）。

3. **开工，模型开始说话。** ControlPlane（`kiana-core/src/lib.rs:start_run_with_id`）把任务交给 KianaHarness，进入 `drive_run` 循环（`kiana-core/src/lib.rs:drive_run`）：harness 每产出一个事件，ControlPlane 就处理一个。harness 这边（`kiana-runner/src/harness.rs:model_step`）拿着五个固定工具的 schema 去问模型。模型回答"我要跑个命令 ls"，harness **不会**去跑这个命令。

4. **构造 CapabilityRequest。** harness 调用 `kiana-runner/src/tools.rs:capability_for_tool`，把模型的调用意图翻译成一份正式申请：操作名 `shell.exec`、能力类型 Process、风险等级按沙箱档位初判（read-only 档填"只读"，workspace-write 档填"本地写"），并把命令内容、工作目录、项目根目录等抄进参数栏。如果模型一口气报了三个工具，harness 只把**第一个**递出去，剩下两个排队（`pending_tools` 队列），一个干完再递下一个——工具调用是严格串行的。填好后 harness 发出一个"申请递交"事件，自己就挂起等消息。

5. **ControlPlane 审批。** 申请进入 `kiana-core/src/lib.rs:broker_harness_capability`，它按顺序做：
   - **核对身份**：`stamp_request_identity` 把 RequestContext 上的角色、部门、会话、项目路径抄进申请参数——handler 拿到的是 ControlPlane 核对过的版本，不是模型自己填的版本。
   - **写流水**：先记一条"收到工具申请"的事件（把命令内容记入本地 JSONL 事件账本，敏感字先脱敏）。
   - **政策判断**（`kiana-policy/src/lib.rs:DefaultPolicyEngine::evaluate`）：按固定顺序过筛子——① 项目不受信？拒绝，连审批机会都不给。② 角色越权（工具不在岗位说明书里、要写的文件路径不在允许清单里、要查的记忆库没权限）？拒绝，这种拒绝**不能靠人工批准翻案**——代码注释原话是"不能因为请求同时是高风险动作就降级成 Ask，再由一次人工批准绕过范围限制"。③ MCP 风险被人为调低？拒绝。④ 操作名命中敏感词（payment、delete、grant 等）或涉及密钥？必须人工审批。⑤ 最后看风险等级：只读→放行；本地写→Safe 档要审批、其他档放行；对外副作用（如 MCP）→必须审批；最高危→必须审批。
   - **关卡复核**（`kiana-gates/src/lib.rs:DefaultGateEngine::evaluate`）：第二双眼睛，只会收紧不会放松——"待审批"不能变成"放行"，"放行但没填授权编号"按拒绝处理。
   - **项目钩子**（放行之后、执行之前）：如果项目配置了 PreToolUse 钩子（项目主人自己写的拦截规则），还会再问一次钩子的意见；钩子说"要问人"但现场没人可问时，按失败处理（`hook_ask_unattended`）。

6. **两种命运。**
   - **直接放行**：ControlPlane 给申请包上 authorization_id，生成 AuthorizedCapabilityRequest，交给 broker（`kiana-capability-broker/src/lib.rs:CapabilityBroker::execute`）。broker 按"（能力类型，操作名）"这个精确键查注册表找到对应 handler。查不到就退回——**绝不做模糊匹配**；注册表也不允许后来的 handler 顶替先来的（重复登记直接报错，防止有人偷偷换掉执行含义）。shell handler 拿到请求后：重新校验沙箱档位（申请里写的档位不算数，handler 只认自己校验过的）、把项目根目录换算成真实路径、用 bubblewrap 把命令关进沙箱执行，30 秒超时，最多收 1MB 输出。执行期间 ControlPlane 同时盯着取消信号，你一按取消，正在跑的工具立刻被撤下。
   - **需要人工审批**：ControlPlane 把申请存进审批存储（`kiana-daemon/src/approval_store.rs`），生成一张审批挑战（ApprovalChallenge）：审批编号、申请内容的指纹（request_hash，防止你批准的是 A 却被人拿去执行 B）、一次性 nonce、5 分钟有效期。同时它在内存的 `pending_invocations` 表（PendingInvocation，一个 HashMap）里记下"这个审批通过了之后，回到哪条流水线、把结果交给哪次运行"。然后整次运行挂起，命令行返回"等待审批"。

7. **人工审批（如果走到这一步）。** 界面上会弹出"模型想执行 `ls`，允许吗？"。你点允许后，客户端把决定连同指纹和 nonce 一起送回（`kiana-entrypoints/src/harness_run.rs` 里能看到把审批挑战转成提示、再把你的选择送回去的过程）。ControlPlane（`kiana-core/src/lib.rs:decide_approval_with_proof`）核验：指纹对不对、nonce 对不对、是不是被用过一次了、过期没有。全对才从存储里取出原申请、从 `pending_invocations` 里取出待办事项，**重新做一遍第 5 步的风险核查**（审批通过了不等于免检），再交 broker 执行。你点拒绝的话，模型会收到一条"审批被拒"的工具结果，它会自己想办法或者放弃。

8. **回执与闭环。** 执行结果（CapabilityResult）回到 ControlPlane：先核对结果编号和申请编号是否一致（不一致按"结果不明"处理，宁可存疑不可冒认）；把疑似密码令牌脱敏；写"工具执行完成/失败"事件；然后把结果作为工具消息喂回模型。模型看到 `ls` 的输出，继续说话或者继续申请下一个工具，如此循环，直到模型纯文字收工。运行结束时，ControlPlane 从事件账本里汇总出一张收据（哪些工具、改了哪些文件），连同最后一段文字一起交还给你——`kiana run --receipt <会话ID>` 事后也能查到这份收据。

### 最重要的分支：程序重启时审批还在等

审批单是写在磁盘上的（JSONL 文件），但"通过之后怎么继续"的待办事项只在内存里。所以重启之后你拿着还有效的审批单来点"允许"，ControlPlane 会先验证你的身份、指纹、nonce（都通过），然后发现内存里没有对应的 PendingInvocation——它会**保留**这张审批单不被消费，记一条 `approval_continuation_unavailable` 事件，然后告诉你"续不上"。宁可卡住，也不在没有监督的情况下放行。这就是为什么 CURRENT_STATUS.md 把"跨进程完整 resume"标为 deferred（推迟实现）。

### 另一个分支：模型填了假名字

模型如果在工具调用里写了 `shell;rm -rf /` 这种名字，`capability_for_tool` 直接报 `tool_unsupported`，整次运行立即失败——不猜、不纠错、不回退。同理，有人绕过模型直接伪造一份"低风险 MCP"申请想送 broker，政策层的 `capability_risk_violation` 会在派发前拦下（`mcp_risk_downgrade` / `mcp_capability_mismatch`）。

## 关键概念速查

| 概念 | 白话解释 | 代码在哪 |
|---|---|---|
| CapabilityRequest | 申请单：模型想用某个能力时填的正式表格，带风险等级 | `kiana-domain`（`CapabilityRequest`）；由 `kiana-runner/src/tools.rs:capability_for_tool` 填写 |
| AuthorizedCapabilityRequest | 包了 authorization_id 的申请；没有它 broker 拒收 | `kiana-domain`（`AuthorizedCapabilityRequest::new`，空授权号直接报错） |
| RequestContext | 请求上下文：谁、哪个会话、哪个项目、信不信任、什么角色 | `kiana-domain`；由 `kiana-daemon/src/lib.rs:handle` 组装 |
| ControlPlane | 审批中枢：唯一有权盖章和派发的地方 | `kiana-core/src/lib.rs` |
| PolicyDecision / GateDecision | 政策判定和关卡复核：放行 / 待审批 / 拒绝 | `kiana-policy/src/lib.rs`、`kiana-gates/src/lib.rs` |
| ApprovalChallenge | 审批挑战：给审批人看的摘要信息，带内容指纹、一次性 nonce、5 分钟有效期 | `kiana-domain`；由 `kiana-daemon/src/approval_store.rs` 签发 |
| PendingInvocation | 只存内存的待办：审批通过后回到哪条流水线的记录 | `kiana-domain`（`PendingInvocation`）；存于 `kiana-core` 的 `pending_invocations` |
| CapabilityBroker | 按（能力类型，操作名）精确派发的 broker，注册表不允许模糊匹配 | `kiana-capability-broker/src/lib.rs:CapabilityBroker` |
| CapabilityHandler | 真正执行能力的适配器（shell、apply_patch、mcp、memory 各一类） | `kiana-daemon/src/harness_capabilities.rs` 等 |
| CapabilityResult | handler 干完活交回来的结构化结果 | `kiana-domain` |
| sandbox | 沙箱档位：read-only（只许看）和 workspace-write（许改工作区），Linux 上靠 bubblewrap 实现 | `kiana-runner/src/harness.rs:normalize_sandbox`；`kiana-daemon/src/harness_sandbox.rs` |
| RiskLevel | 风险等级：只读 / 本地写 / 对外副作用 / 最高危 | `kiana-domain`（`RiskLevel`） |
| permission profile | 权限档位：Safe 最严（本地写也要审批）、Balanced 宽一些 | `kiana-domain`（`PermissionProfile`） |
| EventLog / Receipt | 本地 JSONL 事件账本和收据，事后可查每次工具调用 | `kiana-eventlog`；收据汇总在 `kiana-core/src/lib.rs:run_receipt_from_store` |

## 设计视角：现在最明显的短板

以下都是基于代码现状的观察，不是改进建议。

1. **审批续跑断在内存里。** 审批单落了盘，但 PendingInvocation 只在一个进程内的 HashMap 里（`kiana-core/src/lib.rs:pending_invocations`），重启后审批链路只能走到 `approval_continuation_unavailable` 卡死——磁盘上有审批单，流水线状态却已经不在了。
2. **审批过期是被动发现的。** 审批单 5 分钟过期，但挂起的运行不会主动提醒或刷新；过期之后只有等你再点一次"允许"才会在核验时被拒绝，运行就一直停在那里。
3. **没有工具执行的自动重试或预算控制。** handler 执行失败，错误原文（脱敏后）直接喂回模型，跑不跑、跑几次全看模型自觉；单轮有"最多 32 步"的上限（`kiana-runner/src/harness.rs` 的 `max_steps_per_turn`），但单次工具没有重试机制。
4. **风险等级本质上是申请单自我申报。** shell 的风险只按沙箱档位粗分（`kiana-shell` 的注释自己承认"不能由此推断命令内部一定没有外部副作用"）；敏感操作靠操作名里含不含 payment/delete 等关键词的字符串启发式（`kiana-policy/src/lib.rs:is_sensitive_operation`），换个写法就可能漏网。
5. **授权编号只是本地关联标识，不是密码学凭证。** broker 和策略层的注释都明说：authorization_id"不是签名或可转移凭证"，授权的持久性、可验证性依赖于控制面和事件账本本身，而没有独立的防伪层。
6. **一次模型回合里的多个工具只能串行排队。** 这是刻意为之的串行安全设计，但代价是模型报了五个工具就要走五轮完整的"申请—审批—执行"。

## 相关文档

- `docs/company-os-overview.md`：相关词条见"公司部门对照表"（ControlPlane=行政审批中心、Capability Broker=采购/后勤部、Approval=审批）和"一次 run 的十步流程"第 ⑤–⑩ 步。
- `CURRENT_STATUS.md`：§1 状态与证明等级、§2 能力表（shell/apply_patch broker 主路径、trust/sandbox/role/policy 各行）、§4 高优先级未完成项中的 P1-02（审批精确指纹与续跑）、P1-03（取消与进程树）、P1-04（apply_patch 原子性）、P1-05（事件脱敏）、P1-06（MCP 风险边界）。
- `docs/features/` 兄弟文档：01（模型循环从这里开始，工具申请在它的第 7 步产生）、05（审批挑战的指纹、nonce、有效期等细节）、09（run 的生命周期视角）；全部篇目见 `docs/features/README.md` 索引。
- 想深挖可对照：`docs/company-os-security-constitution.md`（安全宪法）、`docs/company-os-design.md` §10（审批设计）。
