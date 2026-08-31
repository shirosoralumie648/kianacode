# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository snapshot

Kiana is a local-first Rust workspace for a brokered coding-agent runtime. The workspace contains the executable entrypoint plus domain contracts, control-plane policy, an in-process harness, capability adapters, context search, and optional SDK/desktop/MCP surfaces. The workspace uses the stable Rust channel declared in `rust-toolchain.toml` (with `rustfmt`, `clippy`, and `rust-src`). `Cargo.lock` is checked in.

The current product proof ceiling is `local_behavior`: trusted local repositories plus cassette/fake-script execution. Do not describe this checkout as a live provider integration, token-streaming UI, production `~/.local/bin` installer, signed package, enterprise product, or dsh Web clone. `reference/` material is audit-only and is not a workspace member.

## Commands

Run commands from the repository root.

### Rust build, format, lint, and tests

```bash
# Fast executable build and workspace checks
cargo build --bin kiana
cargo check --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets

# Full workspace tests
cargo test --workspace
make test

# Deterministic release gate used by the repository
bash scripts/release-smoke.sh

# Release executable; the package-qualified form is canonical
cargo build --release -p kiana-entrypoints --bin kiana
make release
```

For a focused test, use the package and an optional test-name filter:

```bash
cargo test -p kiana-entrypoints --test cli_web
cargo test -p kiana-entrypoints --test cli_web web_help_does_not_need_a_model
cargo test -p kiana-query context_index
cargo test -p <crate> <test-name-fragment>
```

The release gate additionally runs `cargo fmt --all --check` and
`cargo test --workspace --locked --offline --no-fail-fast`, then builds and exercises
release/install paths. Prefer `--locked --offline` when reproducing that gate or
when network access is not intended. `npm run build` and `npm test` are not
workspace commands: the root `package.json` has dependencies but no build/test
scripts.

### Product and regression smoke gates

```bash
bash scripts/harness-golden-smoke.sh
bash scripts/v03-workbench-smoke.sh
bash scripts/v10-personal-lifecycle-smoke.sh
bash scripts/v10-p0-closeout-smoke.sh
bash scripts/v10-workbench-smoke.sh
cargo test -p kiana-entrypoints --test cli_web
```

For the optional Electron shell, install Node dependencies first, then run its
JavaScript tests:

```bash
npm install
node --test contrib/desktop/tests/*.js
```

Useful Make targets include `make build`, `make dev`, `make release`, `make test`,
`make release-preflight-local`, `make release-smoke`, `make package`,
`make native-computer-mcp`, and `make test-native-computer-mcp`. `make install`
writes to `~/.local/bin` by default; use the temporary `INSTALL_DIR` examples in
`USER.md` or `scripts/package-lifecycle-smoke.sh` for lifecycle testing.

### Running the current product surfaces

```bash
# Trust the repository before local writes
kiana trust .

# Folder workbench / one-shot workbench
kiana
kiana --workdir /path/to/project
kiana run --sandbox workspace-write -- "create GOLDEN_PATH.txt containing hello"

# Planning symposium, then an independent Builder packet run
kiana run --symposium --anti-meeting --sandbox workspace-write --json -- \
  "create GOLDEN_PATH.txt containing hello"
kiana run --packet packet/TASK.json --sandbox workspace-write --json
kiana run --review <author_session_id>

# Loopback web workbench; it uses the same DaemonHost as `kiana run`
kiana web --no-open --bind 127.0.0.1:3080
```

`kiana run` defaults to `read-only`; `workspace-write` requires a trusted
project and an allowed non-safe permission profile. The folder workbench defaults
to `workspace-write`, but trust and control-plane checks still apply. Cassette
runs use `KIANA_HARNESS_SCRIPT=/path/to/script.json`; the integration tests show
the supported scripted model shape.

## Architecture

The main execution path is:

```text
kiana-entrypoints
  -> kiana-client / kiana-protocol envelopes
  -> kiana-daemon::DaemonHost (composition root)
  -> kiana-core::ControlPlane
  -> policy + gates + approvals
  -> capability broker or kiana-runner::KianaHarness
  -> brokered capability result and event receipt
```

