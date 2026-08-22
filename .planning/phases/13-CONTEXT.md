# v0.4.4 Context — Provider degrade on harness

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`
Requirement: CODE-04

## Classify

Phase, not spike. User-visible completion is: a trusted Builder run against a
provider profile that does **not** support tools fails closed with a
machine-readable `unsupported_tools` error. No file is written. The run is
not a fake success.

This is not a live Anthropic / OpenAI / Ollama matrix. Harness currently
sends `stream: Some(false)`, so `unsupported_streaming` is **not** product
green this slice. `kiana run --json` is the same run spine; this slice
proves it on in-process `DaemonHost`, not by compiling the CLI.

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 symposium/packet, v0.4 review, the
matrix draft, MCP stdio, skills/PreToolUse, TUI park, five departments,
RAG, or TeamCreate/SendMessage.

1. **Product proof is DaemonHost.** Add `DaemonHost::with_env_harness()`
   (`KianaHarness::new(model_client::from_env())` + existing `with_runner`
   / MemoryEventLog). Do not format `cli.rs`. Do not add a new `kiana run`
   flag.
2. **Preserve the capability code.** `ProviderModelClient` maps
   `ProviderError::UnsupportedCapability` to `error.code()`
   (`unsupported_tools`). Other errors may still use `to_string()`.
3. **Demo uses fake text-only.** `KIANA_PROVIDER=fake` +
   `KIANA_FAKE_MODEL=fake-text-only`. Clear `KIANA_HARNESS_SCRIPT` so the
   script path cannot mask the provider. Harness still advertises
   `tool_schemas()`, so the first `complete` fails before a tool call.
4. **Do not claim streaming or live providers.** `unsupported_streaming`
   stays unproven. Live keys are not completion.

Demo (same-host):

```text
KIANA_PROVIDER=fake
KIANA_FAKE_MODEL=fake-text-only
trusted + workspace-write
→ run Failed, error contains unsupported_tools
GOLDEN_PATH.txt does not appear
```

## Requirements this phase

CODE-04. PATH/TRUST/SESS/EVD/ROLE/ORCH/SYMP/WB/REV/CODE-01/CODE-02/CODE-03
still true. PATH-03 still `shell` + `apply_patch` + `mcp`.

## Frozen

- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- live Anthropic / OpenAI / Ollama matrix
- `unsupported_streaming` as product-green
- structured Read/Grep/Glob
- HTTP/SSE MCP
- exploding skills into model tools
- `kiana-tools` wiring
- migrating TUI
- formatting `kiana-entrypoints/src/cli.rs`
