# Volume 06: Context、Memory 与 Repo Intelligence

## 1. ContextPack 定义

ContextPack 是一次执行前的可审计上下文包。它不是把所有文件塞进 prompt，而是把当前任务需要的事实、证据、约束和风险压缩成可追溯输入。

ContextPack 包含：

- goal。
- task。
- current state。
- live repo evidence。
- memory summary。
- relevant files。
- relevant commands。
- stack profile。
- risks。
- NOT_BUILDING。
- verification strategy。

## 2. ContextPack 来源

| 来源 | 用途 | 置信度 |
| --- | --- | --- |
| live files | 当前事实 | highest |
| git state | dirty/head/branch | highest |
| command output | 当前验证 | high |
| schema docs | contract | high |
| eventlog | workflow 历史 | high |
| memory | prior learnings | medium |
| old reports | historical | low unless verified |

## 3. Memory 层级

Kiana 采用分层 memory：

### 3.1 Observation Memory

记录：

- what changed。
- bug/fix。
- project gotcha。
- command behavior。
- test failure pattern。

### 3.2 Reasoning Memory

记录：

- 为什么选这个方案。
- 替代方案。
- tradeoff。
- 风险接受。
- 用户偏好。

### 3.3 Git Memory

记录：

- commit-derived facts。
- changed files。
- feature landed。
- tests passed。
- blockers removed。

### 3.4 Workflow Memory

记录：

- workflow goal。
- blockers。
- current next。
- verification commands。
- report history。

## 4. Memory 写入规则

允许写入：

- 有 evidence 的结论。
- 用户明确偏好。
- 已验证的命令行为。
- 已解决的问题模式。

禁止写入：

- 猜测。
- 未验证实现完成。
- 低置信推断。
- 过时事实。

Memory entry 字段：

- id。
- type。
- text。
- source。
- confidence。
- created_at。
- last_verified_at。
- stale_reason。
- related_files。

## 5. Stale Memory

stale 触发：

- git head changed。
- referenced file missing。
- tests changed。
- schema changed。
- user contradicted memory。
- age threshold exceeded。

行为：

- 降权。
- 标注 stale。
- 要求 live verification。
- 不直接注入执行 prompt。

## 6. Repo Intelligence

Repo Intelligence 的目标是让 Kiana 知道：

- 哪些文件相关。
- 哪些符号相关。
- 哪些测试相关。
- 修改会影响谁。
- 哪些路径高风险。

## 7. Repo Map

输入：

- task goal。
- changed files。
- search query。
- symbol names。
- history。

输出：

- ranked files。
- related tests。
- related docs。
- impacted modules。
- confidence。

Ranking 信号：

- path match。
- symbol match。
- imports。
- call graph。
- test references。
- git co-change。
- recent failures。
- memory references。
- graph confidence。

## 8. GitNexus 吸收点

GitNexus 贡献：

- repo analyze。
- knowledge graph。
- context。
- impact。
- trace。
- detect_changes。
- staleness。
- MCP resource/tool model。

Kiana 吸收：

- `impact` 类能力进入 repo impact。
- `trace` 类能力进入 symbol path。
- `detect_changes` 类能力进入 changed-lines -> affected tasks。
- staleness 检查进入 Context Gate。

不吸收：

- 把 graph 当唯一真相。
- 默认要求用户安装 GitNexus。
- 让 repo graph 越权读取。

## 9. graphify 吸收点

graphify 贡献：

- local graph。
- `graph.json`。
- `GRAPH_REPORT.md`。
- `graph.html`。
- EXTRACTED/INFERRED/AMBIGUOUS confidence。

Kiana 吸收：

- graph edge confidence。
- ambiguous edge human review。
- report as evidence artifact。
- graph output as optional repo intelligence artifact。

## 10. Search 模型

搜索层级：

1. exact file path。
2. symbol search。
3. text search。
4. repo map ranking。
5. graph relation。
6. memory search。

搜索输出必须带来源：

- file。
- line。
- symbol。
- memory id。
- confidence。

## 11. Context Budget

ContextPack 不应无限膨胀。

预算策略：

- goal and constraints always included。
- current task always included。
- relevant files summarized。
- full file only when necessary。
- memory compressed。
- command output tail only。
- large graph summarized。

## 12. Context Gate

Pass 条件：

- live evidence fresh。
- stack detected。
- commands known or missing commands recorded。
- dirty state classified。
- memory stale status known。

Fail/Blocked：

- missing target files。
- repo changed significantly。
- dirty state unsafe。
- context too large。
- critical unknown。

## 13. ContextPack 输出结构

章节：

- Goal。
- Current State。
- Task。
- Scope。
- Live Evidence。
- Relevant Files。
- Relevant Commands。
- Memory。
- Risks。
- NOT_BUILDING。
- Verification。
- Open Questions。

## 14. Memory 与 Evidence 的关系

Memory 可以建议：

- 可能相关文件。
- 以前失败原因。
- 用户偏好。
- 常用命令。

Memory 不能证明：

- 当前代码通过测试。
- 当前 release 可用。
- 当前 bug 已修。
- 当前安全无风险。

证明只能来自 evidence。
