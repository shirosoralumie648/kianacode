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

GitHub Actions runs the existing P1-J4-01 lifecycle fixtures, CAP-21 source guard and workspace
compilation. No local runtime tests or smoke commands were run.
