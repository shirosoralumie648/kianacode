<!-- GSD:project-start source:PROJECT.md -->

## Project

**Kiana**

Kiana 是一个面向高级个人用户、公开发行用户、团队和企业的完整 AI Agent 产品。它以同一个本地优先的 `Kiana Core` 支撑 Coding、Academic Research 和 Daily Work 三个能力包，并通过 CLI/TUI、Headless SDK/RPC/MCP、IDE、Desktop、Web/App Server 提供一致的任务执行体验。

产品以 Claude Code 的公开功能覆盖作为 Coding 基线，以 Claude Desktop 的桌面工作区和连接器体验作为桌面基线；同时逐项审计 `reference/` 中的项目，形成经过许可证、安全和产品治理的能力超集。个人核心采用 Open Core 模式开源，官方云与企业自托管版提供同步、远程执行、团队协作和治理能力。

**Core Value:** Kiana 必须在覆盖 Claude Code 公开核心能力的基础上，更可靠地完成真实长任务，并用可验证证据和可恢复状态证明任务确实完成。

### Constraints

- **Tech stack**: 延续 Rust 2021、Tokio、Cargo workspace 和既有 crate 分层；共享契约先进入 `kiana-types` 或对应低层 crate
- **Architecture**: 所有产品入口必须消费同一 runtime、session、tool、policy 和 event contracts，禁止为 Desktop、Web 或能力包复制核心逻辑
- **Providers**: 首版即多供应商；能力不一致时必须显式报告、路由或降级，不能静默假装等价
- **Platforms**: Linux/macOS 原生，Windows/WSL；平台沙箱、路径、终端和安装差异必须进入测试矩阵
- **Data**: Local-first、无需账户、凭据进入系统 Keychain 或服务端 secret vault；同步和遥测均为显式启用
- **Safety**: ProjectTrust、权限档位、网络策略、沙箱和外部副作用审批必须 fail closed
- **Research integrity**: 所有结论、引用、数据、实验和图表保留 provenance；无法验证的内容不得进入最终结论
- **Licensing**: 开源源码仅在许可证兼容时复用；专有产品只进行公开行为层的 clean-room 式独立实现
- **Release quality**: 安装、升级、回滚、恢复、安全、供应链、性能、文档和支持都是功能完成的一部分
- **Commercial delivery**: 个人开源版、官方云和企业自托管版共享核心契约，但租户、RBAC、审计和数据保留边界必须隔离
- **Scope control**: `reference/` 覆盖通过能力矩阵治理；没有矩阵条目、owner、测试和证据的能力不能进入完成统计

<!-- GSD:project-end -->

<!-- GSD:stack-start source:codebase/STACK.md -->

## Technology Stack

## Languages

- Rust 2021 edition - all runtime, CLI, command, task, tool, service, MCP, and package code under workspace crates such as `kiana-entrypoints/`, `kiana-commands/`, `kiana-tasks/`, `kiana-tools/`, and `kiana-types/`.
- Bash - release, packaging, commercial-readiness, smoke, and proof scripts under `scripts/`.
- Python - JSON Schema validation helper at `scripts/validate-json-schema.py`.
- Markdown / JSON Schema / TOML - product documentation, design plans, API contracts, Cargo manifests, and runtime configuration under `docs/`, `docs/schemas/`, `.kiana-example.toml`, and crate `Cargo.toml` files.

## Runtime

- Rust workspace using resolver 2 in `Cargo.toml`.
- Minimum Rust version is declared as `rust-version = "1.96"` in the workspace package metadata.
- Primary binary is `kiana` from `kiana-entrypoints/src/bin/kiana.rs`.
- Cargo with workspace lockfile `Cargo.lock`.
- Shell scripts are plain Bash scripts under `scripts/`; do not introduce another package manager without documenting it in `CONFIG.md` and release scripts.

## Workspace Layout

