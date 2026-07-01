# Bridge Module

Remote control integration layer for Kiana.

## Architecture

The bridge module handles WebSocket/RPC communication between local CLI and claude.ai:

- **API Client** (`api.rs`): HTTP client for environment registration, work polling
- **Transport** (`transport.rs`): WebSocket connection for real-time message exchange
- **Session Manager** (`session.rs`): Track active remote control sessions
- **Work Loop** (`work.rs`): Poll for work items and spawn sessions
- **Types** (`types.rs`): Protocol types (SDKMessage, WorkSecret, etc.)

## Key Concepts

1. **Environment Registration**: Register bridge instance with server
2. **Work Polling**: Long-poll for session work items
3. **Session Spawning**: Connect WebSocket transport per session
4. **Message Routing**: Forward SDK messages between local and remote

## Simplifications from TypeScript

- No UI rendering (ratatui/crossterm will be added separately)
- Work polling dispatches session tasks up to `BridgeConfig.max_sessions`.
  `single-session`, `same-dir`, and `worktree` execution directories are wired.
  `worktree` uses `git worktree` when available and falls back to a snapshot copy
  under `.kiana/bridge-worktrees/`. Managed worktree/snapshot directories are
  removed when the bridge session ends or the session transport cannot connect.
- Active work items send periodic `/heartbeat` requests while their WebSocket
  session is alive. Configure the interval with `--heartbeat-interval-ms` or
  `KIANA_BRIDGE_HEARTBEAT_INTERVAL_MS`; use `0` to disable.
- Completed WebSocket sessions call `stopWork(false)` and archive their remote
  session. Transport failures, failed transport setup, heartbeat lease loss, and
  fatal heartbeat auth/status failures call `reconnectSession` so the backend can
  re-dispatch the session work.
- `stopWork` uses retry/backoff for transient failures before archiving or
  shutdown cleanup continues. Fatal auth/session errors are not retried.
- Sessions have a watchdog timeout matching the reference `sessionTimeoutMs`
  behavior. Configure it with `--session-timeout <seconds>`,
  `--session-timeout-ms`, `KIANA_BRIDGE_SESSION_TIMEOUT`, or
  `KIANA_BRIDGE_SESSION_TIMEOUT_MS`; use `0` to disable.
- `bridge start` tracks active work. On Ctrl+C it removes local session state,
  cleans managed worktree/snapshot directories, calls `stopWork(force=true)`,
  archives active sessions, and only then deregisters the environment.
- Core WebSocket/RPC logic preserved
- Bridge auth uses an injectable token provider. The CLI reads
  `KIANA_BRIDGE_ACCESS_TOKEN`/`CLAUDE_ACCESS_TOKEN` and can run
  `KIANA_BRIDGE_REFRESH_COMMAND` once after HTTP 401 to refresh the token.

## Usage

```rust
use kiana_bridge::{BridgeApiClient, BridgeConfig, SessionManager, WorkPollLoop};
use std::sync::Arc;

let api = Arc::new(BridgeApiClient::new(base_url, token));
let (env_id, secret) = api.register_environment(&config).await?;
let session_manager = Arc::new(SessionManager::new());
let work_loop = WorkPollLoop::new(api, session_manager, config)?;
work_loop.run(env_id, secret).await?;
```

`WorkPollLoop::new` uses a command-backed runner. Remote user messages run in
the per-session directory selected by `BridgeConfig.spawn_mode` and are executed
through:

```bash
kiana -p --output-format json --execute -- <remote prompt>
```

Override that command with `KIANA_BRIDGE_RUNNER_COMMAND` and
`KIANA_BRIDGE_RUNNER_ARGS`. Runner args use shell-like quoting, so values such as
`--system-prompt 'hello world'` stay intact; invalid quoting is reported as a
configuration error. Use `WorkPollLoop::recording(...)` only for tests or
diagnostics where remote messages should be acknowledged without model
execution.

## Reference

Original TypeScript: `reference/src/bridge/` (~12,600 lines, 33 files)
