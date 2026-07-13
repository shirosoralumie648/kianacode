# Codebase Structure

**Analysis Date:** 2026-07-13

## Directory Layout

```text
kianacode/
├── Cargo.toml                     # Rust workspace definition and shared dependency versions
├── Cargo.lock                     # Workspace dependency lockfile
├── kiana-entrypoints/             # Binary, CLI routing, SDK, runner, REPL, TUI, MCP surfaces
├── kiana-commands/                # Slash/local command implementations and command registry
├── kiana-tools/                   # Model/MCP-callable tools, tool registry, permission execution
├── kiana-tasks/                   # WorkflowRun, evidence, integrity, project board, swarm runtime
├── kiana-types/                   # Shared schemas, runtime events, trust, plugin, permission contracts
├── kiana-services/                # API providers, auth/OAuth, compacting, MCP/network/LSP services
├── kiana-query/                   # Repo map, context artifacts/index, token budgets, hooks
├── kiana-skills/                  # Skill discovery, plugin skills, bundled/dynamic/MCP skills
├── kiana-bootstrap/               # Config loading and process-global state helpers
├── kiana-remote/                  # Remote code-session, CCR v2, websocket, git bundle adapters
├── kiana-bridge/                  # Bridge API/session/work polling and SDK message conversion
├── kiana-screens/                 # Ratatui app screens and screen-level state
├── kiana-components/              # Reusable ratatui component primitives
├── kiana-ink/                     # Ink-like rendering experiment/helpers
├── kiana-coordinator/             # Coordinator/worker prompt mode helpers
├── kiana-constants/               # Shared constants, limits, strings, product metadata
├── kiana-modifiers/               # Prompt/modifier helpers
├── kiana-color-diff/              # Color diff utilities
├── kiana-chrome-mcp/              # Chrome MCP and native-host integration
├── kiana-computer-mcp/            # Computer-use MCP surface
├── kiana-computer-input/          # Computer input helpers
├── kiana-screen-capture/          # Screen capture helpers
├── kiana-url-handler/             # Deep-link / URL handler binary
├── docs/                          # Product, architecture, workflow, release, schema documentation
├── scripts/                       # Release, smoke, compliance, proof, packaging scripts
├── reference/                     # Read-only reference projects used for migration research
├── .kiana/                        # Local runtime state: tasks, workflows, agents, teams
├── .planning/codebase/            # Generated GSD codebase map documents
├── dist/                          # Built release/proof artifacts
└── target/                        # Cargo build output, generated and not source authority
```

## Directory Purposes

**`kiana-entrypoints/`:**
- Purpose: Own all process-facing entrypoints and the top-level `kiana` binary.
- Contains: `src/cli.rs`, `src/sdk.rs`, `src/runner.rs`, `src/repl.rs`, `src/mcp.rs`, `src/tui.rs`, `src/bg.rs`, `src/sandbox.rs`, `src/init.rs`.
- Key files: `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/sdk.rs`, `kiana-entrypoints/src/runner.rs`, `kiana-entrypoints/src/mcp.rs`.

**`kiana-commands/`:**
- Purpose: Own local/slash command behavior independent of any one UI surface.
- Contains: `registry.rs`, `types.rs`, one source file per command, local-state helpers, and command integration tests.
- Key files: `kiana-commands/src/registry.rs`, `kiana-commands/src/types.rs`, `kiana-commands/src/tasks.rs`, `kiana-commands/src/trust.rs`, `kiana-commands/tests/*.rs`.

**`kiana-tools/`:**
- Purpose: Own tool schemas, tool implementations, path/access policy, permission execution, and model/MCP-callable workbenches.
- Contains: File/read/write/edit/delete tools, grep/glob, shell, PowerShell, MCP, web, LSP, notebook, agent/team/task/workflow/cron/monitor/user-interaction tools.
- Key files: `kiana-tools/src/tool.rs`, `kiana-tools/src/registry.rs`, `kiana-tools/src/tool_execution.rs`, `kiana-tools/src/permissions.rs`, `kiana-tools/src/bash_tool.rs`, `kiana-tools/src/mcp_tool.rs`.

