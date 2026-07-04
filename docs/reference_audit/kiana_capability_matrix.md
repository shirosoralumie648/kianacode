# Kiana Reference Capability Matrix

## 总体结论

Kiana 的 Rust 重构方向应优先完成 coding-agent core，而不是扩张到完整 IDE/web/cloud 产品。最有价值的参考组合是：

- Rust core and session/tool/runtime: `reference/codex/codex-rs/core/src/session/turn.rs`, `reference/codex/codex-rs/core/src/tools/lifecycle.rs`, `reference/codex/codex-rs/thread-store/src/store.rs`.
- CLI/runtime/RPC/trust/fake provider: `reference/pi/packages/coding-agent/src/core/agent-session-runtime.ts`, `reference/pi/packages/coding-agent/src/modes/rpc/jsonl.ts`, `reference/pi/packages/ai/src/providers/faux.ts`, `reference/pi/packages/coding-agent/src/core/project-trust.ts`.
- Tool validation, terminal, file changes and product shell data: `reference/Roo-Code/src/core/tools/validateToolUse.ts`, `reference/Roo-Code/src/integrations/terminal/TerminalProcess.ts`, `reference/Roo-Code/webview-ui/src/__tests__/fileChangesFromMessages.spec.ts`.
- Provider/config/security: `reference/continue/packages/openai-adapters/src/index.ts`, `reference/continue/packages/terminal-security/src/evaluateTerminalCommandSecurity.ts`, `reference/langchain/libs/standard-tests/README.md`.
- Plugins/skills/hooks: `reference/claude-code-rev-main/src/utils/plugins/pluginLoader.ts`, `reference/claude-code-main (2)/claude-code-main/plugins/plugin-dev/skills/plugin-structure/SKILL.md`, `reference/claude-code-rust/src/plugins/loader.rs`.

当前 Kiana 应进入 feature completion mode：先让 single-agent coding loop 在 CLI/TUI/stream-json/bridge 中有一致、可测试、可恢复的行为；再补 provider matrix、MCP、plugins/skills 和 product shell。多智能体、RAG/vector store、web app server、GUI/SSH、marketplace 等应作为 P2/P3，不应阻塞 P0 parity。

## Capability Matrix

