# Security finding: `kiana mcp-server-http` executes the full tool registry with no authorization

> Recorded 2026-09-28 by the integration owner during the annotation pass over
> `kiana-entrypoints`. **This document reports a finding; it does not fix it.** The fix is an
> architecture decision (see "Options" below), and `AGENTS.md` §10 requires stopping and reporting
> rather than picking a workaround.

## What is reachable

A user who types one of these gets a TCP server that executes tools with no authorization:

```text
kiana mcp-server-http      (also: mcp-server-sse, mcp-server-ws)
kiana mcp-server --http    (also: --sse, --ws)
KIANA_MCP_HTTP_HOST / KIANA_MCP_HTTP_PORT   (default 127.0.0.1:8765)
```

## The evidence chain, link by link

| # | Fact | Where |
|---|---|---|
| 1 | The CLI accepts `mcp-server-http` / `--http` / `--sse` / `--ws` and routes to the HTTP server | `kiana-entrypoints/src/cli.rs` `mcp_server_main` |
| 2 | It calls `start_mcp_http_server(cwd, addr, …)` | `cli.rs:11336` |
| 3 | That binds a TCP listener and serves the axum router | `kiana-entrypoints/src/mcp.rs:483` `start_mcp_http_server` |
| 4 | The router exposes `POST /`, `POST /mcp`, `GET /sse`, `POST /message`, `GET /ws` — **with no auth layer, no middleware, no token** | `mcp.rs` `mcp_http_router` |
| 5 | A `tools/call` lands in `McpServer::handle_call_tool`, which calls `execute_tool_call(&self.registry, None, &mut context, name, &args, None)` | `mcp.rs` `handle_call_tool` |
| 6 | `execute_tool_call`'s second parameter is `enabled_tools: Option<&HashSet<String>>`, and the allow-list check is **skipped entirely when it is `None`** (`if let Some(enabled_tools) = … { if !contains … }`) | `kiana-tools/src/tool_execution.rs:164` |
| 7 | The sixth parameter `permission_handler` is also `None`, so there is no permission prompt | same call site |
| 8 | The default registry contains `FileWriteTool`, `FileEditTool`, **`FileDeleteTool`**, `AgentTool`, `ReplTool`, **`BashTool`**, **`PowerShellTool`**, **`WebFetchTool`**, **`RemoteTriggerTool`**, and the recursive `McpTool` | `kiana-tools/src/registry.rs:52` `create_default_registry` |

Steps 5–7 together mean: **no `ControlPlane`, no policy, no gate, no approval, no EventLog.** The
call never becomes a capability request, so none of the authorization chain that the rest of this
repository is built around is consulted.

## Which frozen items this contradicts

From `AGENTS.md` §7:

- **「不许打开 HTTP MCP（当前返回 `mcp_transport_unsupported`）」** — that guard exists, but on the
  *harness* MCP path (`kiana-daemon/src/harness_mcp.rs:51,157`). This entrypoint's own MCP server
  has no equivalent guard.
- **「不得新增第二条执行循环 / 绕过 `ControlPlane`」** — §4 and §6 of the same file.
- **「模型可见工具面锁死为五个」** — §3. The frozen surface is `shell / apply_patch / mcp /
  memory.search / memory.write` as mediated by `kiana-runner/src/tools.rs`. This path exposes the
  entire `ToolRegistry` instead, unmediated.

## What bounds the severity

Stating this plainly, because a finding that overstates itself is not a finding:

- It is **opt-in**. Nothing starts it automatically; a human must type the subcommand.
- The default bind address is **loopback** (`127.0.0.1`), so it is not exposed to the network by
  default.

## What makes it serious anyway

- **Any local process can reach it.** A loopback socket is not an authorization boundary: every
  process on the machine, every sandboxed tool, and — via ordinary browser reachability of
  loopback — potentially any web page the user visits while it is running.
- **The host is overridable.** `KIANA_MCP_HTTP_HOST=0.0.0.0` turns a local footgun into a network
  exposure, and nothing in the code warns or gates it.
- **The registry is not a benign set.** It contains delete, shell, PowerShell, outbound fetch and
  remote trigger. Unauthenticated `bash` plus `file_delete` is not a read-only convenience surface.
