# 审批与人工确认（什么时候要你点头）

> 一句话：AI 干活遇到"有风险的动作"（写盘、对外请求等）时，系统会按下单、暂停，等你本人点头才继续。
> 本文写的是代码现在的真实样子，不是产品愿景；"做到什么程度"以 CURRENT_STATUS.md 为准。

## 这个功能是干什么的

Kiana 里的 AI 不是想干什么就干什么。每当它要做一个"有风险的动作"——往你的项目里写文件、调用外部服务（MCP，外部工具接口）——控制面（ControlPlane，所有请求必经的授权入口）会先把动作挂起，开一张审批单，等你确认。

审批的逻辑像下单后等你确认付款：不点确认，run 就一直停在 `AwaitingApproval`；审批单 5 分钟不确认就自动作废。

## 现在能干什么 / 不能干什么

**能：**

- 机器会主动暂停等你批准：凡是涉及密钥类操作、对外副作用（比如调用外部 MCP 服务）、"Critical"级高风险动作，一律暂停等审批（`kiana-policy/src/lib.rs:DefaultPolicyEngine::evaluate`）。
- "Safe"档（最保守的权限档位）下，往本地写文件也要审批；"Balanced"/"Autonomous"档位下本地写放行，但仍受角色和沙箱约束（同上）。
- 审批单落盘保存：写在 `~/.kiana/approvals/records.jsonl`（可用环境变量 `KIANA_HOME` 改位置），进程重启后单子还在，还带文件锁防止两个进程同时写坏（`kiana-daemon/src/approval_store.rs:persist_record`）。
- 一次批准只能用一次：批准被"消费"后，同一张单再拿来用会直接被拒（`kiana-daemon/src/approval_store.rs:consume_with_proof`）。
- 审批单 5 分钟不过确认就自动作废（`kiana-daemon/src/approval_store.rs` 开头的 `DEFAULT_APPROVAL_TTL` 常量）。
- 批准是"认人不认单"：会话、操作人、项目目录、信任状态、权限档位、角色、部门、路径白名单八项身份信息必须和开单时完全一致，差一项就作废（`kiana-daemon/src/approval_store.rs:consume_with_proof` 里的绑定比对）。
- 每张单带内容指纹（摘要，见下文），执行前重新验一遍；发现内容被改过，当场把这张单标记为已消费并报"完整性不符"（`kiana-daemon/src/approval_store.rs:consume_with_proof` 末段的 integrity 检查）。
- 等审批时把整次调用保存成 PendingInvocation，批准后原地恢复执行，不用从头再来（`kiana-core/src/lib.rs:resume_approved_invocation`）。
- 你拒绝也会通知到位：runner（真正驱动模型干活的引擎）会收到"被拒了"的结果，模型知道这事没成（`kiana-core/src/lib.rs:resume_approved_invocation` 的 Deny 分支）。
- 全程留痕：开单、批准、拒绝、暂停、续跑每一步都写进事件账本（`approval.requested`、`approval.approved`、`approval.denied`、`run.awaiting_approval` 等事件，`kiana-core/src/lib.rs:authorize_and_execute` 和 `broker_harness_capability`）。
- 命令型操作可以一键放行本地写盘：`kiana --approve-local-write ...` 只对"本地写文件"这一档风险自动代你点批准，其他风险照样要人（`kiana-entrypoints/src/command_dispatch.rs:should_auto_approve_local_write`）。
- `kiana run` 可以外接一个"问人的工具"（`--permission-prompt-tool`，比如通过 MCP 弹窗问你），问完把你的答案送回控制面（`kiana-entrypoints/src/harness_run.rs:run_envelope_with_history_and_permission_handler`）。

**不能：**

