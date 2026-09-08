# 角色、部门与任务流程（规划 → 施工 → 验收 → 结案）

> 一句话：这个功能把"一家公司怎么干活"的分工搬进 Kiana——五种角色各管一段，任务以工单（WorkPacket）在部门之间交接，每一步都留下正式文件。
> 本文写的是代码现在的真实样子；"做到什么程度"以 CURRENT_STATUS.md 为准。

## 这个功能是干什么的

前面几篇讲的是"一个 AI 会话怎么干活"：怎么选模型、怎么申请工具、怎么留痕。这一篇讲的是"公司怎么组织这批员工"：Kiana 在领域层定义了五个部门（Department）和六个角色（Role），一次完整的工作要沿着"立项 Sponsor → 规划 PM/Architect → 施工 Builder → 验收 Reviewer → 结案 Closer"这条流水线走，每个角色能碰什么工具、能写哪些路径、能读哪些记忆，都是写死在代码目录（catalog）里的，不是模型自己说了算。

核心动作有四个，每个对应一条 CLI 命令：

1. **规划会（Symposium）**：`kiana run --symposium`，PM 主持、Architect 参加的有界讨论，产出决策文件 `plan/DECISION.json` 和工单 `packet/TASK.json`。
2. **施工（packet）**：`kiana run --packet packet/TASK.json`，一个全新的 Builder 会话按工单干活——它不继承规划会的任何对话上下文，只看得到工单文本。
3. **验收（review）**：`kiana run --review <作者会话ID>`，一个独立的 Reviewer 会话检查 Builder 的产出，写 `gate/REVIEW.json`。
4. **结案（close）**：`kiana run --close <作者会话ID>`，Closer 核对验收和合并凭证，写收尾记录进 `lessons/`。

为什么要费这么大劲拆角色？最关键的一条是**利益隔离**：Builder 不能列席规划会，因为一个能自己给自己派活、再自己给自己验收的员工，等于没有监督。这条规则不是提示词里"请自觉"的一句话，而是 `kiana-core` 里真实存在的拒绝判断——参会名单里出现 Builder，会议直接不成立。类似的隔离还有：Reviewer 永远不能是作者本人（会话 ID 相同就拒绝）、Reviewer 和 Closer 必须是全新的会话（用过的会话不能再去验收）。

## 现在能干什么 / 不能干什么

**能**（每条有代码或 CURRENT_STATUS 出处）：

