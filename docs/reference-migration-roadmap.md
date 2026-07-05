# Kiana Reference Migration Roadmap

Date: 2026-06-24

This document turns the read-only review of all `reference/` projects into an execution roadmap for Kiana. It is intentionally scoped as a migration plan, not a claim that Kiana is already feature-complete.

## Executive Summary

Kiana currently has a strong Rust foundation: CLI, print mode, REPL, SDK sessions, tool registry, permissions, MCP, skills/plugins, remote/bridge, TUI skeleton, release smoke, and many focused tests. The current product maturity is about **40/100** against the combined reference target.

The right strategy is **core loop first**:

1. Stabilize runtime, session, tool, permission, config, and safety contracts.
2. Add aider-style local code editing workflow: repo map, file set, git safety, AI diff, undo, lint/test repair.
3. Add provider, SDK, plugin, hooks, and project trust infrastructure.
4. Only then expand TUI, app-server, Web UI, IDE, GUI, RAG, and multi-agent studio.

Avoid starting with GUI, IDE, Web console, or RAG. Those surfaces will look impressive early but will amplify instability if the core loop is not finished first.

## Reference Inventory

| Reference | Best source of truth | Borrow | Avoid or treat carefully |
| --- | --- | --- | --- |
| `codex` | Strong Rust agent architecture | app-server protocol, thread/turn/item model, config/permission profiles, sandbox/exec policy, state store, TUI discipline, packaging | OpenAI/ChatGPT-specific cloud services and experimental app-server details |
| `claude-code-rev-main` | Claude Code behavior reference | CLI parity, QueryEngine layering, tool orchestration, settings policy, MCP details, permission lifecycle, TUI shape | Source-map restored tree, shims, hidden stubs, incomplete server modules |
| `claude-code-main (2)` | Public Claude Code plugin and policy examples | plugin fixtures, hooks contract, workflow prompt packs, enterprise settings examples | Not full product source, many assets are Anthropic-specific |
| `aider` | Local repo editing workflow | repo map, editable/read-only files, git safety, auto lint/test repair, AI diff, undo, watch comments | Python edit-format protocol is less useful than Kiana tool-use runner |
| `cline` | SDK and host architecture | typed SDK, provider gateway, local hub, tool lifecycle, release/eval productization | Split between new SDK and old VS Code core; model list is time-sensitive |
| `pi` | Provider, JSONL session tree, TUI, release hardening | provider/model/auth registry, JSONL tree, project trust, RPC events, TUI concepts | No built-in sandbox; Node/npm extension ecosystem is expensive to port |
| `Roo-Code` | VS Code product shell | mode/profile config, skills precedence, typed tool catalogue, state shell, worktree UI | Project is shut down; do not follow as live upstream |
| `continue` | IDE and automation product shape | `.continue`-style agents/checks/rules/prompts, review/checks, indexing plan | Archived; next-edit and review/checks have unfinished boundaries |
| `OpenHands` | Web/app-server control plane | conversation/events/settings/secrets/sandbox/git API, Web console layout | Enterprise license/WIP, Python/React implementation not directly portable |
| `autogen` | Multi-agent runtime contracts | AgentRuntime, Team, Workbench, termination conditions, replay/eval harness | Maintenance mode; Studio is research/non-production |
| `MetaGPT` | Role/workflow and artifacts | role profiles, artifact dependency graph, QA feedback loop, tool recommendation | Heavy Python/Jupyter/RAG dependencies and demo surfaces |
| `langchain` | SDK contracts and provider abstraction | provider registry, model profiles, contract tests, Runnable-like composition, SSRF policy | Not a CLI product; many integrations live outside this repo |
| `claude-code-rust` | Product surface sketch | GUI concepts, MCP prompts/resources, project init, Magic Docs dry-run | Many stubs/mock services; do not use as implementation-quality reference |

## Current Kiana Baseline

Kiana already has:

- Rust workspace with 23 crates in `Cargo.toml`.
- CLI routing in `kiana-entrypoints/src/cli.rs`.
- SDK/session persistence in `kiana-entrypoints/src/sdk.rs`.
- Assistant runner in `kiana-entrypoints/src/runner.rs`.
- Tool trait, registry, execution, and permission primitives in `kiana-tools/src`.
- Slash command registry in `kiana-commands/src/registry.rs`.
- MCP service and commands in `kiana-services/src/mcp.rs`, `kiana-entrypoints/src/mcp.rs`, and `kiana-commands/src/mcp.rs`.
- Skills/plugins loader in `kiana-skills/src`.
- Remote/bridge/CCR work in `kiana-remote/src` and `kiana-bridge/src`.
- TUI and screen skeletons in `kiana-entrypoints/src/tui.rs` and `kiana-screens/src`.
- Release smoke in `scripts/release-smoke.sh` and `RELEASE.md`.

Primary gaps:

- Public runtime/session/tool events still rely too much on generic JSON.
- Session storage is not yet a robust JSONL tree or indexed state store.
- Config, managed policy, permission profiles, project trust, and exec policy are still lighter than the best references.
- Tool orchestration still needs broader workbench lifecycle assertions and mutating serial edge coverage.
- Repo map, editable/read-only file sets, AI diff, undo, and lint/test repair loop are missing or incomplete.
- Provider abstraction is Anthropic-first and lacks a true provider/model/auth registry.
- TUI, Web/app-server, IDE, GUI, and release packaging are not yet reference-grade.

## Target Architecture

The target architecture should be layered so each surface consumes the same stable core.

```text
CLI / TUI / SDK / RPC / App Server / Future Web or IDE
        |
kiana-runtime
  - typed events
  - session tree
  - turn lifecycle
  - tool lifecycle
  - permission requests
        |
kiana-agent-core
  - model/provider gateway
  - tool orchestration
  - context/repo map
  - git safety
  - lint/test repair
        |
kiana-policy
  - config layers
  - permission profiles
  - project trust
  - exec policy
  - network/SSRF policy
        |
kiana-extension
  - skills
  - plugins
  - hooks
  - MCP workbenches
  - project agents/checks/rules/prompts
```

Existing crate names do not need to match this diagram immediately. The important boundary is that all user surfaces should share the same runtime, session, tool, policy, and provider contracts.

## Migration Phases

### Phase 0: Reference Matrix And Tracking

Goal: make the review actionable and prevent duplicated work.

Deliverables:

- Keep this roadmap as the top-level migration document.
- Add a future `docs/reference-feature-matrix.md` when implementation begins.
- Add a future `docs/reference-risk-register.md` if licensing, service coupling, or stubs become blockers.

Exit criteria:

- Every migrated domain has a reference source, Kiana owner path, tests, and risk notes.
- No feature is marked complete only because a command or module exists.

### Phase 1: Runtime And Session Core

References: Codex, Cline, Pi, `claude-code-rev-main`.

Scope:

- Define typed runtime events: user message, assistant message, stream delta, tool call, tool result, permission request, session event, error, result.
- Introduce a JSONL append-only session tree with parent/child turns.
- Preserve compatibility with current SDK session files.
- Add stable SDK/RPC event schema.
- Align CLI, TUI, SDK, and remote bridge on the same event model.

Candidate code areas:

- `kiana-types/src/message.rs`
- `kiana-types/src/tools.rs`
- `kiana-entrypoints/src/sdk.rs`
- `kiana-entrypoints/src/runner.rs`
- `kiana-commands/src/session.rs`
- `kiana-remote/src/sdk_message_adapter.rs`
- `kiana-bridge/src/sdk_message_adapter.rs`

Exit criteria:

- Existing sessions still load.
- New sessions append JSONL entries with parent IDs.
- `fork`, `compact`, `export`, `import`, and `resume` have tests.
- CLI/TUI/SDK/bridge render from the same event schema.

Progress notes:

- 2026-06-23: Added `kiana-types/src/runtime.rs` with typed runtime event payloads for user messages, assistant messages, stream deltas, tool calls, tool results, permission requests, session events, errors, and turn results. Golden coverage lives in `kiana-types/tests/runtime_event_schema.rs`.
- 2026-06-23: Added compatibility writers in `kiana-entrypoints/src/sdk.rs` and `kiana-commands/src/session.rs` that keep legacy `<session_id>.json` session files while appending new SDK and local session-command message writes to `<session_id>/events.jsonl` with `turn-*` parent links. This does not yet complete Phase 1 because JSONL-only reads, replacement-flow rebuilds, and shared CLI/TUI/SDK/bridge rendering still need coverage.
- 2026-06-23: Added JSONL-only read/list fallback for SDK sessions and JSONL-only `--resume` display through the local session command path. Replacement-style SDK writes now rebuild `<session_id>/events.jsonl` instead of leaving stale events. Phase 1 still needs compact/export/import event-tree parity plus shared SDK/RPC/remote/bridge event adapters.
- 2026-06-23: Added SDK/local runner, remote, and bridge adapter helpers that convert stream deltas, tool calls/results, permission requests, errors, and turn results into typed `RuntimeEvent` values while keeping existing CLI/SDK JSON output stable. Focused adapter coverage is in `cargo test -p kiana-entrypoints runtime_event`, `cargo test -p kiana-remote runtime_event`, and `cargo test -p kiana-bridge runtime_event`. Phase 1 still needs CLI/TUI shared rendering adoption and compact/export/import event-tree parity before it can be called complete.
- 2026-06-23: Ran the Phase 1 default verification gate from `docs/superpowers/plans/2026-06-23-runtime-session-core.md`: `cargo fmt --all --check`, `cargo test -p kiana-types`, split `cli_session`/`cli_resume` tests because the documented combined cargo filter is invalid, and `bash scripts/release-smoke.sh`. The release smoke completed with workspace tests, release build, install smoke, and doctor smoke. Phase 1 remains open because the exit criteria still require compact/export/import event-tree parity and shared rendering adoption.
- 2026-06-23: Added command-level event-tree parity for compact/export: `/compact` now rebuilds `<session_id>/events.jsonl` after rewriting messages, and `/export` can read JSONL-only session trees when the legacy `<session_id>.json` file is absent. Focused coverage is in `cargo test -p kiana-commands compact`, `cargo test -p kiana-commands export`, and `cargo test -p kiana-entrypoints --test cli_session`, followed by a fresh `bash scripts/release-smoke.sh`. Phase 1 still remains open because there is no session import command parity yet and CLI/TUI shared rendering still needs to consume the typed runtime schema end-to-end.
- 2026-06-23: Added `kiana session import <session.json> [--force]`, restoring both `<session_id>.json` and `<session_id>/events.jsonl` from exported JSON sessions. Forced replacement removes stale event trees before rebuilding them from imported messages. Focused coverage is in `cargo test -p kiana-entrypoints --test cli_session session_import_restores_legacy_json_and_runtime_events`, `cargo test -p kiana-commands session_import_force_replaces_existing_json_and_event_tree`, `cargo test -p kiana-commands session::tests::`, and `cargo test -p kiana-entrypoints --test cli_session`, followed by `cargo fmt --all --check` and `bash scripts/release-smoke.sh`. Phase 1 still remains open because CLI/TUI shared rendering adoption is not complete.
- 2026-06-23: Completed Phase 1 shared rendering adoption for the remaining local surfaces: TUI resume/refresh now maps SDK transcript messages through typed `RuntimeEvent` before producing `ConversationMessage`, and CLI text export now renders from runtime events, including JSONL-only `tool_call` and `tool_result` entries that were previously dropped. Focused coverage is in `cargo test -p kiana-entrypoints --lib tui::tests::`, `cargo test -p kiana-commands export`, `cargo test -p kiana-entrypoints --test cli_session`, and `cargo test -p kiana-entrypoints --test cli_resume`. With the prior SDK, runner, remote, bridge, session-tree, compact, export, import, fork, and resume coverage, Phase 1 exit criteria are satisfied; Phase 2 is the next migration target.

