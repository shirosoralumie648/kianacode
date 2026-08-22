# v0.4.4 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass
Requirement: CODE-04
PATH-03: unchanged (`shell` + `apply_patch` + `mcp`)

## Demo contract

```text
KIANA_PROVIDER=fake
KIANA_FAKE_MODEL=fake-text-only
trusted + workspace-write
→ run Failed, error contains unsupported_tools
GOLDEN_PATH.txt does not appear
```

Same-host proof is in-process DaemonHost via `DaemonHost::with_env_harness()`.
A provider profile that does not support tools fails before a tool call.
`kiana run --json` is the same run spine; this slice did not compile the CLI
crate. Harness still sends `stream: Some(false)`, so `unsupported_streaming`
is not product-green. Live Anthropic / OpenAI / Ollama is not completion.

## Evidence

| Criterion | Result |
|---|---|
| Capability code is preserved | `ProviderModelClient` maps `UnsupportedCapability` to `error.code()` (`unsupported_tools`) |
| Unit mapping | `text_only_provider_maps_unsupported_tools_code` |
| Product fail-closed | `fake_text_only_provider_fails_closed_with_unsupported_tools`: `ExecutionStatus::Failed`; error contains `unsupported_tools` |
| No fake success / no write | `GOLDEN_PATH.txt` does not appear |
| Env path is the owned harness | `KIANA_PROVIDER=fake` + `KIANA_FAKE_MODEL=fake-text-only`; empty `KIANA_HARNESS_SCRIPT`; `with_env_harness()` |
| Not live / not streaming | Did not mark `unsupported_streaming` or live providers green |
| No new CLI flag | Did not format `cli.rs` |

## Commands run

```
cargo fmt -p kiana-daemon
cargo test -p kiana-daemon --locked --lib -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked -- --test-threads=1
```

All listed targets passed (25 / 30). Did not `cargo fmt --all`, did not format
`kiana-entrypoints/src/cli.rs`, did not compile the CLI crate, did not run
`scripts/release-smoke.sh` or live provider.

## Not claimed

- live Anthropic / OpenAI-compatible / Ollama matrix
- `unsupported_streaming` as a product error (harness uses `stream: Some(false)`)
- compiling `kiana run --json` this slice (same spine, not a CLI binary proof)
- structured Read/Grep/Glob
- HTTP / SSE / WS MCP
- model-visible `skill` tool / full hook suite
- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- migrating TUI
- physical readiness / v1.0 REL-03 (P1-READ still optional; HTTP MCP still open)
