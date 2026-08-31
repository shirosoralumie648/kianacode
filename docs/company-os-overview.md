# Kiana CompanyOS 白话总览（入门导读）

> 文档性质：入门导读（Informative），**不是规范**。
>
> 本文只负责"把事情讲明白"，不定义任何字段、状态机或安全条款。所有术语的正式定义以链接指向的规范文档为准；本文与规范冲突时，以规范为准。
>
> 当前真实能力以 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 为准；可运行的命令以 [`../USER.md`](../USER.md) 为准。

---

## 1. Kiana 是什么

**一句话：Kiana 是一个跑在你自己电脑上的"AI 打工系统"——你当老板布置任务，AI 员工在受控范围内干活，每一步都留下可以查账的记录。**

它和普通"聊天机器人 + 一堆工具"的区别在于：

- 聊天机器人是"说到哪算哪"：它说做完了，你只能信；
- Kiana 要求每次干活都像正规公司接单一样，有四样东西：
  1. **明确的任务**——写成工单（WorkPacket），说清楚要做什么、能动哪些文件、怎么算验收通过；
  2. **明确的权限**——干活前先领"出入证"（Grant），只批需要的、只批一阵子，下级权限永远不能比上级大；
  3. **明确的验收**——干完由另一个不相干的角色（Reviewer）来检查，作者不能自己给自己打分；
  4. **明确的记录**——每个动作写进不可篡改的流水账（EventLog），最后汇总成收据（Receipt），事后随时能查"谁、在哪、干了什么、动了哪些文件"。

"CompanyOS"（公司操作系统）这个名字的意思是：**把"一家正规公司怎么管理工作"的那套方法——立项、规划、执行、监督、收尾——搬进 Agent 系统**。这就是为什么文档里到处都是公司术语（部门、工单、审批、验收、收据）。这不是比喻装饰，而是系统真实的运行结构。

---

## 2. 现在真的能做什么（诚实版）

这个仓库有一条铁律：**不许把"想做的"说成"做完的"**。当前整体证明上限是 `local_behavior`，白话翻译：

> 目前所有能力只在"你自己这台电脑 + 你明确信任（trust）过的本地项目 + 录好的模型脚本（cassette/fake-script）"这个条件下验证过。**还没有接入真实的在线模型服务**（live provider 标记为 `not_supported`），所以现在的 Kiana 更像一台"传动系统装好了、发动机还没接的车"——控制面、审批、沙箱、收据这些机制都在真实运转，但驱动它们的"模型"目前来自录好的脚本。

### 2.1 五分钟上手（可以照抄的顺序）

```bash
# ① 先看一遍系统端到端跑通的样子（自带 cassette 模型脚本，不需要任何配置）
bash scripts/harness-golden-smoke.sh

# ② 信任当前项目（一次性；没有这步，这个目录里任何写盘都会被拒绝）
kiana trust .

# ③ 打开文件夹工作台（TTY：对话区 + 输入框 + 状态行；工作台默认 sandbox 是 workspace-write）
kiana
#    输入需求后回车；Esc 或 Ctrl-C 取消；/sandbox 查看或切换档位；/quit 退出

# ④ 或者跑一次性任务（注意：kiana run 默认只读，写盘必须显式 --sandbox workspace-write）
kiana run --sandbox workspace-write -- "create GOLDEN_PATH.txt containing hello"

# ⑤ 查收据：这个会话调了什么工具、改了哪些文件
kiana run --receipt <会话ID>
#    会话ID 从哪来：--json 输出里的 session_id 字段；工作台的状态行也一直显示 session
```

**预期管理**：因为还没接真实在线模型，第 ③④ 步在没有配模型脚本（环境变量 `KIANA_HARNESS_SCRIPT=/path/to/script.json`）的环境里会**显式失败**（报 `model_unavailable`），而不是假装成功——这不是坏了，这正是"失败可见、拒绝假成功"设计的日常体现。想看"真的写出文件"的完整闭环，第 ① 步的冒烟脚本就是官方演示。

### 2.2 全部入口一览

详细用法和更多脚本见 [`../USER.md`](../USER.md)：