| # | Capability Group | 主要参考证据 | Kiana 当前锚点 | 关键缺口 | 建议方向 | 优先级 |
|---|---|---|---|---|---|---|
| 1 | CLI / print / TUI / RPC modes | `reference/codex/codex-rs/cli/src/main.rs`, `reference/pi/packages/coding-agent/test/print-mode.test.ts`, `reference/pi/packages/coding-agent/src/modes/rpc/rpc-mode.ts`, `reference/cline/docs/cli/cli-reference.mdx` | `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/runner.rs`, `kiana-entrypoints/src/tui.rs` | 多入口是否完全共享同一 runner/event contract 需要锁定 | print/TUI/RPC/bridge 都只转换输入输出，不能各自实现 tool loop | P0 |
| 2 | Runtime events and session store | `reference/codex/codex-rs/thread-store/src/local/live_writer.rs`, `reference/codex/codex-rs/app-server-protocol/src/protocol/event_mapping.rs`, `reference/cline/docs/sdk/reference/events.mdx`, `reference/pi/packages/coding-agent/docs/session-format.md` | `kiana-types/src/runtime.rs`, `kiana-entrypoints/src/sdk.rs`, `kiana-commands/src/session.rs` | session store API、resume/fork/search/archive 与 RuntimeEvent schema 仍需成为 public contract | 抽 session store API，保持 legacy compatibility，给 stream-json/bridge/TUI 统一 replay | P0 |
| 3 | Tool registry, validation and lifecycle | `reference/codex/codex-rs/core/src/tools/registry.rs`, `reference/codex/codex-rs/core/src/tools/lifecycle.rs`, `reference/Roo-Code/src/core/tools/validateToolUse.ts`, `reference/langchain/libs/standard-tests/langchain_tests/unit_tests/tools.py` | `kiana-tools/src/tool.rs`, `kiana-tools/src/registry.rs`, `kiana-tools/src/tool_execution.rs` | validation error 已开始统一为 `tool_result.error` ToolError 元数据；lifecycle event、read-only parallel、mutating serialization 仍需组合 fixture | handler 前统一 validation/permission/lifecycle gate，输出 ToolStart/ToolResult/ToolError | P0 |
| 4 | Permissions, trust, sandbox and shell risk | `reference/codex/codex-rs/execpolicy/src/policy.rs`, `reference/claude-code-rev-main/src/tools/BashTool/bashPermissions.ts`, `reference/continue/packages/terminal-security/src/evaluateTerminalCommandSecurity.ts`, `reference/OpenHands/openhands/app_server/sandbox/README.md` | `kiana-tools/src/permissions.rs`, `kiana-tools/src/exec_policy.rs`, `kiana-tools/src/bash_sandbox.rs`, `kiana-tools/src/powershell_tool.rs` | project trust、sandbox provider boundary、PowerShell parity、risk reason 需要补齐 | fail-closed trust gate；shell-aware classifier；sandbox provider trait；permission request event 带 reason | P0 |
| 5 | Provider and model registry | `reference/cline/apps/cline-hub/src/webview/src/lib/provider-schema.ts`, `reference/continue/packages/openai-adapters/src/apis/Mock.ts`, `reference/pi/packages/ai/src/api-registry.ts`, `reference/langchain/libs/model-profiles/README.md` | `kiana-services/src/api`, `kiana-services/src/auth.rs`, `kiana-commands/src/model.rs` | Anthropic-first，缺少 Provider trait、fake provider、OpenAI-compatible/Ollama/local capability profile | 先实现 Provider trait + fake provider + ModelProfile capability matrix，再扩 provider | P0 |
| 6 | MCP tools, resources and transports | `reference/codex/codex-rs/rmcp-client/src/rmcp_client.rs`, `reference/claude-code-rev-main/src/services/mcp/MCPConnectionManager.tsx`, `reference/Roo-Code/src/services/mcp/McpHub.ts`, `reference/continue/docs/customize/mcp-tools.mdx` | `kiana-services/src/mcp.rs`, `kiana-tools/src/mcp_tool.rs`, `kiana-entrypoints/src/mcp.rs` | dynamic tool/resource discovery、transport state、isError mapping 和 CLI/TUI surfacing 需固化 | stdio/http/sse/ws 按统一 MCP connection state 和 ToolResult error contract 输出 | P0 |
| 7 | Plugins, skills, hooks and extensions | `reference/claude-code-rev-main/src/utils/plugins/pluginLoader.ts`, `reference/claude-code-main (2)/claude-code-main/plugins/hookify/hooks/hooks.json`, `reference/pi/packages/coding-agent/src/core/extensions/loader.ts`, `reference/claude-code-rust/src/skills/executor.rs` | `kiana-skills/src`, `kiana-commands/src/plugin.rs`, `kiana-commands/src/hooks.rs`, `kiana-query/src/stop_hooks.rs` | load audit、trust、warnings、hook event order 和 skill resource policy 不完整 | plugin/skill load 产生日志事件；hook order 可测试；untrusted plugin fail closed | P1 |
| 8 | Local code workflow and git/checkpoints | `reference/aider/aider/repomap.py`, `reference/aider/aider/coders/search_replace.py`, `reference/cline/docs/core-workflows/checkpoints.mdx`, `reference/Roo-Code/src/core/webview/checkpointRestoreHandler.ts` | `kiana-query/src/repo_map.rs`, `kiana-tools/src/file_write.rs`, `kiana-tools/src/file_edit.rs`, `kiana-tools/src/file_delete.rs`, `kiana-types/src/runtime.rs`, `kiana-entrypoints/src/runner.rs`, `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/tui.rs`, `kiana-remote/src/sdk_message_adapter.rs`, `kiana-bridge/src/sdk_message_adapter.rs`, `kiana-commands/src/diff.rs`, `kiana-commands/src/checkpoint.rs`, `kiana-commands/src/review.rs`, `kiana-commands/src/doctor.rs` | Write/Edit/Delete file-change metadata 已进入 `tool_result.changed_files`、RuntimeEvent、stream-json、remote/bridge adapter 和 schema；Delete 使用 read-before-mutate、mtime、editable/read-only guard；`kiana diff --session-changes <session-id> --json` 已可从 session events 聚合 file changes view，TUI `/diff` 已渲染该聚合视图；bridge crate 现在也能从 runtime events 直接生成同形 `kiana.diff.session_changes.v1` 聚合报告；`kiana checkpoint restore ... --json` 已输出 `kiana.checkpoint.restore.v1` 与 `changed_files` 前后存在状态；`kiana doctor --json` 已把 Phase 4 exit criteria 汇总为 `local-coding-workflow` readiness evidence | ToolResult 写 changed_files；diff 读取 session metadata 并聚合；TUI 渲染聚合视图；bridge 聚合 runtime changed_files；checkpoint restore 读取 patch/untracked restore 前后状态；doctor 汇总 repo-map/file-set/checkpoint/review/checks/repair-loop audit evidence | P1 |
| 9 | Product shell, bridge, app server and UI reducer | `reference/codex/codex-rs/app-server/src/request_processors/thread_processor.rs`, `reference/OpenHands/frontend/__tests__/stores/use-event-store.test.ts`, `reference/Roo-Code/src/core/webview/webviewMessageHandler.ts`, `reference/cline/sdk/packages/shared/src/rpc/runtime.ts` | `kiana-bridge/src`, `kiana-remote/src`, `kiana-screens/src`, `kiana-entrypoints/src/tui.rs`, `kiana-entrypoints/src/cli.rs`, `docs/schemas/kiana-app-server-doctor.v1.schema.json`, `docs/schemas/kiana-commercial-release-blockers.v1.schema.json`, `docs/schemas/kiana-local-rc-evidence.v1.schema.json`, `docs/schemas/kiana-source-control-proof.v1.schema.json`, `docs/schemas/kiana-release-signature.v1.schema.json`, `docs/schemas/kiana-enterprise-offline-manifest.v1.schema.json`, `docs/schemas/kiana-commercial-proof-manifest.v1.schema.json`, `docs/schemas/kiana-app-server-live-provider-smoke.v1.schema.json`, `docs/schemas/kiana-app-server-distribution-review.v1.schema.json`, `docs/schemas/kiana-remote-code-session-smoke.v1.schema.json`, `docs/schemas/kiana-product-acceptance.v1.schema.json`, `docs/schemas/kiana-entitlement-proof.v1.schema.json`, `docs/schemas/kiana-release-ops.v1.schema.json`, `docs/schemas/kiana-platform-security-proof.v1.schema.json` | RuntimeEvent 到 UI view-state 的 reducer/golden 仍需扩展；local app-server protocol 已开始固定，包含 conversations/events/files/settings/auth/license/model/context/checks/review/diff/checkpoint、`/app/doctor` readiness wrapper、`/app/release/blockers` commercial blocker report、`/app/release/local-rc-evidence` local RC evidence handoff、`/app/release/source-control` source-control proof handoff、`/app/release/signature` release signature proof handoff、`/app/release/enterprise-offline-manifest` enterprise offline manifest handoff、`/app/release/proof-manifest` commercial proof manifest handoff、`/app/release/live-provider-smoke` live provider proof handoff、`/app/release/remote-code-session-smoke` remote code-session proof handoff、`/app/release/distribution` distribution review handoff、`/app/release/product-acceptance` product acceptance handoff、`/app/release/entitlement` entitlement proof handoff、`/app/release/ops` release operations proof handoff，以及 `/app/release/platform-security` platform security proof handoff | 继续补 RuntimeEvent view reducer fixtures，并让 app-server readiness/release-blockers/local-rc-evidence/source-control/release-signature/enterprise-offline-manifest/proof-manifest/live-provider-smoke/remote-code-session-smoke/distribution-review/product-acceptance/entitlement/release-ops/platform-security/diff/checkpoint/check/review surfaces 成为 Web/IDE 客户端稳定 contract | P1 |
| 10 | Config, rules, modes and profiles | `reference/continue/packages/config-yaml/src/__tests__/index.test.ts`, `reference/Roo-Code/src/shared/modes.ts`, `reference/claude-code-rev-main/src/utils/settings/settings.ts`, `reference/claude-code-main (2)/claude-code-main/examples/settings/settings-strict.json` | `kiana-services/src/config.rs`, `kiana-tools/src/permissions.rs`, `kiana-entrypoints/src/cli.rs` | config merge、mode/profile、rules/tools/provider precedence 缺少 public golden | 保持 Kiana config 格式，增加 config resolved view 和 merge snapshots | P1 |
| 11 | Memory, compaction and workflow artifacts | `reference/pi/packages/coding-agent/src/core/compaction/index.ts`, `reference/claude-code-rust/src/memory/consolidation.rs`, `reference/MetaGPT/tests/data/demo_project/tasks.json`, `reference/langchain/libs/langchain_v1/langchain/agents/middleware/summarization.py` | `kiana-commands/src/compact.rs`, `kiana-commands/src/export.rs`, `kiana-types/src/runtime.rs` | StopReason、compaction event、workflow artifact schema 仍需明确 | compaction/summary 作为 runner policy；workflow artifact 后置但 schema versioned | P1/P2 |
| 12 | Multi-agent, subagent and structured workflows | `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_selector_group_chat.py`, `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/tools/_team.py`, `reference/MetaGPT/metagpt/team.py`, `reference/MetaGPT/tests/metagpt/test_incremental_dev.py` | future `kiana-agents`, `kiana-tools/src/tool_execution.rs` | child session、permission inheritance、depth/budget limit 未定义 | 只先写 subagent tool contract，不实现完整 scheduler | P2 |
| 13 | Evaluation, smoke, doctor and security tests | `reference/aider/benchmark/README.md`, `reference/cline/sdk/scripts/ci-node-smoke.ts`, `reference/OpenHands/tests/unit/server/test_openapi_schema_generation.py`, `reference/langchain/libs/core/tests/unit_tests/test_ssrf_protection.py` | `scripts/release-smoke.sh`, `kiana-commands/src/doctor.rs`, `kiana-services/src/network_policy.rs` | provider standard tests、SSRF/network policy matrix、doctor tool/provider report 需补 | fake default CI；live opt-in；doctor 输出 capability matrix；network policy fixtures | P0/P1 |

