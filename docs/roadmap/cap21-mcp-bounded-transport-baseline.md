# CAP-21 bounded stdio MCP transport baseline

CAP-21 records the existing stdio MCP transport as a bounded protocol slice: initialize/call and
tools/list are deadline-bound; JSON-RPC frames, total bytes, frame count, notification count,
request/result JSON limits and pagination cursors are bounded; foreign IDs, invalid result/error
envelopes, cursor cycles and unsupported server-initiated requests fail closed. `isError` and
protocol/transport failures remain distinct, and an already-sent call is not automatically
reissued.

MCP child ownership uses the shared ProcessSupervisor and retains stop Unknown semantics. Tool
schema/output validation and resource-link non-fetch behavior remain in the existing harness
adapter; HTTP, sampling, elicitation and roots are explicitly unsupported rather than implicit
authority paths.

## Behavioural fixtures

The fixtures live in the `#[cfg(test)] mod tests` of `kiana-daemon/src/harness_mcp.rs` (Linux
only). They drive the production `crate::mcp_stdio::ConfinedMcpClient` — the same bwrap-confined,
per-invocation stdio transport that `McpHandler` uses — against a stdlib-only Python stdio server
(`CAP21_MCP_SERVER`). Each confined process carries a unique marker argument; the host then scans
`/proc/*/cmdline` for that marker to prove that no descendant survives a stop path instead of
trusting the child's own report. `read-only` is the default fixture scope; `workspace-write` is
used only where the host needs the server's own `cap21-trace.jsonl` record of what the confined
process actually received.

Deny-first fixtures, in the order the focused lane runs them:

| fixture | what it proves |
|---|---|
| `mcp_stdin_backpressure_is_bounded` | an unread stdin fails the write at the deadline (`mcp_write_timeout`) without delivering the request, and an answered-then-silent server fails the call at the deadline (`mcp_response_timeout`) with exactly one `tools/call` in the trace |
| `mcp_duplicate_or_foreign_response_id_is_rejected` | a foreign response id, a duplicated response absorbed by a later request, a `result`+`error` envelope and a non-2.0 `jsonrpc` all fail closed, and the child is gone afterwards |
| `mcp_pagination_cycle_and_notification_flood_fail_closed` | a repeated cursor (`mcp_cursor_cycle`), a 200-notification flood (`mcp_notification_budget_exceeded`), an oversized frame (`mcp_response_byte_limit`) and a non-`notifications/` method (`mcp_notification_invalid`) all fail closed; `list_changed` invalidates the catalog and refuses further calls |
| `mcp_server_requests_cannot_bypass_control_plane` | server-initiated sampling/roots/elicitation requests are answered with `-32601` "disabled" errors that echo the request id, produce no result, and leak neither the project path nor a fabricated model answer |

Bounded-lifecycle regression fixtures:

| fixture | what it proves |
|---|---|
| `mcp_bounded_notifications_and_fragmented_frames_are_parsed` | a bounded notification stream and a response split across three writes are still one frame |
| `mcp_paginated_tool_list_is_bounded` | three cursored pages become one sorted catalog; unlimited page count (`mcp_tool_catalog_page_limit`) and tool count (`mcp_tool_catalog_limit`) are refused |
| `mcp_initialize_failure_eof_and_child_crash_all_clean_up` | unsupported protocol version, incomplete initialize, EOF before and after initialize, and a child crash all fail closed and leave no marked descendant |
| `mcp_stderr_diagnostics_are_bounded_evidence` | stderr is retained only as a digest plus retained/truncated counts; raw stderr is never echoed |
| `mcp_cancelled_request_never_leaves_the_child_running` | cancellation stops the in-flight request (`cancelled:mcp_request_stopped`), refuses new business work (`cancelled:mcp_not_sent`), and the stop is confirmed with no survivor |
| `mcp_unanswered_call_stays_unknown_and_is_not_replayed` | through the real `McpRegistry::prepare` + `McpHandler::run`, a sent call without a response returns `ResultUnknown`, keeps `AdapterCommitState::Unknown`, retains its private workspace with reason `mcp_call_unconfirmed`, writes its reconciliation record, and reaches the server exactly once |

`cap21_fixture_configuration_parses_at_startup` additionally pins that the behaviour selector stays
a plain argument of a configuration that `parse_servers` accepts, so no fixture-only config shape
is smuggled in.

## CI wiring

`.github/workflows/cap21-mcp-bounded-transport.yml` installs and verifies `bubblewrap` and
`python3`, then runs the `harness_mcp::tests::` module, each CAP-21 fixture by name with `--exact`
and `--test-threads=1`, the P1-J4-01 lifecycle target, the CAP-21 source guard and the `--no-run`
compilations. The unified `ci.yml` `kiana-daemon` shard runs the whole library test suite with the
same runtime installed, so the fixtures also gate regular pushes. A missing `bwrap`/`python3`
fails the fixtures instead of degrading to an unsandboxed call.

`kiana-core/tests/cap21_mcp_bounded_transport_guard.rs` adds
`cap21_behavioural_fixtures_are_wired_and_deny_first`: every fixture name must exist in the daemon
source, in the focused lane and in this baseline, the runtime verification and `--exact` runs must
stay wired, no fixture may be `#[ignore]`d, and the lane must not grant sandbox bypasses. The
unified `ci.yml` `kiana-daemon` shard installs `bubblewrap` and runs the whole crate (library
included), so the fixtures also gate regular pushes; `python3` comes from the runner image.

## Evidence and limitations

- Proof level: `source`. The fixtures are compiled and run only in CI; no local runtime tests or
  smoke commands were run while authoring them, and no local `cargo test`/`fmt` execution is
  claimed.
- The `/proc` marker scan is a bounded host observation of the current namespace; it does not
  claim anything about descendants unreachable from that namespace.
- Fixtures require `bwrap` and a Python 3 interpreter, and only compile on Linux.
- The fixtures cover the stdio transport only; HTTP/SSE MCP remains explicitly unsupported.
- `result_unknown` for the unanswered call is proven at handler level against
  `MemoryEventLog`; cross-process durability, reconciliation execution and live interoperability
  remain CAP-22/CAP-24 or later evidence.
