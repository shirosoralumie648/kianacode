<!-- refreshed: 2026-07-13 -->
# Architecture

**Analysis Date:** 2026-07-13

## System Overview

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│                         Product Surfaces / Entrypoints                       │
├──────────────────────┬─────────────────────┬────────────────────────────────┤
│ CLI / print / daemon │ REPL / TUI / SDK     │ MCP / Chrome / Computer / URL  │
│ `kiana-entrypoints`  │ `kiana-entrypoints` │ `kiana-entrypoints`, adapters │
└──────────┬───────────┴──────────┬──────────┴───────────────┬────────────────┘
           │                      │                          │
           ▼                      ▼                          ▼
┌──────────────────────┐ ┌────────────────────────┐ ┌──────────────────────────┐
│ Slash command layer  │ │ Assistant turn runner  │ │ Local MCP server          │
│ `kiana-commands/src` │ │ `kiana-entrypoints/src`│ │ `kiana-entrypoints/src/mcp.rs` │
└──────────┬───────────┘ └──────────┬─────────────┘ └──────────┬───────────────┘
           │                        │                          │
           ▼                        ▼                          ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ Runtime capabilities: tools, providers, workflows, skills, context, trust    │
│ `kiana-tools` · `kiana-services` · `kiana-tasks` · `kiana-skills` · `kiana-query` │
└───────────────────────────────┬──────────────────────────────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ Shared contracts and durable state                                           │
│ `kiana-types`, `kiana-constants`, `.kiana/tasks`, `.kiana/workflows`, SDK sessions │
└──────────────────────────────────────────────────────────────────────────────┘
```

Kiana is a Rust workspace whose top-level binary lives in `kiana-entrypoints` and delegates into smaller crates for commands, model/provider execution, tools, tasks/workflows, skills/plugins, query context, remote bridge transport, UI screens, and shared types. Preserve this crate-level layering when adding new functionality: user-facing surfaces route requests, library crates own durable state and policy, and shared contracts live below both.

## Component Responsibilities

| Component | Responsibility | File |
|-----------|----------------|------|
| Entrypoint dispatcher | Parses process arguments, handles top-level modes, and routes to local commands, SDK/print, daemon, bridge, MCP, Chrome, computer MCP, URL, TUI, and background handlers. | `kiana-entrypoints/src/cli.rs` |
| Entrypoint module exports | Exposes CLI, SDK, runner, REPL, MCP, sandbox, init, TUI, and background modules for the binary crate. | `kiana-entrypoints/src/lib.rs` |
| SDK/session API | Creates/resumes sessions, persists legacy JSON plus `events.jsonl`, and calls the assistant runner for executable prompts. | `kiana-entrypoints/src/sdk.rs` |
| Assistant runner | Converts model stream events to runtime events, dispatches tool calls, handles permission prompts, and normalizes turn results. | `kiana-entrypoints/src/runner.rs` |
| MCP server | Exposes Kiana tools/resources/prompts over stdio and HTTP/SSE/WebSocket JSON-RPC using the same tool registry as the runner. | `kiana-entrypoints/src/mcp.rs` |
| Command registry | Registers slash/local commands behind `Command` trait objects. | `kiana-commands/src/registry.rs` |
| Command contract | Defines `Command`, `CommandContext`, `CommandResult`, and `CommandType`. | `kiana-commands/src/types.rs` |
| Task/workflow command surface | Implements `kiana tasks`, workflow lifecycle, swarm dispatch/integration, verification handoff, and disk-backed task lists. | `kiana-commands/src/tasks.rs` |
| Tool registry | Registers read/edit/shell/agent/MCP/team/task/workflow/user-interaction tools behind `Tool` trait objects. | `kiana-tools/src/registry.rs` |
| Tool contract and path policy | Defines `Tool`, `ToolContext`, path resolution, access roots, editable/read-only file sets, validation, and tool errors. | `kiana-tools/src/tool.rs` |
| Tool execution | Applies permission decisions, mailbox grants, structured output handling, and tool call result normalization. | `kiana-tools/src/tool_execution.rs` |
| Provider layer | Defines built-in Anthropic, OpenAI-compatible, Ollama, and fake providers with model profiles and streaming modes. | `kiana-services/src/api/provider.rs` |
| Workflow runtime | Owns durable `WorkflowRun`, DAG, EventLog, state projection, transition, resume/repair, artifacts, and gate types. | `kiana-tasks/src/workflow.rs` |
| Evidence/integrity runtime | Owns verification packets, review packets, HMAC envelopes, ledger summaries, and immutable evidence artifacts. | `kiana-tasks/src/evidence.rs`, `kiana-tasks/src/integrity.rs` |
| Swarm runtime | Builds and persists WorkPacket-based parallel plans, dispatch manifests, execution manifests, and path locks. | `kiana-tasks/src/swarm.rs` |
| Query/context engine | Builds repo maps, context artifacts, context packs, indexes, token budget transitions, and hook orchestration. | `kiana-query/src/lib.rs` |
| Skill/plugin loader | Loads user, project-trusted, bundled, dynamic, MCP, and plugin skills; caches per cwd/trust/plugin state. | `kiana-skills/src/lib.rs` |
| Trust contracts | Stores project trust outside project-local legacy files, computes project roots/IDs, and fails closed on pending or invalid records. | `kiana-types/src/trust.rs` |
| Plugin contracts | Defines plugin manifests, installed/enabled plugin roots, disabled plugin state, and marketplace/component error types. | `kiana-types/src/plugin.rs` |
| Remote/bridge adapters | Convert SDK/runtime messages for remote code sessions, CCR v2 workers, bridge sessions, permissions, and websocket transport. | `kiana-remote/src/lib.rs`, `kiana-bridge/src/lib.rs` |
| UI layer | Holds ratatui screens and reusable components; surfaces should consume command/SDK/runtime contracts rather than duplicate logic. | `kiana-screens/src/lib.rs`, `kiana-components/src/lib.rs` |

## Pattern Overview

**Overall:** Layered Rust workspace with trait registries at the interaction boundaries and durable artifact/event-log state for long-running work.

**Key Characteristics:**
- Use `CommandRegistry` and `ToolRegistry` as the only normal registration points for local commands and tools.
- Keep user surfaces thin: `kiana-entrypoints` routes; `kiana-commands`, `kiana-tools`, `kiana-tasks`, and `kiana-services` own behavior.
- Persist complex project work through append-only workflow events plus materialized state, not ad hoc mutable files.
- Pass runtime context through `CommandContext.app_state` and `ToolContext.app_state`; avoid adding global mutable knobs when state can be explicit.
- Keep shared schemas and enums in `kiana-types` or `kiana-tasks` before exposing them through CLI, SDK, MCP, bridge, or TUI surfaces.

## Layers

**Surface Layer:**
- Purpose: Parse user/process input and expose product modes.
- Location: `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/repl.rs`, `kiana-entrypoints/src/tui.rs`, `kiana-entrypoints/src/mcp.rs`, `kiana-url-handler/src`, `kiana-chrome-mcp/src`, `kiana-computer-mcp/src`.
- Contains: CLI routing, print/resume parsing, daemon/background routing, REPL classification, TUI handlers, MCP transports, native host shims.
- Depends on: `kiana-commands`, `kiana-tools`, `kiana-services`, `kiana-skills`, `kiana-query`, `kiana-bridge`, `kiana-remote`, `kiana-types`.
- Used by: The `kiana` binary and external clients invoking CLI/MCP/URL/native-host surfaces.

**Command Layer:**
- Purpose: Implement slash/local commands and their text/JSON outputs.
- Location: `kiana-commands/src`.
- Contains: One module per command plus `registry.rs`, `types.rs`, local state helpers, and workflow/swarm command integration.
- Depends on: `kiana-bootstrap`, `kiana-query`, `kiana-services`, `kiana-skills`, `kiana-tasks`, `kiana-tools`, `kiana-types`.
- Used by: CLI, REPL, TUI, print-mode local command dispatch, tests, and future UI surfaces.

**Assistant Runtime Layer:**
- Purpose: Run model turns, stream events, compact context, call tools, and persist SDK sessions.
- Location: `kiana-entrypoints/src/runner.rs`, `kiana-entrypoints/src/sdk.rs`, `kiana-services/src/api`.
- Contains: Provider selection, Messages API adapters, stream event conversion, tool loop, session JSON/`events.jsonl` persistence, structured output handling.
- Depends on: `kiana-services`, `kiana-tools`, `kiana-types`, `kiana-query`.
- Used by: SDK prompt APIs, print mode, REPL, remote/bridge adapters, and TUI resume/rendering.

**Tool / Workbench Layer:**
- Purpose: Provide model-callable capabilities with validation and permission boundaries.
- Location: `kiana-tools/src`.
- Contains: File tools, grep/glob, bash/powershell/sandbox, agent/team/task/workflow lifecycle tools, MCP tools, notebook/LSP, web fetch/search, user interaction, cron/monitor, structured output.
- Depends on: `kiana-services`, `kiana-skills`, `kiana-types`, `kiana-query`.
- Used by: Assistant runner, MCP server, command surfaces that need tool execution, and tests.

**Workflow / Project OS Layer:**
- Purpose: Model long-running tasks, workflow DAGs, EventLogs, evidence, verification, review, project boards, and swarms.
- Location: `kiana-tasks/src`, exposed through `kiana-commands/src/tasks.rs`, `kiana-commands/src/validate.rs`, `kiana-commands/src/evidence.rs`, `kiana-commands/src/report.rs`, `kiana-commands/src/audit.rs`.
- Contains: `WorkflowRun`, `WorkflowState`, `WorkflowDagTemplate`, `WorkPacket`, `VerificationPacket`, `ReviewPacket`, HMAC integrity, path locks, project-board projections.
- Depends on: Mostly external serialization/filesystem crates; keep it below command and entrypoint layers.
- Used by: `kiana tasks workflow`, swarm commands, validate/evidence/report/audit gates, and future project orchestration.

**Context / Hook Layer:**
- Purpose: Build repository/context artifacts and run lifecycle hooks.
- Location: `kiana-query/src`.
- Contains: Repo map, context index, copied artifact store, vector search wrappers, token budget reducer, pre/post/session/user/stop hook orchestration.
- Depends on: `kiana-types`.
- Used by: CLI context commands, assistant runner, tool execution hooks, and project planning workflows.

**Extension and Trust Layer:**
- Purpose: Load skills/plugins safely and decide whether project-local resources are allowed.
- Location: `kiana-skills/src`, `kiana-types/src/trust.rs`, `kiana-types/src/plugin.rs`, `kiana-types/src/hooks.rs`, `kiana-tools/src/permissions.rs`.
- Contains: Skill discovery/caching, plugin skill source discovery, dynamic/bundled/MCP skills, project trust records, permission rules/modes, hook config parsing.
- Depends on: `kiana-types`, `kiana-services` for MCP skills.
- Used by: Commands, tools, assistant runner, and plugin-related user surfaces.

## Data Flow

### Local Slash Command Path

1. CLI/REPL/TUI identifies a command and arguments in `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/repl.rs`, or `kiana-entrypoints/src/tui.rs`.
2. The surface resolves `create_default_command_registry()` from `kiana-commands/src/registry.rs`.
3. The command receives `CommandContext { args, app_state }` from `kiana-commands/src/types.rs`.
4. The command returns `CommandResult` text/system/exit metadata; prompt commands can re-enter the assistant path through `CommandType::Prompt`.

### Assistant Prompt Path

1. SDK/print/REPL calls `unstable_v2_prompt*` in `kiana-entrypoints/src/sdk.rs`.
2. Session persistence appends user messages to the default sessions root: `KIANA_SDK_SESSIONS_DIR`, then `KIANA_HOME/sdk-sessions`, then `$HOME/.kiana/sdk-sessions`, then local `.kiana/sdk-sessions`.
3. `run_assistant_turn_with_permission_handler` in `kiana-entrypoints/src/runner.rs` selects a provider from `kiana-services/src/api/provider.rs`.
4. Model stream events are converted into `RuntimeEvent` payloads from `kiana-types/src/runtime.rs`.
5. Tool calls are dispatched through `execute_tool_calls_with_permission_handler` in `kiana-tools/src/tool_execution.rs` and `create_default_registry()` in `kiana-tools/src/registry.rs`.
6. The completed turn writes assistant/result messages and session-tree `events.jsonl` records through `kiana-entrypoints/src/sdk.rs`.

### MCP Tool Path

1. `McpServer::start`, HTTP, SSE, or WebSocket handlers in `kiana-entrypoints/src/mcp.rs` receive JSON-RPC messages.
2. `handle_list_tools` serializes schemas from `ToolRegistry::list_tools()`.
3. Tool calls run through `execute_tool_call` / `execute_tool_calls_with_permission_handler` with a shared `ToolContext`.
4. MCP resources and prompts expose tool status, permission status, and prompt templates such as `kiana-status-report`, `kiana-tool-audit`, and `kiana-permissions-review`.

### Workflow and Swarm Path

1. `kiana tasks workflow ...` enters `TasksCommand` in `kiana-commands/src/tasks.rs`.
2. Workflow initialization uses `initialize_workflow_run` and `default_workflow_template` from `kiana-tasks/src/workflow.rs`.
3. Transitions use `append_workflow_transition`, which validates persisted DAG edges and appends gate/node events.
4. Resume uses `resume_workflow_run` and returns `ready`, `repaired`, or `blocked`; commands must honor `WorkflowResumeStatus::can_continue()`.
5. Swarm commands build `SwarmWorkPacket` values through `kiana-tasks/src/swarm.rs` and persist dispatch/integration artifacts under the workflow artifact directory.

### Skill / Plugin Load Path

1. Callers use `load_all_skills(cwd)` or `load_all_skills_with_trust(cwd, project_trust)` from `kiana-skills/src/lib.rs`.
2. User and `KIANA_HOME` skill dirs are always candidates; project `.claude/skills` and project `.kiana/plugins*` are candidates only when `ProjectTrust::allows_project_resources()`.
3. Plugin skills are prefixed as `plugin_name:skill_name` in `kiana-skills/src/plugins.rs`.
4. Conditional skills are stored separately and activated by path rules through `kiana-skills/src/dynamic.rs`.

**State Management:**
- SDK sessions: legacy `<session_id>.json` plus `<session_id>/events.jsonl` under `KIANA_SDK_SESSIONS_DIR`, `KIANA_HOME/sdk-sessions`, `$HOME/.kiana/sdk-sessions`, or local `.kiana/sdk-sessions`.
- Task lists: `KIANA_TASKS_ROOT` or `<cwd>/.kiana/tasks/<task_list>` via `kiana-commands/src/tasks.rs`.
- Workflows: `.kiana/workflows/<run_id>/` with `workflow_dag.json`, `eventlog.jsonl`, `state.json`, evidence, verification, review, workpacket, and resultpacket directories.
- Project trust: user-store records computed from the project root in `kiana-types/src/trust.rs`; legacy `.kiana/trust.json` is reported but not authoritative.
- Plugins: user `.kiana/plugins` or `KIANA_HOME/plugins`, plus project `.kiana/plugins` and `.kiana/plugins.local` when trusted.

## Key Abstractions

**`Command`:**
- Purpose: Uniform local/slash command execution.
- Examples: `kiana-commands/src/types.rs`, `kiana-commands/src/registry.rs`, command modules under `kiana-commands/src/*.rs`.
- Pattern: Implement `Command`, declare `supports_non_interactive` where safe, register in `create_default_command_registry()`, and add focused command tests.

**`Tool`:**
- Purpose: Uniform model/MCP-callable capability with validation, permission checks, schema, and optional workbench metadata.
- Examples: `kiana-tools/src/tool.rs`, `kiana-tools/src/registry.rs`, `kiana-tools/src/file_read.rs`, `kiana-tools/src/bash_tool.rs`, `kiana-tools/src/mcp_tool.rs`.
- Pattern: Implement `Tool`, enforce path/network/policy checks before side effects, register once in `create_default_registry()`, and keep schema free of top-level combinators.

**`Provider`:**
- Purpose: Adapter boundary between Kiana messages and model backends.
- Examples: `kiana-services/src/api/provider.rs`, `kiana-services/src/api/messages.rs`, `kiana-services/src/api/streaming.rs`.
- Pattern: Define provider metadata, auth method, protocol, streaming support, model profile, capability checks, and focused standard-provider tests.

**`RuntimeEvent`:**
- Purpose: Shared event schema for assistant stream deltas, tool calls/results, permission requests, errors, and turn results.
- Examples: `kiana-types/src/runtime.rs`, adapters in `kiana-entrypoints/src/runner.rs`, `kiana-remote/src/sdk_message_adapter.rs`, `kiana-bridge/src/sdk_message_adapter.rs`.
- Pattern: Convert surface-specific events into runtime events before rendering/exporting; do not invent parallel event schemas in UI or bridge code.

**`WorkflowRun` and `WorkflowState`:**
- Purpose: Durable project/task execution container with DAG, state, evidence, and gates.
- Examples: `kiana-tasks/src/workflow.rs`, `kiana-commands/src/tasks.rs`, tests under `kiana-tasks/tests/workflow_runtime.rs`.
- Pattern: EventLog is the fact source; `state.json` is a recoverable projection. Write transitions through runtime APIs only.

**`WorkPacket` / swarm contracts:**
- Purpose: Bound parallel execution by task, allowed files, verification, budgets, and integration state.
- Examples: `kiana-tasks/src/swarm.rs`, `kiana-commands/src/tasks.rs`, `kiana-tasks/tests/swarm_*.rs`.
- Pattern: Generate/persist packet manifests before worker launch; use path locks and integration gates for cross-worker changes.

**Project trust and permissions:**
- Purpose: Decide whether project-local skills/plugins/hooks/MCP resources may load and whether tools may act.
- Examples: `kiana-types/src/trust.rs`, `kiana-types/src/permissions.rs`, `kiana-tools/src/permissions.rs`, `kiana-commands/src/trust.rs`.
- Pattern: Unknown/untrusted projects withhold project resources; permission denial and ask states must produce structured reasons.

## Entry Points

**Binary CLI:**
- Location: `kiana-entrypoints/src/main.rs` through `kiana-entrypoints/src/cli.rs` and `kiana-entrypoints/Cargo.toml` `[[bin]] name = "kiana"`.
- Triggers: User runs `kiana`, `kiana -p`, `kiana tasks`, `kiana mcp-server`, `kiana daemon`, `kiana tui`, `kiana chrome`, or related aliases.
- Responsibilities: Parse flags, choose the correct product surface, reject invalid flag combinations, and keep top-level routing explicit.

**REPL:**
- Location: `kiana-entrypoints/src/repl.rs`.
- Triggers: No-argument terminal mode or explicit REPL route.
- Responsibilities: Classify `/command`, `!` shell shortcuts, plain prompts, and prompt commands, then dispatch through command registry or SDK prompt execution.

**TUI:**
- Location: `kiana-entrypoints/src/tui.rs`, `kiana-screens/src`, `kiana-components/src`.
- Triggers: `kiana tui` or TUI actions.
- Responsibilities: Render app/screens/history/settings and dispatch user actions through existing command/SDK/runtime contracts.

**SDK:**
- Location: `kiana-entrypoints/src/sdk.rs`.
- Triggers: Print mode, REPL prompt execution, remote/bridge calls, tests, and programmatic callers.
- Responsibilities: Create/resume/list/export/import/fork/update sessions and run executable or record-only prompts.

**MCP server:**
- Location: `kiana-entrypoints/src/mcp.rs`.
- Triggers: `mcp-server`, `mcp-server-stdio`, `mcp-server-http`, `mcp-server-sse`, `mcp-server-ws`, and `--mcp-server`.
- Responsibilities: Expose local tool registry, resources, and prompts through MCP transports.

**Remote / Bridge:**
- Location: `kiana-remote/src`, `kiana-bridge/src`, CLI bridge routes in `kiana-entrypoints/src/cli.rs`.
- Triggers: `remote-control`, `remote-session`, `sync`, `bridge`, CCR v2 worker/client flows.
- Responsibilities: Convert local SDK/runtime messages for remote environments, manage credentials/session APIs, upload bundles, and bridge permissions.

**Native integrations:**
- Location: `kiana-chrome-mcp/src`, `kiana-computer-mcp/src`, `kiana-url-handler/src`, `kiana-computer-input/src`, `kiana-screen-capture/src`.
- Triggers: Chrome native host, computer MCP, deep links, computer input/screen capture capabilities.
- Responsibilities: Keep integration-specific protocol code outside core command/tool/workflow crates.

## Architectural Constraints

- **Async runtime:** Use Tokio-compatible async APIs for model, MCP, network, and long-running I/O paths; keep pure workflow/state helpers sync when they operate on local files and are tested as deterministic units.
- **Trait registries:** New commands and tools must be registered in `kiana-commands/src/registry.rs` or `kiana-tools/src/registry.rs`; avoid hidden side registries except plugin prompt commands loaded by `kiana-commands/src/plugin_commands.rs`.
- **Dependency direction:** Lower crates such as `kiana-types`, `kiana-tasks`, `kiana-query`, and `kiana-services` must not depend on `kiana-entrypoints` or `kiana-commands`. Put shared contracts below surfaces before exposing them.
- **Global state:** Existing process globals include skill caches in `kiana-skills/src/lib.rs`, dynamic skill storage in `kiana-skills/src/dynamic.rs`, and environment/config overlays in `kiana-bootstrap/src/config.rs`. Prefer explicit context or cached immutable snapshots over new mutable globals.
- **Filesystem authority:** `ToolContext::resolve_access_path`, editable/read-only file sets, `KIANA_ACCESS_ROOTS`, `KIANA_TASKS_ROOT`, `KIANA_SDK_SESSIONS_DIR`, project trust store paths, and workflow artifact directories define what code may read/write.
- **Workflow integrity:** Workflow commands must validate EventLog/state/DAG identity through `kiana-tasks/src/workflow.rs`; do not write `state.json`, `eventlog.jsonl`, verification packets, or review packets directly from command code.
- **Trust boundary:** Project `.claude/skills`, project `.kiana/plugins`, and `.kiana/plugins.local` require project trust. User and `KIANA_HOME` skills/plugins are separate trust scopes.
- **Network policy:** WebFetch/WebSearch and HTTP MCP paths must use `kiana-services/src/network_policy.rs` semantics for localhost/private/metadata/redirect handling.

## Anti-Patterns

### Bypassing Registries

**What happens:** A new command or tool is called directly from an entrypoint or runner path instead of being discoverable through `CommandRegistry` or `ToolRegistry`.
**Why it's wrong:** CLI, REPL, TUI, MCP, tests, and model tool schemas diverge when registration is not centralized.
**Do this instead:** Implement the trait in a focused module and register it in `kiana-commands/src/registry.rs` or `kiana-tools/src/registry.rs`.

### Mutating Workflow Artifacts Directly

**What happens:** Code writes `.kiana/workflows/<run_id>/state.json`, `eventlog.jsonl`, or verification/review files without going through `kiana-tasks`.
**Why it's wrong:** Resume/repair, HMAC/integrity, edge validation, duplicate detection, and immutable artifact guarantees can be bypassed.
**Do this instead:** Add or reuse runtime functions in `kiana-tasks/src/workflow.rs`, `kiana-tasks/src/evidence.rs`, or `kiana-tasks/src/integrity.rs`, then expose them via `kiana-commands/src/tasks.rs`, `validate.rs`, or `evidence.rs`.

### Loading Project Resources Without Trust

**What happens:** Code reads project-local skills, plugins, hooks, or MCP configuration before checking `ProjectTrust`.
**Why it's wrong:** Untrusted project code can inject instructions, tools, or hooks into the runtime.
**Do this instead:** Route skill/plugin discovery through `load_all_skills_with_trust` and trust helpers in `kiana-types/src/trust.rs`; mirror this pattern for new project-local resources.

### Embedding Surface-Specific Event Shapes

**What happens:** A new UI, bridge, or remote path invents custom assistant/tool event JSON instead of converting to `RuntimeEvent`.
**Why it's wrong:** Export, resume, TUI rendering, remote bridge, and session replay lose parity.
**Do this instead:** Extend `kiana-types/src/runtime.rs` and update adapters in `kiana-entrypoints/src/runner.rs`, `kiana-remote/src/sdk_message_adapter.rs`, and `kiana-bridge/src/sdk_message_adapter.rs`.

### Hardcoding Persistent Paths

**What happens:** Code assumes `.kiana/sdk-sessions`, `.kiana/tasks`, or a workflow path without honoring environment/app-state overrides.
**Why it's wrong:** Tests, hosted mode, user-configured storage, and remote/bridge modes break.
**Do this instead:** Use `default_sessions_dir()` in `kiana-entrypoints/src/sdk.rs`, `tasks_root()` / `task_list_dir()` in `kiana-commands/src/tasks.rs`, and workflow artifact paths returned by `kiana-tasks`.

## Error Handling

**Strategy:** Boundary crates use `anyhow::Result` for user-facing command/entrypoint flows, while core libraries expose structured `thiserror` enums and typed error codes.

**Patterns:**
- Use `anyhow::Context` in entrypoint/command paths where the caller needs actionable text.
- Use typed errors such as `ToolError`, `ProviderError`, `WorkflowError`, `IntegrityError`, and command-specific errors inside reusable libraries.
- Return structured denial reasons for tool permissions and network policy; do not collapse policy failures into generic I/O errors.
- For workflow resume/integrity issues, fail closed with `blocked` and a recommended action rather than silently repairing unverifiable state.
- Keep command help/usage errors explicit and tested through command registry tests.

## Cross-Cutting Concerns

**Logging:** Use `tracing` in libraries and runtime paths; user-facing commands should return `CommandResult` text/JSON rather than printing from deep library code.

**Validation:** Validate paths, session IDs, workflow IDs, DAG edges, schema versions, JSON packets, tool schemas, provider capabilities, plugin manifests, network targets, and trust records at the boundary where input enters the system.

**Authentication:** API keys and base URLs flow through `kiana-bootstrap/src/config.rs`, provider metadata in `kiana-services/src/api/provider.rs`, remote credential code in `kiana-remote/src`, and auth commands in `kiana-commands/src/auth.rs`, `login.rs`, and `logout.rs`.

**Permissions:** Tool decisions flow through `kiana-tools/src/permissions.rs` and `kiana-tools/src/tool_execution.rs`; project trust flows through `kiana-types/src/trust.rs` and `kiana-commands/src/trust.rs`.

**Configuration:** Base/user/project/local/env/managed-style overlays are represented in `kiana-bootstrap/src/config.rs`, `kiana-types/src/permissions.rs`, and command modules such as `kiana-commands/src/config.rs`, `model.rs`, `theme.rs`, `brief.rs`, and `auto_mode.rs`.

**Release/commercial proof:** Release and audit scripts live under `scripts/`; release-facing commands live in `kiana-commands/src/release.rs`, `doctor.rs`, `audit.rs`, `validate.rs`, `evidence.rs`, and `report.rs`.

---

*Architecture analysis: 2026-07-13*
