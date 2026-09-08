# 一次任务的完整一生（从 kiana run 到 Completed）

> 一句话：你敲下 `kiana run` 之后，这个任务在系统里怎么被创建、怎么一圈圈往前推、卡在审批上时算什么状态、被取消时哪些东西停得下来哪些停不下来、上下文太长时怎么瘦身、最后怎么走到 Completed / Failed / Cancelled。
> 本文写的是代码现在的真实样子；"做到什么程度"以 CURRENT_STATUS.md 为准。

## 这个功能是干什么的

本篇的姊妹篇（02《工具调用与授权脊柱》）讲的是一次工具调用的安全链；本篇往上一层，讲**整个任务**的生命周期。你在命令行或工作台里交给 Kiana 一句话，到拿回最终结果，中间是一个叫 run（运行）的对象在走完自己的一生。

run 的每一步状态都用 `ExecutionStatus`（`kiana-domain/src/lib.rs`）记录：`Accepted`（收下了）→ `Running`（模型循环在转）→ `AwaitingApproval`（有工具申请要你点头，整个 run 挂起）→ 回到 `Running` → 终点是 `Completed` / `Failed` / `Cancelled` 三选一，再加一个特殊的 `ResultUnknown`（结果不明——系统宁可存疑也不假装知道）。生命周期的大权在 ControlPlane（控制面，`kiana-core`）手里：它决定 run 什么时候能开始、挂起、恢复、取消，并把每个状态变化写进 EventLog。模型循环本身住在 kiana-runner 的 `KianaHarness` 里，ControlPlane 通过一条命令/事件通道（`RunnerCommand` / `RunnerEvent`，定义在 `kiana-runner-protocol`）跟它说话。

这篇要讲清楚四件事：run 的状态流转、continue（接着上次会话继续说话）的语义、cancel（取消）的语义和它停不干净的角落、以及对话历史太长时 compaction（上下文压缩）怎么工作。中间还会解释一个容易误解的概念：inbox，它是排队待处理的消息队列。

## 现在能干什么 / 不能干什么

**能**（每条有代码或 CURRENT_STATUS 出处）：

- run 的创建有一整套前置关卡：prompt 为空报 `prompt_required`、角色不存在报 `role_unknown`、角色和部门不匹配报 `role_department_mismatch`、沙箱档位未授权报 `workspace_write_requires_trusted_non_safe_profile` / `role_sandbox_read_only`——任何一关不过，run 根本不会诞生（`kiana-core/src/lib.rs` 的 `start_run_with_id` 和 `authorized_harness_sandbox`）。
- run_id 默认就是 session_id 本身（session_id 是 UUID 时直接解析成 RunId，`start_run_with_id` 开头），所以一个会话对应一个 run，`kiana run --receipt <会话ID>` 能事后查。
- 等审批是 run 的一等状态：模型申请的工具被判定需要人工审批时，整个 run 挂起、返回带审批单的 `AwaitingApproval` 响应；你批准之后，run 从挂起点继续往下跑（`kiana-core/src/lib.rs` 的 `drive_run` 的 `Ok(None)` 分支 + `resume_approved_invocation`）。
- continue 能在同一份对话历史上接着说话：校验是同一个主体（同一操作者、同一项目、同一角色和部门）之后，把新 prompt 追加为下一轮的用户消息，模型循环接着旧消息继续转（`kiana-core/src/lib.rs` 的 `continue_run`、`kiana-runner/src/harness.rs` 的 `continue_run`）。
- cancel 会做一串清理：把该 run 名下所有还在等待的审批单作废、发出取消信号、把 run 从 harness 内存里拿走，并核实 runner 回来的确认（`kiana-core/src/lib.rs` 的 `cancel_run`）。
- 取消正在执行中的工具是即时的：ControlPlane 在执行能力时用 `tokio::select!` 同时盯着取消信号，取消一到就不再等工具结果，记 `run.cancelled`（`broker_harness_capability` 里的 select 分支）。
- 对话历史超过阈值会自动压缩，并在事件流里留一条 `run.compacted` 记录压缩前后的 token 估算（`kiana-runner/src/harness.rs` 的 `model_step` + `kiana-runner/src/compact.rs` 的 `compact_if_needed`）。
- 单轮模型循环有 32 步上限（`max_steps_per_turn`，`harness.rs` 的 `RuntimeConfig::default`），防止模型无限打转。
- 终点判定宁可存疑：事后查收据时如果事件流里一个终点都没有、或者出现互相矛盾的多个终点，返回 `ResultUnknown` / `run_terminal_conflict`，不从残缺记录里推断出成功（`kiana-core/src/lib.rs` 的 `read_receipt`）。

