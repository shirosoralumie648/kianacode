# continue Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/continue` 对 Kiana 的重点价值是 provider/config YAML、OpenAI-compatible adapters、rules/prompts/tools 配置、terminal command security、MCP customization、headless/TUI CLI 和 code indexing/RAG。Kiana 应借鉴配置合同和安全评估边界，不应移植 Continue 的 IDE extension 状态管理或云索引产品逻辑。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| Headless/TUI CLI | 用户可用 headless 或 TUI 模式运行 Continue | `reference/continue/docs/cli/headless-mode.mdx`, `reference/continue/docs/cli/tui-mode.mdx`, `reference/continue/extensions/cli/src/config.ts`, `reference/continue/extensions/cli/src/e2e/headless-mock-llm.test.ts` | CLI 加载 config 后进入 headless/TUI，e2e 使用 mock LLM | `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/tui.rs` | 是 | CLI 产品语义不同，只借 mock e2e |
| Config/rules/prompts/models | 用户通过配置声明 models、rules、prompts、tools | `reference/continue/docs/guides/configuring-models-rules-tools.mdx`, `reference/continue/docs/customize/models.mdx`, `reference/continue/docs/customize/rules.mdx`, `reference/continue/docs/customize/prompts.mdx`, `reference/continue/packages/config-yaml/src/__tests__/index.test.ts` | YAML config parser 输出统一配置对象，docs 作为 public contract | `kiana-services/src/config.rs`, `kiana-entrypoints/src/cli.rs` | 是 | Kiana 需保 TOML/env 兼容，不应切到 YAML-only |
| Provider adapters | OpenAI-compatible API 被适配到多 provider | `reference/continue/packages/openai-adapters/src/index.ts`, `reference/continue/packages/openai-adapters/src/apis/Anthropic.ts`, `reference/continue/packages/openai-adapters/src/apis/OpenAI.ts`, `reference/continue/packages/openai-adapters/src/apis/Mock.ts`, `reference/continue/docs/customize/model-providers/top-level/ollama.mdx` | adapter 层统一 chat/completions 差异，Mock 用于测试 | `kiana-services/src/api`, `kiana-commands/src/model.rs` | 是 | 具体 API payload 不复制，避免过时 |
| Terminal security | 执行命令前评估风险等级 | `reference/continue/packages/terminal-security/src/evaluateTerminalCommandSecurity.ts`, `reference/continue/packages/terminal-security/test/terminalCommandSecurity.test.ts` | 独立包判断命令安全级别，tests 覆盖危险命令 | `kiana-tools/src/exec_policy.rs`, `kiana-tools/src/bash_sandbox.rs` | 是 | 规则需适配 PowerShell/Windows |
| MCP customization | 用户配置 MCP servers 和工具暴露 | `reference/continue/docs/reference/continue-mcp.mdx`, `reference/continue/docs/customize/mcp-tools.mdx`, `reference/continue/docs/customize/deep-dives/mcp.mdx` | docs 定义 MCP config、server、tool 使用方式 | `kiana-services/src/mcp.rs`, `kiana-tools/src/mcp_tool.rs` | 部分 | 文档行为可借，具体实现需看 Kiana MCP |
| PR review/checks | 用户可让 CLI review diff 或用于 GitHub PR bot | `reference/continue/extensions/cli/src/commands/review/diffContext.ts`, `reference/continue/docs/guides/github-pr-review-bot.mdx` | diff context 收集代码差异，review bot 调用模型 | `kiana-commands/src/review.rs`, `kiana-commands/src/checks.rs` | 是 | GitHub bot 可后置 |
| Indexing/RAG | 用户可索引 codebase 并进行上下文检索 | `reference/continue/core/indexing/README.md`, `reference/continue/gui/src/pages/config/features/indexing/IndexingProgress.tsx`, `reference/continue/docs/guides/custom-code-rag.mdx` | indexing pipeline 与 GUI progress 分离 | `kiana-query/src/repo_map.rs`, future `kiana-query` RAG | 部分 | 向量索引成本和隐私边界需明确 |

## 3. 值得概念性借鉴的实现模式

### Pattern: Config as capability graph

来源文件：
- `reference/continue/docs/guides/configuring-models-rules-tools.mdx`
- `reference/continue/packages/config-yaml/src/__tests__/index.test.ts`
- `reference/continue/docs/customize/rules.mdx`

机制摘要：
- models、rules、prompts、tools 在一个配置系统中声明。
- config tests 校验解析和默认值。
- rules 是用户可见的行为约束，不藏在 prompt 拼接里。

Kiana 可借鉴方式：
- 让 Kiana config 显式声明 provider/model、tool allowlist、rules、profiles。
- 用 snapshot/golden 锁定 config merge 结果。

不应该照搬的部分：
- 不切换成 Continue 的 YAML schema。
- 不复制它的 IDE-specific config source precedence。