- 六个角色（Sponsor / PM / Architect / Builder / Reviewer / Closer）和五个部门（initiating / planning / executing / monitoring / closing）的定义、白名单全部写死在领域目录里：`kiana-domain/src/lib.rs` 的 `RoleSpec::catalog`（六个 `RoleSpec` 构造函数）和 `DepartmentSpec::catalog`。
- 每个角色三重白名单真正生效：工具白名单（`kiana-policy/src/lib.rs` 的 `role_decision`，Builder 之外的工具名直接报 `role_tool_denied`）、路径白名单（`RoleSpec::allows_path`，PM 只能写 charter/plan/packet，Closer 只能写 lessons）、记忆集合白名单（`RoleSpec::allows_knowledge` / `allows_memory_write`，比如 Builder 只能写 instance-scratch 这一层）。
- 规划会真的会把 Builder 挡在门外：参会名单与部门不符或出现 Builder，`Symposium::validate` 报 `symposium_builder_not_attendee` / `joint_symposium_frozen`；控制面在 `kiana-core/src/lib.rs` 的 `convene_symposium` 里同样再查一遍。
- 规划会产出落盘：`kiana-core/src/lib.rs` 的 `write_symposium_artifacts` 写 `plan/DECISION.json`（决策）和 `packet/TASK.json`（工单），并且这套写文件的方式经过过 symlink/hardlink 攻击的回归测试（CURRENT_STATUS §4 的 S3 governance-artifact boundary 条目）。
- Builder 拿工单开全新会话：`kiana-core/src/lib.rs` 的 `spawn_from_packet` 校验工单、抢路径锁、走完 Draft→Approved→Assigned→Running 状态机，然后调用 `start_run_with_id` 时**传空的对话历史**（`Vec::new()`），注入的提示就是 `WorkPacket::as_prompt` 生成的工单文本——规划上下文物理上进不来。
- 工单本身有硬校验：`WorkPacket::validate` 要求承办角色必须是 Builder、部门必须是 executing（`packet_role_must_be_builder`），工单状态不能从终态再启动。
- Review 是"作者本人不得自审 + 会话必须全新 + 作者必须是 Builder"三道硬门：`kiana-core/src/lib.rs` 的 `review_author_run` 分别报 `review_author_session_denied`、`review_session_not_fresh`、`review_author_must_be_builder`。评审通过还自动生成合并凭证 MergeReceipt 写盘。
- Closer 结案前逐项核对：验收文件作者对得上、裁决是 pass、合并凭证的 run/会话/文件清单三处一致，任何一处不符都拒绝（`kiana-core/src/lib.rs` 的 `close_author_run`，报 `close_review_not_accepted` / `close_merge_receipt_mismatch` 等）。
- 会话内角色不可换：一个会话一旦以某角色启动，continue / cancel / 查收据都不能换成别的角色或部门（CURRENT_STATUS §4 P1-01，测试 `session_role_and_department_are_immutable_for_continue_cancel_and_receipt`）。
- `--role` 旗子可以指定角色，但只能"确认"不能"越权"：规划会只认 PM、工单只认 Builder、评审只认 Reviewer，配错了直接报错（`kiana-entrypoints/src/harness_run.rs` 的 `symposium_envelope` / `spawn_envelope` / `review_envelope`）。

**不能**（出处是 CURRENT_STATUS.md 的明确记录，或代码里的缺口）：

- **整条流水线的现状是"代码在、局部测过，没到产品级验证"**：CURRENT_STATUS §2 能力表把 "WorkPacket / Symposium / Review 领域对象" 标为 partial + local_behavior，并注明"现有 schema 较窄，独立 Reviewer 和完整生命周期尚未完成"。
- **Review 不看代码**：`review_author_run` 里没有任何模型调用。裁决是从事件里数 Builder 改了哪些文件——改了就算 pass，没改就算 `needs_change`（`files_changed_from_events` + 后面的 verdict 分支）。Reviewer 的角色定义里写了"对照验收标准"，但流程里没有一步真的把代码交给谁去读。
- **Sponsor 没有入口**：立项部门、Sponsor 角色、`charter/DECISION.json` 路径都定义了，但 entrypoints 里没有任何命令路径构造 Sponsor（grep 整个 entrypoints 只有 Closer 被命令用到）；`--role sponsor` 能挂在普通 run 上，但"正式立项"这件事没有产品入口。
- **部门配置里的 gate 名没人消费**：`DepartmentSpec` 每个部门都声明了必须通过的 gate（如 planning 的 `packet_has_acceptance`），但 `kiana-gates` 只做策略结果的收敛转换，全仓库没有任何代码按这些名字执行检查——它们目前只是目录里的描述文字。
- **结案的经验总结是模板生成的**：`write_closing_artifact` 写进 `lessons/LEARNED.md` 的内容是固定文案加文件清单，没有模型参与，也没有读取 Builder 实际过程。
- **Closer 的 "lessons_logged" 这类完成条件同样没有强制执行**（见上一条 gate 名的现状）。
- 真模型仍然是 not_supported（CURRENT_STATUS §2），所以这条流程今天实际跑的是 cassette/fake-script；"多角色协作"的效果没有在真实模型上验证过。

## 代码怎么跑（走读）

以一次完整的"规划 → 施工 → 验收 → 结案"为例。角色目录（RoleSpec）先交代一句：它是策略输入而不是授权令牌（`RoleSpec` 结构体注释原话），真正放行与否还要跟部门、工单、审批、当前请求范围求交集——所以下面每一步你都会看到"角色说了算一半，ControlPlane 说了另一半"。