- `kiana-entrypoints/` - CLI, TUI, app-server, runner, MCP entry points, SDK-facing surfaces, and binary wiring.
- `kiana-commands/` - command implementations for config, auth, model, tasks, trust, plugin, release, eval, evidence, project, audit, report, EDA, and validation flows.
- `kiana-tasks/` - task board, workflow DAG, integrity, evidence, bounded-swarm, and orchestration domain logic.
- `kiana-tools/` - agent/tool execution, Bash sandboxing, MCP tools, LSP/REPL/task/team creation, permissions, remote trigger, discovery, and policy enforcement.
- `kiana-types/` - shared contracts for trust, plugins, events, schemas, runtime, and cross-crate types.
- `kiana-services/` - provider, HTTP, web search/fetch, model/service integrations, and app-service helpers.
- `kiana-skills/` - skill loading, trust-aware skill visibility, plugin skill cache behavior.
- `kiana-query/` - query runtime contracts and stop-hook handling.
- `kiana-remote/` and `kiana-bridge/` - remote sessions, WebSocket/HTTP bridge, runtime event adapters.
- `kiana-chrome-mcp/`, `kiana-computer-mcp/`, `kiana-computer-input/`, and `kiana-screen-capture/` - Chrome/native host, MCP, computer-use, input, and capture integrations.
- `kiana-screens/`, `kiana-components/`, `kiana-ink/`, `kiana-color-diff/`, and `kiana-modifiers/` - TUI, UI primitives, terminal rendering, diffing, and platform input modifiers.
- `kiana-url-handler/` - desktop URL handler binary and platform integrations.

## Frameworks

- `tokio` - async runtime across CLI, services, remote, MCP, and task/tool execution.
- `clap` - CLI argument parsing for command surfaces in `kiana-entrypoints/` and `kiana-commands/`.
- `serde` / `serde_json` / `serde_yaml` / `toml` - configuration, JSON output contracts, schema-backed reports, and serialized state.
- `anyhow` / `thiserror` - error handling conventions across crates.
- `async-trait`, `futures`, `futures-util`, `tokio-stream` - async traits and streaming.
- `reqwest` with `rustls-tls` - outbound HTTP for providers, web surfaces, marketplace/GitHub materialization, and remote/service integrations.
- `axum` with WebSocket support - app-server and local server surfaces from `kiana-entrypoints/`.
- `tokio-tungstenite` - bridge/remote WebSocket transport.
- `url` and `urlencoding` - URL validation and encoding in network/tool surfaces.
- `ratatui`, `crossterm`, `rustyline`, and `colored` - CLI/TUI rendering, input, REPL, and terminal UX.
- `syntect` and `similar` - colorized diffs and syntax-aware rendering.
- `enigo`, `rdev`, `xcap`, `arboard`, and `image` - optional/native computer-use, input, screenshot, clipboard, and image surfaces.
- Rust unit and integration tests are run through Cargo.
- JSON contracts are validated by `scripts/schema-contract-smoke.sh` and `scripts/validate-json-schema.py`.
- Release and packaging are validated through `scripts/release-smoke.sh`, `scripts/package-release.sh`, and `scripts/package-lifecycle-smoke.sh`.

## Key Dependencies

- `tokio` - required for nearly all async execution and test paths.
- `serde_json` - required for CLI `--json`, stream-json, app-server, schemas, MCP, workflow logs, and proof artifacts.
- `reqwest` - required for provider/network/marketplace surfaces; keep `rustls-tls` and shared network policy behavior aligned.
- `sha2` and `ring` - required for trust, evidence, integrity, workflow, HMAC/hash, and release-proof surfaces.
- `uuid` and `chrono` - required for IDs, timestamps, session/workflow events, and proof metadata.
- `walkdir`, `glob`, `ignore`, and `regex` - repo/file discovery, rules, matching, and skill/tool scanning.
- `dirs` - user/home config paths such as `~/.kiana/config.toml`.
- `windows`, `windows-sys`, `libc`, `core-foundation`, `core-graphics`, `x11-dl`, `zbus`, `cocoa`, and `objc` - platform-specific trust, native host, URL handler, input, and UI integrations.