**`kiana-tasks/`:**
- Purpose: Own durable project-operation runtime objects.
- Contains: Workflow DAG/runtime, evidence ledger, integrity/HMAC, project-board projection, swarm planning/dispatch, stop task registry, task ID helpers.
- Key files: `kiana-tasks/src/workflow.rs`, `kiana-tasks/src/evidence.rs`, `kiana-tasks/src/integrity.rs`, `kiana-tasks/src/swarm.rs`, `kiana-tasks/src/project_board.rs`, `kiana-tasks/tests/*.rs`.

**`kiana-types/`:**
- Purpose: Own shared data contracts used across entrypoints, tools, commands, bridge, skills, and services.
- Contains: Runtime events, messages, tools, permissions, trust, plugin manifests, hooks, IDs, logs, simple runtime types.
- Key files: `kiana-types/src/runtime.rs`, `kiana-types/src/trust.rs`, `kiana-types/src/plugin.rs`, `kiana-types/src/permissions.rs`, `kiana-types/src/hooks.rs`.

**`kiana-services/`:**
- Purpose: Own reusable service clients and service-level policies.
- Contains: Anthropic/OpenAI-compatible/Ollama/fake providers, API messages/streaming/retry/logging, auth/OAuth, compaction, MCP helpers, LSP, network policy.
- Key files: `kiana-services/src/api/provider.rs`, `kiana-services/src/api/messages.rs`, `kiana-services/src/network_policy.rs`, `kiana-services/src/mcp.rs`, `kiana-services/tests/provider_standard.rs`.

**`kiana-query/`:**
- Purpose: Own codebase/context intelligence and hook orchestration.
- Contains: Query config/deps, repo map, context artifact ingest/index/search, token budget transitions, stop/pre/post/user/session hooks.
- Key files: `kiana-query/src/lib.rs`, `kiana-query/src/index.rs`, `kiana-query/src/repo_map.rs`, `kiana-query/src/stop_hooks.rs`, `kiana-query/src/transitions.rs`.

**`kiana-skills/`:**
- Purpose: Own skill discovery, parsing, trust-aware loading, plugin-skill loading, dynamic/conditional skills, bundled skills, and MCP skill fetches.
- Contains: `loader.rs`, `plugins.rs`, `dynamic.rs`, `bundled.rs`, `mcp.rs`, `types.rs`.
- Key files: `kiana-skills/src/lib.rs`, `kiana-skills/src/loader.rs`, `kiana-skills/src/plugins.rs`.

**`kiana-bootstrap/`:**
- Purpose: Own startup config and runtime state primitives.
- Contains: Config overlays from files/env/managed policy and global state types.
- Key files: `kiana-bootstrap/src/config.rs`, `kiana-bootstrap/src/state.rs`.

**`kiana-remote/`:**
- Purpose: Own cloud/remote-session integrations and message adapters.
- Contains: Code session API, CCR v2 worker, remote permission bridge, git bundle upload, environment providers, websocket sessions, SDK message adapter.
- Key files: `kiana-remote/src/lib.rs`, `kiana-remote/src/remote_session_manager.rs`, `kiana-remote/src/ccr_v2_worker.rs`, `kiana-remote/src/sdk_message_adapter.rs`.

**`kiana-bridge/`:**
- Purpose: Own bridge-side API/session/poll-loop/work transport.
- Contains: API client, session manager, SDK message adapter, transport, work loop, bridge types.
- Key files: `kiana-bridge/src/lib.rs`, `kiana-bridge/src/api.rs`, `kiana-bridge/src/work.rs`, `kiana-bridge/src/sdk_message_adapter.rs`.

**UI and integration crates:**
- Purpose: Keep UI or integration-specific protocol code outside the core runtime.
- Contains: `kiana-screens/src`, `kiana-components/src`, `kiana-ink/src`, `kiana-chrome-mcp/src`, `kiana-computer-mcp/src`, `kiana-computer-input/src`, `kiana-screen-capture/src`, `kiana-url-handler/src`.
- Key files: `kiana-screens/src/app.rs`, `kiana-components/src/lib.rs`, `kiana-chrome-mcp/src/lib.rs`, `kiana-url-handler/src/main.rs`.