- 重启后批准也续不了跑：PendingInvocation 只放在内存里，进程一重启就没了。磁盘上的审批单虽然还在，但你再点批准，只会得到"续跑材料丢失"（`approval_continuation_unavailable`），不会真的把活捡起来（`kiana-core/src/lib.rs:decide_approval_with_proof` 开头的处理；CURRENT_STATUS.md §2 也明确"跨进程完整 resume"是 deferred，即"排到以后再做"）。
- 普通终端里没有"回头再批"的交互：`kiana run` 如果没接问人工具，收到"等审批"响应后就原样返回，那个任务就停在半路（`kiana-entrypoints/src/harness_run.rs` 的无 handler 分支）。
- 网页工作台（`kiana web`）目前没有审批按钮：我在 `kiana-entrypoints/src/web.rs` 和 `web_ui.rs` 里搜"approval"一个结果都没有，也就是说网页界面上看不到审批提示、也点不了批准（这是我搜索后确认的空白，不是推测）。
- 没有"批量批准"：每个动作一张单，连续改 10 个文件理论上就是 10 次确认，没有"这个会话里同类操作都放行"的选项。
- 没有任何通知机制：等审批不会推送提醒到你手机或别的窗口，过期了你也只有再去看才知道。
- 安检钩子（PreToolHook，工具执行前的最后一道自动检查）说"这个要问人"时，在无人值守的自动流程里不会被转成正式审批单，而是直接失败（`kiana-core/src/lib.rs:broker_harness_capability` 里 `hook_ask_unattended` 分支）。

## 代码怎么跑（走读）

先认识几个角色，下面会反复出现：

- **控制面（ControlPlane，在 `kiana-core`）**：所有请求必经之地，只有它能放行或拒绝。
- **策略引擎（PolicyEngine，在 `kiana-policy`）**：按规则给出结论：放行 / 要问人（Ask）/ 拒绝。
- **关卡（GateEngine，在 `kiana-gates`）**：第二道复核，只能把结论改严、不能改松。
- **审批台账（ApprovalStore，在 `kiana-daemon/src/approval_store.rs`）**：落盘保存审批单的存储层，带文件锁和原子写。
- **runner（在 `kiana-runner`）**：真正带着模型干活的引擎，自己不动手，每次要用工具都向控制面申请。

### 主路径：一次"等你点头"的完整过程

1. **你敲了 `kiana run "帮我改代码"`。** 模型开始干活，中途说"我要执行 apply_patch（改文件工具）改这些文件"。

2. **runner 不自己动手，先递申请。** runner 发出一个"要用工具"的事件（`RunnerEvent::CapabilityRequested`），控制面的 `kiana-core/src/lib.rs:broker_harness_capability` 接手。它先给请求盖身份戳、绑定工作单元（cell，这个任务专属的工作位置），然后记一笔"收到申请"的事件。

3. **策略引擎先过一遍。** `kiana-policy/src/lib.rs:DefaultPolicyEngine::evaluate` 按三层看：
   - 第一层：这个项目你信任（trust，即你明确声明过"这个目录可以干活"）吗？不信任连问都不用问，直接拒。
   - 第二层：角色（Builder/Reviewer 这些岗位）、部门、路径白名单对得上吗？对不上也是直接拒——这一层被拒的事，**人批准也救不回来**。
   - 第三层才轮到风险分级：只读放行；本地写文件在 Safe 档要问人；对外副作用（含一切 MCP 调用）和 Critical 高风险一律要问人；碰密钥类能力一律要问人。

4. **关卡确认"要问人"就挂起。** `kiana-gates/src/lib.rs:DefaultGateEngine::evaluate` 的铁律是只能收紧：规章说"要问人"，它就原样给出"等审批"（`GateDecision::AwaitingApproval`）；就算规章说放行但没给授权编号，它也按拒绝处理（防止出现无法追责的放行）。