## Configuration

- User config lives at `~/.kiana/config.toml`; example configuration is `.kiana-example.toml`.
- Anthropic provider variables include `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`, and `ANTHROPIC_MODEL`.
- OpenAI-compatible provider variables include `KIANA_PROVIDER=openai-compatible`, `KIANA_OPENAI_API_KEY`, `KIANA_OPENAI_BASE_URL`, and `KIANA_OPENAI_MODEL`.
- Ollama provider variables include `KIANA_PROVIDER=ollama`, `KIANA_OLLAMA_BASE_URL`, and `KIANA_OLLAMA_MODEL`.
- Managed policy variables include `KIANA_MANAGED_SETTINGS_FILE`, `KIANA_MANAGED_POLICY_FILE`, and `KIANA_MANAGED_PLUGIN_POLICY_FILE`.
- Workspace build configuration is in root `Cargo.toml`.
- Release profile uses optimized, thin-LTO, stripped binaries in root `Cargo.toml`.
- Native computer-use is gated by the `native-computer-use` feature in `kiana-entrypoints/Cargo.toml`, which enables `kiana-computer-mcp/native`.
- Bash sandbox settings are documented in `.kiana-example.toml` and `CONFIG.md`; Linux controlled environments should prefer `enabled = true`, `failIfUnavailable = true`, and `allowUnsandboxedCommands = false`.

## Platform Requirements

- Rust toolchain compatible with workspace `rust-version = "1.96"`.
- Cargo workspace commands should be run from repository root.
- For sandboxed Bash execution on Linux, install and configure `bubblewrap` / `bwrap` when sandbox is enabled.
- Optional native computer-use builds require platform desktop/input/screenshot dependencies for `kiana-computer-mcp/native`.
- Release artifacts are staged under `dist/`.
- Packaging and smoke scripts live under `scripts/`.
- Commercial proof should use `scripts/commercial-release-blockers-report.sh`, `scripts/release-preflight.sh`, `scripts/release-smoke.sh`, `scripts/package-lifecycle-smoke.sh`, and related proof/report scripts.

## Prescriptive Stack Guidance

- Add shared domain contracts to `kiana-types/` before duplicating JSON shapes across crates.
- Add command-level behavior to `kiana-commands/`, then wire entrypoint dispatch in `kiana-entrypoints/`.
- Keep network access behind `kiana-services/` or policy-aware tool surfaces; do not create ad hoc direct HTTP clients in command code.
- Update `docs/schemas/` and schema smoke scripts whenever a `--json`, stream, app-server, MCP, or release-proof contract changes.
- Keep optional native/platform functionality behind crate features or target-specific dependency sections.

<!-- GSD:stack-end -->

<!-- GSD:conventions-start source:CONVENTIONS.md -->

## Conventions

## Naming Patterns

