# Kiana Company OS

Date: 2026-08-22
Status: 产品架构真相源（多 agent 公司编排，而不是单一万能 agent）
Companion: [DESIGN.md](DESIGN.md)、[PROCESS.md](PROCESS.md)、[PHASES.md](PHASES.md)、[.planning/REQUIREMENTS.md](.planning/REQUIREMENTS.md)

Kiana 的产品不是「一个很能干活的 chat」。它是一家**软件公司的操作系统**：

- **五个 PMP 过程组是部门**（抽象层），不是五步流水线。
- 每个部门里有**独立上下文、独立提示词、独立权限/知识面**的角色。
- 部门内部可以开**有界群聊（Symposium）**；部门之间只交接 **Work Packet**。
- 记忆是分层 RAG，按公司 / 部门 / 角色 / 项目 / 用户 / 会话草稿隔离，检索是工具，命中进收据。
- 副作用一律经 `DaemonHost`。第一个能改文件的员工仍然必须先能跑——那是执行部的 Builder，不是上帝进程。

---

## 1. 为什么要公司，不要单 agent

architect-loop 把单会话失败写成三条（`reference/architect-loop/DESIGN.md`）：

1. **Context rot** — 窗口越大越蠢。
2. **Self-grading** — 写代码的人给自己打分。
3. **Goalpost drift** — 做完再改验收标准，永远能过。

12-factor #10 结论相同（`reference/12-factor-agents/content/factor-10-small-focused-agents.md`）：agent 是更大的**确定性系统**里的积木，一个 agent 3–20 步，过程本身不要交给 LLM 即兴发挥。

Agency Swarm 自己也写了同一条纪律（`reference/agency-swarm/docs/core-framework/agencies/communication-flows.mdx`）：**永远从一个 agent 开始**；同时招两个以上，训练成本会爆炸。

因此：

| 错误形状 | 正确形状 |
|---|---|
| 一个 agent 带 50 个工具 loop until done | 编排器是软件；员工是小 agent |
| PMP 五步当成 prompt 里的愿望清单 | 五过程组是五个部门，部门有编制、门禁、RAG |
| MetaGPT / ChatDev 1.0 全员在共享聊天室互喷 | 部门内 Symposium 有议程、轮次、黑板、决议；部门间只交包 |
| Claude Coordinator：TeamCreate/SendMessage 当核心 | `kiana-coordinator` 里那套冻结，不当产品总线 |
| 先招 8 个角色再让其中一个会写文件 | 先让 Builder 会写文件，但类型系统从第一天就是 Role + Department |
| 把整个仓库/用户史塞进每个 prompt | RAG 是带 `department_id`/`role_id` ACL 的查询工具 |
| coleam00/Archon 的 YAML 工作流直接当公司 | 学它的「过程是软件、节点可 fresh_context」；组织编制另建 |

---

## 2. 三层真相：部门 / 角色 / 工人运行时

```text
Human (Sponsor)
    │
    ▼
Company OS  ─── 五个部门（PMP 过程组）= 抽象层
    │              部门持有过程模板、门禁、部门 RAG、Symposium 权
    │
    ├── 立项部 Initiating
    ├── 规划部 Planning
    ├── 执行部 Executing     ← v0.2 先让这里的 Builder 能写盘
    ├── 监控部 Monitoring
    └── 收尾部 Closing
            │
            ▼
     角色 = 独立 AgentInstance（独立 session + 特有 prompt + 房间钥匙）
            │
            ▼
     Worker Runtime = 今天的 KianaHarness
            │
            ▼
     DaemonHost / Policy / Sandbox / EventLog / MemoryBroker
```

**编排器不是一个超级 LLM。** 它持有公司状态、派工、冻结、合并、开会。部门主席可以是 LLM 角色，但他们产出的是**工件和决议**，下一部门由软件读取，而不是「接着聊」。

对照：

| 参考 | 学 | 不学 |
|---|---|---|
| architect-loop | 拆 plan/execute/review，fresh context，作者≠评审 | 把它当唯一产品形态 |
| pm-skills orchestrator | DELEGATE，不重写 skill，默认 `memory: none`，没有 Agent 套娃 | 每步都停给人看（Kiana 只在写盘/外网/合并停） |
| coleam00/Archon（`reference/Archon-Knowledge`） | YAML/DAG 过程、节点 `context: fresh`、bash 确定性节点、人审批门、worktree 隔离、审计 | 用 Claude/Codex SDK 当工人；单租户安装模型；把 workflow 当成部门编制 |
| Schr0d/Archon（`reference/Archon`） | 规划/监控的确定性冲击半径工具（`--format agent`） | 当成公司 OS |
| Agency Swarm | 角色指令+工具、**定向** `communication_flows`、handoff vs orchestrator-worker | `SendMessage` 当自由总线；handoff 复制全历史 |
| ChatDev 1.0 | CEO/CTO/程序员/评审的公司隐喻、阶段性研讨会 | 共享 ChatChain 当产品 |
| ChatDev 2.0 / MacNet | 图编排、节点可含 memory/human/tool | 零代码平台当 Kiana 骨架 |
| CrewAI | Crew（角色自治）与 Flow（事件驱动）分开 | Crew 里的自由协作当执行部日常 |
| MetaGPT | 角色切分（PM/Architect/Engineer/QA） | `Environment.publish_message` 全员广播 |
| autogen Magentic-One / distributed group chat | `RequestToSpeak`、max_turns、progress ledger | 无界 GroupChat 融合成一份 transcript |
| deepseek subagent | spawn 独立 child，不是 fork 父窗口 | `subagent-fork-in-process` 当默认 |
| 本仓 `kiana-tasks/src/swarm.rs` | WorkPacket / path isolation 的包形状 | 把它当已完成产品 |
| 本仓 `kiana-coordinator` | — | TeamCreate/SendMessage，冻结 |