## P0 Implementation Roadmap

1. Provider trait + fake provider + model capability matrix
   - Reference: `reference/pi/packages/ai/src/providers/faux.ts`, `reference/continue/packages/openai-adapters/src/apis/Mock.ts`, `reference/langchain/libs/model-profiles/README.md`.
   - Kiana modules: `kiana-services/src/api`, `kiana-commands/src/model.rs`, `kiana-entrypoints/src/runner.rs`.
   - Acceptance: no-network fake provider can complete assistant/tool/final loops; unsupported tool capability fails before request; `kiana model list --json` exposes capabilities.

2. Unified tool validation and lifecycle events
   - Reference: `reference/codex/codex-rs/core/src/tools/lifecycle.rs`, `reference/Roo-Code/src/core/tools/validateToolUse.ts`, `reference/Roo-Code/src/core/tools/__tests__/validateToolUse.spec.ts`.
   - Kiana modules: `kiana-tools/src/tool_execution.rs`, `kiana-tools/src/registry.rs`, `kiana-types/src/runtime.rs`.
   - Acceptance: unknown tool, missing argument, permission denied, timeout and handler error all produce typed RuntimeEvent output.

3. Shell risk classifier, project trust and sandbox provider boundary
   - Reference: `reference/continue/packages/terminal-security/src/evaluateTerminalCommandSecurity.ts`, `reference/pi/packages/coding-agent/src/core/project-trust.ts`, `reference/OpenHands/openhands/app_server/sandbox/README.md`.
   - Kiana modules: `kiana-tools/src/exec_policy.rs`, `kiana-tools/src/bash_sandbox.rs`, `kiana-tools/src/powershell_tool.rs`, `kiana-commands/src/trust.rs`.
   - Acceptance: untrusted cwd blocks mutating tools; Bash and PowerShell destructive commands request approval or fail; sandbox unavailable is explicit.

