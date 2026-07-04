# claude-code-main Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/claude-code-main (2)/claude-code-main` 不是完整产品源码，更像公开插件、skills、hooks、settings 和安全示例集合。它主要应影响 Kiana 的 plugin marketplace、skill authoring、hook lifecycle、agent definition、command markdown 和 settings examples，而不是 core runtime。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| Plugin structure | 插件提供 README、commands、agents、skills、hooks、settings 示例 | `reference/claude-code-main (2)/claude-code-main/plugins/README.md`, `reference/claude-code-main (2)/claude-code-main/plugins/plugin-dev/skills/plugin-structure/SKILL.md`, `reference/claude-code-main (2)/claude-code-main/plugins/plugin-dev/skills/plugin-structure/references/manifest-reference.md` | 文档定义 plugin manifest、目录约定和组件类型 | `kiana-types/src/plugin.rs`, `kiana-commands/src/plugin.rs` | 是 | 示例可参考，manifest 字段需映射到 Kiana 自有 schema |
| Command markdown | 插件命令作为 markdown prompt/command 文件安装 | `reference/claude-code-main (2)/claude-code-main/plugins/commit-commands/commands/commit.md`, `reference/claude-code-main (2)/claude-code-main/plugins/pr-review-toolkit/commands/review-pr.md`, `reference/claude-code-main (2)/claude-code-main/plugins/plugin-dev/skills/command-development/SKILL.md` | command 文件带 frontmatter 和 prompt body；命令系统发现并执行 | `kiana-commands/src/plugin.rs`, `kiana-commands/src/registry.rs` | 是 | 只借 command contract，不复制 prompt 文案 |
| Skill authoring | SKILL.md 说明触发条件、引用资料和工作流 | `reference/claude-code-main (2)/claude-code-main/plugins/frontend-design/skills/frontend-design/SKILL.md`, `reference/claude-code-main (2)/claude-code-main/plugins/plugin-dev/skills/skill-development/SKILL.md` | skill 以 markdown 描述能力，必要时引用 references/scripts | `kiana-skills/src/loader.rs`, `kiana-tools/src/discover_skills.rs` | 是 | 文案 license 和品牌指令不可搬 |
| Hook lifecycle examples | Hookify 和 security 插件展示 PreToolUse/PostToolUse/UserPromptSubmit/Stop | `reference/claude-code-main (2)/claude-code-main/plugins/hookify/hooks/hooks.json`, `reference/claude-code-main (2)/claude-code-main/plugins/hookify/hooks/pretooluse.py`, `reference/claude-code-main (2)/claude-code-main/plugins/hookify/hooks/posttooluse.py`, `reference/claude-code-main (2)/claude-code-main/examples/hooks/bash_command_validator_example.py` | hooks.json 声明 lifecycle 事件，脚本读取 stdin JSON 并输出 decision/context | `kiana-query/src/stop_hooks.rs`, `kiana-commands/src/hooks.rs` | 是 | Python examples 不应作为 Kiana runtime 依赖 |
| Agent packs | 插件提供 code explorer/reviewer/architect 等 agent markdown | `reference/claude-code-main (2)/claude-code-main/plugins/feature-dev/agents/code-explorer.md`, `reference/claude-code-main (2)/claude-code-main/plugins/feature-dev/agents/code-reviewer.md`, `reference/claude-code-main (2)/claude-code-main/plugins/plugin-dev/skills/agent-development/SKILL.md` | agent 定义包含角色、工具、触发和系统提示 | `kiana-tools/src/agent.rs`, `kiana-commands/src/agent` equivalent via `agents` command | 部分 | 角色可借鉴，prompt 内容不复制 |
| Settings examples | strict/lax/bash-sandbox settings 展示用户配置组合 | `reference/claude-code-main (2)/claude-code-main/examples/settings/settings-strict.json`, `reference/claude-code-main (2)/claude-code-main/examples/settings/settings-lax.json`, `reference/claude-code-main (2)/claude-code-main/examples/settings/settings-bash-sandbox.json` | 示例展示 permission、sandbox、policy 的配置形态 | `kiana-bootstrap/src/config.rs`, `kiana-tools/src/bash_sandbox.rs` | 是 | 字段名需兼容但不强行复制所有设置 |

## 3. 值得概念性借鉴的实现模式

### Pattern: Plugin as resource bundle

来源文件：
- `reference/claude-code-main (2)/claude-code-main/plugins/plugin-dev/skills/plugin-structure/SKILL.md`
- `reference/claude-code-main (2)/claude-code-main/plugins/plugin-dev/skills/plugin-structure/references/component-patterns.md`
- `reference/claude-code-main (2)/claude-code-main/plugins/commit-commands/README.md`

机制摘要：
- 一个插件可以同时带 commands、agents、skills、hooks、settings。
- 每种资源有独立目录和发现规则。
- manifest 只描述插件元信息和可见性。

Kiana 可借鉴方式：
- 继续让 `kiana-types/src/plugin.rs` 做 manifest 解析。
- 安装/禁用插件时刷新 commands、skills、hooks、MCP、LSP 可见性缓存。

不应该照搬的部分：
- 不复制 Anthropic marketplace metadata。
- 不复制 plugin prompt 文件。

### Pattern: Hooks as stdin/stdout contracts

