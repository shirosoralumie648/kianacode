# CAP-25 Receipt and four-entry parity baseline

CAP-25 keeps EventLog and its typed projections authoritative. `RunReceipt`, `ExecutionReceipt`
and `ReceiptAggregation` carry stable IDs/digests, source cursor/event references, descriptor and
action binding, attempt/effect/stop/fence dimensions, usage/output/file/memory summaries,
redaction/proof status and bounded limitations. `ExecutionReceiptEvidence` adds the server-derived
plan dimensions required by the card: descriptor/binding identity, input/scope/plan digests, the
approval reference, the actual backend, resource-usage and changeset digests, the terminal result
digest and a bounded `limitations` list. Every field is optional and is only ever filled from a
committed fact; missing evidence is reported as a stable limitation instead of a wildcard default,
and an unconfirmed stop or unknown effect keeps the receipt fenced. Owner, project and data-epoch
checks happen before receipt reads; a foreign run, revoked data or contradictory terminal facts
cannot become success.

CLI, Workbench, Web and Desktop use the same protocol/DaemonHost/ControlPlane read path and
entrypoint parity snapshot. The snapshot compares source cursor, event IDs, status, receipt/audit/
health digests and limitations while retaining only the entrypoint label as presentation metadata.
RunStream is a bounded disposable progress projection: progress may lag or drop, but terminal
envelopes are retained and replayed by cursor/epoch. Reconnect rehydrates a receipt/terminal fact
and never re-executes a tool or replays a model turn; late progress and duplicate terminals are
rejected by the bus after the first terminal fact.

Adapter output is normalized and request-ID paired before projection; an invalid or foreign result
is Unknown/fenced. Shell stdout, MCP descriptions and UI notifications are evidence at most and
cannot rewrite EventLog facts or claim external success.

## Behaviour fixtures

The CAP-25 card names three deny fixtures and a reconnect regression. They are wired as follows:

| Card obligation | Fixture | Layer |
|---|---|---|
| `foreign_run_cannot_read_output_or_receipt` | `kiana-core/src/receipt_entrypoint_tests.rs` (`kiana-core/src/receipts.rs` `#[path]`) | receipt/parity read model |
| `invalid_adapter_result_cannot_be_rendered_as_success` | `kiana-domain/tests/cap25_receipt_evidence.rs` + `kiana-core/src/receipt_entrypoint_tests.rs` | contract + execution-receipt projection |
| `late_progress_cannot_overwrite_terminal_outcome` | `kiana-daemon/src/run_stream.rs` tests | real `RunStreamBus` |
| reconnect does not re-execute / re-inject | `reconnect_replays_terminal_without_reinvoking_runner_or_model` | real `RunStreamBus` + runner counter |
| plan evidence is fact-bound | `execution_receipt_projects_plan_evidence_without_inventing_metrics` | `ControlPlane::read_receipt` + parity |
| limitations stay bounded | `execution_receipt_limitations_stay_bounded_and_explicit` | `ControlPlane::read_receipt` |

`kiana-core/tests/cap25_receipt_entrypoint_guard.rs` keeps the contract/projection markers, the
fixture names, the lane wiring and the bypass negative assertions in one place.

GitHub Actions runs receipt, aggregation, parity, protocol, Web sync, RunStream and source-guard
fixtures plus workspace compilation. No local runtime tests or smoke commands were run.