**不能**（CURRENT_STATUS.md 明确记录，或代码里的明确边界）：

- **跨进程 continue 做不到。** 会话绑定（`sessions` 表）、对话历史（harness 内存里的 `ActiveRun`）、审批续跑信息（`pending_invocations`）、取消信号（`cancellations`）全部只活在进程内存里。每次 `kiana run` 都是一个新进程，新进程里 `sessions` 是空的，`kiana run --continue <id>` 会在 `resolve_run_id` 处被 `session_not_found` 拦下。continue 目前只在同一个 DaemonHost 一直活着的场景里可用，比如工作台（`kiana-entrypoints/src/workbench_chat.rs` 在同一进程里调 `continue_envelope_on_host`）。这就是 CURRENT_STATUS.md §2 把"跨进程完整 resume"标为 deferred 的具体含义。
- **取消停不干净的地方确实存在。** 正在跑的 shell 子进程树不保证随取消被清掉（CURRENT_STATUS.md §2 明确"仍需加强……进程树取消"）；取消之前已经执行的写盘不会回滚；取消一个已经跑完的 run 得到的不是"无事可取消"，而是 `ResultUnknown`（`result_unknown:run_not_found`，`kiana-core/tests/control_plane.rs` 的 `runner_not_found_cancel_is_unknown_and_retains_the_fencing` 断言了这个行为）。
- **compaction 不生成真摘要。** 压缩时丢掉全部 assistant 和 tool 消息，只留最近的用户消息，末尾贴一段固定摘要前缀——但摘要正文是写死的"(no summary available)"，并不调用模型去总结（`kiana-runner/src/compact.rs` 的 `build_compacted_history`）。
- token 估算按"4 字节 ≈ 1 token"粗算，对中文、代码和真实 tokenizer 误差可能很大，不能当作供应商请求一定不超限的保证（`compact.rs` 的 `approx_token_count` 注释原话）。
- inbox 的"下一步插话"机制（steer / inject）在 harness 里实现了，但入口层没有任何命令走到它——目前只有测试调用过（`kiana-runner/src/harness.rs:850` 附近的测试）。
- 一次模型调用失败（超时、断网、脚本播完）没有任何重试，整个 run 直接 Failed（`harness.rs` 的 `model_step` 错误分支；01 篇已详述）。
- `ExecutionStatus` 在 domain 层定义了状态迁移表（`can_transition_to`），但 ControlPlane 主路径没有逐处调用它做统一拦截——各状态实际是各分支直接构造出来的，迁移合法性靠的是"每个分支只产生合法终点 + 事后 `read_receipt` 的冲突检测"（代码里检索不到主路径调用，此条为现状观察）。

## 代码怎么跑（走读）

以工作台里输入一句话为例（`kiana run` 的路径相同，只是进程更短命），按时间顺序走。

**第 1 步：建 run，先过身份和授权关。**
请求到达 ControlPlane 后走 `kiana-core/src/lib.rs` 的 `start_run_with_id`。它先记一条 `request.accepted` 事件，然后按顺序检查：prompt 不能是空白（否则 `prompt_required`）、角色必须在花名册上、角色和部门要匹配、沙箱档位要授权——`authorized_harness_sandbox` 规定 workspace-write 需要项目受信、权限档位不是 Safe、且角色本身允许写。全过了记 `run.authorized`，run 这才算"出生"。为什么关卡都压在出生时？因为后面模型循环一转起来，每一步的授权都建立在出生时确定的身份和沙箱档位上，中途换挡是不允许的。

