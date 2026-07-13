# Technology Stack

**Analysis Date:** 2026-07-13

## Languages

**Primary:**
- Rust 2021 edition - all runtime, CLI, command, task, tool, service, MCP, and package code under workspace crates such as `kiana-entrypoints/`, `kiana-commands/`, `kiana-tasks/`, `kiana-tools/`, and `kiana-types/`.

**Secondary:**
- Bash - release, packaging, commercial-readiness, smoke, and proof scripts under `scripts/`.
- Python - JSON Schema validation helper at `scripts/validate-json-schema.py`.
- Markdown / JSON Schema / TOML - product documentation, design plans, API contracts, Cargo manifests, and runtime configuration under `docs/`, `docs/schemas/`, `.kiana-example.toml`, and crate `Cargo.toml` files.

## Runtime

**Environment:**
- Rust workspace using resolver 2 in `Cargo.toml`.
- Minimum Rust version is declared as `rust-version = "1.96"` in the workspace package metadata.
- Primary binary is `kiana` from `kiana-entrypoints/src/bin/kiana.rs`.

**Package Manager:**
- Cargo with workspace lockfile `Cargo.lock`.
- Shell scripts are plain Bash scripts under `scripts/`; do not introduce another package manager without documenting it in `CONFIG.md` and release scripts.

## Workspace Layout

**Core crates:**
- `kiana-entrypoints/` - CLI, TUI, app-server, runner, MCP entry points, SDK-facing surfaces, and binary wiring.
- `kiana-commands/` - command implementations for config, auth, model, tasks, trust, plugin, release, eval, evidence, project, audit, report, EDA, and validation flows.
- `kiana-tasks/` - task board, workflow DAG, integrity, evidence, bounded-swarm, and orchestration domain logic.
- `kiana-tools/` - agent/tool execution, Bash sandboxing, MCP tools, LSP/REPL/task/team creation, permissions, remote trigger, discovery, and policy enforcement.
- `kiana-types/` - shared contracts for trust, plugins, events, schemas, runtime, and cross-crate types.
- `kiana-services/` - provider, HTTP, web search/fetch, model/service integrations, and app-service helpers.

**Interface/support crates:**
- `kiana-skills/` - skill loading, trust-aware skill visibility, plugin skill cache behavior.
- `kiana-query/` - query runtime contracts and stop-hook handling.
- `kiana-remote/` and `kiana-bridge/` - remote sessions, WebSocket/HTTP bridge, runtime event adapters.
- `kiana-chrome-mcp/`, `kiana-computer-mcp/`, `kiana-computer-input/`, and `kiana-screen-capture/` - Chrome/native host, MCP, computer-use, input, and capture integrations.
- `kiana-screens/`, `kiana-components/`, `kiana-ink/`, `kiana-color-diff/`, and `kiana-modifiers/` - TUI, UI primitives, terminal rendering, diffing, and platform input modifiers.
- `kiana-url-handler/` - desktop URL handler binary and platform integrations.

## Frameworks

**Core:**
- `tokio` - async runtime across CLI, services, remote, MCP, and task/tool execution.
- `clap` - CLI argument parsing for command surfaces in `kiana-entrypoints/` and `kiana-commands/`.
- `serde` / `serde_json` / `serde_yaml` / `toml` - configuration, JSON output contracts, schema-backed reports, and serialized state.
- `anyhow` / `thiserror` - error handling conventions across crates.
- `async-trait`, `futures`, `futures-util`, `tokio-stream` - async traits and streaming.

**HTTP / Server / Remote:**
- `reqwest` with `rustls-tls` - outbound HTTP for providers, web surfaces, marketplace/GitHub materialization, and remote/service integrations.
- `axum` with WebSocket support - app-server and local server surfaces from `kiana-entrypoints/`.
- `tokio-tungstenite` - bridge/remote WebSocket transport.
- `url` and `urlencoding` - URL validation and encoding in network/tool surfaces.

**Terminal / UI / Input:**
- `ratatui`, `crossterm`, `rustyline`, and `colored` - CLI/TUI rendering, input, REPL, and terminal UX.
- `syntect` and `similar` - colorized diffs and syntax-aware rendering.
- `enigo`, `rdev`, `xcap`, `arboard`, and `image` - optional/native computer-use, input, screenshot, clipboard, and image surfaces.