**Docs and runtime artifacts:**
- Purpose: Document product/implementation contracts and persist local project operation state.
- Contains: `docs/kiana_project_os/`, `docs/workflow-runtime-design.md`, `docs/reference-migration-roadmap.md`, `.kiana/tasks/`, `.kiana/workflows/`, `scripts/`, `dist/`.
- Key files: `docs/kiana_project_os/01-system-overview-and-product-logic.md`, `docs/workflow-runtime-design.md`, `scripts/release-smoke.sh`.

## Key File Locations

**Entry Points:**
- `kiana-entrypoints/src/cli.rs`: Top-level process argument router and product-surface selection.
- `kiana-entrypoints/src/main.rs`: Binary main module for the `kiana` executable.
- `kiana-entrypoints/src/sdk.rs`: Programmatic session/prompt API and SDK session persistence.
- `kiana-entrypoints/src/runner.rs`: Assistant turn loop, stream conversion, provider calls, and tool loop.
- `kiana-entrypoints/src/mcp.rs`: Local MCP server implementation.
- `kiana-entrypoints/src/repl.rs`: REPL routing for slash commands, shell shortcuts, and prompts.
- `kiana-entrypoints/src/tui.rs`: TUI action handling and screen integration.
- `kiana-url-handler/src/main.rs`: Deep-link handler binary.

**Configuration:**
- `Cargo.toml`: Workspace members, shared package metadata, shared dependency versions.
- `Cargo.lock`: Resolved dependency versions.
- `.kiana-example.toml`: Example Kiana config.
- `kiana-bootstrap/src/config.rs`: Config file/env/managed overlay loading.
- `kiana-types/src/permissions.rs`: Permission-mode and permission-update contract types.
- `kiana-tools/src/permissions.rs`: Effective tool-permission evaluation.
- `kiana-types/src/trust.rs`: Project trust store, root, ID, and pending/tombstone handling.
- `deny.toml`: Cargo-deny policy.

**Core Logic:**
- `kiana-commands/src/registry.rs`: Local command registration.
- `kiana-tools/src/registry.rs`: Tool registration and schema collection.
- `kiana-tools/src/tool.rs`: Tool trait, context, path resolution, access roots, file editability.
- `kiana-tools/src/tool_execution.rs`: Tool-call execution and permission prompt integration.
- `kiana-services/src/api/provider.rs`: Provider registry, model profile, protocol, streaming, capability checks.
- `kiana-tasks/src/workflow.rs`: Workflow runtime, DAG, transitions, resume/repair, state/eventlog.
- `kiana-tasks/src/evidence.rs`: Evidence ledger, verification and review packet APIs.
- `kiana-query/src/index.rs`: Context artifact ingest/index/search.
- `kiana-skills/src/lib.rs`: Trust-aware skill loading and cache.
- `kiana-remote/src/sdk_message_adapter.rs`: Remote runtime/SDK event adapter.
- `kiana-bridge/src/sdk_message_adapter.rs`: Bridge runtime/SDK event adapter.

**Testing:**
- `kiana-entrypoints/tests/*.rs`: CLI, session, resume, stream JSON, auth, export, MCP, eval tests.
- `kiana-commands/tests/*.rs`: Command-level workflow, audit, evidence, report, validate, swarm tests.
- `kiana-tasks/tests/*.rs`: Workflow runtime/integrity, evidence, project board, swarm tests.
- `kiana-services/tests/provider_standard.rs`: Provider contract tests.
- `kiana-bridge/tests/runtime_event_adapter.rs`: Bridge event adapter tests.
- Module-local `#[cfg(test)]` blocks: Registry, config, trust, skill, permission, and helper edge cases.

**Documentation:**
- `docs/kiana_project_os/`: Product OS design volumes.
- `docs/workflow-runtime-design.md`: Workflow runtime protocol and state-machine rules.
- `docs/reference-migration-roadmap.md`: Reference migration plan and baseline.
- `docs/sdk-runtime-events.md`: SDK/runtime event schema documentation.
- `docs/commercial-release-readiness.md`: Commercial readiness tracking.
- `README.md`, `USAGE.md`, `CONFIG.md`, `RELEASE.md`, `SECURITY.md`: User/operator documentation.

