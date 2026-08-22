# v0.4 Phase 3 Context — MCP client through daemon

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`
Requirement: CODE-02

## Classify

Phase, not spike. User-visible completion is: a trusted Builder run can call
one local **stdio** MCP server through `DaemonHost` / `KianaHarness`. The
model-visible tool is `mcp`. Execution is `mcp.call` on the capability
broker. Untrusted projects deny. Reviewer/PM cannot call it.

This is not the legacy `kiana-tools` MCP tool and not
`kiana-entrypoints/src/mcp.rs` (Kiana as MCP *server*).

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 symposium/packet, v0.4 Phase 1 review,
the Phase 2 matrix, TUI park, five departments, RAG, skills-on-harness,
or TeamCreate/SendMessage.

1. **One model tool `mcp`.** Arguments: `server` (optional if exactly one
   configured server), `tool` (required), `arguments` (object). Not N
   exploded MCP tools this slice.
2. **Config is `KIANA_MCP_SERVERS_JSON`.** Same env the legacy client
   already uses. No new `kiana run` flag. Do not format `cli.rs`.
3. **stdio only.** HTTP / SSE / WS → `mcp_transport_unsupported`. No
   marketplace, no SSO, no remote MCP auth.
4. **Policy.** Untrusted → `project_untrusted`. Role without `mcp` →
   `role_tool_denied`. Safe / read-only profile → Ask (non-interactive
   fails closed). Trusted + Balanced (`--sandbox workspace-write`) +
   Builder → Allow. Risk is `ExternalSideEffect`.
5. **PATH-03 extension.** Model-visible tools become `shell`,
   `apply_patch`, `mcp`. Still not the `kiana-tools` registry. Broker
   still executes.

Demo (same-host):

```text
trusted + workspace-write + KIANA_MCP_SERVERS_JSON stdio mock
cassette: mcp { tool: echo, arguments: { message: hello } }
→ result contains hello; harness remains kiana-harness
untrusted → project_untrusted, no child
reviewer → role_tool_denied
```

## Requirements this phase

CODE-02. PATH/TRUST/SESS/EVD/ROLE/ORCH/SYMP/WB/REV/CODE-01 still true.
PATH-03 is extended as above, not deleted.

## Frozen

- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- skills/hooks on harness (CODE-03)
- provider live matrix (CODE-04)
- structured Read/Grep/Glob
- HTTP/SSE MCP
- `kiana-tools` wiring
- migrating TUI
- formatting `kiana-entrypoints/src/cli.rs`