| 命令 | 干什么 | 你会得到什么 |
|---|---|---|
| `kiana trust .` | 信任当前项目 | 之后这个目录才允许写盘（未信任时一次性任务报 `workspace_write_requires_trusted_non_safe_profile`，工作台会先问你） |
| `kiana` / `kiana --workdir <路径>` / `kiana --pick-folder` | 文件夹工作台 | TTY 对话区+输入框+状态行；默认 workspace-write |
| `kiana run [--sandbox workspace-write] -- "..."` | 一次性任务（默认只读） | 终端结果；加 `--json` 得到机器可读输出（含 `session_id`） |
| `kiana run --symposium --anti-meeting --sandbox workspace-write --json -- "..."` | 规划会：PM + Architect 有界讨论（`--anti-meeting` = 跳过辩论轮直接出产物） | `plan/DECISION.json`（决策）+ `packet/TASK.json`（工单） |
| `kiana run --packet packet/TASK.json --sandbox workspace-write --json` | 全新 Builder 按工单干活 | 工单范围内的文件变更；新的会话ID |
| `kiana run --review <作者会话ID>` | 独立评审（确定性门，不跑模型） | `gate/REVIEW.json` |
| `kiana run --close <作者会话ID>` | 收尾部 Closer 归档这单活的经验 | 收尾记录（lessons） |
| `kiana run --continue <会话ID> -- "..."` | 同一 daemon 存活期内接着聊 | 同一会话继续；daemon 重启后续跑会显式失败 |
| `kiana run --cancel <会话ID>` | 取消进行中的运行 | 停止；取消不存在的会话会显式失败 |
| `kiana run --receipt <会话ID>` | 查收据 | 工具调用与改过文件（`files_changed`）清单；**跨重启仍可查** |
| `kiana web --no-open --bind 127.0.0.1:3080` | 浏览器工作台 | 只监听本机；同一个 DaemonHost，不是另一套系统 |
| `bash scripts/install-desktop.sh` → `kiana-desktop` | Electron 桌面壳（需要 Node/npm） | 欢迎页选工作区；壳里承载的还是同一个 Web 工作台 |

除了入口，这些机制也已经打通（同样只到 `local_behavior`，出处见 [`coding-pack-matrix.md`](coding-pack-matrix.md)）：

- **stdio MCP 外部工具**：用 `KIANA_MCP_SERVERS_JSON` 配置本地 MCP server；HTTP 形式明确拒绝（`mcp_transport_unsupported`）；
- **Skill 注入**：受信项目的 Skill 会进入工人的系统提示；未信任项目的 Skill 被扣下；
- **PreToolUse 钩子**：能在工具真正执行前拦截（`hook_blocked`）；
- **六层记忆**：模型可用 `memory.search` / `memory.write`，带角色/部门 ACL，命中会进收据；
- **超预算压缩**：长对话超预算自动 compact，收据里有 `compact.applied` 记录；
- **并行 Builder**：同一核上两个 packet Builder 并行，路径锁防越权（`packet_path_denied`）。

### 2.3 现在明确不能声称的

逐项状态见 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) 的状态表：

- 没有接入真实在线模型（live provider）和逐字流式输出（token streaming）；
- 运行不能跨 daemon 重启"续跑"（收据和评审可以跨重启**查询**，但"接着干活"不行）；
- 没有生产级安装包、签名、自动升级（安装演示用临时目录，见 [`../USER.md`](../USER.md)）；
- 没有支付、外卖、打车、订票、智能家居——这些连接口都还没有，全部标记 `not_supported`；
- 没有企业多租户、远程协作。

---

## 3. 心智模型：一家被严格管理的公司

把下面这张对照表记住，读所有规范文档都会轻松很多：