---

## 3. 部门 = PMP 五个过程组（抽象层，不是流水线）

PMP 的 Initiating / Planning / Executing / Monitoring & Controlling / Closing **不是**「先做 1 再做 2」的单一管道。真实公司里它们**同时存在**：执行部在写代码时，监控部在看关卡，规划部可能同时开变更研讨会。

所以 Kiana 把它们做成**常设部门**：

```text
                    ┌──────── 立项部 ────────┐
                    │ charter / 成功标准     │
                    └──────────┬─────────────┘
                               │ WorkPacket（立项决议）
                    ┌──────────▼─────────────┐
                    │ 规划部                  │
                    │ WBS / 验收 / 路径锁     │
                    └──┬───────────────┬─────┘
          执行包        │               │ 监控包（验收标准冻结）
          ┌────────────▼──┐     ┌──────▼──────────┐
          │ 执行部         │     │ 监控部           │
          │ Builder 写盘   │────►│ Reviewer/QA/审计 │
          └────────────┬───┘     └──────┬──────────┘
                       │ 变更请求        │ 关卡结果
                       └──────►规划部◄───┘
                               │
                    ┌──────────▼─────────────┐
                    │ 收尾部                  │
                    │ receipt / lessons 入库  │
                    └────────────────────────┘
```

部门是控制面对象，不是 prompt 章节：

```text
DepartmentSpec {
  id            // initiating | planning | executing | monitoring | closing
  pmp_group     // 对应过程组
  mission       // 这个部门对什么负责、对什么不负责
  default_roles // RoleSpec[]
  artifacts     // 这个部门允许写入的工件 glob
  rag_collection
  symposium_policy  // 谁能开会、默认 max_rounds、是否允许跨部门联席
  gates         // 离开本部门必须满足的关卡
}
```

### 3.1 立项部 · Initiating

**负责：** 这件事该不该做、成功长什么样、谁在乎、最大风险是什么。  
**不负责：** 写 src、拆 WBS、跑测试。  
**默认编制：**

| 角色 | 看问题的角度 | 工具 | 知识面 |
|---|---|---|---|
| Chair（或软件） | 主持、收敛、写 DecisionRecord | 工件写 charter 路径；开会 | company + initiating + user:prefs |
| SponsorProxy | 代表人类出资人：范围、预算、否决 | 只读 + 投票 | user + initiating |
| Analyst | 问题陈述、现状、约束 | 只读仓库 + memory.search(project) | project + initiating |
| RiskScout | 失败模式、许可证、安全、不可逆 | 只读 | project + role:risk |
| StakeholderMapper | 干系人、接口、谁必须签字 | 只读 | user:prefs + initiating |

**必出工件：** `charter.md`（问题、成功标准、非目标、干系人、风险初表、go/no-go）。  
**参考：** pm-skills `discover-*`、`foundation-stakeholder-briefings`、`deliver-problem-statement`（若存在）；GSD discuss；OpenSpec proposal。  
**门：** 没有成功标准不得进入规划部。

### 3.2 规划部 · Planning

**负责：** 把 charter 变成可派工的包：顺序、验收、路径锁、风险、安全。  
**不负责：** 实现、给自己的设计打分当作关卡通过。  
**默认编制：**

| 角色 | 角度 | 工具 | 知识面 |
|---|---|---|---|
| PM / Chair | 范围、依赖、派工、变更 | 写 plan/packet；开会；无 apply_patch | company + planning + user:prefs |
| Architect | 结构、边界、冲击半径 | 只读 + 确定性分析工具（Schr0d/Archon 形） | project + role:architect |
| Estimator | 工时、并行、路径冲突 | 只读 | planning + project:map |
| QA Planner | 验收标准、反例、证据形状 | 写 acceptance；无 src | planning + role:qa |
| Security | 信任、沙箱、秘密、供应链 | 只读 + policy 查询 | role:security + project |

**必出工件：** `plan.md` + `WorkPacket[]`（每个包：目标、路径锁、验收、禁止事项、允许角色）。  
**参考：** pm-skills `deliver-prd` / `deliver-user-stories` / `deliver-acceptance-criteria` / `deliver-edge-cases` / `_workflows/sprint-planning.md`；architect-loop Strategist；coleam00/Archon 的 `archon-architect.yaml`（度量→诊断→再改，且分析节点 `denied_tools: [Write, Edit, Bash]`）；Schr0d/Archon `analyze --format agent`。  
**门：** 每个执行包必须有冻结的验收；没有路径锁不得并行。

### 3.3 执行部 · Executing

**负责：** 按包改文件、跑允许的验证、交出补丁和证据。  
**不负责：** 改验收标准、给自己 LGTM、开规划研讨会。  
**默认编制：**