- Use Rust module filenames in `snake_case`, matching the domain noun or command name: `kiana-commands/src/local_state.rs`, `kiana-tasks/src/project_board.rs`, `kiana-tools/src/bash_sandbox.rs`, `kiana-types/src/runtime.rs`.
- Keep one top-level command surface per file under `kiana-commands/src/`; add a matching integration test file under `kiana-commands/tests/` when the command has user-visible behavior.
- Name generated schema documents with explicit product prefixes and versions in `docs/schemas/`, for example `docs/schemas/kiana-eval-report.v1.schema.json`.
- Use `snake_case` verbs and explicit domain nouns: `initialize_workflow_run`, `append_workflow_event`, `read_project_trust`, `validate_verification_packet_integrity`.
- Prefer small parsing, validation, read/write, and rendering helpers near the command that owns them: `parse_args`, `context_cwd`, `resolve_run`, and report constructors in `kiana-commands/src/evidence.rs`.
- For command entry points, implement `async fn execute(&self, context: CommandContext) -> Result<CommandResult>` through the shared `Command` trait from `kiana-commands/src/types.rs`.
- Use `snake_case` and keep domain names visible: `workflow_id`, `run_id`, `artifact_dir`, `eventlog_path`, `project_root`.
- Use `args`, `rest`, `subcommand`, and `json_output` consistently in command parsers such as `kiana-commands/src/evidence.rs` and `kiana-commands/src/validate.rs`.
- Use path variables as `Path`/`PathBuf` and name them by role: `root`, `home`, `state_path`, `packet_path`, `plugins_dir`.
- Use `UpperCamelCase` for structs, enums, traits, and error types: `WorkflowRun`, `WorkflowError`, `ProjectTrustRecord`, `BashTool`, `EvidenceCommand`.
- Use `SCREAMING_SNAKE_CASE` for exported constants and schema IDs: `PROJECT_TRUST_SCHEMA`, `WORKFLOW_ARTIFACT_DESCRIPTOR_SCHEMA`, `ACCESS_ROOTS_ENV`.
- Name command structs as `{Domain}Command` and keep them stateless unless a command explicitly owns injected dependencies.

## Code Style

- Use workspace Rust formatting defaults; no `rustfmt.toml`, `.rustfmt.toml`, or `clippy.toml` is detected.
- Run `cargo fmt --all --check` before handing off changes. This is also the first release-smoke gate in `scripts/release-smoke.sh`.
- Keep `Cargo.toml` workspace settings centralized in `Cargo.toml`: resolver `2`, edition `2021`, workspace `rust-version = "1.96"`, and shared dependency versions under `[workspace.dependencies]`.
- Use `deny.toml` as the dependency policy gate: wildcard dependencies are denied, unknown registries/git sources are denied, multiple versions warn, and licenses are allowlisted.
- Keep advisory ignores in `deny.toml` annotated with the owning dependency and revisit reason; do not add silent advisory ignores.
- Prefer `cargo test --workspace --locked --offline --no-fail-fast` for broad correctness because this workspace has no dedicated clippy config.

## Import Organization

- Order imports as local crate modules first, workspace and external crates second, and standard-library imports third.
- Keep related imports together within each layer; platform-gated `std::os::*` imports stay beside their `#[cfg(...)]` usage.
- Use Rust-native `crate::`, `super::`, and workspace crate names; no custom path alias system is detected.
- Keep platform-specific imports beside their gated usage with `#[cfg(target_os = "linux")]`, `#[cfg(unix)]`, or `#[cfg(windows)]`, as in `kiana-commands/src/tasks.rs` and `kiana-types/src/trust.rs`.

## Error Handling

- Use `anyhow::Result` and `anyhow!` in command modules where user-facing error strings are part of CLI behavior, for example `kiana-commands/src/evidence.rs`.
- Use `thiserror::Error` for core/domain libraries where callers need typed errors, for example `WorkflowError` in `kiana-tasks/src/workflow.rs`, `EvidenceError` in `kiana-tasks/src/evidence.rs`, and `ToolError` in `kiana-tools/src/tool.rs`.
- Add path and operation context with `anyhow::Context` / `with_context` around filesystem and process operations, especially in security-sensitive flows such as `kiana-commands/src/tasks.rs`.
- Keep machine-readable error prefixes in command errors when tests assert them, for example `workflow_required`, `workflow_not_found`, and `workflow_inconsistent` in `kiana-commands/src/validate.rs`.
- Do not use `unwrap` or `expect` in production paths except for impossible invariants; tests and fixture builders commonly use `unwrap` for clarity.

## Logging

- Prefer returning exact human text or pretty JSON from commands instead of printing directly; see `CommandResult::text` in `kiana-commands/src/evidence.rs` and `kiana-commands/src/license.rs`.
- Use shell-script stderr for operational failures and diagnostics, with line-aware traps in `scripts/release-smoke.sh`.
- In tests, reserve `eprintln!` for skip messages when optional local tools are unavailable, as in `kiana-tools/src/notebook_execute.rs` and `kiana-tools/src/file_read.rs`.