| 公司里的角色/东西 | Kiana 里的对象 | 白话解释 |
|---|---|---|
| 老板 | **Sponsor** | 唯一的人类决策者：定目标、给预算、批高风险操作、最终验收 |
| 行政审批中心 | **ControlPlane**（控制面） | 所有事情的必经关卡：查身份、查权限、查预算，批了才能干（在 `kiana-core`） |
| 总装配车间 | **DaemonHost** | 把控制面、策略、事件账本、执行器拼装起来的"组合根"，所有入口（CLI/Web/桌面）共用这一个（在 `kiana-daemon`） |
| 五个部门 | **Department** | 立项部（Initiating）、规划部（Planning）、执行部（Executing）、监控部（Monitoring）、收尾部（Closing） |
| 岗位说明书 | **RoleSpec / AgentTemplate** | 规定一个 AI 员工能用什么工具、能碰哪些路径、预算多少；运行时不可自改 |
| 临时项目组/工位 | **Cell** | 一个有生命周期、有预算、有权限边界的执行单元；干完就"退休"（retire） |
| 任务工单 | **WorkPacket** | 跨部门交接工作的唯一正式单位：目标、输入、可写路径、验收标准、截止时间都写死在里面 |
| 出入证/权限卡 | **CapabilityGrant** | "允许你在什么范围内用什么能力"的凭证，短期有效、只能变窄不能变宽 |
| 经费额度 | **BudgetLease** | 一次执行的硬性配额：token、工具调用次数、时长、并发 |
| 采购/后勤部 | **Capability Broker** | 所有工具真正被执行的地方；模型"想用工具"只是提申请，实际执行永远经过 Broker |
| 有议程的会议 | **Symposium** | 有边界的多角色讨论：固定议程、限定轮数、必须产出文件；不是自由群聊 |
| 验收环节 | **Review / Acceptance** | 独立评审 + 按冻结标准做验收决定；评审不能改作者的原始记录 |
| 公司流水账 | **EventLog / RuntimeEvent** | 只能追加、不能修改的事件账本（在 `kiana-eventlog`） |
| 收据/台账 | **Receipt** | 从流水账汇总出来的报告："谁发起、批了什么、改了什么、验证了什么、有没有例外" |

再记住五条"公司纪律"，它们是安全宪法（[`company-os-security-constitution.md`](company-os-security-constitution.md)）十二条的白话浓缩：

1. **干活的人不能自己给自己发权限**——模型说的话、网页内容、工具说明，统统不能变成权限；
2. **下级权限永远不大于上级**——子任务只能领到父任务权限的子集；
3. **没批的事不做，批过期了也不做，拿不准就停**——这叫 fail-closed（失败即关闭）；
4. **每件事都要留记录，记录不能改**——评审、变更、总结都不许改写历史事件；
5. **"说做完了"不算做完**——只有服务端记录的终态和证据才算数。

---

## 4. 一次任务的完整旅程（走读）

### 4.1 跑之前需要知道

驱动整个流程的"模型"当前来自录好的脚本（cassette / fake-script，用环境变量 `KIANA_HARNESS_SCRIPT=/path/to/script.json` 指定；脚本长什么样可以看 `kiana-daemon` 的集成测试）。没有可用模型时，run 会显式失败（`model_unavailable`），不会空转假装成功。下面的走读以"模型可用"为前提。

### 4.2 成功路径：一条命令走完全程

以最简单的一条命令为例：

```bash
kiana trust .
kiana run --sandbox workspace-write -- "create GOLDEN_PATH.txt containing hello"
```

它在系统里走的路是（这也是 [`../CLAUDE.md`](../CLAUDE.md) 里那条"主路径"的白话版）：

```text
你在终端输入命令
  ↓ ① 入口层（kiana-entrypoints）把它包装成标准请求信封（kiana-protocol）
  ↓ ② DaemonHost 接单（kiana-daemon）——它是唯一的"总装配"，CLI/Web/桌面都走它
  ↓ ③ ControlPlane 审查（kiana-core）——项目信任了吗？角色对吗？沙箱档位允许写吗？
  ↓ ④ KianaHarness 开始跑模型循环（kiana-runner）——模型"想→提出动作→看结果→再想"
  ↓ ⑤ 模型想调工具——它只看得见五个固定工具：
  │      shell（跑命令）、apply_patch（改文件）、mcp（外部工具）、
  │      memory.search / memory.write（带 ACL 的记忆读写）
  ↓ ⑥ 工具调用不会直接执行！它变成一个"能力申请"（CapabilityRequest）送回 ControlPlane
  ↓ ⑦ 策略引擎（policy）+ 关卡（gates）+ 审批（approval）逐一放行或拒绝
  ↓ ⑧ 批准后由 Capability Broker 派给真正的执行器，在沙箱里执行（Linux 下用 bubblewrap）
  ↓ ⑨ 每一步都写进 EventLog（JSONL 流水账）
  ↓ ⑩ 运行结束——kiana run --receipt <会话ID> 能看到工具调用和改过的文件
```

第 ⑥ 步是整套系统最重要的一个设计：**模型永远不直接碰你的电脑**。它只能"申请"，申请要过审，执行永远在受控的 Broker 里。这就是文档里反复出现的"brokered（受托执行）"的意思。

### 4.3 失败路径：每道关卡拦下时你会看到什么