**第 1 步：你敲 `kiana run --symposium --anti-meeting --sandbox workspace-write --json -- "目标"。**

`kiana-entrypoints/src/cli.rs` 的参数解析认出 `--symposium`（还有 `--anti-meeting`，`anti_meeting_requires_symposium` 保证它不能单独出现），生成一个全新的随机会话 ID，调 `kiana-entrypoints/src/harness_run.rs` 的 `symposium_envelope`。这里做两件事：如果 `--role` 给了但不是 pm，直接报 `symposium_chair_must_be_pm`；然后无条件把请求角色定为 PM（`metadata.assign_role(&RoleSpec::pm())`）。也就是说，谁主持规划会不是参数决定的，是命令本身决定的。

**第 2 步：ControlPlane 主持资格审查。**

请求经 `kiana-client` 到 `kiana-daemon::DaemonHost`，再进 `kiana-core::ControlPlane` 的 `convene_symposium`。它依次查：议程非空（`symposium_goal_required`）、角色存在且能主持（`symposium_chair_cannot_convene`）、规划会必须 workspace-write 沙箱（`symposium_requires_workspace_write`，因为 PM 要写文件）。都过了才构造 `Symposium` 对象。参会名单不自由组合——`Symposium::department` 直接取部门目录里的角色清单，规划部就是 [pm, architect]，Architect 是只读顾问（工具只有 memory.search、沙箱 read-only）。

**第 3 步：开会，或者跳过会。**

不开 `--anti-meeting` 时，控制面按轮数（默认 4 轮，上限 8）让每个参会角色轮流发言。注意每个发言人是一个**独立的完整 run**：`convene_symposium` 给每个人克隆一份上下文、分配 `{会话ID}-{角色}` 的新会话、套上 Safe 权限档，第一轮 `start_run`、后续轮 `continue_run`，发言文本贴到黑板（Blackboard）上供下一个人看。`--anti-meeting` 则跳过全部发言，直接拿议程当决策。无论开不开，`Symposium::close` 都会校验名单（Builder 出现即 `symposium_builder_not_attendee`，跨部门混编即 `joint_symposium_frozen`），然后产出 `DecisionRecord` 和一张默认工单。

**第 4 步：产物落盘。**

`write_symposium_artifacts` 把决策写进 `plan/DECISION.json`、工单写进 `packet/TASK.json`（`kiana-domain` 的 `DECISION_RECORD_PATH` / `WORK_PACKET_PATH` 常量）。写法走 `write_project_artifact` 的安全通道（目录描述符校验、no-follow、原子重命名，见 CURRENT_STATUS §4），防的是"产物路径被符号链接偷换"这类攻击。失败则整个规划会以 `symposium_artifact_write_failed` 阻断，不会"会开完了但文件没写"地假装成功。

**第 5 步：Builder 拿工单开工，不继承任何对话历史。**

`kiana run --packet packet/TASK.json` 走 `harness_run.rs` 的 `spawn_envelope`：先读文件、反序列化、跑 `WorkPacket::validate`（角色必须 Builder、部门必须 executing）。然后 `ControlPlane::spawn_from_packet` 接手：会话必须是全新的（`spawn_session_not_fresh`）、状态可启动、接着抢 Builder 路径锁（`acquire_builder_path_locks`，两个并行 Builder 的路径不许重叠，这是多会话并发时的防撞机制），把工单的 `path_allow` 装进请求上下文——后面策略层会把它叠加在角色白名单之上求交集（`kiana-policy/src/lib.rs` 的 `role_decision` 里 `packet_path_denied` 分支）。最后调 `start_run_with_id`，传入的历史是空的，提示词是 `WorkPacket::as_prompt()` 生成的工单文本。**这就是"Builder 不继承规划上下文"的实现位置**：不是靠提示词叮嘱，而是历史向量根本没有内容可继承。工单状态机在此自动推进 Draft→Approved→Assigned→Running，并各记一条事件。

**第 6 步（失败分支）：Builder 越界会怎样。**

Builder 干活时每次工具申请都过 `role_decision`：工具不在 Builder 五件套里 → `role_tool_denied`；要写的路径不在角色白名单或工单 path_allow 里 → `role_path_denied` / `packet_path_denied`。这些拒绝属于策略层的硬拒绝，人工批准也救不回来（详见审批篇）。注意角色定义里有一句"Never request danger-full-access"，但它只是提示词；真正挡住的是沙箱档位和策略交集。

**第 7 步：独立 Reviewer 验收——但看的是事件，不是代码。**

`kiana run --review <作者会话ID>` 走 `review_envelope` → `ControlPlane::review_author_run`。三道身份门依次过：作者会话 ID 非空、当前会话不等于作者会话（`review_author_session_denied`）、当前会话没用过（`review_session_not_fresh`）。然后 `events_for_author` 从 EventLog 里捞出该作者在本项目、本主体名下的全部事件，确认有 `run.completed` 且角色是 Builder。裁决逻辑很短：`files_changed_from_events` 从事件里提取改动文件清单，清单非空就是 `pass`，空就是 `needs_change`。通过则生成 `ReviewPacket` 写进 `gate/REVIEW.json`，并顺手写一张 MergeReceipt 合并凭证。整个函数没有一次模型调用。

**第 8 步：Closer 结案，核对凭证而不是执行。**

`kiana run --close <作者会话ID>` 走 `ControlPlane::close_author_run`：Closer 角色校验、会话全新、作者事件存在且完成，然后逐项核对 `gate/REVIEW.json`（作者对得上、裁决 pass）和合并凭证（run ID、会话、文件清单三处一致且 accepted）。全对才生成 `ClosingReceipt` 落盘，并把模板化的经验总结写进 `lessons/LEARNED.md`，记 `closing.completed` 事件。任何一处对不上，报 `close_review_not_accepted` / `close_merge_receipt_mismatch` 之类的错误拒绝结案。

## 关键概念速查

| 概念 | 一句话解释 | 代码在哪 |
|---|---|---|
| RoleSpec | 一个角色的权限快照：提示词、工具白名单、沙箱档位、路径白名单、记忆授权、能否主持/投票、步数上限 | `kiana-domain/src/lib.rs` 的 `RoleSpec` 及六个构造函数 |
| DepartmentSpec | 一个部门的职责、角色清单、可触达产物路径、声明的 gate | `kiana-domain/src/lib.rs` 的 `DepartmentSpec::catalog` |
| Symposium | 有边界的多角色讨论：固定议程、限轮数（默认 4，上限 8）、名单锁死在部门内 | `kiana-domain/src/lib.rs` 的 `Symposium`、`kiana-core/src/lib.rs` 的 `convene_symposium` |
| Blackboard | 会议发言的公共面板：每人的主张（claim）贴上去，后续发言人可见 | `kiana-domain/src/lib.rs` 的 `Blackboard` / `SymposiumClaim` |
| WorkPacket | 工单：目标、输入、依赖、可写路径、验收标准、禁做事项，Builder 的唯一正式输入 | `kiana-domain/src/lib.rs` 的 `WorkPacket`、`WorkPacket::as_prompt` |
| `plan/DECISION.json` / `packet/TASK.json` | 规划会的两份产物：决策记录 + 工单 | `kiana-domain` 常量 `DECISION_RECORD_PATH` / `WORK_PACKET_PATH`，写在 `kiana-core/src/lib.rs` 的 `write_symposium_artifacts` |
| `gate/REVIEW.json` | Reviewer 的验收文件：裁决（pass / needs_change）、审过的文件清单 | `kiana-domain` 常量 `REVIEW_PACKET_PATH`，写在 `review_author_run` |
| MergeReceipt | 验收通过时自动生成的合并凭证，结案时用来对账 | `kiana-core/src/lib.rs` 的 `review_author_run` 内构造 |
| ClosingReceipt | 结案凭证：把作者 run、评审会话、Closer 会话、文件清单钉在一起 | `kiana-core/src/lib.rs` 的 `close_author_run` |
| `--anti-meeting` | 跳过辩论轮，直接拿议程出决策和工单 | `kiana-entrypoints/src/cli.rs` 参数解析 + `Symposium::close(skipped_meeting)` |
| 路径锁 | 同一 ControlPlane 上并行 Builder 的工单路径不许重叠，防并发越权 | `kiana-core/src/lib.rs` 的 `acquire_builder_path_locks` |
| joint_symposium_frozen | 跨部门混编会议的固定拒绝码：参会者必须全部属于同一部门 | `kiana-domain/src/lib.rs` 的 `Symposium::validate` |

## 设计视角：现在最明显的短板

1. **验收的"独立"体现在身份上，没体现在判断上。** Reviewer 确实是一个全新会话、不可能是作者，但它的裁决只是"事件里有没有文件改动清单"（`review_author_run` 的 verdict 分支）：改了就 pass。角色定义里承诺的"对照验收标准"没有任何执行路径，工单里写的 acceptance 列表也没有被逐条核对。
2. **部门 gate 名是装饰。** `DepartmentSpec` 声明了每个部门的完成门槛（`packet_has_acceptance`、`receipt`、`lessons_logged`），但全仓库没有消费方——`kiana-gates` 只做策略结果到 Gate 决定的转换。也就是说"这个部门必须过什么关"目前只写在目录里，不写在执行里。
3. **流水线的头和尾是断的。** Sponsor 没有任何命令入口（charter 立项产物定义了路径但没人写），Closer 的经验总结是写死的模板文案加文件清单。"立项 → 结案"的首尾两端今天只是领域对象，不是产品能力。
4. **角色是本地自我申报的。** `--role` 只是 CLI 请求里的一个字段，没有独立身份认证；CURRENT_STATUS §2 明说"固定本地主体、role/department assignment 和持久身份仍未完成"。P1-01 已经把"会话内角色不可换"做实了，但"谁有资格领这个角色"没有答案——这在本机单人场景下可接受，也是当前边界本身。
5. **会议发言的成本模型很重。** 每个发言人每轮是一个完整 run（独立会话、独立模型调用），4 轮 × 2 人就是 8 次；而发言产出的黑板文本质量完全依赖模型（今天实际是 cassette），规划会的价值还没被真实模型检验过。
6. **规划会强制 workspace-write 但发言线程套 Safe 档。** 主持上下文要求 workspace-write（`symposium_requires_workspace_write`），发言克隆的上下文却设成 Safe 权限档（`convene_symposium` 内部），两条规则并存，为什么这样组合在代码里没有注释说明（未在代码里确认其意图）。

## 相关文档

- `docs/company-os-overview.md`：§2 命令表的规划会/工单/评审/结案四行；§4.4 "规划版旅程：五个部门轮流上场"；§5 术语表的 Sponsor、Department、WorkPacket、Symposium 词条。
- `CURRENT_STATUS.md`：§2 能力表 "WorkPacket / Symposium / Review 领域对象 | partial | source/local_behavior" 与 "基础 trust / sandbox / role / path / memory policy" 行；§4 高优先级未完成项 P1-01（会话角色/部门不可变）、P3-02（Planner → fresh Builder → independent Reviewer → Closer 全链）。
- `docs/features/01-model-execution.md`：模型与会话从哪来——本文每个发言人和 Builder 都是一次它描述的 run。
- `docs/features/02-tool-call-spine.md`：角色工具白名单在哪条链上生效（`role_decision` 是它讲的"申请—审批—执行"流水线的第一道身份关）。
- `docs/features/04-trust-sandbox-path.md`：trust / sandbox / 路径白名单的细节，与本文的工单 path_allow 直接衔接。
- `docs/features/05-approvals.md`、`docs/features/06-eventlog-receipts.md`：审批与留痕——Review 读事件、Closer 对账，依赖的都是 06 篇写的 EventLog。
