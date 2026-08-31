# 参考 Agent 审计

这套审计从 `reference/` 中 26 个主要 Agent/Agent Framework 项目的源码、测试、manifest 和 executable entrypoint 追踪请求全流程。审计代理被要求跳过 README、CLAUDE/AGENTS/USER、docs、changelog 和 git history。

## 覆盖状态

| # | 项目 | 状态 | 报告 |
|---:|---|---|---|
| 1 | `codex` | 完整结构化审计 | [codex](01-codex.md) |
| 2 | `deepseek-harness` | 完整结构化审计 | [deepseek-harness](02-deepseek-harness.md) |
| 3 | `claude-code-rust` | 完整结构化审计 | [claude-code-rust](03-claude-code-rust.md) |
| 4 | `aider` | 完整结构化审计 | [aider](04-aider.md) |
| 5 | `cline` | 完整结构化审计 | [cline](05-cline.md) |
| 6 | `openhands` | 完整结构化审计 | [openhands](06-openhands.md) |
| 7 | `goose` | 完整结构化审计 | [goose](07-goose.md) |
| 8 | `opencode` | 完整结构化审计 | [opencode](08-opencode.md) |
| 9 | `continue` | 完整结构化审计 | [continue](09-continue.md) |
| 10 | `roo-code` | 完整结构化审计 | [roo-code](10-roo-code.md) |
| 11 | `crush` | 完整结构化审计 | [crush](11-crush.md) |
| 12 | `pi` | 完整结构化审计 | [pi](12-pi.md) |
| 13 | `mini-swe-agent` | 完整结构化审计 | [mini-swe-agent](13-mini-swe-agent.md) |
| 14 | `gpt-pilot` | 完整结构化审计 | [gpt-pilot](14-gpt-pilot.md) |
| 15 | `agent-framework` | 完整结构化审计 | [agent-framework](15-agent-framework.md) |
| 16 | `adk-python` | 完整结构化审计 | [adk-python](16-adk-python.md) |
| 17 | `openai-agents-python` | 完整结构化审计 | [openai-agents-python](17-openai-agents-python.md) |
| 18 | `pydantic-ai` | 完整结构化审计 | [pydantic-ai](18-pydantic-ai.md) |
| 19 | `autogen` | 完整结构化审计 | [autogen](19-autogen.md) |
| 20 | `agency-swarm` | 完整结构化审计 | [agency-swarm](20-agency-swarm.md) |
| 21 | `agno` | 完整结构化审计 | [agno](21-agno.md) |
| 22 | `crewAI` | 完整结构化审计 | [crewAI](22-crewai.md) |
| 23 | `ChatDev` | 完整结构化审计 | [ChatDev](23-chatdev.md) |
| 24 | `MetaGPT` | 完整结构化审计 | [MetaGPT](24-metagpt.md) |
| 25 | `letta-code` | 完整结构化审计 | [letta-code](25-letta-code.md) |
| 26 | `12-factor-agents` | 完整结构化审计 | [12-factor-agents](26-12-factor-agents.md) |

## 阅读顺序

1. 先看 [统一 Agent 流程图](00-unified-agent-flow.md)。
2. 再看各项目报告，重点阅读“状态、工具、风险、Kiana 映射”。
3. 最后看 [Kiana 映射与实施顺序](99-kiana-mapping.md)。

## 证据规则

- `audited` 只表示该代理返回了结构化源码审计，不表示项目本身没有缺陷。
- `partial/unavailable` 表示审计没有完成，不能把它解释成“未发现问题”。
- 报告中的风险是源码证据推导出的工程判断，影响和优先级属于审计分析。