### Pattern: Terminal security as standalone evaluator

来源文件：
- `reference/continue/packages/terminal-security/src/evaluateTerminalCommandSecurity.ts`
- `reference/continue/packages/terminal-security/test/terminalCommandSecurity.test.ts`

机制摘要：
- 命令安全评估独立于终端执行。
- 测试只喂命令字符串和环境上下文。
- 输出风险等级供 UI/agent 决策。

Kiana 可借鉴方式：
- 保持 `kiana-tools/src/exec_policy.rs` 作为纯策略层。
- Bash 和 PowerShell 共用 risk classification contract。

不应该照搬的部分：
- 不复制规则文本。
- 不忽略 Windows-specific shell parsing。

## 4. Behavior Contracts to Port into Kiana

### Contract: Provider Adapter Capability Check

Input:
- provider id
- model id
- request features: tools, streaming, vision, structured output

Decision:
- provider 是否支持目标 feature。
- request 是否需要转换成 OpenAI-compatible payload。
- local provider 是否允许无 auth。

Execution:
- 解析 model profile。
- 构建 adapter request。
- 在发送前返回 unsupported feature error。

Output:
- provider response stream。
- typed capability error。

Runtime Events:
- SessionEvent
- StreamDelta
- RuntimeError

Acceptance Tests:
- mock provider 支持 tools 并能完成 tool call。
- 不支持 tools 的 model 在发送前失败。
- local/Ollama profile 不要求云 API key。

### Contract: Command Risk Classification

Input:
- shell kind
- command text
- cwd
- permission profile

Decision:
- 命令是 read-only、mutating、destructive 还是 unknown。
- 是否需要 permission request。
- 是否允许 non-interactive 自动执行。

Execution:
- 使用 shell-aware parser/classifier。
- 生成风险理由。
- 在 Bash/PowerShell handler 前阻断或放行。

Output:
- risk level。
- permission decision。

Runtime Events:
- PermissionRequest
- ToolCall
- ToolResult
- RuntimeError

Acceptance Tests:
- `rm -rf` / destructive PowerShell 删除被标为 destructive。
- `git status` 被标为 read-only。
- unknown command 在 restricted profile 下需要确认。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| Config capability graph | 有 config/services 基础 | provider/model/rules/tools/profile 合并结果缺少 public golden | `kiana-services/src/config.rs`, `kiana-entrypoints/src/cli.rs` | config merge snapshots | P1 |
| Provider adapters | Anthropic-first | OpenAI-compatible/Ollama/mock adapter 不完整 | `kiana-services/src/api`, `kiana-commands/src/model.rs` | mock adapter tool tests | P0 |
| Terminal security | exec_policy 和 sandbox 已有 | shell-aware risk explanation 和 PowerShell parity 需加强 | `kiana-tools/src/exec_policy.rs`, `kiana-tools/src/powershell_tool.rs` | Bash/PowerShell risk matrix | P0 |
| Indexing/RAG | repo_map 有基础 | semantic indexing/RAG 不是完整 capability | `kiana-query/src/repo_map.rs` | privacy and offline indexing fixtures | P3 |

## 6. Atomic Implementation Tasks

### Task: Add Provider Capability Matrix

Goal:
- 在 Kiana 中给每个 provider/model 定义工具、streaming、vision、structured output 能力。

Scope:
- 允许修改 `kiana-services/src/api`, `kiana-commands/src/model.rs`, `kiana-services/src/config.rs`。
- 不允许移除现有 Anthropic 配置路径。

Implementation Notes:
- ModelProfile 应可序列化并被 CLI 输出。
- unsupported feature 应在 request 前失败。

Acceptance Criteria:
- `kiana model list` 显示 provider/model capabilities。
- fake/mock provider 可声明 tools=true。
- 不支持 tools 的 model 触发 typed error。

Tests:
- `cargo test -p kiana-services model_profile`
- `cargo test -p kiana-entrypoints cli_model`

Manual Verification:
- `kiana model list --json`

### Task: Expand Shell Risk Classifier

Goal:
- 让 Bash 和 PowerShell 命令在执行前都有可解释风险等级。

Scope:
- 允许修改 `kiana-tools/src/exec_policy.rs`, `kiana-tools/src/bash_tool.rs`, `kiana-tools/src/powershell_tool.rs`。
- 不允许降低现有 destructive command 阻断。

Implementation Notes:
- classifier 输出 risk、reason、requires_approval。
- PowerShell 删除、移动、网络下载需有专项 fixtures。

Acceptance Criteria:
- Bash 与 PowerShell destructive 命令均被阻断或请求批准。
- read-only 命令在 permissive profile 下可执行。
- risk reason 出现在 permission request event 中。

Tests:
- `cargo test -p kiana-tools exec_policy`
- `cargo test -p kiana-tools powershell_policy`

Manual Verification:
- `kiana -p "run rm -rf temp" --permission-profile default`