**Local Artifacts:**
- `.kiana/tasks/`: Disk-backed task lists, defaulting from `kiana-commands/src/tasks.rs`.
- `.kiana/workflows/<run_id>/`: Workflow DAG, EventLog, state, evidence, verification, review, workpacket, resultpacket artifacts.
- `.planning/codebase/`: Generated codebase map output.
- `dist/`: Release candidates, manifests, compliance/proof artifacts.

## Naming Conventions

**Files:**
- Rust modules use snake_case: `kiana-commands/src/auto_mode.rs`, `kiana-tools/src/tool_execution.rs`, `kiana-tasks/src/project_board.rs`.
- Command modules generally match slash command names: `model.rs` for `/model`, `trust.rs` for `/trust`, `reload_plugins.rs` for `/reload-plugins`.
- Tool modules describe the callable capability: `file_read.rs`, `file_write.rs`, `bash_tool.rs`, `mcp_tool.rs`, `task_create.rs`.
- Integration crates keep protocol names in crate names: `kiana-chrome-mcp`, `kiana-computer-mcp`, `kiana-url-handler`.

**Directories:**
- Workspace crates use the `kiana-` prefix and kebab-case names.
- Tests live beside the crate they verify under `<crate>/tests/` for integration tests or `#[cfg(test)]` inside source modules for unit tests.
- Durable local runtime state lives under `.kiana/`; generated analysis/planning artifacts live under `.planning/`; release/build artifacts live under `dist/` and `target/`.

**Types and traits:**
- Public traits use role nouns: `Command`, `Tool`, `Provider`, `PermissionPromptHandler`.
- Durable workflow/domain structs use explicit contract names: `WorkflowRun`, `WorkflowState`, `WorkflowDagTemplate`, `VerificationPacket`, `ReviewPacket`, `SwarmWorkPacket`.
- Schema constants use uppercase `*_SCHEMA` naming in task/evidence/workflow modules.

**Commands and tools:**
- User command names are kebab-case strings returned by `Command::name()`: `auto-mode`, `reload-plugins`.
- Tool names are the schema-facing names returned by `Tool::name()`: `Read`, `Grep`, `Bash`, `MCP`, `TaskCreate`, `ToolSearch`.
- Plugin skill names are prefixed in code as `plugin_name:skill_name`.

## Dependency Direction

Use this order when deciding where new code belongs:

1. `kiana-types` and `kiana-constants`: shared schemas, enums, and constants.
2. `kiana-services`, `kiana-bootstrap`, `kiana-query`, `kiana-tasks`: reusable service/runtime libraries.
3. `kiana-skills` and `kiana-tools`: extension and capability layers.
4. `kiana-commands`: user-facing command wrappers over core libraries.
5. `kiana-entrypoints`, `kiana-screens`, `kiana-remote`, `kiana-bridge`: product surfaces and transports.

Do not add dependencies from lower library crates back into `kiana-entrypoints` or `kiana-commands`. If two upper surfaces need the same type, move the type down into `kiana-types` or the owning runtime crate.

## Where to Add New Code

**New local/slash command:**
- Primary code: `kiana-commands/src/<command>.rs`.
- Registration: `kiana-commands/src/lib.rs` and `kiana-commands/src/registry.rs`.
- Tests: `kiana-commands/tests/<command>_command.rs` plus unit tests in the module for parsing edge cases.
- Surface routing: Only modify `kiana-entrypoints/src/cli.rs` if the command is a top-level process mode, not an ordinary slash/local command.

**New top-level CLI mode or flag:**
- Primary code: `kiana-entrypoints/src/cli.rs`.
- Shared behavior: Put reusable logic in `kiana-commands`, `kiana-services`, `kiana-tasks`, or `kiana-types` first.
- Tests: `kiana-entrypoints/tests/cli_<feature>.rs`.

**New model provider or model capability:**
- Primary code: `kiana-services/src/api/provider.rs`.
- Message/stream schema: `kiana-services/src/api/messages.rs` and `kiana-services/src/api/streaming.rs`.
- Config exposure: `kiana-bootstrap/src/config.rs` and `kiana-commands/src/model.rs` if user-facing.
- Tests: `kiana-services/tests/provider_standard.rs` and focused runner/CLI smoke tests if behavior reaches print mode.