这套系统的灵魂不在成功路径，而在"拦得住、拦得明白"。同一条路上，各个关卡拒绝时给的是稳定错误码，不是含糊报错，更不是假成功：

| 你做了什么 | 在哪一步被拦 | 结果 |
|---|---|---|
| 项目没 trust 就要写盘 | ③ ControlPlane | `project_untrusted` / `workspace_write_requires_trusted_non_safe_profile`；文件不会出现 |
| 用默认只读沙箱要求写盘 | ③/⑦ | 拒绝写盘，文件不会出现 |
| 没配模型就跑 | ④ 之前 | `model_unavailable` |
| 空 prompt | 入口校验 | `prompt_required` |
| 指定不存在的角色 | ③ | `role_unknown` |
| Builder 写工单以外的路径 | ⑦ | `packet_path_denied` |
| 配了 HTTP 形式的 MCP server | ⑦ | `mcp_transport_unsupported`（只支持 stdio） |
| 取消一个不存在的会话 | ② | 显式失败，不装作取消成功 |

每个新能力都被要求**先证明"拦得住"（负向证据），再证明"跑得通"（黄金路径）**——这是安全宪法和发布门的硬要求。

### 4.4 规划版旅程：五个部门轮流上场

```text
kiana run --symposium --anti-meeting --sandbox workspace-write --json -- "目标"
  规划会：PM + Architect 有界讨论（Builder 不许列席；--anti-meeting 跳过辩论直接出产物）
  → 产出 plan/DECISION.json（决策）+ packet/TASK.json（工单）

kiana run --packet packet/TASK.json --sandbox workspace-write --json
  一个全新的 Builder 会话按工单干活
  → 工单是它唯一的输入（不复制规划会的聊天记录）；只能写工单允许的路径

kiana run --review <builder会话ID>
  独立 Reviewer 检查产出（确定性门：不跑模型、不能改作者的原始记录）
  → 写出 gate/REVIEW.json

kiana run --close <builder会话ID>
  收尾部 Closer 归档这单活的经验教训（lessons）
```

这条链就是第 3 节公司比喻的落地：规划部开会 → 出工单 → 执行部干活 → 监控部把关 → 收尾部归档。每个环节都是独立会话、各留各的账。

---

## 5. 术语小词典

按主题分组。每条只给白话解释和出处；正式定义永远以"详见"那份文档为准。

### 5.1 业务侧（"为什么做这件事"）

| 术语 | 白话解释 | 详见 |
|---|---|---|
| Organization | 组织根：个人版就是"你的公司"，未来团队版才有多租户 | [`company-os-domain-contracts.md`](company-os-domain-contracts.md) |
| Objective | 目标：带指标、基线、目标值和时间窗的结果承诺，比如"三个月内把 X 提升到 Y" | 同上 |
| Initiative | 立项提案：还没变成项目的想法/问题，要先过评估 | 同上 |
| Project | 项目：有范围、成功标准、预算和生命周期的交付单元 | 同上 |
| Milestone | 里程碑：项目里的阶段性交付点，各自有验收标准 | 同上 |
| Acceptance | 验收：按"冻结那一刻的标准"由独立决策人接受或拒绝 | 同上 |
| Delivery | 交付：什么产物、什么版本、交给了谁、对方确认了吗 | 同上 |
| Outcome | 结果测量：交付之后，当初的目标指标到底有没有实现（**交付成功 ≠ 目标实现**） | 同上 |
| ChangeRequest | 变更申请：想改范围/预算/验收标准？必须走这个流程产生新版本，不能偷偷改 | 同上 |
| Risk / Incident | 风险是"还没发生的隐患"；事故是"已经发生或无法排除的异常"，两者不能混 | 同上 |

### 5.2 执行侧（"谁在干活"）