| 角色 | 角度 | 工具 | 知识面 |
|---|---|---|---|
| Tech Lead | 拆包内顺序、集成风险（可选，v0.4+） | 只读 + 派工；默认不写 src | executing + project |
| Builder | 实现 | `shell` + `apply_patch` + 路径锁内 | 本包路径 + project:code（检索）+ **默认不进 user-private** |
| Integrator | 多包合并、冲突（v0.6 并行后） | 受限 patch | executing + project |

**必出工件：** patch + 测试/命令证据 + 文件变更清单。  
**参考：** Codex/deepseek/pi harness；Kiana `KianaHarness`；coleam00/Archon `implement` 节点 `fresh_context: true` 循环直到任务完。  
**门：** 出执行部必须有收据；无收据监控部拒收。  
**v0.2 唯一必须先真的部门。**

### 3.4 监控部 · Monitoring & Controlling

**负责：** 关卡、变更控制、审计、作者回避评审。  
**不负责：** 替 Builder 改代码（发现问题时打回执行部或规划部）。  
**默认编制：**

| 角色 | 角度 | 工具 | 知识面 |
|---|---|---|---|
| Reviewer | 与验收对照，不是品味秀 | 只读；**session 不得等于作者** | project:events + 该包验收 + **看不到 Builder 未提交草稿集** |
| QA | 跑/核对证据是否满足 QA Planner 的标准 | 只读或隔离验证沙箱 | role:qa + 该包证据 |
| Change Controller | 超范围/改验收 → 退回规划部 | 无写 src | planning 决议 + monitoring |
| Auditor | 完整性、provenance、许可证 | 只读 eventlog | monitoring + company:policy |

**必出工件：** gate 记录（pass/fail/needs-change）、ReviewPacket。  
**参考：** architect-loop cohesion review；pm-skills `pm-critic`；coleam00/Archon `archon-review-block`（多评审再 synthesize）；Schr0d/Archon `diff` 当架构门。  
**门：** Reviewer 与 Builder 不得同一 `session_id` / 同一 `role_instance`。

### 3.5 收尾部 · Closing

**负责：** 关闭、入库、复盘、把教训写进该写的 RAG 层。  
**不负责：** 再开一摊新范围（那是新的立项）。  
**默认编制：**

| 角色 | 角度 | 工具 | 知识面 |
|---|---|---|---|
| Closer / PM | 对照 charter 宣布完成或失败 | 写 receipt | 全部部门工件（只读） |
| Librarian | 把决议/教训写入正确集合 | `memory.write`；不可改 src | 按密级写入 department/role/project/user |
| Retro Facilitator | 过程教训，不是点名羞辱 | 只读 + 写 lessons | company + 本项目 closing |

**必出工件：** receipt（含 RAG 命中、角色、部门、文件变更）+ lessons。  
**参考：** pm-skills `iterate-lessons-log` / `iterate-retrospective`；memorix Git Memory；MemPalace diary。

### 3.6 跨部门通信 = 交接，不是全公司微信

```text
允许：Department A --WorkPacket--> Department B
允许：明确召开的 JointSymposium（点名出席、限时、有议程）
禁止：任意角色 SendMessage 给任意角色
禁止：执行部 Builder 默认列席规划研讨会（保护工人上下文）
```

WorkPacket 是唯一跨部门产品：

```text
WorkPacket {
  id, from_dept, to_dept, assignee_role
  goal, path_allow, acceptance, forbidden
  inputs[]          // 上一部门工件引用
  knowledge_grants  // 本包额外打开的集合
  parent_symposium  // 若由会议产生
}
```

形状参考本仓 `kiana-tasks/src/swarm.rs`，产品完成仍以 harness 收据为准。

---

## 4. 角色契约（每个 agent 必须是这个对象）

```text
RoleSpec {
  id, department_id
  prompt            // 该角色系统提示，产品拥有（12-factor #2）
  prompt_hash       // 进收据，可复现
  tools             // 允许的 capability 名；默认拒绝
  sandbox           // read-only | workspace-write | none
  path_allow        // glob；Builder 只能动派工路径
  knowledge_grants  // 可查询的记忆集合
  can_convene       // 能否召开本部门 Symposium
  can_vote          // 会议投票权
  model_profile     // 规划用强模型，执行用便宜模型（PEAR）
  max_steps
}

AgentInstance {
  role: RoleSpec
  session_id        // 独立上下文，不继承编排器聊天
  work_packet_id    // 唯一任务；开会时为 symposium_id
  retrieved         // 本回合 RAG 命中，写入收据
}
```

原则：

1. **独立上下文。** 派工 = 新 session。历史通过 work packet + RAG 注入。deepseek 有 `subagent-fork-in-process`——Kiana **默认不用 fork**，默认 spawn 空白。coleam00/Archon 节点默认 `context: fresh`，学这个。
2. **特有提示词。** 存在 `kiana-skills` / 角色包，不写死在 `runner.rs`。
3. **权限随角色。** Policy 看 `RoleSpec`，不是全局「用户已 trust」。项目 trust 是大门；部门是楼层；角色是房间钥匙。
4. **知识随角色。** 见 §7。同一句 `memory.search("验收")`，QA Planner 和 Builder 看到的集合不同。
5. **小而专注。** 一个实例 3–20 步；开不完就拆包，不要把部门使命塞进一个人。

v0.2 可以只实现 `builder` + `department=executing`，但类型和派工 API 不许再出现「无角色的上帝 harness」。