### Phase 2: Config, Permissions, Trust, And Safety

References: Codex, `claude-code-rev-main`, Pi, LangChain.

Scope:

- Introduce config layers: defaults, user, project, local, env, CLI flag, managed policy.
- Add permission profiles: read-only, workspace, full, ask, plan.
- Add project trust before loading project `.kiana`, `.claude`, plugins, skills, hooks, and MCP configs.
- Add exec policy rules for Bash/PowerShell and future OS sandbox integration.
- Add network/SSRF policy shared by WebFetch, WebSearch, and HTTP MCP.

Candidate code areas:

- `kiana-bootstrap/src/config.rs`
- `kiana-tools/src/permissions.rs`
- `kiana-tools/src/bash_tool.rs`
- `kiana-tools/src/powershell_tool.rs`
- `kiana-tools/src/agent.rs`
- `kiana-tools/src/web_fetch.rs`
- `kiana-tools/src/web_search.rs`
- `kiana-services/src/mcp.rs`
- `kiana-query/src/stop_hooks.rs`
- `kiana-skills/src/loader.rs`
- `kiana-skills/src/plugins.rs`

Exit criteria:

- Permission precedence is covered by tests: deny, allow, ask, mode.
- Untrusted projects cannot load local resources.
- Dangerous commands produce structured denial reasons.
- Network policy handles localhost, metadata IPs, file URLs, redirects, and private ranges.

Progress:

- 2026-06-23: Started Phase 2 project-trust gate by adding a typed `ProjectTrust` contract and trust-aware loaders. Explicitly untrusted app/session state now filters project `.claude/skills` from `kiana-skills`, `kiana skills`, `discover_skills`, `SkillTool`, and agent skill context/preload paths while preserving user skills. MCP server resolution now ignores project `.mcp.json` when the app/session marks the project untrusted while still honoring env-provided MCP config. Focused coverage is in `cargo test -p kiana-skills --lib`, `cargo test -p kiana-tools --lib`, and `cargo test -p kiana-commands skills`. Remaining Phase 2 trust work includes `.kiana` agents/hooks/plugins, explicit trust UX/state persistence, permission profiles, exec policy, and network/SSRF policy.
- 2026-06-23: Extended the trust gate to project agent definitions. Explicitly untrusted app/session state now prevents `Agent` from resolving project `.kiana/agents`, `.claude/agents`, `.kiana/agents-local`, and `.claude/agents-local` definitions, and the built-in `claude-code-guide` no longer injects those project agents into its dynamic configuration context while the project is untrusted. Built-in and plugin agents remain available. Focused coverage is in `cargo test -p kiana-tools --lib`. Remaining Phase 2 trust work includes project hooks/plugins, explicit trust UX/state persistence, permission profiles, exec policy, and network/SSRF policy.
- 2026-06-23: Extended the trust gate to project plugin marketplace configuration. Explicitly untrusted app/session state now skips project and local `.kiana/plugin-marketplaces*.json` entries when loading marketplace entries, so project-local marketplace sources cannot feed plugin list/update/install flows until the project is trusted. User marketplace entries remain available. Focused coverage is in `cargo test -p kiana-commands plugin_marketplace_list_ignores_project_config_when_project_is_untrusted`. Remaining Phase 2 trust work includes runtime project plugin roots if they are introduced, project hooks, explicit trust UX/state persistence, permission profiles, exec policy, and network/SSRF policy.
- 2026-06-23: Extended the trust gate to project hooks. Trusted projects now load `.kiana/hooks.json` alongside user/home hook files for query stop hooks and `Agent` lifecycle hooks; explicitly untrusted app/session state skips project hooks while preserving env, user/home, plugin, and agent-frontmatter hook sources. Focused coverage is in `cargo test -p kiana-query --lib` and `cargo test -p kiana-tools --lib`. Remaining Phase 2 trust work includes explicit trust UX/state persistence, permission profiles, exec policy, and network/SSRF policy.
- 2026-06-23: Closed the explicit project trust UX/state persistence gap by exposing `kiana trust` through the default command registry, re-exporting the trust-file helpers from `kiana-types`, and keeping `.kiana/trust.json` as the app-state fallback for untrusted/trusted project decisions. Focused coverage is in `cargo test -p kiana-commands default_registry_includes_core_commands`, `cargo test -p kiana-commands trust`, `cargo test -p kiana-types trust`, plus a binary smoke with `cargo run -q -p kiana-entrypoints --bin kiana -- trust status`. Remaining Phase 2 work includes permission profiles, exec policy, and network/SSRF policy.
- 2026-06-23: Added first-class permission profiles across file/env/session/CLI layers. `kiana permissions profile <read-only|workspace|full|ask|plan>` now persists a profile preset, status reports profile plus effective mode, `KIANA_PERMISSION_PROFILE` and `permission_profile` app state feed the tool permission decision, and global `--permission-profile` exposes the same profile for prompt runs. `read-only` maps to plan-style mutation denial, `workspace` to default, `full` to `bypassPermissions`, `ask` to ask, and `plan` to plan. Focused coverage is in `cargo test -p kiana-tools read_only_profile_denies_mutating_tools_but_allows_reads`, `cargo test -p kiana-commands profile_command_persists_permission_profile_and_status_reports_it`, `cargo test -p kiana-entrypoints runtime_flags_are_stripped_before_command`, `cargo test -p kiana-entrypoints apply_runtime_flags_sets_permission_profile_env`, and `cargo test -p kiana-entrypoints --test cli_help routed_help_flags_print_usage_instead_of_running_commands`. Remaining Phase 2 work includes deeper permission precedence coverage, exec policy, and network/SSRF policy.
- 2026-06-23: Added WebFetch network/SSRF policy with structured URL parsing and redirect validation. `validate_input` and `call` now reject non-HTTP schemes, missing hosts, localhost, loopback, private, link-local, metadata, and other non-public IP ranges before sending, and redirect targets are checked through the same policy. Focused coverage is in `cargo test -p kiana-tools web_fetch`.
- 2026-06-23: Added a shared Bash/PowerShell exec policy classifier for obvious destructive commands. Bash and PowerShell now reject policy-denied commands through both `validate_input` and direct `call` paths with `exec policy denied ...` reasons before shell execution. Current coverage blocks cases such as `rm -rf /`, `sudo ...`, `dd ... of=/dev/...`, `mkfs.*`, recursive `chmod 777 /`, PowerShell `Remove-Item -Recurse -Force C:\`, `Clear-Disk`, `Restart-Computer`, and `Format-Volume`, while preserving safe echo/output commands. Focused coverage is in `cargo test -p kiana-tools exec_policy_rejects_dangerous_bash_commands`, `cargo test -p kiana-tools exec_policy_rejects_dangerous_powershell_commands`, and `cargo test -p kiana-tools --lib`.
- 2026-06-24: Moved HTTP URL validation into shared `kiana-services::network_policy` and wired it through WebFetch plus HTTP/SSE MCP. WebFetch keeps strict public-target SSRF protection. HTTP and SSE MCP allow configured local/private initial URLs for common local MCP servers, but deny link-local and metadata-style targets, cross-origin HTTP redirects, and SSE endpoint events that escape the configured origin with visible `network policy denied ...` errors. Focused coverage is in `cargo test -p kiana-services network_policy_tests`, `cargo test -p kiana-services blocks_cross_origin`, `cargo test -p kiana-services mcp::tests::http_client_lists_and_calls_tools`, `cargo test -p kiana-services mcp::tests::sse_client_lists_and_calls_tools`, and `cargo test -p kiana-tools web_fetch`.
- 2026-06-24: Wired WebSearch into the same shared network policy. WebSearch now builds its fixed DuckDuckGo HTML source URL through `HttpNetworkSurface::WebSearch`, rejects non-public configured endpoints before sending, and validates redirects with the same public-target policy. Focused coverage is in `cargo test -p kiana-services web_search_policy_denies_local_private_and_redirect_targets` and `cargo test -p kiana-tools web_search`. Remaining Phase 2 work includes deeper permission precedence coverage and auditing non-tool service HTTP clients only where they become user/tool-controlled network targets.
- 2026-06-24: Added explicit permission precedence coverage for deny/allow/ask/mode ordering and config-layer override behavior. The permission layer now has regression tests proving deny rules win over allow, ask, and permissive modes; allow rules win before ask rules when the allow pattern matches; ask rules win over permissive modes when allow misses; and session/CLI app state mode/profile overrides env and file defaults. Focused coverage is in `cargo test -p kiana-tools precedence` and `cargo test -p kiana-tools session_mode_overrides_env_profile_and_file_mode`. Remaining Phase 2 work includes managed policy precedence, richer shell/OS sandbox integration, and any future HTTP target audits when new user-controlled clients are introduced.
- 2026-06-24: Added initial managed policy precedence for config and permissions. `KIANA_MANAGED_SETTINGS_FILE` or shared `KIANA_MANAGED_POLICY_FILE` now applies a final config overlay after base config, remote settings, user settings, settings JSON, and `ANTHROPIC_*` env values, including the convenience getter paths. Permission policy loads from `KIANA_MANAGED_PERMISSIONS_FILE` before shared `KIANA_MANAGED_POLICY_FILE`, supports flat or nested `permissions` JSON, and keeps managed deny/ask/allow/profile rules from being bypassed by user file, env, or session/app-state settings. `kiana permissions status` reports the managed policy file, load status, managed allow/deny/ask rules, and parse/read errors. Focused coverage is in `cargo test -p kiana-bootstrap load_config_applies_managed_settings_file_after_env_and_user_overlays`, `cargo test -p kiana-bootstrap config_getters_use_managed_settings_overlay_after_env`, `cargo test -p kiana-tools managed_policy_overrides_session_env_and_user_permissions`, and `cargo test -p kiana-commands status_reports_managed_policy_rules`. Remaining Phase 2 work is stronger shell/OS sandbox integration, richer shell parsing, and any additional enterprise policy surfaces discovered during later parity work.
- 2026-06-24: Added the first richer shell parsing slice to exec policy. Bash/PowerShell policy now tokenizes with quote awareness and command boundaries, so literal examples such as `echo "rm -rf /"` and `Write-Output "Remove-Item ..."` are not mistaken for executable destructive commands. The same parser still denies real destructive commands behind command separators, absolute command paths like `/bin/rm -rf /` and `/usr/bin/sudo ...`, and nested wrapper execution through `bash -c ...` and `pwsh -Command ...`. Focused coverage is in `cargo test -p kiana-tools exec_policy_rejects_dangerous_bash_commands` and `cargo test -p kiana-tools exec_policy_rejects_dangerous_powershell_commands`. Remaining Phase 2 work is stronger OS-level sandbox enforcement and deeper shell edge cases where future tests reveal real bypasses.
- 2026-06-24: Tightened Bash sandbox config wiring. Runner sandbox options and config-file defaults now share the same normalization path, so `sandbox = true` from config becomes `{ "enabled": true }` instead of a boolean app-state value that BashTool cannot read; invalid sandbox config types now fail early. `.kiana-example.toml` and `CONFIG.md` document the recommended `enabled`, `failIfUnavailable`, and `allowUnsandboxedCommands` settings for controlled Linux environments. Focused coverage is in `cargo test -p kiana-entrypoints sandbox_option_accepts_options_and_config_default`. Remaining Phase 2 risk is platform-specific sandbox depth and future edge cases, not the basic config-to-BashTool wiring.
- 2026-06-24: Added shared Bash sandbox readiness diagnostics and exposed them through `kiana doctor`. BashTool and doctor now share `kiana-tools::bash_sandbox` for `enabled`, `failIfUnavailable`, `allowUnsandboxedCommands`, `bwrapPath`/`KIANA_BWRAP_PATH`, and PATH probing semantics. `kiana doctor` reports `bash_sandbox: enabled=... status=ready|unavailable|disabled runtime=... ... bwrap=...` and warns when a required sandbox lacks `bwrap`. Focused coverage is in `cargo test -p kiana-tools bash_sandbox`, `cargo test -p kiana-commands doctor_reports_ready_bash_sandbox_when_bwrap_is_available`, and `cargo test -p kiana-commands doctor_warns_when_required_bash_sandbox_lacks_bwrap`. Remaining Phase 2 risk is platform-specific sandbox depth and deeper shell edge cases where future tests reveal real bypasses.

### Phase 3: Tool Orchestration And MCP Parity

References: Codex, `claude-code-rev-main`, AutoGen, Roo-Code.

Scope:

- Execute consecutive read-only tools concurrently when safe.
- Execute mutating tools serially.
- Emit structured tool lifecycle events.
- Add MCP resource templates, prompts, status, auth errors, and stronger fixture coverage.
- Introduce a Workbench concept for tool groups that need lifecycle: MCP, browser, computer, notebook, future RAG.

Candidate code areas:

- `kiana-tools/src/tool.rs`
- `kiana-tools/src/tool_execution.rs`
- `kiana-tools/src/registry.rs`
- `kiana-tools/src/mcp_tool.rs`
- `kiana-entrypoints/src/runner.rs`
- `kiana-services/src/mcp.rs`
- `kiana-entrypoints/src/mcp.rs`

Exit criteria:

- Read/Grep/Glob batches can run concurrently.
- Write/Edit/Bash/PowerShell remain serial.
- Tool events are visible in CLI/TUI/SDK/remote.
- Fake MCP servers cover tools, resources, prompts, resource templates, and error states.

Progress:

- 2026-06-24: Added the first tool orchestration slice for consecutive read-only tool calls. Runner now routes model tool-use blocks through a shared batch executor that concurrently runs eligible `is_read_only && is_concurrency_safe` tools after allow-only permission preflight, preserves original tool-result order, keeps deny/ask or mutating tools on the existing serial path, and merges read-file/app state back into the main `ToolContext`. The registered `Read`, `Grep`, and `Glob` tools are now marked read-only and concurrency-safe, while mutating tools remain serial. Focused coverage is in `cargo test -p kiana-tools default_registry_marks_core_read_tools_as_concurrent_read_only` and `cargo test -p kiana-entrypoints run_assistant_turn_batches_read_only_tools_and_preserves_read_state_for_later_edit`. Remaining Phase 3 work includes explicit lifecycle event coverage across CLI/TUI/SDK/remote and stronger fake-server fixtures.
- 2026-06-24: Added MCP resource template parity for the client, local server, and tool surfaces. `McpClient` now probes `resources/templates/list` for stdio, HTTP, SSE, and WebSocket transports and caches `resourceTemplates`; the local Kiana MCP server exposes `resources/templates/list` plus a `kiana://tool/{name}` resource template; `ListMcpResourceTemplatesTool` lists templates from configured MCP servers; and `mcp status`, `status`, and `doctor` report `resource_templates` as a wired MCP protocol surface. Focused coverage is in `cargo test -p kiana-services http_client_lists_resource_templates`, `cargo test -p kiana-entrypoints resources_templates_list_expose_local_tool_template`, `cargo test -p kiana-tools lists_mcp_resource_templates_when_server_config_is_provided`, `cargo test -p kiana-tools --lib`, and `cargo test -p kiana-services --lib`. Remaining Phase 3 work includes explicit lifecycle event coverage across CLI/TUI/SDK/remote before release claims.
- 2026-07-05: Exposed MCP prompt list/get as model-facing tools. `ListMcpPromptsTool` now lists configured server prompts with argument metadata, `GetMcpPromptTool` retrieves prompt messages by name and arguments, both tools are read-only/concurrency-safe under the `mcp` workbench, and the default registry plus permission profile allowlist expose them alongside MCP resources/templates. Focused coverage is in `cargo test -p kiana-tools mcp_prompt --locked`, `cargo test -p kiana-tools --locked`, and `cargo test -p kiana-entrypoints run_assistant_turn_streaming_surfaces_mcp_prompt_lifecycle_events --locked`. Remaining Phase 3 work includes broader prompt-tool lifecycle assertions across TUI/SDK/remote surfaces before release claims.
- 2026-07-05: Updated `kiana doctor --json` reference capability readiness so `tool-lifecycle-mcp` publicly reports `mcp-prompts`, `mcp-prompt-list-get`, and `mcp-prompt-lifecycle` evidence alongside the existing MCP tool/resource/template lifecycle evidence. Focused coverage is in `cargo test -p kiana-commands --locked doctor`.
- 2026-06-24: Added the first MCP auth error-state fixture. HTTP and SSE MCP transports now classify 401/403 responses as `auth_error` with visible `MCP HTTP/SSE authentication failed: status=..., body=...` messages instead of generic client errors, and `kiana mcp status` reports `error_states: auth_error`. Focused coverage is in `cargo test -p kiana-services reports_missing_auth_as_auth_error` and `cargo test -p kiana-commands mcp_reports_recorded_invocations`. Remaining Phase 3 work includes explicit lifecycle event coverage across CLI/TUI/SDK/remote before release claims.
- 2026-06-24: Added the first Workbench metadata slice for lifecycle-bearing tool groups. `Tool::workbench()` now exposes optional group metadata, the default registry schema includes `workbench` only when a tool declares it, MCP tools are grouped under `mcp`, `NotebookEdit` is grouped under `notebook`, and the local Kiana MCP server exposes the same metadata through both `tools/list` and `kiana://tool/{name}` resources. Focused coverage is in `cargo test -p kiana-tools default_registry_exposes_lifecycle_workbench_metadata`, `cargo test -p kiana-entrypoints list_tools_exposes_default_registry`, and `cargo test -p kiana-entrypoints resources_list_and_read_expose_local_status`. Remaining Phase 3 work includes deeper workbench lifecycle behavior and explicit lifecycle event coverage across CLI/TUI/SDK/remote before release claims.
- 2026-06-24: Threaded Workbench metadata into structured tool lifecycle runtime events. `RuntimeToolCallEvent` and `RuntimeToolResultEvent` now carry optional `workbench`; the local runner enriches tool calls/results from the default registry, while remote and bridge SDK adapters preserve `workbench` from tool-use/tool-result message blocks. Focused coverage is in `cargo test -p kiana-entrypoints runner_runtime_event_adapter_enriches_workbench_lifecycle_events`, `cargo test -p kiana-remote remote_sdk_adapter_emits_runtime_events_for_messages_tools_and_results`, and `cargo test -p kiana-bridge bridge_sdk_adapter_emits_runtime_events_for_messages_tools_and_results`. Remaining Phase 3 work includes fuller lifecycle event assertions in public CLI/stream-json and TUI flows plus stronger fake-server fixtures before release claims.
- 2026-06-24: Exposed Workbench lifecycle metadata in the TUI transcript path. Runtime tool call/result events and persisted tool-use/tool-result blocks now render `workbench: ...` in tool conversation messages when the metadata is present, keeping older events unchanged when it is absent. Focused coverage is in `cargo test -p kiana-entrypoints maps_runtime_events_to_conversation_messages`. Remaining Phase 3 work includes public CLI/stream-json lifecycle assertions, live TUI streaming edge coverage, and stronger fake-server fixtures before release claims.
- 2026-06-24: Exposed structured runtime lifecycle events through public CLI `stream-json` partial output. `--include-partial-messages` stream wrappers now retain the existing raw `stream_event` payload while adding `runtime_events` generated by the shared runner adapter, so streamed tool-call events include `workbench` metadata such as `mcp` without changing the outer event type. Focused coverage is in `cargo test -p kiana-entrypoints stream_json_partial_event_exposes_tool_lifecycle_runtime_events`. Remaining Phase 3 work includes live TUI streaming edge coverage and stronger fake-server fixtures before release claims.
- 2026-06-24: Covered live TUI streaming lifecycle display for Workbench tools. Streaming tool-use messages now resolve `workbench` metadata from the default registry, remember it by `tool_use_id` for the active prompt, and append the same metadata when the matching tool result is streamed back into the live REPL message. Focused coverage is in `cargo test -p kiana-entrypoints prompt_tool_lifecycle_messages_include_live_workbench_metadata`. Remaining Phase 3 work includes stronger fake-server fixtures for lifecycle/error/tool surfaces before release claims.
- 2026-06-24: Strengthened the MCP fake-server error fixture. The HTTP MCP fixture can now return a protocol-level tool result with `isError: true`; `McpTool` preserves that state in both model-facing `tool_result.is_error` and the shared `ToolExecutionResult.is_error`, so runner/runtime surfaces no longer treat MCP tool errors as plain successful text. Focused coverage is in `cargo test -p kiana-tools mcp_tool_error_result_marks_tool_execution_error`. Remaining Phase 3 work includes broader cross-transport fake-server lifecycle/error assertions before release claims.
- 2026-06-24: Extended MCP fake-server tool-error coverage across stdio, HTTP, SSE, and WebSocket transports. The shared service-layer fake server and the stdio Python fixture now return `isError: true` for a `fail` tool call, and `McpClient::call_tool` preserves that protocol result shape across every supported client transport. Focused coverage is in `cargo test -p kiana-services mock_mcp_transports_surface_tool_error_results`. Remaining Phase 3 work includes explicit cross-surface lifecycle fake-server assertions before release claims.
- 2026-06-24: Added explicit runner lifecycle fixtures for MCP protocol error results over HTTP, SSE, and WebSocket. A streaming fake model now calls the `MCP` tool against fake MCP servers that return `isError: true`; the tests assert the runner emits `RunnerStreamEvent::ToolResult { name: "MCP", is_error: true }`, the structured runtime tool-call/result events carry `workbench: "mcp"`, and the model feedback blocks include `tool_result.is_error: true`. Focused coverage is in `cargo test -p kiana-entrypoints run_assistant_turn_streaming_surfaces_mcp_error_lifecycle_events`, `cargo test -p kiana-entrypoints run_assistant_turn_streaming_surfaces_sse_mcp_error_lifecycle_events`, and `cargo test -p kiana-entrypoints run_assistant_turn_streaming_surfaces_ws_mcp_error_lifecycle_events`. Remaining Phase 3 work now shifts to any still-missing public surface assertions before release claims.
- 2026-06-24: Wired CLI `--include-partial-messages` stream-json output to the SDK local runner-event stream instead of the model-only stream wrapper, so public partial output can expose tool-result lifecycle events as well as model stream events. MCP tool-error results now serialize as `stream_event` rows with `event.type: "tool_result"`, `is_error: true`, result content, and matching runtime `tool_result` metadata including `workbench: "mcp"`. Focused coverage is in `cargo test -p kiana-entrypoints stream_json_partial_event_exposes_tool`. Remaining Phase 3 work is auditing any additional public lifecycle/error surfaces introduced by future MCP or workbench features before release claims.

