# Bridge Module Conversion Summary

## Overview
Converted TypeScript bridge module (33 files, ~12,600 lines) to Rust with minimal core implementation.

## Files Created

### Core Module Files
1. **Cargo.toml** - Package manifest with rustls-tls dependencies
2. **src/lib.rs** - Module exports
3. **src/types.rs** - Protocol types (SDKMessage, WorkSecret, BridgeConfig, etc.)
4. **src/api.rs** - HTTP client for environment/work APIs
5. **src/transport.rs** - WebSocket transport layer
6. **src/session.rs** - Session state management
7. **src/work.rs** - Work poll loop coordinator

### Documentation
- **README.md** - Architecture overview and usage guide
- **examples/simple.rs** - Minimal usage example

## Key Design Decisions

### 1. Minimal Core Implementation
- Focused on WebSocket/RPC integration layer only
- Deferred UI rendering (ratatui will be added separately)
- Work polling dispatches session tasks up to `BridgeConfig.max_sessions`.
  `SpawnMode::Worktree` selects an isolated per-session directory via
  `git worktree` or snapshot copy fallback, then removes the managed
  worktree/snapshot directory when the session ends.
- Active work items send `/heartbeat` requests while their WebSocket session is
  alive. The interval is configurable and `0` disables heartbeat.
- Completed WebSocket sessions call `stopWork(false)` and archive the session.
  Transport failures, failed transport setup, heartbeat lease loss, and fatal
  heartbeat auth/status failures call `reconnectSession` so the backend can
  re-dispatch the work instead of leaving it stranded locally.
- `stopWork` uses retry/backoff for transient failures before archiving or
  shutdown cleanup continues. Fatal auth/session errors are not retried.
- Sessions have a watchdog timeout matching the reference `sessionTimeoutMs`
  behavior. The default is 24 hours and `0` disables the watchdog.
- Command-backed session execution now supervises child processes explicitly:
  stdout/stderr are drained concurrently, the session timeout is inherited by
  default, `KIANA_BRIDGE_RUNNER_TIMEOUT_MS` can override it, and hung children
  are killed as a process group on Unix or a process tree on Windows.
- Optional `KIANA_BRIDGE_RUNNER_MODE=stream-json` keeps one child process per
  bridge session, writes user messages as NDJSON to stdin, reads SDK messages
  back from stdout, forwards server `control_response` messages back to the
  child, forwards refreshed `CLAUDE_CODE_SESSION_ACCESS_TOKEN` values via
  `update_environment_variables`, supports `can_use_tool` request payloads, and
  tears the child down during session cleanup.
- In stream-json mode, the default child arguments now match the reference
  session-runner shape: `--print --sdk-url <url> --session-id <id>
  --input-format stream-json --output-format stream-json
  --replay-user-messages`. When `--debug-file` or
  `KIANA_BRIDGE_DEBUG_FILE` is configured, each session gets a suffixed debug
  log path and a sibling `bridge-transcript-<session>.jsonl` file containing
  raw child stdout NDJSON. The local CLI accepts `--sdk-url`/`--debug-file`
  and uses `--sdk-url` to enter a bridge-oriented interactive stdin/stdout
  loop instead of waiting for EOF before processing stream-json input.
- The SDK URL CLI bridge loop now has an injectable IO smoke covering
  `can_use_tool` control-request round-trip approval. After each prompt it
  emits a stream-json `result` event and falls back to structured output when
  assistant text/status are absent, avoiding empty assistant messages after
  tool-only turns.
- The bridge work loop now has a higher-level smoke with a mock HTTP bridge API,
  mock session-ingress WebSocket, and real stream-json child process. It proves
  the executing `WorkPollLoop::new` path can poll work, ack it, forward a
  WebSocket user message into the child, round-trip a child `can_use_tool`
  request through the WebSocket server, return the child assistant message, then
  finish with `stopWork(false)` and `archiveSession`. This is still a mock
  bridge-service proof; true hosted bridge/session-ingress validation remains a
  separate parity gate.
- The Session Ingress transport now mirrors the reference ingress router's UUID
  replay shields: UUIDs sent locally are remembered so echoed user/assistant
  messages are dropped, and inbound user UUIDs are also remembered so server
  history replay cannot execute the same user message twice.
- Runner responses now update bridge activity history with reference-style
  activity summaries for assistant text, assistant `tool_use` blocks, and
  `can_use_tool` permission requests. This gives status consumers meaningful
  work-in-progress signals instead of only recording inbound user messages.
- Stream-json runner output now tolerates common SDK lines beyond
  request/response pairs: `system`, `stream_event`, `tool_progress`, and
  `result`. Non-response output is consumed without crashing the session, and
  result/error plus partial text progress are recorded as session activities.