---

## 5. 部门内部怎么开会（Symposium = 有界群聊）

用户要的「角色群聊、互相探讨」**做**，但做成**会议对象**，不是把所有人丢进同一份 transcript。

反面已经验证过：

- MetaGPT `Environment.publish_message`：全员观察，上下文腐烂。
- autogen `GroupChat` 无界轮转：聊天成为产品，决议稀释在对话里。
- ChatDev 1.0 功能研讨会：公司隐喻对，共享 ChatChain 当运行时错。
- Agency Swarm `SendMessage`：只允许作为**定向、ACL 过的**委派，不能当公司微信。
- 本仓 `kiana-coordinator` TeamCreate/SendMessage：**冻结**。

### 5.1 会议对象

```text
Symposium {
  id
  department_id          // 或 joint: [planning, monitoring]
  type                   // decision | brainstorm | review | kickoff | change-control
  agenda                 // 出席者可见
  chair                  // 人 或 角色 或 软件
  attendees[]            // 点名；默认不含执行部 Builder
  max_rounds             // 硬顶，建议 3–8
  max_tokens
  blackboard {
    claims[]             // {speaker, text, evidence_refs}
    votes[]              // {role, stance, reason}
    draft_decision
  }
  status                 // proposed | anti-meeting-checked | running | closed | aborted
  output                 // DecisionRecord → 该部门 RAG（聊天本身不是产品）
}
```

每个发言者保持**私有 session**。共享的只有黑板（结构化），不是融合成员历史。

发言协议学 autogen distributed group chat 的 `RequestToSpeak`（`reference/autogen/python/samples/core_distributed-group-chat/_agents.py`）：

1. Chair / 软件选下一个发言人（或 round-robin，但必须有人能结束）。
2. 被点名的角色读黑板 + 自己的 RAG，产出一条 **Claim 或 Vote 或 Draft**。
3. 写进黑板，不把私有 scratch 广播出去。
4. 到达 max_rounds / 全员投票 / Chair 宣布关闭 → 必须写出 `DecisionRecord`。
5. 写不出决议 = 失败可见，不得假装对齐。

Magentic-One 可学的是 **progress ledger + max_stalls 后重规划**，不是它的全员 transcript 终局答案。

### 5.2 会议技能包（pm-skills 原样当规划部/立项部技能，不当运行时）

| 技能 | 路径 | 在 Kiana 里干什么 |
|---|---|---|
| 先问该不该开 | `foundation-meeting-agenda` 的 anti-meeting check | 无 tradeoff/冲突/共创/关系/阻塞升级 → 改成异步包，不开会 |
| 出席者议程 | `foundation-meeting-agenda` | Symposium.agenda |
| 主席私有备战 | `foundation-meeting-brief` | 只进 Chair 的 instance scratch，不进部门 RAG |
| 会后纪要 | `foundation-meeting-recap` | 从黑板生成 recap 工件 |
| 跨会综合 | `foundation-meeting-synthesize` | Librarian 用；区分「决议演化」和「未解矛盾」 |

合同：`reference/pm-skills/docs/reference/skill-families/meeting-skills-contract.md`。

### 5.3 什么时候开会，什么时候不要

**开：** 规划部要在「拆两个大包 vs 一个竖切」之间做权衡；监控部对是否放行有分歧；立项部 go/no-go。  
**不开：** 执行部日常写文件；已经有冻结验收的包；纯状态同步；一个人就能定的事。

执行部 Builder **默认不出席**规划 Symposium。需要他们的事实时，规划部发只读调查包，Builder 交工件回来，而不是把工人拉进辩论。

### 5.4 版本切分（开会能力）

| 版本 | 会议能力 |
|---|---|
| v0.2 | 无会议。收据上可以有 `department=executing` |
| v0.3 | **一个**有界 Symposium：规划部 PM + Architect，产出一个 WorkPacket 给 Builder |
| v0.4 | 监控部 Reviewer 独立评审；需要时开 change-control 会（PM + Change Controller + Reviewer） |
| v0.5 | 五部门都可开会；跨部门联席是显式对象；决议进部门 RAG |
| v1.x | 多租户部门会议 ACL |

---

## 6. 权限模型（项目大门 × 部门楼层 × 角色房间）

```text
ProjectTrust（已有）     未信任 → 全公司不许副作用
    ×
Department.artifacts     部门可写的工件根
    ×
RolePolicy.tools         角色工具白名单
    ×
RolePolicy.path_allow    路径锁
    ×
RolePolicy.knowledge     记忆 ACL（集合 × 密级）
    ×
Sandbox                  read-only / workspace-write
    ×
Approval                 外网、密钥、合并、跨路径写、跨部门写 = Ask
    ×
Symposium ACL            未点名不得发言、不得读主席 brief
```

例子：

- Builder：`shell`、`apply_patch`、`memory.search(project:code, project:docs, role:builder)`；不能 `memory.search(user-private)`；不能 `memory.search(planning:unreleased-debate)`；不能 spawn 子 agent；不能召开规划会。
- PM：无 `apply_patch`；可写 `charter/plan`；可召开本部门会；可搜 `project` + `user:prefs` + `planning`。
- Architect：只读 + 冲击半径工具；规划会投票；不写 src。
- Reviewer：只读工具 + `memory.search(project:events, monitoring, 该包验收)`；看不到 Builder instance scratch。
- Librarian：可 `memory.write` 到被授权集合，不可改 src。