**第 2 步：先登记会话绑定和取消监听，再启动模型循环。**
正式发送 `RunnerCommand::start` 之前，`start_run_with_id` 做两件容易忽略的事：`remember_session` 把"这个 session 绑定这个 run"记进内存的 sessions 表（continue 和 cancel 都靠它找到 run），`watch_cancel` 装好取消信号监听。代码注释明确说这两件事必须发生在第一次模型调用之前，否则一次进行中的取消就没有办法中止正在执行的 shell。然后 `RunnerCommand::start` 发给 harness（`kiana-runner/src/harness.rs` 的 `start`）：它建一个 `ActiveRun`——对话消息列表、inbox、待执行工具队列、步数计数器——把系统指令变成第一条 system 消息，把你的 prompt 变成第一条 user 消息，发出 `Started` 事件，进入 `model_step`。

**第 3 步：model_step，一圈的内部动作。**
`model_step`（`harness.rs`）每圈做四件事，顺序固定：① 检查步数，超过 `max_steps_per_turn`（默认 32）就 Failed 结束本轮；② 把 inbox 里 NextStep 队列的消息取出追加为用户消息（这就是 steering 的消费点，虽然目前没有入口往里塞）；③ 调 `compact_if_needed` 检查历史长度，超了就压缩（下面单独讲）；④ 拿着全部消息和五个工具的说明书去问模型。模型回纯文字 → 记 `Delta` 事件，如果下一步没有待插话，发 `Completed` 收工；模型要调工具 → 调用排队进 `pending_tools`，只把第一张变成 `CapabilityRequested` 事件发出去（02 篇讲的链条从这里开始）。

**第 4 步：drive_run，ControlPlane 的事件处理循环。**
`kiana-core/src/lib.rs` 的 `drive_run` 是 ControlPlane 侧的循环：harness 每产出一个事件，它处理一个。`Started`/`Delta` 记事件；`CapabilityRequested` 送进 `broker_harness_capability` 走审批执行链，工具结果回来后把 runner 产生的新事件续进队列，循环继续转；`Completed` 记 `run.completed`；`Failed` 记 `run.failed`。三个出口值得注意：

- **要审批**：`broker_harness_capability` 返回 `Ok(None)`，`drive_run` 从 `pending_invocations` 里找到对应的待办，返回 `AwaitingApproval` 响应（带审批单）。此时对话历史和工具队列都还留在 harness 内存里，run 处于挂起状态。
- **失败**：按错误前缀分类——`run_not_found` 算 `Blocked`、`result_unknown:` 前缀算 `ResultUnknown`、`cancelled:` 前缀算 `Cancelled`、其余算 `Failed`。
- **既没成也没败**：事件队列空了却没有 `Completed`，报 `run_result_missing`，也是 `ResultUnknown`。宁可存疑，不假装成功。

**第 5 步：审批回来，run 在另一个请求里继续执行。**
你批准之后，`decide_approval_with_proof` → `resume_approved_invocation`（`kiana-core/src/lib.rs`）重新核验风险、执行能力、把结果用 `RunnerCommand::CapabilityResult` 发回 harness。harness 的 `on_capability_result` 先核对结果编号和排队里等的是不是同一个（不匹配直接 `capability_result_mismatch` 失败），把结果追加为 tool 消息，然后要么发出下一张工具申请、要么进入下一圈 `model_step`。注意一个细节：这次续跑发生在**审批决定那个请求**的处理过程中，`drive_run` 会再转一圈直到新的终点。你拒绝的话，runner 会收到 `approval_denied` 的失败结果，但由此产生的后续事件被丢弃、不再驱动——run 就此停在原地。

**第 6 步：收尾。**
正常完成时 `drive_run` 末尾做三件事：`clear_cancel` 撤掉取消监听、`remember_session` 刷新会话绑定（让后续 receipt/continue 找得到）、从 EventLog 汇总出收据（`run_receipt_from_store`）并记一条 `run.receipt` 事件，随 `Completed` 响应一起返回。