- Server/client control routing now responds promptly to reference
  `bridgeMessaging.ts` control requests for `initialize`, `interrupt`,
  `set_model`, `set_max_thinking_tokens`, `set_permission_mode`,
  `can_use_tool`, and unknown future subtypes. Unsupported mutable controls
  return explicit error `control_response` messages instead of hanging the
  remote session.
- One-shot command runners now persist per-session `set_model`,
  `set_permission_mode`, and `set_max_thinking_tokens` control overrides and
  apply them to subsequent child executions through `ANTHROPIC_MODEL`,
  `KIANA_PERMISSION_MODE`, and `KIANA_MAX_THINKING_TOKENS`, so remote control
  changes affect the next prompt even without the reusable stream-json child
  mode.
- `bridge start` tracks active work items. On Ctrl+C shutdown it removes local
  session state, cleans managed worktree/snapshot directories, calls
  `stopWork(force=true)`, archives active sessions, then deregisters the bridge
  environment. The CLI start path now uses the same command-backed runner as
  `WorkPollLoop::new`, so stream-json child reuse, debug/transcript logging,
  timeout, and cleanup behavior are exercised by the real bridge command.
- Work ingress now follows the reference recovery shape more closely: poisoned
  work secrets are stopped with OAuth-backed `stopWork`, non-session work items
  are acknowledged instead of being redelivered forever, and repeated work for
  an already-running session refreshes the shared work lease plus child access
  token instead of spawning a duplicate local session.
- The poll loop no longer suppresses polling just because the bridge is at
  session capacity. It still defers new sessions at capacity, but synchronously
  handles no-permit work such as existing-session token refreshes and
  healthcheck/poison-work cleanup before the next poll.

### 2. Dependencies
- **tokio-tungstenite** - WebSocket with rustls-tls (avoid OpenSSL)
- **reqwest** - HTTP client with rustls backend
- **serde/serde_json** - Protocol serialization
- **futures-util** - Stream utilities

### 3. Simplified from TypeScript
| TypeScript Feature | Rust Status |
|-------------------|-------------|
| Environment registration | ✅ Implemented |
| Work polling | ✅ Implemented |
| WebSocket transport | ✅ Implemented |
| Session management | ✅ Basic implementation |
| Multi-session spawn modes | ✅ Concurrent dispatch up to `max_sessions`; per-session directories wired |
| TUI status display | ❌ Deferred (bridgeUI.ts) |
| Git worktree isolation | ✅ Implemented with snapshot fallback |
| Managed worktree cleanup | ✅ Implemented for session end and transport-connect failure |
| Token refresh on 401 | ✅ Implemented for OAuth-scoped bridge API calls via injectable auth provider |
| Work lease heartbeat | ✅ Implemented with configurable interval |
| Session archive/reconnect | ✅ Implemented for normal completion and re-dispatch paths |
| Session timeout watchdog | ✅ Implemented with CLI/env overrides |
| Active work shutdown cleanup | ✅ Force stop/archive/cleanup before environment deregistration |
| Command runner argument parsing | ✅ Shell-like quoted args with invalid-quote errors |
| Command runner child supervision | ✅ Timeout, stderr capture, process-group/tree kill |
| Stream-json child reuse | ✅ Optional per-session child process mode with reference-shaped default args, control_response forwarding, token update forwarding, debug logs, transcript files, SDK URL CLI loop smoke for permission/result/assistant output, and WorkPollLoop/API/WS/child closed-loop smoke |
| Runner activity extraction | ✅ Text, tool_use, can_use_tool, result/error, system init, stream text delta, and tool_progress summaries recorded in session activity history |
| Server control_request routing | ⚠️ Prompt success/error responses for initialize, interrupt, set_model, set_max_thinking_tokens, set_permission_mode, can_use_tool, and unknown subtypes; one-shot set_model/set_permission_mode/set_max_thinking_tokens overrides are applied to subsequent child runs; richer local callbacks still pending |
| Work ingress recovery | ✅ Bad work secrets call stopWork, non-session work is acked, repeated session work refreshes the existing session lease/token, and the poll loop still processes those refreshes while at capacity |
| stopWork retry/backoff | ✅ Retries transient failures; skips fatal auth/session statuses |
| CCR v2 protocol | ❌ Deferred (replBridgeTransport.ts) |

## Architecture