## Comments

- Comment security, concurrency, and persistence invariants that are not obvious from the type signature; `kiana-types/src/trust.rs` documents pending-marker read safety.
- Comment dependency policy exceptions in `deny.toml` with the upstream package and revisit reason.
- Keep implementation comments focused on why a guard exists; avoid broad status or roadmap comments inside source files.
- Public API rustdoc is sparse in current code; when adding exported types in `kiana-tasks/src/lib.rs`, `kiana-types/src/lib.rs`, or `kiana-tools/src/lib.rs`, prefer concise comments only for non-obvious contracts.

## Function Design

- Keep domain operations factored into parse, validate, execute, and render helpers. Add focused helpers and tests instead of extending large orchestration branches indefinitely.
- Pass `&Path`, `&CommandContext`, and borrowed strings where possible; return owned report types or `serde_json::Value` only at serialization boundaries.
- Use `Result<T, DomainError>` in library crates and `anyhow::Result<CommandResult>` in command crates. Machine-readable reports should use serializable structs with an explicit `schema` field.

## Module Design

- Use each crate's `lib.rs` as its stable export surface; keep internal helpers private or `pub(crate)`.
- Add commands as focused modules under `kiana-commands/src/`, implement `Command`, and register them in `kiana-commands/src/registry.rs`.
- Preserve established serde casing and optional-field conventions for public contracts instead of inventing parallel shapes.
- Normalize and validate filesystem paths before I/O, preserving no-follow, pending-marker, and lease invariants in trust and workflow code.

<!-- GSD:conventions-end -->

<!-- GSD:architecture-start source:ARCHITECTURE.md -->

## Architecture

## System Overview

```text
Product surfaces: CLI / REPL / TUI / SDK / MCP / native integrations
                              |
                              v
Entrypoints: kiana-entrypoints (routing, adapters, assistant runner)
                              |
                              v
Capabilities: commands / tools / providers / workflows / skills / context
                              |
                              v
Shared contracts and state: kiana-types / EventLog / .kiana artifacts
```

Kiana is a layered Rust workspace. User-facing surfaces route requests, domain crates own behavior and durable state, and shared contracts live below both. New surfaces must reuse the same runtime, policy, event, and persistence contracts.

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

**Overall:** Layered Rust workspace with trait registries at interaction boundaries and durable artifact/EventLog state for long-running work.

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

1. CLI, REPL, or TUI identifies the command and arguments.
2. The surface resolves `create_default_command_registry()` and supplies `CommandContext`.
3. The command returns `CommandResult`; prompt commands may re-enter the assistant path through `CommandType::Prompt`.

### Assistant Prompt Path

1. SDK, print mode, or REPL calls `unstable_v2_prompt*` and persists the user message in the configured session root.
2. The runner selects a provider, converts model stream events to `RuntimeEvent`, and dispatches tool calls through the shared `ToolRegistry` with permission checks.
3. The completed turn appends assistant/result messages and session-tree `events.jsonl` records.

### MCP Tool Path

1. stdio, HTTP, SSE, or WebSocket handlers receive JSON-RPC messages.
2. Tool schemas come from `ToolRegistry::list_tools()`; calls run through the same execution and permission path as local assistant tools.
3. MCP resources and prompts expose status, policy, and diagnostic templates without creating a separate runtime model.

### Workflow and Swarm Path

1. `kiana tasks workflow ...` enters `TasksCommand` and initializes a durable workflow through `kiana-tasks`.
2. Transitions validate DAG edges and append events; resume returns `ready`, `repaired`, or `blocked` and callers must honor that status.
3. Swarm work packets and dispatch/integration manifests are persisted under the workflow artifact directory before execution or integration.

