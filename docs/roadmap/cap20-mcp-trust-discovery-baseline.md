# CAP-20 MCP trusted configuration and discovery baseline

CAP-20 records the existing MCP registry/stdio boundary as a roadmap-gated slice. Startup parses
the server configuration, rejects duplicate/unknown/ambiguous servers and unsupported transport,
pins configuration/hash and project trust into the prepared snapshot, and requires the actual
sealed executable/configuration bytes to match before spawn. Discovery produces a protocol/tool
catalog digest and committed `mcp.discovery_committed` fact before business calls; serverInfo or
response success is not itself trust or authority.

The invocation runs in the shared narrow bwrap/ProcessSupervisor boundary, with explicit env and
stdio lifecycle cleanup. HTTP transport remains explicitly unsupported in this product slice.

## Behaviour fixtures (2026-10-17, CI-only)

The card's deny-first and success/regression obligations are covered by behavioural fixtures in
`kiana-daemon/src/harness_mcp.rs`'s test module. They drive the production boundary — the startup
`parse_servers`, `McpRegistry::prepare`/`McpHandler::run` and the real confined
`ConfinedMcpClient` — against a stdlib-only Python stdio MCP server, and they spawn the planned
bwrap boundary (a missing `bwrap`/`python3` fails the fixture instead of falling back to the host):

- `startup_config_parse_rejects_untrusted_shapes`: duplicate names, the 32-server cap, remote
  process fields and implicit environment/credential keys are refused during the one startup parse.
- `untrusted_or_swapped_mcp_binary_cannot_start`: an untrusted project never reaches a process,
  and a binary swapped after preparation is rejected by the pinned executable hash.
- `unknown_server_or_ambiguous_tool_is_rejected`: an unconfigured server and an ambiguous/unknown
  advertised tool fail closed.
- `discovery_has_no_implicit_host_or_network_authority`: HTTP/SSE/WS discovery is refused (never
  downgraded to a local call) and a cancelled discovery never starts the pinned process.
- `stdio_fixture_negotiates_lists_tools_and_has_no_network`: the fixture negotiates protocol
  `2025-06-18`, lists its tools, echoes a call, proves an outbound connection to a host listener is
  denied, and confirms stop. The snapshot records `server_id`, `transport`, `env_keys`,
  `credential_refs`, the pinned executable hash and the config source.

GitHub Actions runs the existing P1-J4 MCP lifecycle fixtures, the CAP-20 dry-run/behaviour
fixtures, the CAP-20 source guard and the workspace compilation shards. No local runtime tests or
smoke commands were run; the fixtures need `bwrap` and `python3`, and any missing tool fails closed.
