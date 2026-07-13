# External Integrations

**Analysis Date:** 2026-07-13

## AI Providers

**Anthropic:**
- Default provider path uses Anthropic-style API configuration from `CONFIG.md`, `.kiana-example.toml`, and provider code in `kiana-services/` / `kiana-entrypoints/`.
- Configuration keys include `api_key`, `base_url`, and `model` in `~/.kiana/config.toml`, plus `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`, and `ANTHROPIC_MODEL`.
- Use `kiana config resolved --json`, `kiana auth status --json`, `kiana model list --json`, `kiana model catalog --json`, and `kiana model smoke --json` to inspect effective provider state.

**OpenAI-compatible:**
- OpenAI-compatible text provider is selected through `KIANA_PROVIDER="openai-compatible"`.
- Provider-specific variables are `KIANA_OPENAI_API_KEY`, `KIANA_OPENAI_BASE_URL`, and `KIANA_OPENAI_MODEL`.
- The provider supports Chat Completions text and function-style tools according to `CONFIG.md`; use explicit `--tools ""` for text-only paths.

**Ollama:**
- Local Ollama provider is selected through `KIANA_PROVIDER="ollama"`.
- Variables are `KIANA_OLLAMA_BASE_URL` and `KIANA_OLLAMA_MODEL`.
- Model catalog live lookup uses `<base_url>/api/tags`; text/tool loop uses Ollama chat semantics.

## Live Network Surfaces

**HTTP client stack:**
- `reqwest` is used by `kiana-services/`, `kiana-tools/`, `kiana-remote/`, `kiana-bridge/`, and `kiana-commands/` for provider, marketplace, remote, and service calls.
- Network-facing code should route through shared public-target validation and SSRF/private-address protections before any user-controlled request.

**Web tools:**
- WebFetch and WebSearch behavior is implemented in `kiana-tools/` and `kiana-services/`.
- Search/catalog smoke paths are intentionally gated: `KIANA_MODEL_CATALOG_LIVE=1`, `kiana model catalog --live --json`, `KIANA_PROVIDER_SMOKE_LIVE=1`, and `kiana model smoke --live --json` opt into live network checks.

## MCP And Tooling Integrations

**Local MCP / tool protocol:**
- `kiana-entrypoints/src/mcp.rs` exposes local MCP-style server behavior from the main binary.
- `kiana-tools/src/mcp_tool.rs` implements MCP tool invocation surfaces.
- `kiana-computer-mcp/src/lib.rs` exposes a JSON-RPC/MCP server for computer-use tools such as status, screenshot, input, and clipboard behavior.
- `kiana-chrome-mcp/` handles Chrome/native-host integration, browser automation, and native messaging host manifests.

**Computer use:**
- `kiana-computer-input/`, `kiana-computer-mcp/`, and `kiana-screen-capture/` integrate with desktop input/screenshot/clipboard packages.
- Native computer-use is optional and must be enabled through the `native-computer-use` feature in `kiana-entrypoints/Cargo.toml`.

**Chrome native host:**
- `kiana-chrome-mcp/src/native_install.rs` writes platform-specific native host wrapper/manifest plans.
- Chrome host manifest locations cover Linux, macOS, and Windows paths; Windows uses registry/native host conventions.
- Treat generated host manifests and wrapper scripts as platform-sensitive artifacts requiring dedicated tests.

## Remote And Bridge Integrations

**Remote sessions:**
- `kiana-remote/` contains remote session/client behavior and uses `reqwest`, `tokio-tungstenite`, streaming, and typed runtime events.
- `kiana-bridge/` adapts runtime events and bridge transport through HTTP/WebSocket dependencies.

**App server:**
- `kiana-entrypoints/` uses `axum` with WebSocket support for local app-server style surfaces.
- App-server JSON outputs are schema-backed; keep `docs/schemas/` synchronized with Rust emitters.

## Plugin, Skill, Agent, And Marketplace Integrations

**Plugins:**
- `kiana-commands/src/plugin.rs` handles plugin marketplace and install commands.
- `kiana-types/src/plugin.rs` holds shared plugin contract utilities such as installed plugin roots.
- Marketplace sources include local files, HTTP URLs, Git/GitHub sources, and repository subdirectories; unsupported sources must remain explicit in command output and docs.