**Testing / Validation:**
- Rust unit and integration tests are run through Cargo.
- JSON contracts are validated by `scripts/schema-contract-smoke.sh` and `scripts/validate-json-schema.py`.
- Release and packaging are validated through `scripts/release-smoke.sh`, `scripts/package-release.sh`, and `scripts/package-lifecycle-smoke.sh`.

## Key Dependencies

**Critical:**
- `tokio` - required for nearly all async execution and test paths.
- `serde_json` - required for CLI `--json`, stream-json, app-server, schemas, MCP, workflow logs, and proof artifacts.
- `reqwest` - required for provider/network/marketplace surfaces; keep `rustls-tls` and shared network policy behavior aligned.
- `sha2` and `ring` - required for trust, evidence, integrity, workflow, HMAC/hash, and release-proof surfaces.
- `uuid` and `chrono` - required for IDs, timestamps, session/workflow events, and proof metadata.

**Infrastructure:**
- `walkdir`, `glob`, `ignore`, and `regex` - repo/file discovery, rules, matching, and skill/tool scanning.
- `dirs` - user/home config paths such as `~/.kiana/config.toml`.
- `windows`, `windows-sys`, `libc`, `core-foundation`, `core-graphics`, `x11-dl`, `zbus`, `cocoa`, and `objc` - platform-specific trust, native host, URL handler, input, and UI integrations.

## Configuration

**Environment:**
- User config lives at `~/.kiana/config.toml`; example configuration is `.kiana-example.toml`.
- Anthropic provider variables include `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`, and `ANTHROPIC_MODEL`.
- OpenAI-compatible provider variables include `KIANA_PROVIDER=openai-compatible`, `KIANA_OPENAI_API_KEY`, `KIANA_OPENAI_BASE_URL`, and `KIANA_OPENAI_MODEL`.
- Ollama provider variables include `KIANA_PROVIDER=ollama`, `KIANA_OLLAMA_BASE_URL`, and `KIANA_OLLAMA_MODEL`.
- Managed policy variables include `KIANA_MANAGED_SETTINGS_FILE`, `KIANA_MANAGED_POLICY_FILE`, and `KIANA_MANAGED_PLUGIN_POLICY_FILE`.

**Build:**
- Workspace build configuration is in root `Cargo.toml`.
- Release profile uses optimized, thin-LTO, stripped binaries in root `Cargo.toml`.
- Native computer-use is gated by the `native-computer-use` feature in `kiana-entrypoints/Cargo.toml`, which enables `kiana-computer-mcp/native`.
- Bash sandbox settings are documented in `.kiana-example.toml` and `CONFIG.md`; Linux controlled environments should prefer `enabled = true`, `failIfUnavailable = true`, and `allowUnsandboxedCommands = false`.

## Platform Requirements

**Development:**
- Rust toolchain compatible with workspace `rust-version = "1.96"`.
- Cargo workspace commands should be run from repository root.
- For sandboxed Bash execution on Linux, install and configure `bubblewrap` / `bwrap` when sandbox is enabled.
- Optional native computer-use builds require platform desktop/input/screenshot dependencies for `kiana-computer-mcp/native`.

**Production / Distribution:**
- Release artifacts are staged under `dist/`.
- Packaging and smoke scripts live under `scripts/`.
- Commercial proof should use `scripts/commercial-release-blockers-report.sh`, `scripts/release-preflight.sh`, `scripts/release-smoke.sh`, `scripts/package-lifecycle-smoke.sh`, and related proof/report scripts.

## Prescriptive Stack Guidance

- Add shared domain contracts to `kiana-types/` before duplicating JSON shapes across crates.
- Add command-level behavior to `kiana-commands/`, then wire entrypoint dispatch in `kiana-entrypoints/`.
- Keep network access behind `kiana-services/` or policy-aware tool surfaces; do not create ad hoc direct HTTP clients in command code.
- Update `docs/schemas/` and schema smoke scripts whenever a `--json`, stream, app-server, MCP, or release-proof contract changes.
- Keep optional native/platform functionality behind crate features or target-specific dependency sections.

---

*Stack analysis: 2026-07-13*