| 术语 | 白话解释 | 详见 |
|---|---|---|
| Department | 五个常设部门：立项/规划/执行/监控/收尾，对应 PMP 五个过程组 | [`company-os-design.md`](company-os-design.md) §3 |
| RoleSpec | 角色规格：PM、Architect、Builder、Reviewer、Closer 等各自的工具、沙箱、路径和记忆权限（比如规划角色写不了 `src/`） | `kiana-domain`；[`company-os-design.md`](company-os-design.md) |
| AgentTemplate | Agent 模板：预制好的"岗位配置"，版本固定，运行时不可修改 | [`company-os-design.md`](company-os-design.md) §5.3 |
| Cell | 执行单元：一次有预算、有权限、有父子关系的"临时工位"；文档里"细胞分裂"就是指 Cell 派生子 Cell | [`company-os-design.md`](company-os-design.md) §5.4、§6 |
| SpawnPlan | 分裂计划：想创建子 Cell？先提交计划，过校验、预留预算，才许创建 | [`company-os-design.md`](company-os-design.md) §5.5 |
| WorkPacket | 任务工单：跨部门交接的唯一正式单位（见上文对照表） | [`company-os-domain-contracts.md`](company-os-domain-contracts.md)；[`company-os-design.md`](company-os-design.md) §5.1 |
| Symposium | 有界会议：固定议程和轮数、必须产出文件的多角色讨论；规划会和监控会都不让 Builder 列席（这条边界已冻结） | [`company-os-design.md`](company-os-design.md)；[`../COMPANY.md`](../COMPANY.md) |
| WorkFingerprint | 任务指纹：同样的活儿不重复干——指纹相同的任务应复用结果或拒绝重复创建 | [`company-os-design.md`](company-os-design.md) §6.3 |

### 5.3 权限与安全侧（"凭什么让你干"）

| 术语 | 白话解释 | 详见 |
|---|---|---|
| ControlPlane | 控制面：所有请求的审批中枢（见上文对照表） | [`company-os-design.md`](company-os-design.md)；`kiana-core` |
| Policy / Gate | 策略与关卡：机器自动判断"这个申请合不合规"的两道检查 | `kiana-policy` / `kiana-gates` |
| Approval | 审批：需要人（或明确授权的规则）点头的申请；绑定精确内容摘要，一次性消费，过期作废 | [`company-os-design.md`](company-os-design.md) §10 |
| CapabilityGrant | 能力授权：短期、精确、不可转借的"出入证" | [`company-os-design.md`](company-os-design.md) §5.6 |
| BudgetLease | 预算租约：token/工具次数/时长/并发的硬配额 | 同上 |
| SupervisionLease | 监督租约：心跳、检查点、卡死判定、重试上限——防止子任务"失联装死" | 同上 |
| Broker | 受托执行器：所有真实执行的必经之路（见上文对照表） | `kiana-capability-broker` |
| trust | 项目信任：`kiana trust .` 把一个目录标记为受信；未受信项目一切写操作被拒 | [`../USER.md`](../USER.md) |
| sandbox | 沙箱档位：`read-only`（只读）/ `workspace-write`（可写工作区）。注意默认值因入口而异：`kiana run` 默认只读，文件夹工作台默认 workspace-write（但都要求项目已 trust）；工作区外一律拒绝 | [`../USER.md`](../USER.md)；[`../CLAUDE.md`](../CLAUDE.md) |
| fail-closed | 失败即关闭：任何拿不准的情况（没批、过期、越界、校验失败）一律拒绝，绝不"先做了再说" | 全部规范通用 |
| R0–R5 | 风险分级：R0 只读 → R1 改本地文件 → R2 起草对外内容 → R3 真的发出去 → R4 花钱/法律承诺 → R5 碰物理世界（门锁/燃气/车，默认禁止自治） | [`company-os-security-constitution.md`](company-os-security-constitution.md) §3 |
| TOCTOU | "检查时"和"使用时"的时间差攻击：检查的是 A 文件，执行时已被换成 B。防它要锁定文件身份 | [`company-os-security-constitution.md`](company-os-security-constitution.md) SEC-07 |
| fencing | 围栏：取消/撤销时层层设卡（授权层→派发层→执行层→进程层），确保"停了就是真停了" | 同上 SEC-08 |

### 5.4 运行时侧（"一次执行长什么样"）