4. Runner policy order and StopReason
   - Reference: `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/conditions/_terminations.py`, `reference/langchain/libs/langchain_v1/langchain/agents/middleware/human_in_the_loop.py`, `reference/langchain/libs/langchain_v1/langchain/agents/middleware/model_fallback.py`.
   - Kiana modules: `kiana-entrypoints/src/runner.rs`, `kiana-types/src/runtime.rs`.
   - Acceptance: permission runs before retry; fallback never replays successful mutating tools; every terminal result has stop_reason.

5. Public session store and replay contract
   - Reference: `reference/codex/codex-rs/thread-store/src/store.rs`, `reference/codex/codex-rs/thread-store/src/local/live_writer.rs`, `reference/pi/packages/coding-agent/docs/session-format.md`.
   - Kiana modules: `kiana-entrypoints/src/sdk.rs`, `kiana-commands/src/session.rs`, `kiana-commands/src/export.rs`.
   - Acceptance: resume/fork/compact/import all use one store API; stream-json replay does not duplicate terminal events; legacy sessions remain readable.

6. MCP dynamic tool/resource parity
   - Reference: `reference/codex/codex-rs/rmcp-client/src/rmcp_client.rs`, `reference/claude-code-rev-main/src/tools/MCPTool/MCPTool.ts`, `reference/Roo-Code/src/services/mcp/McpHub.ts`.
   - Kiana modules: `kiana-services/src/mcp.rs`, `kiana-tools/src/mcp_tool.rs`, `kiana-entrypoints/src/mcp.rs`.
   - Acceptance: MCP tool discovery, resource read, isError result and server disconnect all produce stable events.