```
┌─────────────────────────────────────────────┐
│             BridgeApiClient                 │
│  - register_environment()                   │
│  - poll_for_work()                          │
│  - acknowledge_work()                       │
│  - stop_work()                              │
│  - heartbeat_work()                         │
│  - archive_session()                        │
│  - reconnect_session()                      │
└─────────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────┐
│             WorkPollLoop                    │
│  - Long-poll for session work items        │
│  - Decode work secrets                      │
│  - Spawn WebSocket transports               │
└─────────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────┐
│             Transport                       │
│  - WebSocket connection per session         │
│  - send(SDKMessage)                         │
│  - recv() -> SDKMessage                     │
└─────────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────┐
│           SessionManager                    │
│  - Track active sessions                    │
│  - Activity history (ring buffer)           │
└─────────────────────────────────────────────┘
```

## Usage Example

```rust
use kiana_bridge::{BridgeApiClient, BridgeConfig, SessionManager, WorkPollLoop};
use std::sync::Arc;

let api = Arc::new(BridgeApiClient::new(base_url, token));
let (env_id, secret) = api.register_environment(&config).await?;
let session_manager = Arc::new(SessionManager::new());

let work_loop = WorkPollLoop::new(api, session_manager, config)?;
work_loop.run(env_id, secret).await?;
```

`WorkPollLoop::new` now routes user messages through a command-backed runner in
the per-session directory selected by `BridgeConfig.spawn_mode`. By default it executes
`kiana -p --output-format json --execute -- <prompt>`; set
`KIANA_BRIDGE_RUNNER_COMMAND` and `KIANA_BRIDGE_RUNNER_ARGS` to use another local
runner. `KIANA_BRIDGE_RUNNER_ARGS` is parsed with shell-like quoting and invalid
quotes are returned as configuration errors. Runner children inherit
`BridgeConfig.session_timeout_ms` by default; set `KIANA_BRIDGE_RUNNER_TIMEOUT_MS`
to override it, or `0` to disable runner timeout. Set `KIANA_BRIDGE_RUNNER_MODE=stream-json`
for the reusable per-session NDJSON child process mode. `WorkPollLoop::recording(...)`
is the explicit record-only diagnostic mode.

## Compilation Status

✅ **SUCCESS** - Workspace tests pass after the current bridge runtime slice

```bash
$ cargo test --workspace --no-fail-fast
test result: ok
```

## Next Steps

### Near-term (Essential Features)
1. Full `sessionRunner.ts` parity for true Session Ingress child transport, remaining local callbacks beyond one-shot model/permission overrides, broader activity extraction for remaining SDK event shapes, and real hosted bridge/session-ingress validation
2. Broader OAuth store integration and proactive token refresh scheduling beyond the current CLI refresh-command hook

### Long-term (Full Parity)
1. TUI status display with ratatui
2. CCR v2 protocol support (SSETransport + CCRClient)
3. Session spawning (child process management)
4. Full bridgeMain.ts orchestration

## Reference Mapping

| TypeScript File | Rust Equivalent | Status |
|----------------|----------------|--------|
| types.ts | types.rs | ✅ Core types |
| bridgeApi.ts | api.rs | ✅ HTTP client |
| replBridgeTransport.ts | transport.rs | ⚠️ Session Ingress WebSocket and CCR v2 worker transport implemented, including UUID echo/replay dedup, inbound user running-state reporting with same-state dedup, stream event batching, delivery flush, internal transcript persistence, and outbound permission lifecycle worker-state reporting; hosted service validation remains pending |
| bridgeMessaging.ts | work.rs control_request handling | ⚠️ Prompt responses for known/unknown server control requests implemented; one-shot model, permission-mode, and max-thinking-token transitions now affect subsequent child runs; richer local callback effects remain pending |
| sessionRunner.ts | work.rs command runner + CLI bridge stream-json loop | ⚠️ Child supervision, optional per-session stream-json reuse, reference-shaped default args, control_response forwarding, token update stdin forwarding, debug/transcript logging, common SDK output tolerance, and activity extraction implemented; true Session Ingress child transport still pending |
| bridgeMain.ts | work.rs + kiana-entrypoints bridge CLI | ⚠️ Real start path uses command-backed runner and shutdown cleanup; broader orchestration remains simplified |
| bridgeUI.ts | - | ❌ TUI pending |
| jwtUtils.ts | api.rs + CLI auth provider + runner token update hook | ⚠️ 401 refresh retry and child token update forwarding implemented; full OAuth store scheduler pending |

## Absolute File Paths

All files written to:
`/media/shirosora/4A183E5C183E46EB/codestorage/kianacode/kiana-bridge/`

- `Cargo.toml`
- `README.md`
- `src/lib.rs`
- `src/types.rs`
- `src/api.rs`
- `src/transport.rs`
- `src/session.rs`
- `src/work.rs`
- `examples/simple.rs`