**Skills:**
- `kiana-skills/` loads skills from user, project, and plugin roots with trust-aware filtering.
- `kiana-tools/src/discover_skills.rs` and related tool code expose skills to agents/tool execution.
- Project-local skill visibility must stay gated by effective project trust.

**Agents/hooks:**
- Project and user agent surfaces exist under directories such as `.kiana/agents/` and `.agents/`.
- Hook loading touches `kiana-query/src/stop_hooks.rs` and tool/agent lifecycle paths; project hooks must stay trust-gated.

## Trust, Policy, And Managed Configuration

**Trust root:**
- `kiana-types/src/trust.rs` implements project trust semantics and should remain the central source for effective trust decisions.
- Project-local `.kiana/` files must not authorize their own trust-sensitive behavior.

**Managed policy:**
- Managed configuration is loaded from `KIANA_MANAGED_SETTINGS_FILE` and `KIANA_MANAGED_POLICY_FILE`.
- Managed plugin installation policy can use `KIANA_MANAGED_PLUGIN_POLICY_FILE`.
- `CONFIG.md` describes managed overlays and plugin policy behavior; tests should verify deny/allow precedence when policy changes.

**Bash sandbox / permissions:**
- Bash tool execution and permission profiles are implemented in `kiana-tools/src/bash_tool.rs`, `kiana-tools/src/bash_sandbox.rs`, `kiana-tools/src/permissions.rs`, and `kiana-tools/src/tool_execution.rs`.
- Linux sandbox settings are documented in `.kiana-example.toml`; production configurations should fail closed when sandboxing is required but unavailable.

## Release, Distribution, And Compliance Integrations

**Packaging:**
- `scripts/package-release.sh`, `scripts/package-lifecycle-smoke.sh`, `scripts/install-release-binary.sh`, and `dist/` define local release artifact packaging and install validation.

**Commercial readiness:**
- `scripts/commercial-release-blockers-report.sh` classifies `external_blocking` and `local_blocking`.
- `scripts/commercial-release-handoff-smoke.sh`, `scripts/release-preflight.sh`, `scripts/release-smoke.sh`, and `scripts/product-shell-smoke.sh` are release gates.
- Proof/report integrations include `scripts/entitlement-proof-report.sh`, `scripts/release-ops-report.sh`, `scripts/source-control-proof-report.sh`, `scripts/platform-security-proof-report.sh`, `scripts/distribution-review-report.sh`, `scripts/product-acceptance-report.sh`, and `scripts/stage-commercial-release-proofs.sh`.

**Schemas:**
- JSON contracts live in `docs/schemas/` and include workflow, release, trust, memory, eval, EDA, and swarm contracts.
- `scripts/schema-contract-smoke.sh` and `scripts/validate-json-schema.py` are the integration points for schema validation.

## Data Stores And Persistence

**Local filesystem:**
- User config persists under `~/.kiana/config.toml`.
- Project/task/workflow state uses repository-local and Kiana-home files through `kiana-tasks/`, `kiana-types/`, and `kiana-commands/`.
- Release artifacts and proof outputs live under `dist/`.

**Event and proof files:**
- Workflow EventLog, evidence ledger, verification packets, and recovery journals are file-backed through `kiana-tasks/src/`.
- Session/runtime event trees are handled by `kiana-entrypoints/`, `kiana-commands/`, `kiana-remote/`, and `kiana-bridge/`.

**Database:**
- No relational database dependency is declared in the workspace manifests inspected. If persistence grows beyond files, add an explicit crate dependency and document migration/backup behavior.

## Integration Guidance

- Never read or print secrets from `.env` or config files; use masked previews from `kiana config resolved --json`.
- Keep live provider/network checks opt-in unless the command explicitly requests live smoke behavior.
- When adding a new external service, define its environment/config keys in `CONFIG.md`, add schema-backed JSON status where useful, and include skip reasons for offline/default execution.
- When adding MCP tools, update `kiana-tools/`, relevant server surfaces in `kiana-entrypoints/` or MCP crates, and tests that assert JSON-RPC/MCP schema shape.
- When adding release/commercial proof, update both `scripts/commercial-release-blockers-report.sh` and the matching `docs/schemas/` contract.

---

*Integration analysis: 2026-07-13*