- **Contracts:** `kiana-domain` owns stable IDs, request context, roles/departments, permission profiles, work packets, symposium/review records, capability requests/results, execution statuses, and path/memory invariants. `kiana-protocol` is the versioned wire envelope (`kiana.protocol.v1`) used by clients and the daemon. `kiana-ports` defines the interfaces between the core and adapters.
- **Control plane:** `kiana-core` is the authorization and lifecycle authority. It validates request identity and trust, evaluates `kiana-policy` and `kiana-gates`, stages approvals, dispatches capabilities through the broker, coordinates run/continue/cancel, records ordered events, writes symposium/review artifacts, and enforces Builder path locks. Do not bypass it to execute model-visible tools.
- **Composition and runtime:** `kiana-daemon` constructs the local graph: `ControlPlane`, policy/gate engines, event store, capability broker, and the `KianaHarness` runner. `kiana-runner` owns the in-process model loop, inbox/continue semantics, compaction, and model-visible tool schemas; tool calls become capability requests and are not executed directly by the harness.
- **Adapters and services:** `kiana-services`, `kiana-tools`, `kiana-capability-broker`, and the specialized MCP/input/screen crates implement capabilities behind the broker. `kiana-query` provides context indexing, repo maps, search/vector search, artifact stores, and context packs. `kiana-eventlog` persists runtime events; `kiana-client` transports protocol requests in-process for the local CLI.
- **Entrypoints:** `kiana-entrypoints/src/bin/kiana.rs` initializes process state and cleanup, then `cli.rs` routes `run`, `web`, workbench, architecture, MCP, SDK, and other commands. `workbench.rs` and `web.rs` are current user-facing surfaces over `DaemonHost`; `kiana tui` remains parked on the legacy SDK stream and is not the product path. `kiana-bridge` and related SDK/remote crates are separate compatibility/integration surfaces, not a replacement for the local control-plane path.
- **Role boundaries:** planning PM/Architect, executing Builder, monitoring Reviewer, initiating Sponsor, and closing Closer have distinct tools, sandbox modes, path allows, and memory grants defined in `kiana-domain::RoleSpec` and `DepartmentSpec`. Planning symposiums emit `plan/DECISION.json` and `packet/TASK.json`; Builder packet execution is a fresh session; monitoring writes `gate/REVIEW.json`.

When changing a wire field, status, artifact schema, role, capability operation,
or path rule, trace both the domain/protocol type and its control-plane/entrypoint
callers. Preserve the fail-closed trust, sandbox, role, path, approval, and event
receipt behavior.

## Repository-specific boundaries

- Keep the product spine on `DaemonHost`; do not introduce a second execution loop in an entrypoint or adapter.
- Keep model/tool execution brokered. A model-visible tool name is not permission to run it directly.
- Web is loopback-only and intentionally does not claim token streaming. HTTP MCP is unsupported for the current product path; stdio MCP is the supported route.
- Concurrent Builder packet runs share one `ControlPlane` and use path locks; overlapping or unauthorized paths must fail closed.
- Do not add Builder participation to planning/monitoring symposiums or turn them into a joint symposium; those attendee boundaries are frozen.
- For multi-agent changes, follow `AGENTS.md`: never put two writers in one worktree, and let the integration owner reconcile shared manifests and lockfiles. Do not auto-commit, push, merge, release, or delete worktrees without explicit authorization.

## Source-of-truth documents

- `README.md`: current product claims and quick-start examples.
- `USER.md`: runnable local, workbench, web, desktop, install, and cassette workflows plus honest limitations.
- `DESIGN.md`, `PROCESS.md`, `PHASES.md`, and `COMPANY.md`: version boundaries, evidence/release process, phase-to-crate mapping, and department/role design.
- `Makefile` and `scripts/`: executable build, audit, smoke, packaging, and release gates; prefer these over stale examples in `run.sh`.