7. Stream-json/RPC public schema fixtures
   - Reference: `reference/pi/packages/coding-agent/src/modes/rpc/jsonl.ts`, `reference/pi/packages/coding-agent/test/rpc-jsonl.test.ts`, `reference/cline/docs/sdk/reference/events.mdx`.
   - Kiana modules: `kiana-entrypoints/src/runner.rs`, `kiana-bridge/src`, `kiana-types/src/runtime.rs`.
   - Acceptance: invalid JSON returns typed error; request id ordering is stable; unknown future events are forward-compatible.

8. Provider standard tests and release smoke split
   - Reference: `reference/langchain/libs/standard-tests/README.md`, `reference/MetaGPT/tests/metagpt/provider/test_anthropic_api.py`, `reference/cline/sdk/scripts/ci-node-smoke.ts`.
   - Kiana modules: `kiana-services/tests/provider_standard.rs`, `scripts/release-smoke.sh`.
   - Acceptance: fake provider runs in default CI; cloud provider live smoke is opt-in; skip reasons are explicit.

9. RuntimeEvent view reducer fixtures
   - Reference: `reference/OpenHands/frontend/__tests__/utils/handle-event-for-ui.test.ts`, `reference/OpenHands/frontend/__tests__/stores/use-event-store.test.ts`, `reference/Roo-Code/webview-ui/src/__tests__/FileChangesPanel.spec.tsx`.
   - Kiana modules: `kiana-types/src/runtime.rs`, `kiana-screens/src`, `kiana-bridge/src`.
   - Acceptance: TUI/bridge derive the same tool status from the same event sequence; tool error does not hide final result.