`kiana-policy` 今天只看 `project_trusted` 和 risk。Company OS 要把 `role_id` + `department_id` 放进 `RequestContext`。

---

## 7. 记忆：六层 RAG，不是两套文件夹

用户要的不是「再做一个向量库」。要的是：**大项目记得住、用户本人记得住、不同角色看见不同东西。**

```text
Company RAG        公司剧本、组织图、过程模板、通用门禁
Department RAG     该部门过程模板、Symposium 决议、部门教训
Role RAG           工艺知识（怎么写 packet / 怎么做评审 / 怎么打补丁）
Project RAG        这个仓库的事实（代码、ADR、事件、任务）
User RAG           这个人的偏好、沟通风格、跨项目约束
Instance scratch   本 session 草稿，随实例销毁，默认不晋升
```

检索契约：

- 工具：`memory.search` / `memory.write`（结构化，12-factor #4）。
- 请求必须带 `role_id` + `department_id`。MemoryBroker 用 `knowledge_grants` 过滤集合、路径、密级。
- **命中必须进收据**（谁查了什么、用了哪几条、来自哪一层）。无法指认来源的内容不得进入 Reviewer 的「已验证」结论。
- 禁止把检索结果静默拼进系统提示当永久背景；本回合注入，下回合重新查。
- 晋升规则：instance → role/department/project/user 必须显式 `memory.write` + 密级；聊天记录不得自动入库。

密级：

```text
public          任何人可检索（README、公开 ADR）
company         本公司角色
department:*    该部门角色（规划决议默认 planning，不自动给 Builder）
role:*          该角色工艺（pm vs builder vs reviewer）
project         本项目事实
packet          仅被派到该 WorkPacket 的实例
user-private    仅 Human + 经授权的 PM（质量偏好可以降级为 project）
scratch         仅本 instance
```

对照：

| 参考 | 学 | 放哪一层 |
|---|---|---|
| memorix | 项目记忆活在 git 仓、跨 chat 存活；不要做成必须先开 MCP | Project |
| MemPalace | wings/rooms/drawers、本地可检索、日记 | User；部门可用 rooms 隐喻 |
| coleam00/Archon v1（`archive/v1-task-management-rag`） | 知识引擎与任务分离 | Company/Project 的后期索引，不在 v0.2 重做一套 Python RAG |
| Letta / MemGPT | 有状态记忆、core vs archival | Role + User 的晋升；本仓 `reference/letta` 现已是落地页，源码在 letta-code，浅克隆勿编造 |
| graphify / GitNexus | 代码图 | Project 后期 |
| Schr0d/Archon | 依赖/冲击半径 JSON | Planning/Monitoring 的确定性工具输出，写入 Project RAG |
| `kiana-query` | 已有 repo map / token budget | 先接到 MemoryBroker |
| CrewAI memory / ChatDev memory node | 「记忆是节点/组件」 | 学分层，不学框架绑定 |

**打开条件：** Builder 黄金路径已绿。否则「记忆系统」会变成又一个空 crate。

Librarian（收尾部）是唯一默认拥有跨层 `memory.write` 的角色；PM 只能写本部门工件路径；Builder 默认只能写 packet scratch 和代码。

---

## 8. 黄金路径（公司形，不是单聊形）

v0.2 仍然是：

```bash
kiana run -- "给这个仓库加一个健康检查接口"
# 收据强制 role=builder，department=executing
```

v0.3 起变成：

```bash
kiana company run -- "给这个仓库加一个健康检查接口"
# 立项可跳过（小任务默认短 charter）
# 规划部 PM+Architect 开一次有界会 → 一个 WorkPacket
# 执行部 Builder 独立上下文写盘
# 监控部（v0.4）独立 Reviewer
# 收尾部落收据
```

小任务允许**软件跳过开会**（anti-meeting check 通过 → 异步包）。公司 OS 的价值是跳过也留下章程式记录，而不是强迫每个 typo 开立项大会。

---

## 9. 协作如何变快（以及怎样不变成吵闹）

快的来源：

1. **规划与执行拆开**（PEAR：弱规划伤害大于弱执行）。贵模型当 PM/Architect，便宜模型当 Builder。
2. **一人一包一上下文**，做完就扔窗口。
3. **可并行的包才并行**，靠路径锁；冲突包串行。
4. **会只开该开的。** 会议有硬顶；决议进部门 RAG，下一次不必重聊。
5. **确定性节点能不用模型就不用。** coleam00/Archon 的 bash 测试节点、Schr0d/Archon 的冲击半径、schema smoke——这些不是角色。

不变成吵闹：

- 跨部门没有自由 IM。
- Builder 不列席规划会。
- 群聊不能代替 WorkPacket。
- 冻结 `kiana-coordinator` 总线。

### 9.1 Swarm 看谁：ruflo，但只看执行部并行

「Swarm」在 reference 里至少有三个意思，不要混：

