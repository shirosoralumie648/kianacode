# CAP-25 Receipt and four-entry parity baseline

CAP-25 keeps EventLog and its typed projections authoritative. `RunReceipt`, `ExecutionReceipt`
and `ReceiptAggregation` carry stable IDs/digests, source cursor/event references, descriptor and
action binding, attempt/effect/stop/fence dimensions, usage/output/file/memory summaries,
redaction/proof status and bounded limitations. Owner, project and data-epoch checks happen before
receipt reads; a foreign run, revoked data or contradictory terminal facts cannot become success.

CLI, Workbench, Web and Desktop use the same protocol/DaemonHost/ControlPlane read path and
entrypoint parity snapshot. The snapshot compares source cursor, event IDs, status, receipt/audit/
health digests and limitations while retaining only the entrypoint label as presentation metadata.
RunStream is a bounded disposable progress projection: progress may lag or drop, but terminal
envelopes are retained and replayed by cursor/epoch. Reconnect rehydrates a receipt/terminal fact
and never re-executes a tool or replays a model turn.

Adapter output is normalized and request-ID paired before projection; an invalid or foreign result
is Unknown/fenced. Shell stdout, MCP descriptions and UI notifications are evidence at most and
cannot rewrite EventLog facts or claim external success.

GitHub Actions runs receipt, aggregation, parity, protocol, Web sync, RunStream and source-guard
fixtures plus workspace compilation. No local runtime tests or smoke commands were run.