**New model-callable tool:**
- Primary code: `kiana-tools/src/<tool>.rs`.
- Trait/schema: implement `Tool` from `kiana-tools/src/tool.rs`.
- Registration: `kiana-tools/src/lib.rs` and `kiana-tools/src/registry.rs`.
- Permission/policy: reuse `kiana-tools/src/permissions.rs`, `ToolContext` path helpers, and service network policy where relevant.
- Tests: module unit tests plus registry/schema tests if schema metadata changes.

**New workflow/runtime feature:**
- Primary code: `kiana-tasks/src/workflow.rs`, `evidence.rs`, `integrity.rs`, `project_board.rs`, or `swarm.rs`.
- Command wrapper: `kiana-commands/src/tasks.rs`, `validate.rs`, `evidence.rs`, `report.rs`, or `audit.rs`.
- Tests: `kiana-tasks/tests/workflow_*.rs` for core state and `kiana-commands/tests/*_command.rs` for CLI behavior.
- Rule: Write workflow state through runtime APIs; never directly patch `.kiana/workflows/<run_id>/state.json` from command code.

**New evidence, review, or verification packet:**
- Primary code: `kiana-tasks/src/evidence.rs` and schema constants there.
- Integrity: `kiana-tasks/src/integrity.rs` when HMAC/envelope behavior changes.
- Command exposure: `kiana-commands/src/evidence.rs`, `validate.rs`, or `report.rs`.
- Tests: `kiana-tasks/tests/evidence_ledger.rs`, `review_packet.rs`, and command-level tests.

**New context/repo intelligence feature:**
- Primary code: `kiana-query/src/index.rs`, `repo_map.rs`, `transitions.rs`, or `token_budget.rs`.
- Command exposure: `kiana-commands/src/context.rs`.
- Tool exposure: `kiana-tools/src/tool_search.rs`, `grep.rs`, `glob.rs`, or a new tool module if model-callable.
- Tests: module unit tests and command tests for JSON/text contract.

**New skill/plugin behavior:**
- Skill loading: `kiana-skills/src/loader.rs`, `kiana-skills/src/lib.rs`, `kiana-skills/src/dynamic.rs`.
- Plugin sources: `kiana-skills/src/plugins.rs` and shared plugin contracts in `kiana-types/src/plugin.rs`.
- Command exposure: `kiana-commands/src/skills.rs`, `plugin.rs`, `plugin_commands.rs`, or `reload_plugins.rs`.
- Trust: use `ProjectTrust` from `kiana-types/src/trust.rs`; project-local resources require trusted state.

**New permission/trust/policy feature:**
- Shared contracts: `kiana-types/src/permissions.rs`, `kiana-types/src/trust.rs`, `kiana-types/src/hooks.rs`.
- Tool enforcement: `kiana-tools/src/permissions.rs`, `kiana-tools/src/tool_execution.rs`, `kiana-tools/src/exec_policy.rs`.
- Command exposure: `kiana-commands/src/permissions.rs`, `kiana-commands/src/trust.rs`, `kiana-commands/src/hooks.rs`.
- Tests: module-level trust/permission tests and command tests.

**New UI screen or TUI behavior:**
- App/screen state: `kiana-screens/src/<screen>.rs` or `kiana-screens/src/app.rs`.
- Reusable widgets: `kiana-components/src/<component>.rs`.
- Entrypoint wiring: `kiana-entrypoints/src/tui.rs`.
- Rule: Use command/SDK/runtime contracts; do not duplicate business logic in screen code.

**New remote/bridge feature:**
- Remote code-session/client behavior: `kiana-remote/src`.
- Bridge polling/session behavior: `kiana-bridge/src`.
- Event conversion: `kiana-remote/src/sdk_message_adapter.rs` or `kiana-bridge/src/sdk_message_adapter.rs`.
- Tests: crate-specific adapter/session tests plus CLI route tests when exposed through `kiana-entrypoints/src/cli.rs`.

**New MCP or native integration:**
- Core MCP server/tool exposure: `kiana-entrypoints/src/mcp.rs` and `kiana-tools/src/mcp_tool.rs`.
- Service-level MCP/client behavior: `kiana-services/src/mcp.rs`.
- Chrome integration: `kiana-chrome-mcp/src`.
- Computer-use integration: `kiana-computer-mcp/src`, `kiana-computer-input/src`, `kiana-screen-capture/src`.
- URL/deep link behavior: `kiana-url-handler/src`.