5. **开审批单。** 控制面调 `kiana-daemon/src/approval_store.rs:stage` 开单。开单时干两件关键的事：
   - **算指纹（request_hash，摘要）**：把"要干的事的全部内容 + 当时的全部身份环境"拼成一份键排好序的标准 JSON，用 SHA-256（一种"内容变一点、指纹就完全不同"的算法）算出一串指纹（`kiana-daemon/src/approval_store.rs:capability_request_hash`）。之后任何人想把申请里的"改 A 文件"换成"删 B 目录"，指纹都对不上，当场暴露。
   - **绑定（binding）**：把会话、操作人、项目目录、信任状态、权限档、角色、部门、路径白名单这八项记在单子上，意思是"这张单只对当时这个人在这个项目里的这次申请有效"。

6. **单子落盘、激活、挂起现场。** 单子先落盘（写文件用的是"先写临时文件、再一步改名"的稳妥写法，外加文件锁，防止写一半断电留半个文件）。然后 `activate` 把单子从"已开"（Staged）转成"生效中"（Active）。接着控制面把这次调用保存成一个 `PendingInvocation`（定义在 `kiana-domain/src/lib.rs`，存放处是 `kiana-core/src/lib.rs` 里叫 `pending_invocations` 的内存表）——相当于存了个待办：run 编号、当时的请求内容、上下文快照、沙箱档位全都在里面，等着批准后原地恢复。最后控制面向上返回"暂停中、等审批"。

7. **怎么把问题递到你面前。** 三种场合：
   - `kiana run` 接了 `--permission-prompt-tool`：入口层把挑战（挑战就是那张单的摘要信息：单号、风险、指纹、过期时间、原因）打包递给问人工具，你答"允许"或"拒绝"（`kiana-entrypoints/src/harness_run.rs`）。
   - 命令型操作带 `--approve-local-write`：`kiana-entrypoints/src/command_dispatch.rs:execute_command` 看到挑战风险正好是"本地写文件"，就自动替你点批准——注意仅此一档，高风险照样停下来。
   - 什么都没接：响应原样返回，任务停着（见上面"不能"清单）。

8. **你点了批准。** 你的答案走协议信封（`kiana-protocol`）→ DaemonHost（`kiana-daemon/src/lib.rs`）→ 控制面的 `kiana-core/src/lib.rs:decide_approval_with_proof`。注意请求里带了**两样证据**：内容指纹和一次性 nonce（即审批单号本身）。

9. **逐项核验，一条不过就整体拒绝。** 台账的 `kiana-daemon/src/approval_store.rs:consume_with_proof` 逐条检查：单子存在吗？被用过吗？过期吗？被取消吗？状态正常吗？八项绑定信息全对吗？两样证据是成对出现的吗（只给一半按"证据不全"拒，而且**不会**把单子作废）？指纹对吗？nonce 对吗？最后再重新算一遍整份申请的指纹和单子上存的比。发现不一致——说明有人动过手脚——控制面立刻把这张单标记为已消费再报错，冒用者没有第二次尝试机会。全过，单子走"生效中→已批准→已消费"，到此作废。

10. **恢复执行。** 回到 `kiana-core/src/lib.rs:resume_approved_invocation`：控制面把保存的请求恢复、重新绑定工作单元（cell）、带上沙箱档，交给 capability broker 执行；结果记流水后，以"工具干完了，结果是这个"还给 runner，runner 接着往下跑，直到整个任务完成。你拒绝的话，runner 收到的是"approval_denied"的失败结果——模型知道被拒了，会另想办法或收工。

### 分支一：等审批的时候你取消了任务

任务等待期间你喊停（取消），控制面会把那张"生效中"的审批单作废掉（invalidate，状态转为 Cancelled）。之后就算有人再拿这张单来要求执行，也会在"已被取消"这一条上被拒。CURRENT_STATUS.md 记录了一个已知粗糙点：取消时如果作废动作本身报错，错误目前会被忽略。

### 分支二：程序重启了，你回来点批准

磁盘上的单子还在（未过期的会恢复"生效中"状态），但内存里的 PendingInvocation 已经没了。这时你点批准，控制面会先查待办：没有待办就**只验证、不消费**——单子不被消费掉，同时记一条"续跑材料丢失"的事件，返回"阻塞"状态。这个设计的意思很清楚：宁可让这张单悬着，也不假装续跑成功。