10. Network and URL policy matrix
    - Reference: `reference/langchain/libs/core/tests/unit_tests/test_ssrf_protection.py`, `reference/langchain/libs/core/tests/unit_tests/test_ssrf_policy_transport.py`, `reference/codex/codex-rs/core/src/config/permission_profile_catalog.rs`.
    - Kiana modules: `kiana-services/src/network_policy.rs`, `kiana-tools/src/mcp_tool.rs`, `kiana-tools/src/permissions.rs`.
    - Acceptance: localhost/private IP/file URL policy is covered; MCP/http tools use the same network policy; blocked network access is surfaced as typed error.

## 不建议照搬的内容

| Source | 不建议照搬 | 原因 | Kiana 替代方向 |
|---|---|---|---|
| `reference/codex/codex-rs/app-server-protocol/src/protocol/v2/thread.rs` | 完整 app-server thread protocol | 与 OpenAI desktop/app-server 产品耦合，体量大 | 保留 Kiana RuntimeEvent 和 lightweight bridge protocol |
| `reference/claude-code-rev-main/src/main.tsx` | React/Ink UI 主循环和品牌提示词 | TypeScript UI 与 Claude Code 产品特定 | 借 processUserInput 行为，Kiana TUI 保持 Rust-first |
| `reference/claude-code-main (2)/claude-code-main/plugins/frontend-design/skills/frontend-design/SKILL.md` | 具体技能内容和提示词 | 属于 Claude Code plugin 内容资产 | 只借 plugin/skill manifest 与 resource loading contract |
| `reference/aider/aider/coders/search_replace.py` | Python edit algorithm 源码 | 不能复制源代码，且 Kiana 已有 Rust edit tools | 借 acceptance tests：exact match、ambiguous hunk、rollback |
| `reference/cline/apps/cline-hub/src/webview/src/lib/provider-model-catalog.ts` | 静态 provider/model catalog | 时间敏感且产品耦合 | Kiana 用 ModelProfile + docs/live refresh policy |
| `reference/Roo-Code/webview-ui/src/App.tsx` | VS Code webview UI | 宿主 API 和前端状态耦合 | Kiana 先做 RuntimeEvent view reducer，再决定 shell |
| `reference/continue/core/indexing/README.md` | 云/IDE 索引产品形态 | RAG/vector 成本、隐私和依赖复杂 | 先增强 `kiana-query/src/repo_map.rs`，RAG 后置 |
| `reference/OpenHands/containers/README.md` | 默认 Docker sandbox 平台 | Kiana 需支持本地/Windows-first，Docker 不能作为默认依赖 | sandbox provider trait + explicit provider selection |
| `reference/autogen/python/packages/autogen-agentchat/src/autogen_agentchat/teams/_group_chat/_swarm_group_chat.py` | 完整 swarm/team scheduler | 多智能体调度不是当前 core parity | 先定义 subagent tool contract 和 depth/budget limit |
| `reference/MetaGPT/metagpt/team.py` | 软件公司式角色流程默认化 | 过度流程化，会拖慢普通 coding loop | workflow artifacts 作为 opt-in mode |
| `reference/langchain/libs/core/tests/unit_tests/runnables/test_runnable.py` | 泛型 Runnable 框架 | 抽象层过厚，不符合 Kiana focused CLI agent | 借 event streaming/standard test 思路 |
| `reference/claude-code-rust/src/gui/app.rs` | GUI/web/SSH 全产品扩张 | repo 更像能力草图，成熟度风险高 | 只用于 product surface checklist，不作为实现蓝图 |
