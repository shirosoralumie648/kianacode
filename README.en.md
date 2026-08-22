# Kiana

Kiana is a local-first AI agent workspace. One Rust core serves Coding, Academic Research, and Daily Work through shared runtime, session, tool, policy, workflow, and event contracts, and exposes them through CLI, TUI, SDK/RPC/MCP, Desktop, and Web/App Server surfaces.

The repository is still in an architecture-migration period. The workspace builds and tests, but that is not a claim that Kiana already replaces Claude Code, Claude Desktop, or the `reference/` corpus. Completion is determined by the capability matrix, tests, runtime smoke, and release evidence.

**Current milestone is v0.2 Runnable Local Agent MVP:** land one working local coding-agent golden path (`kiana -p` / `kiana run` / `kiana tui` + one real provider + read/edit/shell) before expanding into a general-purpose assistant. The old 24-phase design corpus has been deleted. See [docs/planning-current.md](docs/planning-current.md) and [`.planning/ROADMAP.md`](.planning/ROADMAP.md).

[中文 README](README.md) · [Docs index](docs/README.md) · [Quick start](QUICKSTART.md)

## Architecture

Product surfaces must consume the same control plane:

```text
CLI / TUI / SDK / MCP / Remote
        -> kiana-entrypoints
        -> kiana-client / kiana-protocol
        -> kiana-daemon (composition root)
        -> kiana-core (control plane)
        -> kiana-capability-broker
        -> tools / query / services adapters
```

Live status:

```bash
./target/debug/kiana architecture status --json
```

At the time of writing this reports `control_plane=kiana-core`, `composition_root=kiana-daemon`, `runner=kiana-runner`, `harness=kiana-harness`, `capability_mode=brokered`, and `legacy_edges_remaining=9`.

See [docs/architecture.md](docs/architecture.md) for crate responsibilities and invariants.

## Quick start

Requires the workspace `rust-version` (currently 1.96):

```bash
cargo build -p kiana-entrypoints --bin kiana --locked
./target/debug/kiana --help
ANTHROPIC_API_KEY=<key> ./target/debug/kiana -p "summarize this repository"
./target/debug/kiana tui
```

Configuration: [CONFIG.md](CONFIG.md). User guide: [USAGE.md](USAGE.md).

## Verification

```bash
cargo fmt --all --check
cargo test --workspace --locked --offline --no-fail-fast
bash scripts/schema-contract-smoke.sh
bash scripts/release-smoke.sh
```

`kiana-computer-input::tests::test_init` may fail on Linux without a desktop input session. Treat that as an environment boundary, not workspace-wide green.

## License

MIT OR Apache-2.0. Projects under `reference/` keep their own licenses; source reuse requires an independent license audit.
