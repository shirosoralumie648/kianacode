# CAP-20 MCP trusted configuration and discovery baseline

CAP-20 records the existing MCP registry/stdio boundary as a roadmap-gated slice. Startup parses
the server configuration, rejects duplicate/unknown/ambiguous servers and unsupported transport,
pins configuration/hash and project trust into the prepared snapshot, and requires the actual
sealed executable/configuration bytes to match before spawn. Discovery produces a protocol/tool
catalog digest and committed `mcp.discovery_committed` fact before business calls; serverInfo or
response success is not itself trust or authority.

The invocation runs in the shared narrow bwrap/ProcessSupervisor boundary, with explicit env and
stdio lifecycle cleanup. HTTP transport remains explicitly unsupported in this product slice.

GitHub Actions runs the existing P1-J4 MCP lifecycle fixtures, CAP-20 source guard and workspace
compilation. No local runtime tests or smoke commands were run.