| 词 | 实际是什么 | Kiana 里对应 |
|---|---|---|
| Agency Swarm | 角色公司 + 定向 `communication_flows` | 部门/角色编制（§3–§4） |
| 本仓 `kiana-tasks/src/swarm.rs` | WorkPacket、path lock、`git_worktree` 隔离路径 | **包格式**；还不是产品 |
| **ruflo-swarm**（`reference/ruflo/plugins/ruflo-swarm`、`v3/@claude-flow/swarm`） | 多工人协调：拓扑、worktree、MessageBus、agent pool | **v0.6 执行部并行 Builder** 的主教材 |

**对，并行 swarm 看 ruflo 比看 CrewAI / MetaGPT / Agency Swarm 更对。** 它已经把工人隔离和协调记录拆开：Codex/Claude 写代码，Ruflo 记协调。这和 Kiana「编排器是软件、工人是 harness」同构。

从 ruflo **要学的**（插件 README 的 anti-drift 默认几乎可直接当 v0.6 纪律）：

| ruflo | Kiana |
|---|---|
| `topology=hierarchical`，协调者抓发散 | 确定性编排器（软件，不是 LLM Queen）派包、收包 |
| `maxAgents` 6–8，专门化、不重叠 | 并行 Builder 默认 2–8；冲突路径串行 |
| 每 agent 一个 git worktree | 沿用 `swarm.rs` 的 `.kiana/swarm-worktrees/{dispatch}/{task}` |
| MessageBus + 有界 retry（他们刚修过 unreachable `message.failed`） | 包/事件，不是自由 IM；失败可见 |
| `agent_spawn` / terminate / pool / health | AgentInstance 生命周期 |
| 一回合能做完的事不要开 swarm | anti-meeting 的执行部版本 |
| `swarm-state` 独立 namespace | 执行部 RAG / packet 级记忆，不和 User RAG 混 |

从 ruflo **明确不学**：

- **TeamCreate / SendMessage** 当产品总线（插件 README 写明配对 Claude Code 这套；Kiana 已冻结）
- LLM **Queen** 当编排器（Queen 若存在，最多是执行部 Tech Lead 角色，仍出工件，不直接指挥写盘）
- Raft / Byzantine / Gossip / CRDT 当编码共识（评审投票用 DecisionRecord 即可）
- 默认 15 人、上限 100+、Flash Attention / MoE / Hive-Mind 集体记忆
- 工人共享 swarm memory（打穿角色 ACL，context rot）
- 把 ruflo 当工人运行时：它包的是 Claude Code / Codex，Kiana 工人仍是 `KianaHarness`

对照顺序：v0.2 一个 Builder → v0.3 规划会出一个包 → v0.6 才并行多个 Builder。没有路径锁和收据之前不要开 swarm。

---

## 10. 版本怎么切（每个阶段做什么、参考什么）

不另起 24-phase。执行集一次只打开一个版本。下表是**部门能力**叠在原产品阶梯上。

### v0.2 — Runnable Local Agent（已本地绿）

**部门现实：** 只有执行部的一个 Builder；收据带 `role=builder`（推荐同时带 `department=executing`）。  
**必须先真的：** 受信写盘、session continue/cancel、磁盘收据。  
**做什么：** 见 `PHASES.md` v0.2 Phase 1–4。  
**参考：** Codex/deepseek/pi harness；12-factor 2/5/6/9/12。  
**明确不做：** 招满编制、Symposium、RAG、MCP、拆 `cli.rs`。

### v0.3 — 公司内核（两部门 + 一次有界会）（当前）

**部门现实：** 规划部 + 执行部。一次 Symposium（PM+Architect）产出一个 packet，Builder 独立消费。  
**必须先真的：** 第一入口好用、eval、安装/升级/回滚。  
**本版工作包（详见 PHASES）：** RoleSpec 入库；编排器派独立 Builder；**Symposium WP**；eval；安装。  
**参考：** architect-loop；pm-skills DELEGATE；coleam00/Archon YAML + `context: fresh`；Agency Swarm 定向 communication_flows（只学 ACL，不学 SendMessage 总线）；autogen `RequestToSpeak` + max_rounds。  
**明确不做：** 五部门全开、部门 RAG、Reviewer、TeamCreate。

### v0.4 — Coding pack + 监控部最小编制

**部门现实：** 监控部出现：Reviewer ≠ 作者。角色包 = skills。Schr0d/Archon 形冲击半径可作为规划/监控的确定性工具。  
**必须先真的：** 公开行为矩阵草稿签字；MCP 若做，走 daemon。  
**参考：** Claude Code 公开行为（非 git dump 只对照行为）；cline/Codex MCP 时间线；architect-loop review；coleam00/Archon `archon-review-block`。  
**明确不做：** 企业 RBAC、用户宫殿记忆。

### v0.5 — 五部门 + 六层 RAG ACL

**部门现实：** 立项/规划/执行/监控/收尾都有磁盘工件；部门 RAG + 角色 RAG 上线；compact/resume 真能用。  
**必须先真的：** v0.4 核心路径可用。  
**参考：** pm-skills 全过程 skill；OpenSpec/GSD/planning-with-files 落盘；MemPalace/memorix；graphify/GitNexus 后期；coleam00/Archon v1 RAG 只作知识引擎形状。  
**明确不做：** 恢复 24-phase 语料；把聊天自动灌进 User RAG。

### v0.6 — 并行执行部 + 同一 Company API

并行 Builder + Integrator；SDK/IDE 只看见 Company API。路径锁与 worktree 必须先绿。  
并行调度主教材：`reference/ruflo/plugins/ruflo-swarm`（隔离、上限、层次拓扑），工人仍是 `KianaHarness`。

