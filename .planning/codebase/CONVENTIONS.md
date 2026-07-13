# Coding Conventions

**Analysis Date:** 2026-07-13

## Naming Patterns

**Files:**
- Use Rust module filenames in `snake_case`, matching the domain noun or command name: `kiana-commands/src/workflow_transition_command.rs`, `kiana-tasks/src/project_board.rs`, `kiana-tools/src/bash_sandbox.rs`, `kiana-types/src/runtime.rs`.
- Keep one top-level command surface per file under `kiana-commands/src/`; add a matching integration test file under `kiana-commands/tests/` when the command has user-visible behavior.
- Name generated schema documents with explicit product prefixes and versions in `docs/schemas/`, for example `docs/schemas/kiana-eval-report.v1.schema.json`.

**Functions:**
- Use `snake_case` verbs and explicit domain nouns: `initialize_workflow_run`, `append_workflow_event`, `read_project_trust`, `validate_verification_packet_integrity`.
- Prefer small parsing, validation, read/write, and rendering helpers near the command that owns them: `parse_args`, `context_cwd`, `resolve_run`, and report constructors in `kiana-commands/src/evidence.rs`.
- For command entry points, implement `async fn execute(&self, context: CommandContext) -> Result<CommandResult>` through the shared `Command` trait from `kiana-commands/src/types.rs`.

**Variables:**
- Use `snake_case` and keep domain names visible: `workflow_id`, `run_id`, `artifact_dir`, `eventlog_path`, `project_root`.
- Use `args`, `rest`, `subcommand`, and `json_output` consistently in command parsers such as `kiana-commands/src/evidence.rs` and `kiana-commands/src/validate.rs`.
- Use path variables as `Path`/`PathBuf` and name them by role: `root`, `home`, `state_path`, `packet_path`, `plugins_dir`.

**Types:**
- Use `UpperCamelCase` for structs, enums, traits, and error types: `WorkflowRun`, `WorkflowError`, `ProjectTrustRecord`, `BashTool`, `EvidenceCommand`.
- Use `SCREAMING_SNAKE_CASE` for exported constants and schema IDs: `PROJECT_TRUST_SCHEMA`, `WORKFLOW_ARTIFACT_DESCRIPTOR_SCHEMA`, `ACCESS_ROOTS_ENV`.
- Name command structs as `{Domain}Command` and keep them stateless unless a command explicitly owns injected dependencies.

## Code Style

**Formatting:**
- Use workspace Rust formatting defaults; no `rustfmt.toml`, `.rustfmt.toml`, or `clippy.toml` is detected.
- Run `cargo fmt --all --check` before handing off changes. This is also the first release-smoke gate in `scripts/release-smoke.sh`.
- Keep `Cargo.toml` workspace settings centralized in `Cargo.toml`: resolver `2`, edition `2021`, workspace `rust-version = "1.96"`, and shared dependency versions under `[workspace.dependencies]`.

**Linting:**
- Use `deny.toml` as the dependency policy gate: wildcard dependencies are denied, unknown registries/git sources are denied, multiple versions warn, and licenses are allowlisted.
- Keep advisory ignores in `deny.toml` annotated with the owning dependency and revisit reason; do not add silent advisory ignores.
- Prefer `cargo test --workspace --locked --offline --no-fail-fast` for broad correctness because this workspace has no dedicated clippy config.

## Import Organization

**Order:**
1. Local crate imports, especially `crate::...` modules and nearby domain helpers.
2. Workspace crate imports such as `kiana_tasks`, `kiana_tools`, `kiana_types`, and external crates such as `anyhow`, `serde`, `serde_json`, `tokio`.
3. Standard library imports such as `std::collections`, `std::fs`, `std::path`, and platform-gated `std::os::*` imports.

**Path Aliases:**
- Use Rust-native `crate::`, `super::`, and workspace crate names; no custom path alias system is detected.
- Keep platform-specific imports beside their gated usage with `#[cfg(target_os = "linux")]`, `#[cfg(unix)]`, or `#[cfg(windows)]`, as in `kiana-commands/src/tasks.rs` and `kiana-types/src/trust.rs`.

## Error Handling

