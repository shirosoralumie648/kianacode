# Volume 18: 参考仓库到功能落地剧本

## 1. 目标

参考仓库不能只停留在“看过”。每个参考必须转成 Kiana 的功能机制、边界和验收。

## 2. 落地模板

每个参考结论必须回答：

- 参考谁。
- 借鉴什么。
- 不借鉴什么。
- 落到哪个 Kiana 模块。
- P0/P1/P2 哪一阶段。
- 验收证据是什么。

## 3. Runtime 类参考

### 3.1 codex

借鉴：

- protocol。
- exec policy。
- sandbox/trust。
- MCP。

落地：

- PolicyDecision。
- tool registry。
- exec profile。

验收：

- shell 命令能解释 allow/ask/deny。

### 3.2 claude-code-rev-main

借鉴：

- query loop。
- tool_use/tool_result pairing。
- structured IO。

落地：

- runtime event。
- tool call pairing。
- orphan tool result repair。

### 3.3 cline/Roo-Code

借鉴：

- patch apply。
- checkpoint。
- safe edit。
- worktree/Kanban。

落地：

- file edit policy。
- checkpoint。
- path lock。

## 4. Project OS 类参考

### 4.1 planning-with-files

借鉴：

- task_plan。
- findings。
- progress。
- state。

落地：

- `.kiana/workflows/<id>/`。
- persistent planning。

### 4.2 get-shit-done

借鉴：

- capture/spec/plan/execute/validate/ship/review。

落地：

- WorkflowRun phases。
- Project commands。

### 4.3 gstack

借鉴：

- plan review。
- eng review。
- QA。
- ship。
- retro。

落地：

- acceptance gates。
- review synthesis。
- delivery summary。

### 4.4 architect-loop

借鉴：

- orchestrator/strategist/builder。
- fresh context。
- frozen checks。
- typed evidence。

落地：

- bounded swarm。
- WorkPacket。
- verification before integration。

边界：

- Kiana 不采用无 approval gate 的高风险默认。

## 5. Memory 类参考

### 5.1 memorix

借鉴：

- Observation Memory。
- Reasoning Memory。
- Git Memory。
- orchestration locks。
- dashboard。

落地：

- memory layers。
- memory proposal。
- stale memory。

### 5.2 claude-memory / MemPalace

借鉴：

- session ingest。
- local memory。
- semantic/full-text search。

落地：

- ContextPack。
- source/confidence。

## 6. Repo Intelligence 类参考

### 6.1 GitNexus

借鉴：

- analyze。
- context。
- impact。
- trace。
- detect_changes。
- staleness。

落地：

- repo map。
- impact analysis。
- context gate staleness。

### 6.2 graphify

借鉴：

- graph.json。
- GRAPH_REPORT。
- EXTRACTED/INFERRED/AMBIGUOUS。

落地：

- graph confidence。
- ambiguous review。

### 6.3 aider

借鉴：

- repo map。
- git-aware edit。

落地：

- edit scope。
- diff safety。

## 7. Multi-Agent 类参考

### 7.1 autogen

借鉴：

- worker protocol。
- termination。

边界：

- 不采用自由 speaker 群聊。

### 7.2 MetaGPT

借鉴：

- role/action。

边界：

- 不堆角色名。

### 7.3 ruflo

借鉴：

- workflow。
- swarm。
- autopilot。
- cost。
- witness。

落地：

- bounded swarm。
- usage ledger。
- evidence witness。

## 8. Ecosystem 类参考

### 8.1 everything-claude-code

借鉴：

- commands。
- skills。
- agents。
- rules。
- review。
- testing。

落地：

- capability map。
- plugin taxonomy。

### 8.2 ECC

借鉴：

- cross-harness packaging。
- operator status。
- AgentShield/security。
- MCP policy。

落地：

- plugin trust。
- status payload。
- security readiness。

### 8.3 ai-coding-guide

借鉴：

- 中文教程式 onboarding。
- 权限/安全/MCP/subagent/worktree 教学。

落地：

- docs/DX。
- `/help` 风格。

## 9. Security 类参考

### 9.1 Strix

借鉴：

- validated findings。
- PoC evidence。
- security report。

落地：

- security finding evidence。
- critical blocker。

边界：

- P0 不自动做攻击性扫描。

## 10. UI 类参考

参考：

- pi。
- emdash。
- herdr。
- OpenHands。
- ECC HUD。

落地：

- statusline。
- tool cards。
- workflow board。
- evidence timeline。

边界：

- UI 不替代 core protocol。

## 11. EDA 落地

EDA 没有单一参考仓库。

来源：

- 用户明确方向。
- Project OS。
- Review gates。
- Policy model。

落地：

- `/eda review`。
- BOM risk。
- DFM checklist。
- bring-up plan。

## 12. 参考吸收验收

一个参考机制进入 Kiana backlog 前必须有：

- Kiana 模块。
- phase。
- schema/command。
- evidence。
- risk boundary。