**重要分支：cancel 到底停了什么。**
`cancel_run`（`kiana-core/src/lib.rs`）按顺序做：① 找出该 run 名下所有还在 `pending_invocations` 里等着的审批，逐个 `invalidate` 并从待办里移除——任何一张作废失败，整个 cancel 以 `approval_invalidation_failed` 失败，不会留一张能用的审批单在半路；② `signal_cancel` 点亮取消信号——正在 `broker_harness_capability` 里执行的工具会立刻从 select 分支退出；③ 发 `RunnerCommand::Cancel`，harness 的 `cancel` 把 `ActiveRun` 从内存里整个拿走（`take_run` 是取出即删除），返回 `cancelled:<原因>`；④ ControlPlane 核实 runner 的回话：必须全是 `cancelled:` 前缀、没有 `Completed`、run_id 全部对得上，才记 `run.cancelled` 返回 `Cancelled`；任何一条不满足都降级为 `ResultUnknown`。停不下来的部分：已经提交给操作系统的子进程树（CURRENT_STATUS §2 承认进程树取消仍需加强）、已经落盘的文件修改、以及模型调用本身——如果取消到达时模型请求正在途中，harness 只是丢了 run 对象，在途的请求结果回来后不再有人消费它。

**compaction 怎么工作。**
每个 `model_step` 开头，`compact_if_needed`（`kiana-runner/src/compact.rs`）用"字节数 ÷ 4"估算整段历史的 token 数：没超过触发阈值（默认 32,000）就原样返回；超了就走 `build_compacted_history`——从后往前挑用户消息，装进 20,000 token 的保留预算，太老的用户消息和**全部** assistant/tool 消息丢弃，最后追加一条固定摘要消息。摘要前缀的措辞模仿 Codex（"另一个模型已经做了一部分工作，这是它的摘要"），但正文是写死的"(no summary available)"——所以压缩的本质是**有损截断加占位标记**，不是真的总结。压缩只改 harness 内存里的消息视图，EventLog 里的历史事件不受影响；ControlPlane 收到 `Compacted` 事件会记一条 `run.compacted`，带压缩前后的 token 数。

**inbox 是什么。**
`kiana-runner/src/inbox.rs` 的 `Inbox` 是 harness 里按时间边界分开的两个消息队列：`next_turn`（下一轮模型开始前消费，start 和 continue 的 prompt 都先经过它）和 `next_step`（当前这一圈的下一个 step 边界消费，给"干到一半插话"用）。它的 `claim` 语义是取出即删除：消息一旦被某个轮次领走，即使后续执行被拒，也不会自动塞回去重复处理。文件头注释明确说 inbox 只保存进程内的调度状态，不是 EventLog，也不是跨进程恢复的事实来源。如前所述，`next_step` 这条路目前只有机制、没有产品入口。

## 关键概念速查