**Patterns:**
- Use `anyhow::Result` and `anyhow!` in command modules where user-facing error strings are part of CLI behavior, for example `kiana-commands/src/evidence.rs`.
- Use `thiserror::Error` for core/domain libraries where callers need typed errors, for example `WorkflowError` in `kiana-tasks/src/workflow.rs`, `EvidenceError` in `kiana-tasks/src/evidence.rs`, and `ToolError` in `kiana-tools/src/tool.rs`.
- Add path and operation context with `anyhow::Context` / `with_context` around filesystem and process operations, especially in security-sensitive flows such as `kiana-commands/src/tasks.rs`.
- Keep machine-readable error prefixes in command errors when tests assert them, for example `workflow_required`, `workflow_not_found`, and `workflow_inconsistent` in `kiana-commands/src/validate.rs`.
- Do not use `unwrap` or `expect` in production paths except for impossible invariants; tests and fixture builders commonly use `unwrap` for clarity.

## Logging

**Framework:** `tracing` is available in workspace dependencies and service crates, while many CLI paths return structured text/JSON through `CommandResult`.

**Patterns:**
- Prefer returning exact human text or pretty JSON from commands instead of printing directly; see `CommandResult::text` in `kiana-commands/src/evidence.rs` and `kiana-commands/src/license.rs`.
- Use shell-script stderr for operational failures and diagnostics, with line-aware traps in `scripts/release-smoke.sh`.
- In tests, reserve `eprintln!` for skip messages when optional local tools are unavailable, as in `kiana-tools/src/notebook_execute.rs` and `kiana-tools/src/file_read.rs`.

## Comments

**When to Comment:**
- Comment security, concurrency, and persistence invariants that are not obvious from the type signature; `kiana-types/src/trust.rs` documents pending-marker read safety.
- Comment dependency policy exceptions in `deny.toml` with the upstream package and revisit reason.
- Keep implementation comments focused on why a guard exists; avoid broad status or roadmap comments inside source files.

**Rustdoc:**
- Public API rustdoc is sparse in current code; when adding exported types in `kiana-tasks/src/lib.rs`, `kiana-types/src/lib.rs`, or `kiana-tools/src/lib.rs`, prefer concise comments only for non-obvious contracts.

## Function Design

**Size:** Keep domain operations factored into parse/validate/execute/render helpers. Large orchestration commands such as `kiana-commands/src/tasks.rs` exist, but new functionality should land as focused helper functions with tests rather than extending a single branch indefinitely.

**Parameters:** Pass `&Path`, `&CommandContext`, and borrowed strings where possible; return owned report structs or `serde_json::Value` only at serialization boundaries.

**Return Values:** Use `Result<T, DomainError>` in library crates and `anyhow::Result<CommandResult>` in command crates. For JSON CLI output, define serializable report structs with a `schema` field and use `serde_json::to_string_pretty`.

## Module Design

**Exports:** Each crate root exposes stable modules and reexports frequently used types through `pub use`, for example `kiana-tasks/src/lib.rs` and `kiana-types/src/lib.rs`.

**Barrel Files:** Use crate `lib.rs` as the only barrel-style export surface. Keep internal helpers private with `mod` or `pub(crate)`; examples include `mod local_state` and `mod eda_netlist` in `kiana-commands/src/lib.rs`.

**Command Modules:** Add commands by creating a module under `kiana-commands/src/`, implementing `Command`, and registering it through the command registry in `kiana-commands/src/registry.rs`.

**Serialization Contracts:** Use explicit `schema` fields, serde casing attributes, and `skip_serializing_if` for optional fields. Match casing to the existing contract: `snake_case` for workflow/task events in `kiana-tasks/src/workflow.rs`, `camelCase` for compatibility surfaces in `kiana-types/src/permissions.rs`, and `kebab-case` for plugin component names in `kiana-types/src/plugin.rs`.

**Filesystem Contracts:** Normalize and validate paths through helpers before reading or writing. Preserve no-follow, pending-marker, and lease patterns in trust/workflow code such as `kiana-types/src/trust.rs` and `kiana-commands/src/tasks.rs`.

---

*Convention analysis: 2026-07-13*