### v1.0 — 能交付的个人公司

立项到关闭有收据；安装升级文档；Coding pack 核心路径。Daily/Research 只有同一核已有黄金路径才算。

### v1.x — 多租户部门 / 企业 RBAC

个人公司已经能干活之后才做。Open Core 不掺企业契约。

**冻结加一条：** 不许用招满角色或上 RAG 来代替 Builder 会写文件。

---

## 11. 落到现有 crate

| 能力 | 用什么 | 不要用什么 |
|---|---|---|
| 工人循环 | `kiana-runner::KianaHarness` | `runner.rs` 上帝 loop |
| 副作用 | `kiana-daemon` broker | `kiana-tools` 注册表 |
| 部门/过程 | 重做 `kiana-workflow` + DepartmentSpec | 旧 24-phase 语料 |
| 工作包 / 并行 swarm | `kiana-tasks` WorkPacket + path lock + worktree 形状；v0.6 按 ruflo-swarm 把调度做真 | 把现有 swarm schema 当完成；ruflo Queen/BFT/SendMessage |
| Symposium | 新对象：私有 session + 黑板 + DecisionRecord | `kiana-coordinator` Team 工具；autogen GroupChat 融历史 |
| 角色提示 | `kiana-skills` 做成 Role pack / 部门 pack | `WORKER_AGENT` 常量 |
| 分层记忆 | `kiana-query` → MemoryBroker | 每个角色各扫一遍全仓；静默 prompt stuffing |
| 事件 | 磁盘 `kiana-eventlog`（Phase 3） | `MemoryEventLog` 当产品 |
| 策略 | `kiana-policy` 增加 role/department/knowledge | 全局 yolo |
| 冲击半径 | 规划/监控的确定性工具端口（后期可接 Schr0d/Archon） | 用 LLM 猜依赖 |

---

## 12. 两个 Archon 怎么用（直接回答「是不是比较适合」）

仓库里原来的 `reference/Archon` 是 **Schr0d/Archon**：多语言依赖/冲击半径 CLI，给 AI 的 plan-execute-review 加护栏。它**适合作为规划部 Architect 和监控部的确定性工具**，不适合当公司 OS。

新克隆的 `reference/Archon-Knowledge` 是 **coleam00/Archon**：YAML 工作流引擎，AI 节点与 bash 节点混排，节点可 `context: fresh`，人审批门，worktree 隔离，全审计。旧版「知识库 + RAG + 任务」在 `archive/v1-task-management-rag`。它**非常适合作为「过程是软件」的参考**，也接近「公司操作系统」的引擎层；但它：

- 没有 PMP 部门编制；
- 工人是外挂 Claude/Codex SDK，不是自有 harness；
- 单租户安装模型与 Kiana Open Core 不同；
- 群聊/角色 ACL/六层 RAG 不是它的产品中心。

**结论：** 把 coleam00/Archon 当**过程引擎教材**，把 Schr0d/Archon 当**架构门教材**，把 Agency Swarm / ChatDev 1.0 / pm-skills meeting family 当**组织与会议教材**。Kiana 的工人必须仍是 `KianaHarness`。

其它已克隆（第一批组织教材）：

| 目录 | 适合当什么 | 不适合当什么 |
|---|---|---|
| `reference/agency-swarm` | 定向 communication_flows、角色工具分家、先招一个 | 自由 SendMessage；handoff 复制全历史 |
| `reference/ChatDev` | 1.0 公司隐喻与阶段研讨会；2.0 图节点含 memory/human | 共享 ChatChain 运行时 |
| `reference/crewAI` | Crew vs Flow 分开：自治讨论 ≠ 事件过程 | 用 Crew 跑执行部日常写盘 |
| `reference/letta` / `letta-oss` | 归档说明：V1 服务器已 archive | 源码；与 `letta-code` 不是同一棵树 |

第二批（2026-08-22，补公司 OS / 记忆 / 隔离 / peer harness）：