来源文件：
- `reference/claude-code-main (2)/claude-code-main/plugins/hookify/hooks/hooks.json`
- `reference/claude-code-main (2)/claude-code-main/plugins/plugin-dev/skills/hook-development/SKILL.md`
- `reference/claude-code-main (2)/claude-code-main/plugins/plugin-dev/skills/hook-development/references/patterns.md`

机制摘要：
- hook runner 通过 stdin 传入 JSON。
- hook 脚本输出 JSON decision、reason、additionalContext 或 updateInput。
- runtime 根据输出决定继续、阻断、询问或注入上下文。

Kiana 可借鉴方式：
- 固化 hook payload schema。
- 在 Kiana tests 中使用小 shell/Python fixture，不把 Python runtime 作为产品依赖。

不应该照搬的部分：
- 不把 hookify 的 rule engine 移植进核心。
- 不让 hook 输出直接修改文件。

## 4. Behavior Contracts to Port into Kiana

### Contract: Plugin Install Resource Visibility

Input:
- plugin source path/url/git
- plugin manifest
- enable/disable state

Decision:
- manifest 是否有效。
- project 是否 trusted。
- plugin 是否 enabled。

Execution:
- materialize plugin 到 Kiana plugin dir。
- 发现 commands/skills/hooks/agents/MCP/LSP。
- 刷新进程内缓存。

Output:
- plugin list/status。
- 新资源可被 CLI/REPL/TUI 发现。

Runtime Events:
- SessionEvent
- RuntimeError

Acceptance Tests:
- install 后 skill list 能看到插件 skill。
- disable 后 commands/skills/hooks 不再可见。
- untrusted project 不加载 project marketplace。

### Contract: Hook Script JSON Outcome

Input:
- lifecycle event name
- cwd/session/tool/user prompt payload
- configured command

Decision:
- 是否允许执行 hook source。
- hook 输出是否为合法 JSON。
- decision 是否 block/deny/ask/allow/update_input/add_context。

Execution:
- spawn hook command with timeout。
- parse stdout JSON。
- merge allowed outcome into runner/tool context。

Output:
- allowed, denied, ask, update_input, add_context 或 hook error。

Runtime Events:
- SessionEvent
- PermissionRequest
- RuntimeError

Acceptance Tests:
- invalid hook JSON 不崩溃并产生错误。
- SessionStart add_context 进入 system context。
- PreToolUse deny 阻断 mutating tool。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| Plugin install | 已支持 URL/git/github/git-subdir marketplace 主链路，并支持 cached `npm` `file:` 离线包源 | 真实 npm registry、pip plugin source、真实 GitHub marketplace live smoke 未补 | `kiana-commands/src/plugin.rs`, `kiana-types/src/plugin.rs` | local http/git/npm-file + opt-in live smoke | P2 |
| Resource cache | plugin skill cache 已按 enabled roots 刷新 | commands/hooks/agents/MCP/LSP 全资源 disable visibility 需补齐 | `kiana-skills/src`, `kiana-commands/src/plugin.rs`, `kiana-tools/src/lsp_tool.rs` | resource visibility matrix | P1 |
| Hook CLI | 已有 hooks add/remove/status | hook schema validation 与 timeout/error event 需更明确 | `kiana-commands/src/hooks.rs`, `kiana-query/src/stop_hooks.rs` | malformed hook fixtures | P1 |
| Agent definitions | 已支持多来源 agent discovery 和 built-ins | background agent lifecycle、auto-memory 完整语义未完成 | `kiana-tools/src/agent.rs` | agent runtime fixtures | P2 |

## 6. Atomic Implementation Tasks

### Task: Plugin Disable Visibility Fixture Pack

Goal:
- 确保插件 disabled 后其 commands、skills、hooks、agents、MCP、LSP 都从当前进程消失。

Scope:
- 允许修改 `kiana-skills/src`, `kiana-commands/src/plugin.rs`, `kiana-tools/src/agent.rs`, `kiana-tools/src/lsp_tool.rs`。
- 不允许重写 plugin install 流程。

Implementation Notes:
- 使用本地临时插件 fixture。
- 先测试，再小修缓存 key。

Acceptance Criteria:
- disable 后同进程 list 不显示资源。
- enable 后资源恢复。
- project untrusted 时 project plugin resources 不加载。

Tests:
- `cargo test -p kiana-skills --lib`
- `cargo test -p kiana-commands plugin`
- `cargo test -p kiana-tools --lib`

Manual Verification:
- `kiana plugin install <local-fixture>`
- `kiana plugin disable <name>`

### Task: Hook JSON Schema Diagnostics

Goal:
- 对 hook stdin/outcome 增加可测试诊断，避免 silent failure。

Scope:
- 允许修改 `kiana-query/src/stop_hooks.rs`, `kiana-commands/src/hooks.rs`。
- 不允许新增 hook language runtime。

Implementation Notes:
- 输出错误要包含 event、hook source、exit status 或 parse error。
- 保留现有 allow/deny/ask/update_input/add_context。

Acceptance Criteria:
- invalid JSON 有 RuntimeError。
- timeout 有明确错误。
- non-zero exit 可配置为 deny 或 warning。

Tests:
- `cargo test -p kiana-query hook`
- `cargo test -p kiana-commands hooks`