### Skill / Plugin Load Path

1. Callers load skills with cwd and project-trust context.
2. User-level roots are candidates by default; project skills and plugins require trusted project resources.
3. Plugin skills are namespaced, and conditional skills activate only through declared path rules.

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
- Location: `kiana-entrypoints/src/bin/kiana.rs` through `kiana-entrypoints/src/cli.rs` and `kiana-entrypoints/Cargo.toml` `[[bin]] name = "kiana"`.
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

- Do not call new commands or tools directly from an entrypoint or runner. Implement the shared trait and register it in `CommandRegistry` or `ToolRegistry` so every surface sees the same capability.

### Mutating Workflow Artifacts Directly

- Do not write workflow state, EventLog, verification, or review artifacts outside `kiana-tasks`; doing so bypasses transition, integrity, resume, and duplicate checks.

### Loading Project Resources Without Trust

- Do not load project-local skills, plugins, hooks, or MCP configuration before checking `ProjectTrust`; untrusted resources can inject instructions or executable capabilities.

### Embedding Surface-Specific Event Shapes

- Do not invent UI-, bridge-, or remote-only assistant event schemas. Extend `RuntimeEvent` and update all adapters so replay and cross-surface behavior remain aligned.

### Hardcoding Persistent Paths

- Do not assume local `.kiana` paths. Use session, task, workflow, and app-state path resolvers so tests, hosted mode, and configured storage roots remain valid.

## Error Handling

**Strategy:** Boundary crates use `anyhow::Result` for user-facing flows; reusable libraries expose structured errors and stable codes.

- Use `anyhow::Context` in entrypoint/command paths where the caller needs actionable text.
- Use typed errors such as `ToolError`, `ProviderError`, `WorkflowError`, `IntegrityError`, and command-specific errors inside reusable libraries.
- Return structured denial reasons for tool permissions and network policy; do not collapse policy failures into generic I/O errors.
- For workflow resume/integrity issues, fail closed with `blocked` and a recommended action rather than silently repairing unverifiable state.
- Keep command help/usage errors explicit and tested through command registry tests.

## Cross-Cutting Concerns

- **Logging:** Use `tracing` in runtime/library paths; user-facing commands return `CommandResult` text or JSON instead of printing deep inside libraries.
- **Validation:** Validate paths, IDs, DAG edges, schema versions, packets, tool schemas, provider capabilities, plugin manifests, network targets, and trust records at ingress boundaries.
- **Authentication and permissions:** Keep credentials in provider/remote auth boundaries, and route all tool and project-resource decisions through shared permission and trust contracts.
- **Configuration:** Preserve deterministic base, user, project, local, environment, and managed-policy overlays.
- **Release proof:** Release-facing commands and scripts must emit auditable build, schema, packaging, security, and readiness evidence.

<!-- GSD:architecture-end -->

<!-- GSD:skills-start source:skills/ -->

## Project Skills

No project skills found. Add skills to any of: `.claude/skills/`, `.agents/skills/`, `.cursor/skills/`, `.github/skills/`, or `.codex/skills/` with a `SKILL.md` index file.
<!-- GSD:skills-end -->

<!-- GSD:workflow-start source:GSD defaults -->

## GSD Workflow Enforcement

Before using Edit, Write, or other file-changing tools, start work through a GSD command so planning artifacts and execution context stay in sync.

Use these entry points:

- `/gsd-quick` for small fixes, doc updates, and ad-hoc tasks
- `/gsd-debug` for investigation and bug fixing
- `/gsd-execute-phase` for planned phase work

Do not make direct repo edits outside a GSD workflow unless the user explicitly asks to bypass it.
<!-- GSD:workflow-end -->

<!-- GSD:profile-start -->

## Developer Profile

> Profile not yet configured. Run `/gsd-profile-user` to generate your developer profile.
> This section is managed by `generate-claude-profile` -- do not edit manually.
<!-- GSD:profile-end -->