| 目录 | 适合当什么 | 不适合当什么 |
|---|---|---|
| `reference/gastown` | **最接近 coding 公司**：Town/Rig、mailbox、identity、handoff、Beads 账本、Refinery 合并队列、Witness/Deacon watchdog、scheduler 容量阀 | Mayor 当上帝 agent；polecat 术语当产品语言；工人=Claude/Codex SDK；v0.2 扩到 20–30 agent |
| `reference/beads` | 项目记忆=可 claim 的依赖图；并行 Builder 抢包 | 替换 WorkPacket；当公司 OS |
| `reference/graphiti` | 六层 RAG 的时序图教材：事实有 `valid_at`/`invalid_at` 和 provenance | Python Graphiti 服务进 `kiana-core` |
| `reference/letta-code` | 记忆块、MemFS（git 跟踪上下文）、`memory-confinement`、agent-scoped skills | Letta Cloud；换掉 `KianaHarness` |
| `reference/openai-agents-python` | 类型化 handoff、guardrail、HITL、sandbox agent | SDK 当运行时；handoff 复制全历史（Agency Swarm 已踩过） |
| `reference/adk-python` | Sequential / Parallel / Loop **当软件**；部门过程不要即兴 | Parallel 无隔离写盘；Google ADK 当产品 |
| `reference/agno` | Team≠Agent；knowledge/memory/session 按队切 | Python 框架当核 |
| `reference/agent-framework` | Magentic 过程的 checkpoint + HITL | Magentic-One orchestrator 当 Mayor |
| `reference/pydantic-ai` | RoleContract=typed deps；`pydantic_graph` 当过程图 | Python 运行时 |
| `reference/a2a` | Role Card（发现+能力广告）+ 任务生命周期 | v0.2 实现完整 A2A 总线 |
| `reference/spec-kit` | constitution / specify / plan / tasks，规划部工件 | 替换 OpenSpec/GSD |
| `reference/claude-task-master` | PRD→任务图技能形状 | Node 任务板当运行时 |
| `reference/container-use` | v0.6 并行 Builder 的环境隔离+合并（补 ruflo worktree） | Dagger 当工人；v0.2 就上 |
| `reference/mini-swe-agent` | 黄金路径最小循环：线性历史、动作独立 | 丢掉 `apply_patch`；bash-only 当教条 |
| `reference/opencode` / `goose` / `crush` | 当代 peer harness：CLI+Desktop 同核、recipes、小 TUI | 换栈；安装脚本当完成 |
| `reference/gpt-pilot` | 最多对照「PRD→整应用」阶段名 | **禁止运行**（曾供应链投毒，已停更；HEAD 是清理提交） |

---

## 13. 参考索引（公司主题）

| 主题 | 读 |
|---|---|
| 为何拆角色/上下文 | `reference/architect-loop/DESIGN.md`、12-factor #10/#8/#3 |
| 过程当软件 / fresh node | `reference/Archon-Knowledge/README.md`、`.archon/workflows/defaults/archon-idea-to-pr.yaml`、`archon-architect.yaml`；`reference/adk-python` Sequential/Loop；`reference/pydantic-ai/docs/graph.md` |
| 冲击半径门 | `reference/Archon/README.md`、`skill.md`、`post-agent-integration.md` |
| PMP/产品过程与 skill | `reference/pm-skills/{README.md,agents,skills,_workflows}` |
| 会议族 | `reference/pm-skills/skills/foundation-meeting-{agenda,brief,recap,synthesize}/` |
| 定向组织通信 | `reference/agency-swarm/docs/core-framework/agencies/communication-flows.mdx`；`reference/openai-agents-python/docs` handoffs |
| 公司隐喻 | `reference/ChatDev/README-zh.md`（1.0 vs 2.0）；`reference/MetaGPT/metagpt/roles/` |
| **coding 公司编排（最接近）** | `reference/gastown/README.md`（Mayor/Town/Rig/Polecat/mailbox/hooks）；`docs/design/scheduler.md`、`docs/design/escalation.md`、`docs/concepts/molecules.md` |
| 角色发现 / 任务生命周期 | `reference/a2a/docs/topics/{agent-discovery.md,life-of-a-task.md}` |
| 有界发言权 | `reference/autogen/python/samples/core_distributed-group-chat/_agents.py`；Magentic-One orchestrator；`reference/agent-framework` Magentic checkpoint |
| 派工与独立 child | `reference/deepseek-harness/packages/subagent/README.md` |
| 执行部并行 swarm | `reference/ruflo/plugins/ruflo-swarm/README.md`、`v3/@claude-flow/swarm/{src/message-bus.ts,src/workers/worker-dispatch.ts,src/topology-manager.ts}`；`reference/container-use/{README.md,environment/}`；本仓 `kiana-tasks/src/swarm.rs` 只当包形状 |
| 规划部工件 | `reference/spec-kit/{README.md,templates,spec-driven.md}`；`reference/claude-task-master` parse-prd |
| 项目记忆 | `reference/memorix/README.md`、`reference/graphify`、`reference/GitNexus`、`reference/beads/{README.md,claim.go,issueops/}`、`kiana-query` |
| 时序 / 用户 / 宫殿记忆 | `reference/graphiti/README.md`、`graphiti_core/`；`reference/letta-code/{README.md,src/memory-confinement.ts}`；`reference/MemPalace/README.md` |
| 最小工人循环 | `reference/mini-swe-agent/src/minisweagent/agents/default.py`；对照本仓 `KianaHarness`（仍要 `apply_patch`） |
| 当代 peer harness | `reference/opencode`、`reference/goose`、`reference/crush` |
| 过程落盘 | OpenSpec、GSD、planning-with-files、architect-loop skills、spec-kit constitution |
| 反面 | `kiana-coordinator` Team 工具；ruflo 插件里的 TeamCreate/SendMessage、Queen/BFT/100-agent hive-mind；autogen/langchain 无界群聊；claude-code-rust 一天 1.0；ChatDev 共享 ChatChain；MetaGPT 全员广播；gastown Mayor 上帝；gpt-pilot **禁止运行** |

---

## 14. 下一动作

1. 接受本文件为北星：Kiana = 部门制 Company OS；PMP 是部门抽象层；群聊是 Symposium；RAG 分层带 ACL。
2. v0.2 Phase 1 discuss 现为四个产品决策 + 本组织模型已锁定（不在 Phase 1 实现五部门）。
3. 不要现在实现 8 角色、不要现在做 RAG、不要现在做群聊。RAG 在 v0.5，Symposium 在 v0.3，写盘在 v0.2。
4. 未要求前不提交这些设计文件。