### Phase 4: Local Coding Workflow

References: aider, Continue, Roo-Code.

Scope:

- Add repo map with token budget, stable ordering, ignore rules, and language-aware symbols.
- Add editable/read-only file set tracking.
- Add AI diff for changes since the last assistant turn.
- Add git safety: dirty state detection, checkpoint, undo, and restore.
- Add lint/test repair loop.
- Add local `review` and `checks` commands using isolated worktrees and dry-run patch output.

Candidate code areas:

- `kiana-query/src`
- `kiana-tools/src/file_read.rs`
- `kiana-tools/src/file_edit.rs`
- `kiana-tools/src/bash_tool.rs`
- `kiana-commands/src/diff.rs`
- `kiana-commands/src/commit.rs`
- `kiana-commands/src/session.rs`
- new focused modules under an existing crate or a future `kiana-repo` crate

Exit criteria:

- Fixture repos produce stable repo maps.
- Dirty user changes are never overwritten without explicit approval.
- Undo restores AI changes without removing user changes.
- A mock model can complete `edit -> test fail -> repair -> pass`.
- `kiana review --dry-run --json` produces deterministic output.
- `kiana review --json` runs discovered checks in an isolated worktree and reports deterministic local findings.
- `kiana checks --json` runs discovered gates in an isolated worktree without mutating the user's active worktree.

Progress:

