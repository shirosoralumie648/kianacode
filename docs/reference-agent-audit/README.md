# 参考 Agent 审计

这套审计从 `reference/` 中 26 个主要 Agent/Agent Framework 项目的源码、测试、manifest 和 executable entrypoint 追踪请求全流程。审计代理被要求跳过 README、CLAUDE/AGENTS/USER、docs、changelog 和 git history。

`reference/` 下还有数十个目录没有结构化审计（如 grok-build、temporal-sdk-python、beads、graphiti、mem0、container-use、gastown、a2a、spec-kit、OpenSpec 等）。**目录里有不等于已审计**：只有本表列出的 26 份报告才算结构化审计，其余在参考矩阵里只作目录级参考；补审计是待办，见文末。

## 覆盖状态

| # | 项目 | 状态 | 最后核验日 | 基准 commit | 报告 |
|---:|---|---|---|---|---|
| 1 | `codex` | 已过时，待重审 | 2026-08-25/26 | `d6489472f3` | [codex](01-codex.md) |
| 2 | `deepseek-harness` | 已过时，待重审 | 2026-08-25/26 | `c389f96bf` | [deepseek-harness](02-deepseek-harness.md) |
| 3 | `claude-code-rust` | 已核验仍有效（0 提交） | 2026-08-25/26 | `4b87a363` | [claude-code-rust](03-claude-code-rust.md) |
| 4 | `aider` | 已核验仍有效（0 提交） | 2026-08-25/26 | `5dc9490bb` | [aider](04-aider.md) |
| 5 | `cline` | 完整结构化审计（已漂移，未重审） | 2026-08-25/26 | `fc28a5fe3` | [cline](05-cline.md) |
| 6 | `openhands` | 完整结构化审计（已漂移，未重审） | 2026-08-25/26 | `f7fb0c4b2` | [openhands](06-openhands.md) |
| 7 | `goose` | 已过时，待重审 | 2026-08-25/26 | `5e90925` | [goose](07-goose.md) |
| 8 | `opencode` | 边缘漂移，只核对被引用路径 | 2026-08-25/26 | `d6855b6` | [opencode](08-opencode.md) |
| 9 | `continue` | 已核验仍有效（0 提交） | 2026-08-25/26 | `5522c6f44` | [continue](09-continue.md) |
| 10 | `roo-code` | 已核验仍有效（0 提交） | 2026-08-25/26 | `b867ec914` | [roo-code](10-roo-code.md) |
| 11 | `crush` | 边缘漂移，只核对被引用路径 | 2026-08-25/26 | `563d658` | [crush](11-crush.md) |
| 12 | `pi` | 完整结构化审计（已漂移，未重审） | 2026-08-25/26 | `96617628e` | [pi](12-pi.md) |
| 13 | `mini-swe-agent` | 完整结构化审计（已漂移，未重审） | 2026-08-25/26 | `04d809c` | [mini-swe-agent](13-mini-swe-agent.md) |
| 14 | `gpt-pilot` | 已核验仍有效（0 提交） | 2026-08-25/26 | `9b763fd` | [gpt-pilot](14-gpt-pilot.md) |
| 15 | `agent-framework` | 已过时，待重审 | 2026-08-25/26 | `aea4dc2` | [agent-framework](15-agent-framework.md) |
| 16 | `adk-python` | 已过时，待重审 | 2026-08-25/26 | `b018062` | [adk-python](16-adk-python.md) |
| 17 | `openai-agents-python` | 完整结构化审计（已漂移，未重审） | 2026-08-25/26 | `f355af6` | [openai-agents-python](17-openai-agents-python.md) |
| 18 | `pydantic-ai` | 完整结构化审计（已漂移，未重审） | 2026-08-25/26 | `62f1e83` | [pydantic-ai](18-pydantic-ai.md) |
| 19 | `autogen` | 已核验仍有效（0 提交） | 2026-08-25/26 | `027ecf0a3` | [autogen](19-autogen.md) |
| 20 | `agency-swarm` | 完整结构化审计（已漂移，未重审） | 2026-08-25/26 | `5cd5a0d` | [agency-swarm](20-agency-swarm.md) |
| 21 | `agno` | 完整结构化审计（已漂移，未重审） | 2026-08-25/26 | `f974c17` | [agno](21-agno.md) |
| 22 | `crewAI` | 完整结构化审计（已漂移，未重审） | 2026-08-25/26 | `34199c2` | [crewAI](22-crewai.md) |
| 23 | `ChatDev` | 已核验仍有效（0 提交） | 2026-08-25/26 | `4fb2db0` | [ChatDev](23-chatdev.md) |
| 24 | `MetaGPT` | 已核验仍有效（0 提交） | 2026-08-25/26 | `11cdf466` | [MetaGPT](24-metagpt.md) |
| 25 | `letta-code` | 完整结构化审计（已漂移，未重审） | 2026-08-25/26 | `6bc41be` | [letta-code](25-letta-code.md) |
| 26 | `12-factor-agents` | 已核验仍有效（0 提交） | 2026-08-25/26 | `d20c728` | [12-factor-agents](26-12-factor-agents.md) |