| 术语 | 白话解释 | 详见 |
|---|---|---|
| Session | 会话：长期容器，绑定你、工作区和项目 | [`company-os-platform-architecture.md`](company-os-platform-architecture.md) §4 |
| Turn | 轮次：你一次输入到系统停下来的一个来回 | 同上 |
| Run | 运行：一次执行生命周期，可排队/运行/暂停/取消 | 同上 |
| Invocation | 能力调用：一次具体的工具/能力请求，有独立身份，可恢复、可追责 | 同上 |
| RuntimeEvent | 运行事件：流水账里的一条不可变记录 | `kiana-eventlog` |
| Artifact | 产物：可版本化的文件/工件（区别于事件：事件是"事实"，产物是"东西"） | [`company-os-operations-governance.md`](company-os-operations-governance.md) §6 |
| Receipt | 收据：从事件和证据汇总的报告（**证明"系统记录了什么"，不自动证明"现实世界发生了什么"**） | [`company-os-design.md`](company-os-design.md) §11 |
| Transcript | 对话记录：只是给人看的视图，**不是**事实源，随时可丢弃 | 同上 |
| result_unknown | 结果未知：可能已经产生副作用但确认不了（超时/崩溃/日志丢失）。它是一等状态，禁止自动重试，必须对账 | [`company-os-security-constitution.md`](company-os-security-constitution.md) SEC-09 |
| PendingInvocation | 挂起的调用：等审批时把调用"冻结"起来，批准后原地续跑，而不是报错重来（目标设计，尚未完成） | [`company-os-design.md`](company-os-design.md) §10.1 |
| cassette / fake-script | 模型录像带：预先录好的模型响应脚本（`KIANA_HARNESS_SCRIPT` 环境变量指定），让测试不依赖真模型、永远可复现 | [`../USER.md`](../USER.md) |

### 5.5 平台能力侧（"模型看什么、系统怎么扩展"）

| 术语 | 白话解释 | 详见 |
|---|---|---|
| ContextPlan | 上下文计划：一次模型请求"应该看到什么"的可审计清单，不是随手拼接的字符串 | [`company-os-platform-architecture.md`](company-os-platform-architecture.md) §5 |
| Memory | 分层记忆：六个访问层（公司 / 部门 / 角色 / 项目 / 用户 / 实例草稿 InstanceScratch）× 多种内容类型；检索结果必须带来源，且永远不能变成权限 | 同上 §5.3 |
| Compaction | 历史压缩：长对话压成摘要接着聊；摘要是新产物，原始事件永远保留 | 同上 §6 |
| Prompt Cache | 提示词缓存：模型服务商复用相同前缀省钱；**它只是省钱手段，不是持久化**（缓存命中不代表状态已保存） | 同上 §6 |
| CapabilityDescriptor | 能力说明书：一个工具的身份证——版本、风险、参数结构、审批策略等，同时喂给模型、搜索、策略和审计 | 同上 §7 |
| Tool Search | 工具搜索：工具多了不能全塞给模型，先按角色/风险过滤再检索出前几名候选；**搜到 ≠ 有权用** | 同上 §7.2 |
| MCP | 外部工具协议：Kiana 作为客户端调外部工具服务器。当前只支持 stdio（本地子进程），HTTP 明确不支持 | 同上 §7.3；[`coding-pack-matrix.md`](coding-pack-matrix.md) |
| Workflow | 工作流：确定性流程图——顺序、重试、超时、补偿由软件保证，模型只能提建议，不能改流程状态（目标设计） | 同上 §8 |
| Swarm | 有界多 Agent：受控的"分头干活再合并"，必须有父级、分区、预算、深度、合并规则；不是自由群聊（目标设计） | 同上 §9 |
| NormalizedEvent | 归一化事件：不同模型服务商的流式输出格式各异，先统一翻译成内部格式再进系统 | 同上 §10 |
| EvalSuite / GoldenTrace | 评测集 / 黄金轨迹：可重放的测试用例和"标准答案录像"，改了模型/提示词之后跑一遍防退步（目标设计） | [`company-os-quality-ecosystem.md`](company-os-quality-ecosystem.md) §3 |

### 5.6 状态与证据侧（"怎么防止吹牛"）

| 术语 | 白话解释 | 详见 |
|---|---|---|
| feature_status | 做没做：`implemented`（代码在）/ `partial`（有一部分）/ `target`（目标，还没做）/ `deferred`（明确延后）/ `not_supported`（现在必须拒绝） | [`README.md`](README.md) §4 |
| proof_level | 证到什么程度：`source` → `local_behavior` → `durable` → `live` → `physical`（见下一节） | 同上 |
| canonical owner | 户口所在地：每个概念只有一个正式定义处（哪份文档 + 哪个 crate），其他地方只能引用不能重新定义 | [`company-os-spec-index.md`](company-os-spec-index.md) §4 |
| Evidence Ledger | 证据账本：状态提升必须绑定源码快照、精确命令和结果 | [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) |
| Gate 0 | 第零道门：格式检查、编译、lint、全量测试、冒烟脚本全绿才算通过；当前尚未整体通过 | [`company-os-implementation-outline.md`](company-os-implementation-outline.md) §6 |