## 关键概念速查

| 概念 | 白话解释 | 代码在哪 |
|---|---|---|
| 审批挑战（ApprovalChallenge） | 开给审批人看的摘要信息：单号、风险等级、指纹、过期时间、原因 | `kiana-domain/src/lib.rs` |
| 指纹（request_hash / digest） | 申请内容+身份环境的 SHA-256 内容指纹，防止批准被挪用到别的请求上 | `kiana-daemon/src/approval_store.rs:capability_request_hash` |
| 绑定（ApprovalBinding） | 开单时记录的八项身份信息，使用时必须完全一致 | `kiana-daemon/src/approval_store.rs` |
| 一次性 nonce | 实际就是审批单号本身，只能用一次 | `kiana-daemon/src/approval_store.rs:stage` |
| 待办（PendingInvocation） | 等审批时保存的整次调用快照，批准后原地恢复执行 | 定义在 `kiana-domain/src/lib.rs`；存放在 `kiana-core/src/lib.rs:pending_invocations` |
| 审批单的一生（ApprovalState） | 已开→生效中→已批准→已消费；另有过期/取消/拒绝三个终点 | `kiana-domain/src/lib.rs:ApprovalState` |
| 审批台账（ApprovalStore） | 落盘的审批存储：`~/.kiana/approvals/records.jsonl`，带文件锁和原子写 | `kiana-daemon/src/approval_store.rs` |
| 有效期（TTL） | 审批单开出 5 分钟内有效，写死在代码里 | `kiana-daemon/src/approval_store.rs:DEFAULT_APPROVAL_TTL` |
| 安检钩子（PreToolHook） | 放行之后、执行之前的最后一道自动检查，可以说"拦下"或"要问人" | `kiana-daemon/src/pre_tool_hooks.rs` |
| 一键放行本地写（--approve-local-write） | 命令型操作里，仅"本地写文件"档的挑战自动代批 | `kiana-entrypoints/src/command_dispatch.rs:should_auto_approve_local_write` |

## 设计视角：现在最明显的短板

- **PendingInvocation 只在内存里。** 重启之后磁盘单还在、人也能点批准，但活续不上，只能拿到"续跑材料丢失"。跨进程恢复在 CURRENT_STATUS.md 里是明确排到以后的。
- **等审批完全靠人盯着。** 没有任何推送、提醒或轮询接口；普通终端场景下任务停在半路，网页工作台连审批按钮都还没有（我在 web 相关入口文件里搜索确认过没有 approval 相关代码）。
- **没有批量授权。** 粒度是"一次一单"，一个任务里连续多次写盘就可能多次打断你，没有"同类操作本次会话内放行"的机制。
- **保质期写死 5 分钟且无提示。** 你要是离开十几分钟，单子必然过期，而且过期没有通知，只能重试时才知道。
- **PreToolHook 的"要问人"和审批系统没打通。** 无人值守时钩子的提问请求直接把这次执行判失败（`hook_ask_unattended`），而不是转成正式审批单等人，两条"要人确认"的通路目前是断开的。

## 相关文档

- `docs/company-os-overview.md`：词条"行政审批中心（ControlPlane）"、"Approval"（§10 有设计说明）、"PendingInvocation"。注意：overview 里把 PendingInvocation 标成"目标设计，尚未完成"，但代码里同进程内的"批准后原地续跑"已经实现（`kiana-core/src/lib.rs:resume_approved_invocation`），没实现的只是跨进程恢复——本文以代码为准。
- `CURRENT_STATUS.md`：§2 能力表中"跨进程完整 resume：deferred"一行；P1-04（审批台账临时文件防碰撞加固，2026-09-07）；"cancel_after_awaiting_approval"相关证据块（等审批时取消的回归测试）；"durable PendingInvocation/cancellation reconciliation"在 limitation 里反复出现。
- `docs/features/01-model-execution.md`：兄弟篇，讲模型执行主链路——审批是插在这条链路中间的一环。