- **There is no audit trail.** Because nothing goes through `ControlPlane`, the executions leave no
  event, no receipt and no decision record. An incident caused through this path would be invisible
  to everything this repository has built for investigating exactly that.

## Options (the decision this document exists to escalate)

1. **Refuse the transport, the way the harness path already does.** Make the HTTP/SSE/WS variants
   return `mcp_transport_unsupported`, matching `harness_mcp.rs`. Smallest change, immediately
   removes the surface, and is consistent with an item `AGENTS.md` already froze. Cost: the feature
   stops existing.
2. **Keep it, but route tool calls through `ControlPlane` as capability requests** so policy, gates,
   approval and the EventLog all apply. This is the option that makes the feature legitimate, and it
   is a substantial change: the server would need a `RequestContext`, and the five-tool frozen
   surface question would have to be answered for the MCP path too.
3. **Keep it with an explicit opt-in secret and a loopback-only bind**, and document the residual
   risk. Cheapest of the three that preserves the feature, and still weaker than 2 — an opt-in
   secret is a bearer token on a socket that any local process can reach.

The choice belongs to the architecture owner. This repository's own convention (§10) is to stop
here rather than pick one.

## What was deliberately not done

No code was changed. The finding was found while annotating `kiana-entrypoints` for a reader who
does not know the architecture, and the annotation pass is explicitly forbidden from changing
behaviour or "fixing things along the way".

---

## Addendum (2026-09-28): the same pattern exists in `runner.rs`, and the existing guard does not see it

While annotating `kiana-entrypoints`, the same class of bypass was found in
`kiana-entrypoints/src/runner.rs`. It is **not** currently reachable, and the difference is worth
stating precisely, because it shows exactly where the existing defences stop.

### What `runner.rs` contains

| Group | What it does | Reachable? |
|---|---|---|
| `run_assistant_turn*`, `call_tool` | A complete model-to-tool-to-model loop that calls `kiana_tools::execute_tool_call(s)` directly. No `DaemonHost`, no `ControlPlane` — the file contains **zero** references to either. | **No.** Nothing in the current tree calls them. |
| `run_resident_teammate_loop_with` | Contains no tool execution at all. It takes the "run one model turn" function as an injected callback; the CLI passes `crate::sdk::unstable_v2_prompt`, which reaches `harness_run.rs` -> `KianaClient` -> `DaemonHost` -> `ControlPlane`. | **Yes, and it is authorized.** |

So the legacy loop in `runner.rs` is exactly the "second execution loop" `AGENTS.md` forbids — and it
is genuinely dead from the product's perspective.

### What keeps it dead, and why that is not enough

`kiana-entrypoints/tests/cli_architecture.rs` reads seven product files (`cli.rs`, `harness_run.rs`,
`repl.rs`, `tui.rs`, `bg.rs`, `mcp.rs`, `lib.rs`) and asserts that **none of them contains the
string `run_assistant_turn`**. Any new product-path reference fails CI. That guard works.

But it matches a **symbol name**, not a **pattern**. It cannot see "calls `execute_tool_call`
directly", because that phrase appears nowhere in the forbidden list. `mcp.rs` executes tools
directly and never mentions `run_assistant_turn` — so the guard passes it, while the HTTP transport
in the same file is live and reachable.

That is the generalisable finding: **a quarantine that names what it forbids will always be
circumventible by doing the same thing under a different name.** The forbidden unit here should be
the *pattern* — any direct call into `kiana_tools::tool_execution` from a product file — rather than
one identifier.

`runner.rs` also keeps the legacy loop alive by another route: its own `call_tool_*` unit tests
still exercise it, so removing the loop means removing those tests too. "Legacy" here currently
means "guarded and unused", not "removed".

### Recommendation (additive to the three options above)

Add a fourth guard, in the same spirit as `cli_architecture.rs`, that asserts no file listed as a
product surface calls `kiana_tools::tool_execution::execute_tool_call` directly. That closes the
pattern hole without requiring the behavioural refactor, and it would have caught `mcp.rs` at the
moment it was written.