---

## 6. 两个状态维度，用做菜打比方

仓库里所有能力都要同时记两个维度，因为"做没做"和"证到什么程度"是两回事：

**feature_status（做没做）**，五档白话：

- `implemented`：代码路径在（可信到什么程度，还要看下面的证明等级）；
- `partial`：有一部分，或者有已知的绕过口子；
- `target`：目标，还没做；
- `deferred`：想做，但明确排到以后；
- `not_supported`：现在必须拒绝或显示不可用，不许假装能用。

**proof_level（证到什么程度）**——用"学做一道菜"打比方：

| 等级 | 正式含义 | 做菜版 |
|---|---|---|
| `source` | 源码/文档里有定义 | 菜谱上写了这道菜 |
| `local_behavior` | 固定本机 + 受信项目 + 录好的脚本下可复现 | 在我家厨房、用我家的锅、照着录像做出来过 |
| `durable` | 重启、崩溃、重放后仍可证明 | 中途停电重来，还是能做出来 |
| `live` | 真实外部服务验证过 | 在真饭店的后厨做出来过 |
| `physical` | 真实设备/物理效果验证过 | 客人真的吃到了这道菜 |

配套的五条"永远不成立"的推断（[`README.md`](README.md) §4），白话版：

- **有类型 ≠ 已强制**——代码里定义了 `Grant` 结构体，不代表系统真的在检查它；
- **单元测试通过 ≠ local_behavior**——测了一个函数，不代表整条产品路径能跑；
- **local_behavior ≠ durable**——我家厨房做出来过，不代表停电重来还行；
- **Receipt 存在 ≠ 现实结果正确**——收据只证明"记了账"，不证明"外面的世界真的变了"；
- **参考项目有实现 ≠ Kiana 已实现**——别人家会做这道菜，不等于我会。

---

## 7. 为什么这些文档写得像法律条文

`docs/` 里除了本文，几乎全是"规范目标"（Normative Target）文档。它们又长又硬，是刻意的，原因有三个：

1. **读者主要是"来干活的 AI"和未来的自己**。这个仓库的开发大量由 AI 完成，AI 最容易犯的错就是：把"想做"写成"做完了"、悄悄放宽测试让它变绿、在两个地方把同一个概念定义得不一样。规范文档就是拿来拴住这些行为的。
2. **一个概念只许有一个定义处**（canonical owner）。所以文档之间大量互相链接而不复制内容——读起来"跳来跳去"，但保证了不会出现两份互相矛盾的定义。
3. **先写"不许做什么"，再写"怎么做"**。每个能力都要求先证明拒绝路径、失败路径、取消路径（负向证据），才允许庆祝成功路径。这让文档看起来充满禁令，但这正是它的工作。

所以正确的读法是：

- **别从头读到尾**——规范是拿来"查"的，不是拿来"读"的；
- **想了解全貌**：读本文 + [`company-os-design.md`](company-os-design.md) 的 §0–§4 就够了；
- **想知道现状**：只看 [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md)，规范里写的都是目标；
- **具体术语查不到**：回到本文 §5 的词典，按"详见"跳过去。

---

## 8. 常见疑问（FAQ）

**Q1：为什么 Builder（执行者）不许参加规划会和监控会？**
防止"运动员兼裁判"。执行者参与制定验收标准，标准会不知不觉向"好实现"倾斜；执行者坐进评审席，评审就不再独立。所以规划会产出的工单（packet）是 Builder 的唯一输入，Builder 用全新会话干活——它的理解只能来自工单本身，工单写得不清楚会立刻暴露；评审（Review）则由不同于作者的角色在新会话里做。这两条边界已冻结，见 [`../CLAUDE.md`](../CLAUDE.md)。

**Q2：都有 Receipt 了，为什么还说"不能证明业务结果"？**
Receipt 证明"系统记录了什么"（调了哪些工具、改了哪些文件），不证明"现实世界因此变好了"。比如：交付了代码 ≠ 用户指标提升了。后者要靠 Outcome 对象在测量窗口里用真实观测数据回答。这是"执行事实"和"公司事实"两条链分开的原因，见 [`company-os-domain-contracts.md`](company-os-domain-contracts.md) §1。

**Q3：Web 只监听 127.0.0.1（本机），为什么文档还说"loopback 不是认证"？**
因为你电脑上跑的**其他任何程序**也能访问 127.0.0.1。只监听本机挡住了外部网络，但挡不住本机上的恶意进程。所以安全宪法 SEC-06 要求未来加上 token 或受保护的 Unix socket 才算有认证。