- 2026-06-24: Added the first local coding workflow repo-map slice. `kiana-query::repo_map` now builds deterministic repo maps from fixture repos with stable path ordering, common generated-directory ignores, basic root `.gitignore` handling, language detection, Rust/Python/TypeScript symbol summaries, and a configurable token budget that reports truncation/omitted files. `kiana context repo-map [--json] [--max-tokens N]` exposes the map from the active command `cwd`. Focused coverage is in `cargo test -p kiana-query repo_map` and `cargo test -p kiana-commands context_repo_map_json_uses_cwd_and_budget`. Remaining Phase 4 work includes editable/read-only file tracking, runner-created assistant-turn checkpoints, dirty-state/checkpoint/undo safety, repair loops, and deterministic `review`/`checks` commands.
- 2026-06-24: Added the first structured dirty-state surface for local coding workflow safety. `kiana diff --json` now respects the active command `cwd`, reports whether the directory is inside a git repo, whether it is dirty, parsed `git status --short` file states, and staged/unstaged diff-stat sections. This gives later checkpoint, undo, review, and checks commands a deterministic local state contract instead of scraping human text. Focused coverage is in `cargo test -p kiana-commands diff_json_reports_dirty_state_from_command_cwd`. Remaining Phase 4 work includes checkpoint creation, undo/restore semantics that preserve user changes, runner-created assistant-turn checkpoints, repair loops, and deterministic `review`/`checks` commands.
- 2026-06-24: Added the first git safety checkpoint surface. `kiana checkpoint [--json]` now respects the active command `cwd`, creates a checkpoint under `KIANA_HOME/checkpoints/<repo>/<id>/` instead of dirtying the project tree, writes `manifest.json`, captures staged and unstaged binary patches, and copies untracked regular files for later restore/undo work. Focused coverage is in `cargo test -p kiana-commands checkpoint_json_creates_git_checkpoint_outside_worktree`. Remaining Phase 4 work includes undo/restore semantics that preserve user changes, runner-created assistant-turn checkpoints, editable/read-only file tracking, repair loops, and deterministic `review`/`checks` commands.
- 2026-06-24: Added the first editable/read-only file guard at the tool context layer. `ToolContext` now recognizes `editable_files` plus `read_only_files`/`readonly_files` app-state sets, and `Write`/`Edit` enforce them during both validation and execution so later runner/session surfaces can safely separate files the assistant may mutate from files it may only inspect. Focused coverage is in `cargo test -p kiana-tools write_rejects_files_marked_read_only` and `cargo test -p kiana-tools edit_rejects_files_not_listed_as_editable`. Remaining Phase 4 work includes session/CLI file-set management, undo/restore semantics that preserve user changes, runner-created assistant-turn checkpoints, repair loops, and deterministic `review`/`checks` commands.
- 2026-06-24: Wired editable/read-only file sets into the local runner option contract. `run_assistant_turn` and streaming runner setup now accept `editable_files`/`editableFiles` and `read_only_files`/`readOnlyFiles`/`readonly_files`/`readonlyFiles`, seed those sets into `ToolContext.app_state`, and preserve the earlier `Write`/`Edit` enforcement through real fake-model loops. Focused coverage is in `cargo test -p kiana-entrypoints run_assistant_turn_applies_editable_file_options_to_tool_context` and `cargo test -p kiana-entrypoints run_assistant_turn_applies_read_only_file_options_to_tool_context`. Remaining Phase 4 work includes user-facing CLI/session file-set management, undo/restore semantics that preserve user changes, runner-created assistant-turn checkpoints, repair loops, and deterministic `review`/`checks` commands.
- 2026-06-24: Added user-facing prompt CLI flags for editable/read-only file sets. `kiana --editable-file <path>`, `--editableFile=<path>`, `--read-only-file <path>`, `--readOnlyFile=<path>`, and `readonly` aliases are parsed before prompt text and written into runner options as `editable_files` and `read_only_files`, so `kiana -p/--print` and resumed prompt execution can drive the earlier `Write`/`Edit` guard without manual SDK wiring. Focused coverage is in `cargo test -p kiana-entrypoints runtime_flags_accept_file_set_values`, `cargo test -p kiana-entrypoints runtime_flags_accept_readonly_file_aliases`, and `cargo test -p kiana-entrypoints prompt_runtime_options_include_file_sets`. Remaining Phase 4 work includes durable session-level file-set management, undo/restore semantics that preserve user changes, runner-created assistant-turn checkpoints, repair loops, and deterministic `review`/`checks` commands.
- 2026-06-24: Added durable session-level file-set management. SDK session JSON now preserves `editable_files` and `read_only_files` while remaining compatible with older session files, `kiana session files [session_id|current]` can show, set, and clear those sets, and persisted prompt execution inherits the session file sets into runner options unless the current prompt supplies explicit file-set overrides. Focused coverage is in `cargo test -p kiana-commands session_files_updates_and_shows_durable_file_sets`, `cargo test -p kiana-entrypoints top_level_session_files_updates_durable_file_sets`, and `cargo test -p kiana-entrypoints prompt_options_inherit_session_file_sets_without_overwriting_explicit_options`. Remaining Phase 4 work includes undo/restore semantics that preserve user changes, runner-created assistant-turn checkpoints, repair loops, and deterministic `review`/`checks` commands.
- 2026-06-24: Added a conservative checkpoint restore surface. `kiana checkpoint restore <checkpoint-dir-or-manifest> [--json]` verifies that the checkpoint belongs to the active repository, reapplies non-empty staged/unstaged patch artifacts only after `git apply --check --binary` succeeds, restores checkpointed untracked files only when the target path is absent, and reports conflicts instead of overwriting later user edits when the target already has different bytes. Focused coverage is in `cargo test -p kiana-commands checkpoint_restore_recovers_removed_untracked_file`, `cargo test -p kiana-commands checkpoint_restore_refuses_to_overwrite_user_changes`, and `cargo test -p kiana-commands checkpoint_restore_reapplies_clean_unstaged_patch`. Remaining Phase 4 work includes runner-created assistant-turn checkpoints, full assistant-turn undo UX, repair loops, and deterministic `review`/`checks` commands.
- 2026-06-24: Added the first deterministic review dry-run contract. `kiana review --dry-run --json` is a local read-only command that reports the active git repository, stable dirty-file state, staged/unstaged patch previews, and the planned isolated review steps without calling a model or mutating the worktree. Two consecutive runs over the same fixture repo produce byte-identical JSON. Focused coverage is in `cargo test -p kiana-commands review_dry_run_json_reports_deterministic_patch_plan`. Remaining Phase 4 work includes runner-created assistant-turn checkpoints, full assistant-turn undo UX, real isolated review/check execution, repair loops, and a deterministic `checks` command.
- 2026-06-24: Added the first deterministic checks dry-run contract. `kiana checks --dry-run --json` is a local read-only command that discovers stable quality gates from the active repository without executing them. Cargo workspaces currently produce `cargo fmt --all --check`, `cargo check --workspace`, and `cargo test --workspace --no-fail-fast`; repos with `scripts/release-smoke.sh` also include the release smoke gate. Two consecutive runs over the same fixture repo produce byte-identical JSON. Focused coverage is in `cargo test -p kiana-commands checks_dry_run_json_reports_deterministic_discovered_gates`. Remaining Phase 4 work includes runner-created assistant-turn checkpoints, full assistant-turn undo UX, real isolated review/check execution, and repair loops.
- 2026-06-24: Added the first checkpoint-baseline and last-assistant AI diff contract. `kiana diff --from-checkpoint <checkpoint-dir-or-manifest> --json` rebuilds the checkpoint baseline in a temporary git worktree from `HEAD`, staged/unstaged patch artifacts, and checkpointed untracked files, then reports only changes made after that checkpoint with normalized patch paths. This prevents pre-existing user dirty changes from being counted as new assistant changes. `kiana checkpoint --assistant-turn --session-id <id> --turn-id <id> --json` records assistant-turn metadata, and `kiana diff --last-assistant --json` selects the latest matching checkpoint for the active repository. Focused coverage is in `cargo test -p kiana-commands diff_from_checkpoint_json_reports_changes_after_checkpoint` and `cargo test -p kiana-commands diff_last_assistant_json_uses_latest_assistant_turn_checkpoint`. Remaining Phase 4 work includes runner-created assistant-turn checkpoints, full assistant-turn undo UX, real isolated review/check execution, and repair loops.
- 2026-06-24: Wired assistant-turn checkpoint creation into the real runner boundary. Both `run_assistant_turn` and the streaming runner create an assistant-turn checkpoint at turn start when `session_id` and `turn_id` are present, so `kiana diff --last-assistant --json` can compare against the pre-tool baseline and exclude pre-existing user dirty changes. Focused coverage is in `cargo test -p kiana-entrypoints run_assistant_turn_creates_assistant_checkpoint_for_last_assistant_diff` and `cargo test -p kiana-entrypoints run_assistant_turn_streaming_creates_assistant_checkpoint_for_last_assistant_diff`. Remaining Phase 4 work includes full assistant-turn undo UX, real isolated review/check execution, and repair loops.
- 2026-06-24: Added the first assistant-turn undo UX. `kiana checkpoint undo --last-assistant [--json]` selects the latest assistant-turn checkpoint, materializes the checkpoint baseline from `HEAD` plus captured staged/unstaged/untracked artifacts, restores modified files to that baseline, and removes files created after the checkpoint. This covers the immediate "undo my last assistant turn" path while preserving user dirty content that existed before the assistant turn. Focused coverage is in `cargo test -p kiana-commands checkpoint_undo_last_assistant_restores_checkpoint_baseline`. Remaining Phase 4 work includes richer late-user-edit conflict UX, real isolated review/check execution, and repair loops.
- 2026-06-24: Added the first real isolated checks execution. `kiana checks --json` now creates a temporary detached git worktree when the repository has a `HEAD`, applies the current staged patch, unstaged patch, and untracked regular files into that worktree, runs discovered gates there, and removes the worktree afterward. This lets checks validate the user's current changes while preventing gate scripts from mutating the active worktree. Focused coverage is in `cargo test -p kiana-commands checks_json_runs_gates_in_isolated_worktree_without_mutating_current_tree`. Remaining Phase 4 work includes richer late-user-edit conflict UX, real isolated review execution, and repair loops.
- 2026-06-24: Added the first real isolated review execution. `kiana review --json` now keeps the existing dirty-state and patch-preview report, runs the discovered checks through the isolated checks executor, and emits deterministic local findings for failed or skipped gates. This gives the review command an executing, non-mutating path instead of only a dry-run plan. Focused coverage is in `cargo test -p kiana-commands review_json_runs_checks_in_isolated_worktree_without_mutating_current_tree`. Remaining Phase 4 work includes richer late-user-edit conflict UX and repair loops.
- 2026-06-24: Added the first opt-in lint/test repair loop at the non-streaming runner boundary. `run_assistant_turn` accepts `repairChecks`/`repair_checks` plus bounded repair-attempt options, runs `kiana review --json` after an assistant end-turn with no tool uses, feeds failed or skipped isolated-check findings back to the model, and lets a mock model complete `edit -> test fail -> repair -> pass`. Focused coverage is in `cargo test -p kiana-entrypoints run_assistant_turn_repairs_failed_checks_with_mock_model`. Remaining Phase 4 work includes richer late-user-edit conflict UX and public CLI/streaming repair-loop UX.
- 2026-06-24: Exposed the non-streaming CLI repair-loop controls. `kiana --repair-checks -p <prompt>` and `--repair-check-attempts <n>` are parsed as runtime flags, documented in help output, and injected into prompt execution as the runner's `repairChecks` and `repairCheckAttempts` options. Focused coverage is in `cargo test -p kiana-entrypoints repair_loop`. Remaining Phase 4 work includes richer late-user-edit conflict UX and streaming repair-loop UX until the streaming runner path is covered.
- 2026-06-24: Wired the streaming runner to the same opt-in repair loop. Streaming prompt execution now honors `repairChecks`/`repair_checks` and bounded repair-attempt options, runs isolated review after an assistant end-turn with no tool uses, feeds failed or skipped gate findings back to the model, and lets a streaming mock model complete `edit -> test fail -> repair -> pass`. Focused coverage is in `cargo test -p kiana-entrypoints run_assistant_turn_streaming_repairs_failed_checks_with_mock_model`. Remaining Phase 4 work includes richer late-user-edit conflict UX.
- 2026-06-24: Added late post-turn user-edit conflict protection for assistant-turn undo. The runner now records an assistant-final snapshot on successful assistant turns, and `kiana checkpoint undo --last-assistant` reports `changed_after_assistant_turn` instead of restoring over a tracked file that the user edited after the assistant finished. Focused coverage is in `cargo test -p kiana-entrypoints run_assistant_turn_undo_reports_late_user_edit_conflict`.
- 2026-07-05: Added the first session-event file-change aggregation view. `kiana diff --session-changes <session-id> --json` now reads runtime `tool_result.changed_files` metadata from the session `events.jsonl`, validates session ids, filters unsafe paths, reports sorted files with operations and sources, and returns a deterministic empty `kiana.diff.session_changes.v1` report for legacy events without metadata. TUI `/diff` now renders that same schema as a file changes preview instead of raw JSON. Focused coverage is in `cargo test -p kiana-commands diff_session_changes_json --locked` and `cargo test -p kiana-entrypoints tui_session_changes_diff_json_result_renders_file_changes_preview --locked`.
- 2026-07-05: Added checkpoint restore before/after file-change comparison. `kiana checkpoint restore <checkpoint-dir-or-manifest> --json` now emits `schema = "kiana.checkpoint.restore.v1"` and a deterministic `changed_files` list for restored patch and untracked paths, including `operation`, `source`, `before_exists`, `after_exists`, and `changed` so product file-change panels can explain exactly what restore modified. Focused coverage is in `cargo test -p kiana-commands checkpoint_restore_json_reports_before_after_file_changes --locked` plus the broader `cargo test -p kiana-commands checkpoint_restore --locked` regression group.
- 2026-07-05: Added bridge-side consumption of the same session file-change aggregate contract. `kiana-bridge::bridge_session_changes_report()` now aggregates bridge `RuntimeEvent` tool results with `changed_files`, filters unsafe paths, sorts files deterministically, and serializes the same `kiana.diff.session_changes.v1` shape used by the CLI and TUI. Focused coverage is in `cargo test -p kiana-bridge bridge_session_changes_report_aggregates_runtime_changed_files --locked`.
- 2026-07-05: Added bridge-side consumption of the same app-server event view contract. `kiana-bridge::bridge_events_view_report()` now projects bridge `RuntimeEvent` sequences into `kiana.app-server.events-view.v1` with stable role/content/source metadata and tool-result changed-file passthrough, while suppressing duplicate SDK message tool blocks and repeated terminal assistant text. Focused coverage is in `cargo test -p kiana-bridge bridge_events_view_report_projects_runtime_events_for_clients --locked`.
- 2026-07-05: Added conservative file deletion to the tool surface. `Delete` now follows the existing file mutation safety model by requiring access-root approval, editable/read-only checks, an existing regular file, prior read state, and unchanged mtime before removing the file. Its model-facing `tool_result` emits `changed_files` metadata with `operation = "delete"` and `source = "Delete"`, so Write/Edit/Delete all feed the same file-change aggregation path. Focused coverage is in `cargo test -p kiana-tools delete_ --locked` and `cargo test -p kiana-tools default_registry_exposes_delegation_and_task_tools --locked`.
- 2026-07-05: Closed the Phase 4 local-coding workflow audit in the public readiness surface. `kiana doctor --json` now reports `local-coding-workflow` with explicit evidence for deterministic repo-map budget behavior, editable/read-only file sets, Write/Edit/Delete file changes, checkpoint create/restore/undo, last-assistant diff, isolated review/checks, non-streaming and streaming repair loops, and late-user-edit conflict protection. Focused coverage is in `cargo test -p kiana-commands doctor_json_reports_complete_local_coding_workflow_audit --locked`. Phase 4 local workflow is now locally ready; commercial release remains blocked only by external release proofs.
- 2026-07-05: Exposed the same doctor readiness matrix through the direct-connect product shell. `/app/doctor` now wraps the pinned `kiana.doctor.v1` report as `kiana.app-server.doctor.v1`, advertises `doctor.read` in the `/app` contract, and lets Web/IDE clients display reference capability readiness plus `local-coding-workflow` evidence without shelling out. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked` and `cargo test -p kiana-entrypoints direct_connect_app_contract_schemas_match_packaged_schema_files --locked`.
- 2026-07-05: Exposed resolved config audit through the direct-connect product shell. `/app/config/resolved` now returns pinned `kiana.app-server.config-resolved.v1` wrapping `kiana.config-resolved.v1`, advertises `config.resolved.read`, and lets Web/IDE clients inspect redacted effective config values plus source precedence without shelling out. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Added reducer-friendly event summaries to direct-connect event snapshots. `/app/conversations/{session_id}/events` now returns a pinned `summary` object with turn count, event-type counts, tool-result totals/errors, changed-file paths, and terminal status/stop-reason, letting Web/IDE clients build conversation views and file-change badges without rescanning the full RuntimeEvent array. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Added a pinned app-server event view reducer contract. `/app/conversations/{session_id}/events` now returns `view.schema = "kiana.app-server.events-view.v1"` and `view.messages` with stable role/content/source metadata plus changed-file passthrough on tool results, letting Web/IDE clients render chat transcripts and file-change panels without reimplementing the RuntimeEvent reducer. Duplicate terminal `result.assistant_text` is suppressed when it repeats the last assistant message. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`, `cargo test -p kiana-entrypoints direct_connect_app_contract_schemas_match_packaged_schema_files --locked`, and `bash scripts/schema-contract-smoke.sh`.
- 2026-07-05: Exposed commercial release blockers through the direct-connect product shell. `/app/release/blockers` now returns the pinned `kiana.commercial-release-blockers.v1` report and advertises `release.blockers.read` in the `/app` contract, so Web/IDE clients can display local versus external commercial release blockers without rerunning Cargo, live smoke, signing, or publication gates. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed local RC evidence through the direct-connect product shell. `/app/release/local-rc-evidence` now returns the pinned `kiana.local-rc-evidence.v1` proof from `KIANA_LOCAL_RC_EVIDENCE_OUT` or `dist/proofs/local-rc-evidence.json`, advertises `release.local_rc_evidence.read`, and lets Web/IDE release-owner clients inspect artifact, manifest, proof, lifecycle-smoke, and blocker handoff evidence without rerunning heavyweight release gates from a GET request. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed source-control proof through the direct-connect product shell. `/app/release/source-control` now returns the pinned `kiana.source-control-proof.v1` proof from the same staged handoff paths used by commercial blocker reporting, advertises `release.source_control.read`, and lets Web/IDE release-owner clients inspect local RC versus accepted remote/tag source-control status without rerunning source-control checks from a GET request. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed release signature proof through the direct-connect product shell. `/app/release/signature` now returns the pinned `kiana.release-signature.v1` proof from explicit env handoff paths or packaged release signature files beside archives, advertises `release.signature.read`, and lets Web/IDE release-owner clients inspect target, archive, signer, signature file, and verification status without rerunning signing or verification commands from a GET request. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed enterprise offline manifest through the direct-connect product shell. `/app/release/enterprise-offline-manifest` now returns the pinned `kiana.enterprise.offline-manifest.v1` manifest from explicit env handoff paths or `DIST_DIR/manifests/enterprise/offline-manifest.json`, advertises `release.enterprise_offline_manifest.read`, and lets Web/IDE release-owner clients inspect artifact URLs, checksums, bundle-relative paths, channels, and generation metadata without rerunning distribution manifest generation or commercial artifact verification from a GET request. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed commercial proof manifest through the direct-connect product shell. `/app/release/proof-manifest` now returns the pinned `kiana.commercial-proof-manifest.v1` manifest from explicit env handoff paths or `DIST_DIR/proofs/PROOF-MANIFEST.json`, advertises `release.proof_manifest.read`, and lets Web/IDE release-owner clients inspect staged proof categories, accepted/live counts, platform coverage, source paths, destination paths, and sha256 handoff evidence without rerunning proof staging from a GET request. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed live provider smoke proof through the direct-connect product shell. `/app/release/live-provider-smoke` now returns the pinned `kiana.app-server.live-provider-smoke.v1` wrapper around `kiana.model-catalog.v1` and `kiana.model-smoke.v1` proof files from explicit env handoff paths, staged `DIST_DIR/proofs/live-smoke/provider/`, or generated `target/live-smoke/provider/`, advertises `release.live_provider_smoke.read`, and lets Web/IDE release-owner clients inspect live provider catalog and text/tool smoke evidence without rerunning provider live services from a GET request. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed offline provider smoke through the direct-connect product shell. `/app/models/smoke` now returns the pinned `kiana.model-smoke.v1` fake-provider text/tool smoke report, advertises `model.smoke.read`, and gives Web/IDE clients a local model-smoke readiness surface without shelling out or running live provider services. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed model profile list through the direct-connect product shell. `/app/models/list` now returns pinned `kiana.model-list.v1` wrapping `model list --json`, advertises `model.list.read`, and lets Web/IDE clients render provider/model capability pickers without shelling out. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed remote code-session smoke proof through the direct-connect product shell. `/app/release/remote-code-session-smoke` now returns the pinned `kiana.remote-code-session-smoke.v1` proof from explicit env handoff paths, staged `DIST_DIR/proofs/live-smoke/remote/code-session-smoke.json`, or generated `target/live-smoke/remote/code-session-smoke.json`, advertises `release.remote_code_session_smoke.read`, and lets Web/IDE release-owner clients inspect remote CCR/session smoke evidence without rerunning remote live services from a GET request. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed distribution review through the direct-connect product shell. `/app/release/distribution` now returns the pinned `kiana.app-server.distribution-review.v1` report from `KIANA_DISTRIBUTION_REVIEW_DIST_DIR` or `DIST_DIR`, advertises `release.distribution_review.read`, and lets Web/IDE release-owner clients inspect packaged artifacts, checksum files, Homebrew/winget state, enterprise offline manifest validity, and remaining distribution blockers without rerunning manifest generation or treating external publication as accepted. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed product acceptance through the direct-connect product shell. `/app/release/product-acceptance` now returns the pinned `kiana.product-acceptance.v1` proof from the same staged handoff paths used by commercial blocker reporting, advertises `release.product_acceptance.read`, and lets Web/IDE release-owner clients inspect headless local RC versus accepted target-customer workflow status without rerunning product acceptance gates from a GET request. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed entitlement proof through the direct-connect product shell. `/app/release/entitlement` now returns the pinned `kiana.entitlement-proof.v1` proof from the same staged handoff paths used by commercial blocker reporting, advertises `release.entitlement.read`, and lets Web/IDE release-owner clients inspect local RC versus production account/license entitlement status without rerunning entitlement proof gates from a GET request. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed release-operations proof through the direct-connect product shell. `/app/release/ops` now returns the pinned `kiana.release-ops.v1` proof from the same staged handoff paths used by commercial blocker reporting, advertises `release.ops.read`, and lets Web/IDE release-owner clients inspect local RC versus accepted release-operations status without rerunning release-ops gates from a GET request. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Exposed platform-security proof through the direct-connect product shell. `/app/release/platform-security` now returns the pinned `kiana.platform-security-proof.v1` proof from the same staged handoff paths used by commercial blocker reporting, advertises `release.platform_security.read`, and lets Web/IDE release-owner clients inspect local RC versus accepted platform isolation status without rerunning platform-security gates from a GET request. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.

