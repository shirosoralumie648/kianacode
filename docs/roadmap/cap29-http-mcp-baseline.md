# CAP-29 Streamable HTTP MCP baseline

CAP-29 adds a product-path HTTP/SSE MCP adapter behind the same ControlPlane permit, discovery
snapshot, catalog/schema validation and adapter-result boundary as stdio MCP. HTTP/SSE wire
behavior is supplied by the existing bounded service transport: JSON-RPC initialize/notify/tool
calls, SSE endpoint/message handling, request IDs, response errors, timeouts, bounded HTTP client
body behavior and same-origin redirect checks are covered by its mock fixtures.

The daemon wrapper validates the configured HTTP/SSE endpoint, rejects raw credential headers and
process-only fields, keeps the HTTP session identifier non-authoritative, records whether a
business call was sent, and converts disconnect/error after send to `result_unknown` rather than a
safe retry. HTTP calls still require the existing MCP discovery/catalog/schema/approval facts;
remote effects are not claimed to be isolated by local bwrap.

The initial product slice intentionally leaves live provider credentials, audience-specific token
injection, durable SSE reconnect/replay and physical cross-origin interoperability as explicit
follow-up evidence boundaries. GitHub Actions runs the service mock HTTP/SSE fixtures, product MCP
lifecycle guard, CAP-22 regression guard and workspace compilation. No local runtime tests or
smoke commands were run.