| 概念 | 一句话解释 | 代码在哪 |
|---|---|---|
| run / RunId | 一次任务执行的完整生命周期；run_id 默认就是 session_id 解析成的 UUID | `kiana-core/src/lib.rs:start_run_with_id` |
| ExecutionStatus | run 的九个状态：Accepted / Denied / AwaitingApproval / Running / Completed / Failed / Cancelled / ResultUnknown / Blocked | `kiana-domain/src/lib.rs:ExecutionStatus` |
| SessionBinding / sessions 表 | "session ↔ run"的内存绑定，带操作者、项目、角色，continue 和 cancel 靠它找 run | `kiana-core/src/lib.rs:remember_session`、`resolve_run_id` |
| drive_run | ControlPlane 侧的事件处理循环：逐个处理 runner 事件，直到出现终点或挂起 | `kiana-core/src/lib.rs:drive_run` |
| ActiveRun | harness 内存里的一次运行实体：消息列表、inbox、工具队列、步数 | `kiana-runner/src/harness.rs`（结构体 `ActiveRun`） |
| Inbox | 按时间边界分开的待处理消息队列（next_turn / next_step），取出即删除 | `kiana-runner/src/inbox.rs:Inbox` |
| compaction | 历史超过约 32k token 时，只留最近用户消息加占位摘要的有损压缩 | `kiana-runner/src/compact.rs:compact_if_needed` |
| max_steps_per_turn | 单轮模型循环的 32 步上限，超出直接 Failed | `kiana-runner/src/harness.rs`（`RuntimeConfig::default`） |
| cancel watch | 每个 run 一份的取消信号（watch channel），工具执行时用它即时中断 | `kiana-core/src/lib.rs:watch_cancel` / `signal_cancel` |
| PendingInvocation | 审批挂起时记在内存的待办：审批通过后回到哪条流水线 | `kiana-core/src/lib.rs:pending_invocations`（02 篇详述） |
| ResultUnknown | 结果不明的终点：事件缺失、自相矛盾、取消确认不一致时的诚实答案 | `kiana-core/src/lib.rs:drive_run`、`read_receipt` |
| run_terminal_conflict | 事件流里出现多个互相矛盾的终点时收据查询返回的错误 | `kiana-core/src/lib.rs:read_receipt` |

## 设计视角：现在最明显的短板

以下是基于代码现状的观察，不是改进建议。

1. **run 的一生几乎没有一处是持久的。** 会话绑定、对话历史、审批续跑待办、取消信号、工具队列全在进程内存的 HashMap 里，只有 EventLog 和审批单落盘。进程一退，磁盘上有完整的事件流水，却没有一样东西能让 run 在新进程里接着跑——CURRENT_STATUS 把"跨进程完整 resume"标为 deferred，指的就是这组内存状态。
2. **取消的"干净"是分层的。** 审批单和 run 对象停得干净（作废、移除、核实确认），但已经交给操作系统的进程树和已经落盘的写操作不受取消影响；取消一个已结束的 run 返回 `ResultUnknown` 而不是"无事可取消"，行为正确但对你来说信息量偏低。
3. **compaction 是有损截断，不是摘要。** 全部 assistant 和 tool 消息（工具干了什么、模型推理过什么）在压缩时直接丢弃，摘要位置是一个写死的占位符；触发阈值和保留预算都建立在"4 字节 ≈ 1 token"的粗估上。对长任务的连续性来说，这是当前最大的上下文质量缺口。
4. **状态机有一半在纸面上。** `ExecutionStatus::can_transition_to` 在 domain 层定义了完整迁移表并配了测试，但主路径没有统一执行它；终点一致性依赖事后 `read_receipt` 的冲突检测兜底。防线存在，但位置靠后。
5. **每轮 32 步的预算花完即失败。** `max_steps_per_turn` 触发时整个 run 以 Failed 收场，模型没有机会输出一句收尾说明；步数也没有跨轮的总量预算，靠 continue 可以无限开新轮。

## 相关文档

- `docs/company-os-overview.md`：§2.2 全部入口一览（run / --continue / --cancel / --receipt 的用法）；§4 "一次任务的完整旅程（走读）"——本篇是那张十步流程图的生命周期细节版；§5 术语词典的 Run 词条。
- `CURRENT_STATUS.md`：§2 能力表的"跨进程完整 resume | deferred"（本篇 continue/cancel 边界的直接出处）、"CLI / Workbench / loopback Web 共用 DaemonHost | partial"（工作台里 continue/cancel 可用的原因）、"shell / apply_patch broker 主路径 | partial"（进程树取消仍需加强的出处）。
- `docs/features/01-model-execution.md`：模型循环的内部（model_step 的第 ④ 步、失败不重试、cassette 机制）。
- `docs/features/02-tool-call-spine.md`：姊妹篇——单次工具调用从申请到回执的安全链（本篇走读第 3–5 步只做了接口级描述）。
- `docs/features/05-approvals.md`：审批挑战的指纹、nonce、5 分钟有效期等细节。
- `docs/features/06-eventlog-receipts.md`：EventLog 与收据的落盘细节（本篇的终点判定依赖它的记录）。