### Phase 5: Skills, Plugins, Hooks, And Project Automation

References: `claude-code-main (2)`, Cline, Pi, Continue.

Scope:

- Formalize project resources:
  - `.kiana/agents`
  - `.kiana/checks`
  - `.kiana/rules`
  - `.kiana/prompts`
  - `.kiana/hooks`
- Implement hook events: PreToolUse, PostToolUse, UserPromptSubmit, SessionStart, Stop.
- Support hook outcomes: allow, deny, ask, block, update input, add context.
- Strengthen plugin manifest validation and marketplace fixtures.
- Keep `.claude` compatibility where useful.

Candidate code areas:

- `kiana-skills/src`
- `kiana-commands/src/plugin.rs`
- `kiana-commands/src/skills.rs`
- `kiana-commands/src/hooks.rs`
- `kiana-query/src/stop_hooks.rs`
- `kiana-tools/src/tool_execution.rs`
- `kiana-types/src/plugin.rs`

Exit criteria:

- Installing a plugin makes its commands/skills/hooks visible.
- Disabling a plugin removes all of its resources.
- Security hook fixtures can block dangerous tool calls.
- SessionStart can add context without editing user prompt text.
- Project resources are ignored until project trust is granted.

Progress:

- 2026-06-24: Started Phase 5 with the first generic `PreToolUse` security hook gate. `kiana-query::stop_hooks` now exposes a `PreToolUse` runner that reuses the existing env, home, trusted-project, and plugin hook resolution path, and `kiana-tools::tool_execution` calls it before validation, permission prompts, or `Tool::call`. A blocking hook returns a tool error and prevents the tool from mutating the worktree. Focused coverage is in `cargo test -p kiana-tools pre_tool_use_hook_blocks_mutating_tool_before_call`. Remaining Phase 5 work includes public hook command management for the new events, `PostToolUse`, `UserPromptSubmit`, `SessionStart` context injection, richer hook outcomes, plugin enable/disable resource visibility, and project-resource trust fixtures across the new hook events.
- 2026-06-24: Exposed the first public hook-management surface for the new generic hook events. `kiana hooks add pre-tool-use <command>`, `remove`, `clear`, `json`, and `status` now persist and report `PreToolUse` commands using the same `KIANA_PRE_TOOL_USE_HOOKS` env slot consumed by the tool-execution gate. Focused coverage is in `cargo test -p kiana-commands hooks_adds_pre_tool_use_hook_to_persistent_file`. Remaining Phase 5 work includes public management and runtime wiring for `PostToolUse`, `UserPromptSubmit`, `SessionStart`, richer hook outcomes, plugin enable/disable resource visibility, and project-resource trust fixtures across the new hook events.
- 2026-06-24: Added the first `SessionStart` add-context runtime path. `kiana-query::stop_hooks` now runs `SessionStart` hooks from env, home, trusted-project, and plugin hook sources and extracts `add_context`/`addContext` output. Non-streaming and streaming runner setup append that context to the request `system` field before the first model call, leaving the original user prompt message unchanged. Focused coverage is in `cargo test -p kiana-entrypoints run_assistant_turn_session_start_hook_adds_context_without_rewriting_user_prompt`. Remaining Phase 5 work includes public `kiana hooks` management for `SessionStart`, `PostToolUse`, and `UserPromptSubmit`, richer hook outcomes such as deny/ask/update-input, plugin enable/disable resource visibility, and project-resource trust fixtures across the new hook events.
- 2026-06-24: Exposed public `kiana hooks` management for `SessionStart`. `kiana hooks add session-start <command>`, `remove`, `clear`, `json`, and `status` now persist and report `SessionStart` commands using the same `KIANA_SESSION_START_HOOKS` env slot consumed by the runner context-injection path. Focused coverage is in `cargo test -p kiana-commands hooks_adds_session_start_hook_to_persistent_file`. Remaining Phase 5 work includes runtime and public management for `PostToolUse` and `UserPromptSubmit`, richer hook outcomes such as deny/ask/update-input, plugin enable/disable resource visibility, and project-resource trust fixtures across the new hook events.
- 2026-06-24: Added the first `PostToolUse` runtime hook path. `kiana-query::stop_hooks` now exposes a `PostToolUse` runner that reuses the existing env, home, trusted-project, and plugin hook resolution path, and `kiana-tools::tool_execution` calls it after tool execution so hooks can inspect the model-facing `tool_result` payload without rewriting the completed tool output. Focused coverage is in `cargo test -p kiana-tools post_tool_use_hook_runs_after_successful_tool_call`. Remaining Phase 5 work includes public `kiana hooks` management for `PostToolUse` and `UserPromptSubmit`, richer hook outcomes such as deny/ask/update-input, plugin enable/disable resource visibility, and project-resource trust fixtures across the new hook events.
- 2026-06-24: Exposed public `kiana hooks` management for `PostToolUse`. `kiana hooks add post-tool-use <command>`, `remove`, `clear`, `json`, and `status` now persist and report `PostToolUse` commands using the same `KIANA_POST_TOOL_USE_HOOKS` env slot consumed by the post-tool runtime path. Focused coverage is in `cargo test -p kiana-commands hooks_adds_post_tool_use_hook_to_persistent_file`. Remaining Phase 5 work includes runtime and public management for `UserPromptSubmit`, richer hook outcomes such as deny/ask/update-input, plugin enable/disable resource visibility, and project-resource trust fixtures across the new hook events.
- 2026-06-24: Added the first `UserPromptSubmit` add-context runtime path and public hook-management surface. `kiana-query::stop_hooks` now exposes a `UserPromptSubmit` runner with `user_prompt`/`userPrompt` payload fields and the shared env, home, trusted-project, and plugin hook resolver. Non-streaming and streaming runner setup append returned `add_context`/`addContext` text to the request `system` field before the first model call without rewriting the original user prompt message. `kiana hooks add user-prompt-submit <command>`, `remove`, `clear`, `json`, and `status` now persist and report `UserPromptSubmit` commands using `KIANA_USER_PROMPT_SUBMIT_HOOKS`. Focused coverage is in `cargo test -p kiana-entrypoints run_assistant_turn_user_prompt_submit_hook_adds_context_without_rewriting_user_prompt` and `cargo test -p kiana-commands hooks_adds_user_prompt_submit_hook_to_persistent_file`. Remaining Phase 5 work includes richer hook outcomes such as deny/ask/update-input, plugin enable/disable resource visibility, and project-resource trust fixtures across the new hook events.
- 2026-06-24: Added the first explicit richer hook outcome alias. Hook output with `decision: "deny"`/`"denied"`/`"reject"`/`"rejected"` now maps to the same blocking path as `decision: "block"`, preserving the hook-provided `reason` and preventing a `PreToolUse`-guarded tool from mutating the worktree. Focused coverage is in `cargo test -p kiana-tools pre_tool_use_hook_denies_mutating_tool_before_call`. Remaining Phase 5 work includes `ask` outcomes, plugin enable/disable resource visibility, and project-resource trust fixtures across the new hook events.
- 2026-06-24: Wired `update_input`/`updateInput` hook outcomes through `PreToolUse` and `UserPromptSubmit`. `PreToolUse` can now rewrite tool input before validation, permission checks, and execution, while `UserPromptSubmit` can replace the submitted user prompt before the first model request and still add system context. Focused coverage is in `cargo test -p kiana-tools pre_tool_use_hook_updates_tool_input_before_call` and `cargo test -p kiana-entrypoints run_assistant_turn_user_prompt_submit_hook_updates_prompt_input`. Remaining Phase 5 work includes `ask` outcomes, plugin enable/disable resource visibility, and project-resource trust fixtures across the new hook events.
- 2026-06-25: Wired `decision: "ask"` hook outcomes through the `PreToolUse` execution gate. Ask decisions now require the existing permission-prompt surface before the tool executes: approved prompts continue into normal permission policy and execution, while denied prompts return a tool error without mutation. Focused coverage is in `cargo test -p kiana-tools pre_tool_use_hook_ask_requests_permission_before_call`, `cargo test -p kiana-tools pre_tool_use_hook_ask_honors_permission_denial`, and `cargo test -p kiana-tools pre_tool_use_hook`. Remaining Phase 5 work includes plugin enable/disable resource visibility and project-resource trust fixtures across the new hook events.
- 2026-06-25: Closed the same-process plugin skill visibility cache gap. `kiana-skills::load_all_skills_with_trust` now keys its registry cache by cwd, project trust, and the currently enabled plugin roots, so a newly installed plugin skill becomes visible after an earlier skill-list cache fill, and a disabled plugin skill disappears without restarting the process. Focused coverage is in `cargo test -p kiana-skills plugin_skill_cache_refreshes --lib` and `cargo test -p kiana-skills --lib`. Remaining Phase 5 work includes plugin enable/disable visibility fixtures beyond prompt commands, agents, MCP, LSP, and skills, plus project-resource trust fixtures across the new hook events.
- 2026-07-05: Added direct project-trust source-boundary fixtures for `SessionStart`, `UserPromptSubmit`, and `PostToolUse`. The new coverage proves home hook sources remain active, trusted project `.kiana/hooks.json` sources load, and explicitly untrusted project hook sources are ignored for session context injection, prompt context/update-input, and post-tool blocking outcomes. Focused coverage is in `cargo test -p kiana-query --locked project_trust_source_boundary`. Remaining Phase 5 work includes plugin enable/disable visibility fixtures beyond prompt commands, agents, MCP, LSP, skills, and hooks, plus signed receipts and richer third-party product plugin contracts.
- 2026-07-05: Added plugin component JSON preflight to `kiana plugin validate`. Installed plugins now fail validation when packaged `.mcp.json` is malformed or lacks usable server entries, `.lsp.json` server configs omit commands, `hooks/hooks.json` does not match the shared hook schema, or `app.json` is not a JSON object, catching commercial plugin contract failures before MCP, LSP, hook, or app-server runtime discovery. Focused coverage is in `cargo test -p kiana-commands --locked plugin_validate_rejects_invalid_component_json_files`. Remaining Phase 5 work includes signed receipts and richer third-party product plugin contracts.
- 2026-07-05: Exposed plugin app manifest metadata through the direct-connect product shell. `/app/plugins` now includes parsed `app_manifest` metadata from plugin `app.json` files for enabled app components, and the pinned `kiana.app-server.plugins.v1` schema allows object/null manifest payloads so Web/IDE clients can discover third-party plugin app ids, titles, entries, and routes without shelling out. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked` and `cargo test -p kiana-entrypoints direct_connect_app_contract_schemas_match_packaged_schema_files --locked`. Remaining Phase 5 work is reduced to signed receipts and deeper third-party product plugin contracts beyond manifest metadata.
- 2026-07-05: Closed the local marketplace remote-source install gap. Local or cached `marketplace.json` files can now declare remote Git plugin source objects, and `kiana plugin install <plugin>@<marketplace>` resolves those objects through the same clone/cache path used by remote marketplace manifests instead of requiring only relative `path` sources. Focused coverage is in `cargo test -p kiana-commands plugin_install_from_local_marketplace_file_remote_git_source --locked`. Remaining Phase 5 work is reduced to signed receipts and deeper third-party product plugin contracts beyond Git/GitHub/npm-file source handling.
- 2026-07-05: Added constrained offline Python package plugin source support. Remote marketplace entries with `source = "pip"` now accept `package = "file:<relative-path>"` and resolve the package path inside the cached marketplace base with the same traversal checks used for npm file packages, keeping Python-distributed plugin bundles usable without arbitrary pip/network installation. Focused coverage is in `cargo test -p kiana-commands plugin_install_from_remote_marketplace_pip_file_package_source --locked`. Remaining Phase 5 work is reduced to signed receipts and deeper third-party product plugin contracts beyond Git/GitHub/npm/pip-file source handling.
- 2026-07-05: Preserved marketplace signature metadata in plugin install receipts. Marketplace entries can now include a `signature` object, `kiana plugin install` carries it into `.kiana-install-receipt.json`, the pinned receipt schema covers signer/key/content-hash/signature fields, and the stable receipt seal covers that provenance metadata alongside file hashes. Release smoke now installs a signed marketplace fixture and verifies signature metadata in the installed receipt. Focused coverage is in `cargo test -p kiana-commands plugin_install_records_marketplace_signature_metadata_in_receipt --locked`, `cargo test -p kiana-commands plugin --locked`, `bash scripts/schema-contract-smoke.sh`, and `bash scripts/release-smoke.sh`. Remaining Phase 5 work is reduced to external signature verification policy and deeper third-party product plugin contracts beyond provenance capture.
- 2026-07-05: Added managed plugin policy signature requirements. Enterprise policy files can now set `plugins.requireSignature = true` alongside allow/deny marketplace rules; `kiana plugin install` rejects unsigned marketplace entries or mismatched signature `contentHash` values before copying plugin files while signed matching marketplace entries continue into receipt/integrity handling. The managed policy schema now pins `requireSignature`, release smoke exercises the policy against the signed marketplace fixture, and preflight checks the schema plus smoke anchor. Focused coverage is in `cargo test -p kiana-commands plugin_install_rejects_unsigned_marketplace_when_managed_policy_requires_signature --locked`, `cargo test -p kiana-commands plugin_install_rejects_signature_content_hash_mismatch_when_managed_policy_requires_signature --locked`, `cargo test -p kiana-commands plugin --locked`, `bash scripts/schema-contract-smoke.sh`, and `bash scripts/release-smoke.sh`. Remaining Phase 5 work is reduced to cryptographic external signature verification and deeper third-party product plugin contracts beyond provenance capture.

### Phase 6: Provider, Model, And Auth Registry

References: Pi, Cline, LangChain, aider.

Scope:

- Add provider trait and registry.
- Keep Anthropic provider behavior unchanged.
- Add OpenAI-compatible and Ollama/local mockable providers.
- Add model profiles: context window, tool support, structured output, vision, reasoning, streaming.
- Add auth resolution: env key, config key, token file, future OAuth.

Candidate code areas:

- `kiana-services/src/api`
- `kiana-services/src/auth.rs`
- `kiana-services/src/oauth.rs`
- `kiana-commands/src/model.rs`
- `kiana-bootstrap/src/config.rs`
- `kiana-constants/src/api_limits.rs`

Exit criteria:

- Fake providers cover all core paths without network.
- Optional live smoke is explicitly gated by env variables.
- Model capability checks drive tool/vision/structured-output availability.
- Anthropic remains the default and passes existing smoke tests.

### Phase 7: Product Shell

References: Codex, Cline, Roo-Code, OpenHands, Pi.

Scope:

- TUI polish: history cells, command popup, approval overlay, resume picker, model selector, settings panels.
- Local hub: multiple clients can attach to one session.
- App-server: conversations, events, settings, secrets, sandbox, git endpoints.
- Future Web UI: chat, terminal, browser, changes, settings.
- IDE extension only after app-server protocol is stable.

Candidate code areas:

- `kiana-entrypoints/src/tui.rs`
- `kiana-screens/src`
- `kiana-components/src`
- `kiana-ink/src`
- `kiana-entrypoints/src/cli.rs`
- `kiana-bridge/src`
- `kiana-remote/src`

Exit criteria:

- TUI has snapshot or pseudo-terminal tests for key flows.
- Permission modal queue recovers after interruption.
- App-server has JSON fixtures and contract tests.
- Web or IDE clients consume stable protocol, not internal Rust-only types.

Progress:

- 2026-07-05: Strengthened the direct-connect `/app/secrets` readiness surface from an empty redaction placeholder into a pinned `kiana.app-server.secrets.v1` credential-metadata contract. Web/IDE clients can now inspect Anthropic API-key, Anthropic OAuth token-file, OpenAI-compatible API-key, and enterprise license-key source/status summaries without receiving raw secret values; the endpoint still advertises `read_supported=false` and `write_supported=false`. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.
- 2026-07-05: Added the direct-connect `/app/models/current` readiness surface for model selectors. The new pinned `kiana.app-server.model-current.v1` response reports the effective source, provider id, model id, and resolved capability profile for the active model so Web/IDE clients can render the current selection without shelling out or duplicating config precedence logic. Focused coverage is in `cargo test -p kiana-entrypoints direct_connect_app_contract_exposes_product_shell_endpoints --locked`.

### Phase 8: Advanced Agent And Knowledge Features

References: AutoGen, MetaGPT, LangChain, OpenHands.

Scope:

- Role profiles: PM, Architect, Engineer, QA, Data Analyst.
- Artifact store: PRD, design, tasks, source, tests, dependency graph.
- Team runtime with termination conditions and replay.
- Local RAG/indexing: first FTS and snippets, then embeddings.
- Notebook/data interpreter as an isolated worker or MCP.
- Eval harness and benchmark fixtures.

Exit criteria:

- Multi-agent workflows can run with deterministic fake models.
- Context-pack snippet dependency changes are traceable through a deterministic `kiana.context-artifact-graph.v1`; broader durable artifact dependency tracking still belongs to the later artifact-store/team-runtime work.
- Indexing is incremental and disabled by default until configured.
- Notebook execution is isolated, timed, and permission-gated.

## P0 Backlog

The first implementation wave should contain these ten deliverables:

1. Runtime event schema and JSON golden tests.
2. JSONL session tree with compatibility adapter.
3. Config layers and source precedence tests.
4. Permission profiles and project trust gate.
5. Read-only concurrent tool orchestration.
6. MCP prompts/resources/resource-template fixtures.
7. Repo map prototype with token budget.
8. Git checkpoint, AI diff, and undo.
9. Lint/test repair loop with mock model.
10. Plugin/skills/hooks compatibility fixture pack.

## Suggested Implementation Order

1. Create a formal implementation plan for Phase 1 only.
2. Finish Phase 1 and run full workspace tests.
3. Create implementation plans for Phase 2 and Phase 3.
4. Finish Phase 2 before enabling project trust-dependent plugins or hooks.
5. Finish Phase 3 before adding more tool surfaces.
6. Start Phase 4 only after session and permission events are stable.
7. Defer Phase 7 and Phase 8 until Phase 1 through Phase 6 have usable acceptance tests.

## Verification Strategy

Default local gate:

```bash
bash scripts/release-smoke.sh
```

Focused gates should be added per migration domain:

- Runtime/session: JSON golden tests and old session migration tests.
- Permissions: precedence fixtures and non-interactive approval failures.
- Tools: orchestration order tests and tool event snapshots.
- MCP: fake stdio/http/sse/ws servers.
- Repo workflow: fixture repo tests for map, diff, checkpoint, undo.
- Provider: fake provider tests plus optional live smoke.
- TUI: pseudo-terminal or ratatui snapshot tests.
- App-server: request/response JSON contract tests.

Live service tests must remain opt-in through environment variables. The default CI should not require paid APIs or remote service credentials.

## Risk Register

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Copying stubs from reference repos | False completion claims | Require behavior tests before marking complete |
| Starting with GUI or IDE | Unstable product surface | Finish runtime/session/policy/tool contracts first |
| Provider sprawl | High maintenance cost | Start with Anthropic, OpenAI-compatible, Ollama/mock |
| Project plugins before trust gate | Security exposure | Trust gate before project resources |
| Hooks before permission policy | Inconsistent decisions | Implement permissions and hook result contract together |
| Indexing/RAG too early | Large complexity increase | Start with repo map, lexical snippets, and deterministic snippet artifact graphs before adding embeddings or vector stores |
| Remote/live validation gaps | Misleading release readiness | Keep live smoke opt-in and explicitly reported |
| Existing docs overstate progress | Planning confusion | Treat `RELEASE.md` and smoke gates as current truth |

## Progress Targets

| Milestone | Expected maturity |
| --- | --- |
| Current state | 40/100 |
| Phase 1 through Phase 3 complete | 60/100 |
| Phase 4 through Phase 6 complete | 75/100 |
| Phase 7 complete | 85/100 |
| Phase 8 plus release packaging complete | 90+/100 |

## Decision

Proceed with **core loop first**. The next document should be a task-level implementation plan for Phase 1: runtime event schema and session tree. That plan should be small enough to implement with tests and without touching product UI.