**Q4：为什么不给模型 Read/Grep/Glob 这种文件工具？**
刻意收窄工具面（Codex 形状）：模型只看得见五个固定工具——`shell`、`apply_patch`、`mcp`，以及后来加入的 `memory.search`/`memory.write`；搜索文件就用 `shell` 跑 `rg`/`ls`/`cat`，不再单做一套 Read/Grep/Glob 注册表。工具越少，审批面越小、行为越可控。这条决策在 [`coding-pack-matrix.md`](coding-pack-matrix.md) §0.4 和 §1 的"P0 搜索策略"里锁死了（P1-READ 已明确跳过）。

**Q5：`result_unknown`（结果未知）为什么不能自动重试？**
因为"未知"意味着**副作用可能已经发生**。想象一次付款请求超时了——你不知道钱到底扣没扣，这时自动重试可能造成重复扣款。正确做法是先对账（reconciliation）搞清楚事实，重试本身也是新的副作用，需要新的授权。

**Q6：为什么规范反复强调"不许自动 commit / push / merge"？**
git 提交是对外可见、难以撤销的副作用，而且这个仓库常有多个 AI 并行工作（见 [`../AGENTS.md`](../AGENTS.md)：一个工作树只许一个写者）。提交权保留给人（或明确授权的流程），防止 AI 把没验证的东西固化进历史。

**Q7：`kiana tui` 为什么被"park"（停放）了？**
它走的还是旧的 SDK 流式通道，不是 `DaemonHost` 产品主路径。留着它但不再演进，是为了避免同时维护两套执行脊柱——这正是"单一执行脊柱"不变量（所有入口共用一个 DaemonHost）的体现。

**Q8：HTTP MCP 为什么不支持？**
stdio MCP 是本地子进程，风险边界清楚；HTTP MCP 意味着远程调用，涉及一整套没做的安全设计（认证、传输、注入面）。没做就明确拒绝（返回 `mcp_transport_unsupported`），而不是"能连就先连上"——这就是 fail-closed。

**Q9：文档里一会儿 P0/P1/P2，一会儿 Slice A/B/C，一会儿 R0–R5，怎么区分？**
三套编号是三个不相干的维度：**P0–P6** 是实施阶段（先做什么后做什么）；**Slice A–M** 是工程切片（把工作切成可认领的块，见实施大纲）；**R0–R5** 是风险等级（一个动作有多危险）。另外 [`coding-pack-matrix.md`](coding-pack-matrix.md) 里的 P0/P1 特指"Coding pack 里哪些行为是 v1.0 核心"，范围只限那张表。

**Q10：「dump」是什么意思？**
指 `reference/` 里没有 git 历史的还原代码树（比如 `claude-code-rev-main`）。它们只能当"某个工具名存在过"的证明，许可证不明，**禁止**照抄源码。相关规则见 [`coding-pack-matrix.md`](coding-pack-matrix.md) §9 的许可证边界。

---

## 9. 按你的需求选阅读路径

| 你想干什么 | 阅读顺序 |
|---|---|
| 只想上手用 | [`../USER.md`](../USER.md) → 根 [`../README.md`](../README.md) |
| 想理解整个系统 | 本文 → [`company-os-design.md`](company-os-design.md) → 想深入哪块再看哪份规范 |
| 想知道现在到底做到哪了 | [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) → [`coding-pack-matrix.md`](coding-pack-matrix.md) |
| 想动手写代码 | [`../CURRENT_STATUS.md`](../CURRENT_STATUS.md) → [`company-os-implementation-outline.md`](company-os-implementation-outline.md) → 对应领域的规范 |
| 想改文档 | [`README.md`](README.md) 的权威顺序和变更规则 |
| 想看别的 Agent 项目怎么做 | [`company-os-reference-matrix.md`](company-os-reference-matrix.md) → [`reference-agent-audit/`](reference-agent-audit/README.md) |

两处指引的分工：**本表是"按目的抄近道"**，适合带着具体问题来的人；[`README.md`](README.md) §6 的"推荐阅读路径"是**逐份按顺序读的完整路线**（按贡献者类型分），两者不矛盾，拿不准时以 README 为准。每份文档具体讲什么，见 README §2 的文档地图；每份规范文档的开头现在也都有一段《本文速览》导读。