**New documentation or release proof:**
- Product/system design: `docs/kiana_project_os/`.
- Runtime protocol: `docs/workflow-runtime-design.md` or `docs/sdk-runtime-events.md`.
- Release/compliance/proof scripts: `scripts/`.
- User docs: `README.md`, `USAGE.md`, `CONFIG.md`, `RELEASE.md`, `SECURITY.md`.

**New workspace crate:**
- Add crate directory `kiana-<name>/` with `Cargo.toml` and `src/lib.rs`.
- Add it to `Cargo.toml` `[workspace].members`.
- Keep dependency direction below the surface that consumes it.
- Add focused tests under `kiana-<name>/tests/` when behavior is externally observable.

## Special Directories

**`.kiana/`:**
- Purpose: Local runtime state for this checkout.
- Generated: Yes.
- Committed: Project-dependent; treat current contents as local runtime artifacts unless explicitly in scope.
- Notes: Contains `.kiana/tasks`, `.kiana/workflows`, agents, and teams.

**`.planning/codebase/`:**
- Purpose: Generated GSD codebase map consumed by future planning/execution agents.
- Generated: Yes.
- Committed: Depends on GSD workflow policy.
- Notes: This run writes `ARCHITECTURE.md` and `STRUCTURE.md`.

**`docs/kiana_project_os/`:**
- Purpose: Product OS design volumes and software-logic references.
- Generated: No.
- Committed: Yes.
- Notes: Use these docs for product intent, but confirm current implementation in source before claiming a feature exists.

**`reference/`:**
- Purpose: Read-only reference projects used for migration comparison and design inspiration.
- Generated: No.
- Committed: Project-dependent.
- Notes: Do not import code directly from references without license and architecture review.

**`scripts/`:**
- Purpose: Release, smoke, compliance, proof, package, schema, and platform verification scripts.
- Generated: No.
- Committed: Yes.
- Notes: Add scripts here only when they are operator-facing or CI/release-relevant; keep ad hoc local experiments elsewhere.

**`dist/`:**
- Purpose: Release candidate binaries, manifests, compliance outputs, and proof artifacts.
- Generated: Yes.
- Committed: Usually no unless a release artifact policy says otherwise.
- Notes: Do not treat `dist/` contents as source truth for implementation.

**`target/`:**
- Purpose: Cargo build output.
- Generated: Yes.
- Committed: No.
- Notes: Ignore for code mapping and searches unless diagnosing build artifacts.

**`.claude/`:**
- Purpose: Local/project prompt or Claude-style support files.
- Generated: Mixed.
- Committed: Project-dependent.
- Notes: Kiana's skill loader checks project `.claude/skills` only when the project is trusted; this checkout has `.claude/ralph-loop.local.md` but no detected `.claude/skills` directory in the mapped scope.

**Project skill directories:**
- Purpose: Mapper agent looked for `.codex/skills/` and `.agents/skills/` per GSD mapper instructions.
- Generated: Not detected.
- Committed: Not applicable.
- Notes: No project `SKILL.md` files were detected in those two directories during this mapping run.

## Practical Placement Rules

- Put shared public data contracts in `kiana-types` before exposing them through CLI, SDK, MCP, TUI, remote, or bridge code.
- Put durable workflow logic in `kiana-tasks`; expose it through commands after the core runtime has tests.
- Put model-callable behavior in `kiana-tools`; expose it to MCP by default only if registry, schema, and permission behavior are correct.
- Put provider protocol changes in `kiana-services`; keep runner code focused on orchestration and event conversion.
- Put ordinary user commands in `kiana-commands`; keep `kiana-entrypoints/src/cli.rs` for top-level modes and argument routing.
- Put project-local extension discovery behind trust checks in `kiana-skills` and `kiana-types/src/trust.rs`.
- Put UI presentation in `kiana-screens` or `kiana-components`; call command/SDK/runtime APIs instead of duplicating domain logic.
- Put release/compliance proof automation in `scripts/` and keep source changes in crates, not generated `dist/` artifacts.

---

*Structure analysis: 2026-07-13*