> **核验口径**：`最后核验日` 是报告内容最后一次对照源码的日期（本批结构化审计为 2026-08-25/26）；`基准 commit` 是该次核验时 `reference/<项目>` 的 HEAD。2026-09-08 只复核了各仓库的新鲜度，**没有重审全文**。
>
> - **已核验仍有效（0 提交）**：自审计日以来无新提交，结论仍适用——roo-code / Roo-Code、aider、continue、claude-code-rust、gpt-pilot、autogen、ChatDev、MetaGPT、12-factor-agents（9 份报告；`Roo-Code` 与 `roo-code` 同源）。
> - **边缘漂移，只核对被引用路径**：crush、opencode 有提交，但集中在 legal / docs / UI 或新增包；只需核对 Kiana 实际引用的路径（crush 的 cancel / permission、opencode 的 event projector），不做全量重审。
> - **已过时，待重审**：codex、deepseek-harness、adk-python、goose、agent-framework，顺序见文末待办。
> - **已漂移，未重审**：其余有提交的项目按轻量维护处理（决策记录 #8：结构化审计 + 最后核验日 + 只核对被引用路径），需要时再单独排期。

## 阅读顺序

1. 先看 [统一 Agent 流程图](00-unified-agent-flow.md)。
2. 再看各项目报告，重点阅读“状态、工具、风险、Kiana 映射”。
3. 最后看 [Kiana 映射与实施顺序](99-kiana-mapping.md)。

## 证据规则

- `audited` 只表示该代理返回了结构化源码审计，不表示项目本身没有缺陷。
- `partial/unavailable` 表示审计没有完成，不能把它解释成“未发现问题”。
- 报告中的风险是源码证据推导出的工程判断，影响和优先级属于审计分析。

## 待办：重审与补审计（本轮不执行）

以下按改进建议书 S-1 / S-2 / S-3 / S-6 登记。**本轮只加了「最后核验日 / 基准 commit」列，没有执行任何重审或补审计。**

### 1. 重审已过时的结构化审计（S-1，5 份）

顺序按决策记录 #7：

1. `01-codex.md`（审批 P0）
2. `02-deepseek-harness.md`（P0 主参考）
3. 新增 `27-grok-build.md`（检查点 / 确定性，见下）
4. 其余按 S-1 清单排后：`07-goose.md`、`16-adk-python.md`、`15-agent-framework.md`

重审只取审批归属与状态机形状，不引入 Windows 服务化 / 远程 provisioning / 图编排主线。

### 2. 新增 grok-build 审计（S-2）

新增 `27-grok-build.md`：xai-workflow journal（req_hash + 稠密 seq + Divergence fail-closed）、xai-hunk-tracker 的 hunk 归因、CoW worktree 快照、Landlock / Seatbelt 沙箱、events.jsonl。只取 workflow / journal / hunk / worktree / sandbox 形状，不引入其 provider、voice、dashboard、遥测与远程服务。

### 3. 补审计（S-3）

按能力补结构化审计，只取形状、不引入依赖：temporal-sdk-python（本地 history-replay 语义）、container-use（environment 状态机与「所有副作用经 environment」）、beads（blocked / claim 谓词与冲突类型）、graphiti / mem0（时间 schema 与 op 生命周期）、a2a（TaskState 映射与 artifact / part 形状）、Archon-Knowledge（DAG + isolation 后端路由）、spec-kit / OpenSpec（工件模板链与 validate）。

### 4. 低优先补审计（S-6）

superpowers、planning-with-files、claude-task-master、graphify、gastown。只取形状：按需技能索引（只读注入，不得成为执行路径）、attest 校验、依赖建模、extractor 接口与 cache freshness、checkpoint 字段与 estop 熔断。gastown 的 mail / nudge / mayor 属于冻结的自由消息总线，只取 checkpoint / estop。

### 5. 明确排除

`promptfoo-full` 的远程地址配置指向本仓自身，无法核对上游，不作为参考来源，也不纳入补审计。
