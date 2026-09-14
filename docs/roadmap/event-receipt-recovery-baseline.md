# ER-00 Event / Receipt / Recovery Baseline

> This document is a source and static-check baseline for ER-00. It records what
> the current tree owns and what it does not prove. It is not an implementation
> claim for ER-01 or later steps. Runtime behavior is evidence from GitHub CI only;
> no local test binary was run for this baseline.

## Snapshot

```text
source_snapshot: 0a29de510c240584a972dad6ef14c4ca6a0dfced
source_worktree: clean and pushed before this documentation follow-up
scope: kiana-domain journal/state contracts, kiana-ports EventStorePort,
       kiana-eventlog Memory/JSONL adapters, kiana-core event/history/projection/
       receipt/recovery readers, kiana-daemon approval/run-stream adapters,
       kiana-query context/index caches
```

The source snapshot is deliberately kept separate from this documentation
commit. The relevant source hashes at that snapshot are:

| File | SHA-256 |
|---|---|
| `kiana-eventlog/src/lib.rs` | `61aa7bcc9c221474d3ffa75a323ba7931f1da8e7ce96107da1d1d7cb7fabd20c` |
| `kiana-eventlog/src/event_store_core.rs` | `506a8222a757a89e6516a12b6260daa7f35af2b3da49c17a23ca7cdbcc9d0b1e` |
| `kiana-eventlog/src/journal_core.rs` | `84ef64bc8208b2ead45d1d05feb0265e94129b898cd67d04f5ed189d388bba56` |
| `kiana-eventlog/src/jsonl.rs` | `f549e5de5bc4e197f268b865327bd1d0d7a4c10208daa3e7c13ceb68188e5c5f` |
| `kiana-eventlog/src/memory.rs` | `bcb98daa0a9548384a9dafdd9d5af2aefacca3d01d699056ba86477d3276e9e9` |
| `kiana-domain/src/journal.rs` | `4dbd656d03ec12e7821ffac254307227419423cbaf74ccb99a2b819c5eeedd48` |
| `kiana-domain/src/states.rs` | `dca28dc71ef75e5dd92a25bed399349ca286bc7c5c0e514e11de20d6d1491ae3` |
| `kiana-core/src/events.rs` | `2f5da5345ae1a2f738b976f283eace1f5e012b8c442e044b6935504b6c83f616` |
| `kiana-core/src/receipts.rs` | `84b92462efd3ada1e1260fd403517f8aaf0aae36462c3044b14ce98a499de571` |
| `kiana-core/src/projection.rs` | `673d6904a6ab3705041f649462fef30f57283f714f537e8bf1605d4635573777` |
| `kiana-core/src/recovery.rs` | `60730c9cd1c8f96fad80b454c003aebd9ff9a88ed409689229e2d064f9c3c47e` |
| `kiana-core/src/history.rs` | `91e1998251ac9ffd1444c06555421c2ec957c81d5dccda7bc7bfbe0cf5955645` |
| `kiana-daemon/src/approval_store.rs` | `e013b20c983f4555d7256980ed96c3785136c2fc148568a3a9a7ee34e6695929` |
| `kiana-daemon/src/journal_approvals.rs` | `47358e2f5a7df804d2b9494a985dc6fd25e08b7e17cec95883bad6997e04b67d` |
| `kiana-daemon/src/run_stream.rs` | `750bb78e46e5f1c42d3a773d0afcf468cc6713032ef47e30ffa100f3313db5c6` |
| `kiana-query/src/index.rs` | `773ff030488faed91546143f601131c84e11004a7f50890d26eddaa9020df150` |

## Fact Ownership

| Concern | Current fact owner | Read-only or cache boundary |
|---|---|---|
| Command transition and commit identity | `TransitionBatch`, `CommandReceipt`, `CommitOutcome` in `kiana-domain`; `EventStorePort::commit_transition` | Receipt projection may expose the result, but cannot create a commit or dispatch permission |
| Runtime events and order | `EventStorePort` implemented by `MemoryEventLog` or `JsonlEventLog` | `RuntimeEvent` is the durable-shaped record; UI stream, transcript, and handler return values are projections |
| Run state and terminal outcome | committed `run.*` / `approval.*` events folded by `kiana-core::projection` | `ControlPlane` memory is not the authority; conflicting or missing terminal facts fail closed |
| Receipt | `kiana-core::receipts::read_receipt` rebuilt from filtered EventStore events | Receipt is a redacted read model; it is not proof that an external effect happened |
| Approval | approval events and `JournalApprovalStore`/`ApprovalStore` adapters | pending lists and UI approval cards are projections; an approval decision does not itself prove dispatch |
| Recovery material | run snapshot/checkpoint and approval proof validated by `kiana-core::recovery` | runner continuation and UI state are not recovery facts; missing or stale material is rejected |
| Context, index, and memory maps | `kiana-query` and memory adapters | cache files/reports are acceleration or derived data and cannot replace EventLog facts |
| Run stream and notifications | committed event projection in `kiana-daemon::run_stream` | delivery/stream acknowledgement is not a terminal business result |

## Command, Event, Effect, Receipt

The current boundary is:

```text
Request/command
  -> ControlPlane identity, policy, gate, approval, budget and lifecycle checks
  -> EventStore append/transition (the authority commit and CommandReceipt)
  -> Broker/Runner effect
  -> capability/process/provider result event
  -> run terminal/result event
  -> Receipt projection from the EventStore
```

The source already records `request_id`, `event_id`, aggregate metadata, stream
version and optional idempotency key. A transition frame also records command
digest, commit id, cursor bounds, event ids and aggregate versions. The chain is
not yet mandatory for every legacy event, and an external provider or business
side effect does not acquire exactly-once semantics merely because a local result
event was written. Those are ER-02, ER-13 and ER-14+ concerns.

## EventStore Capability Baseline

`EventStorePort` defaults to explicit unsupported errors for atomic transitions,
command receipts, cursor reads and full reads. Its documentation states that a
port implementation does not by itself prove persistence, `fsync`, cross-process
locking or corruption recovery.

| Adapter | atomic transitions | durable commits | command receipts | cursor reads | writer / limits |
|---|---:|---:|---:|---:|---|
| `MemoryEventLog` | true | false | true | true | v2; 4 MiB frame; 256 events/batch |
| `JsonlEventLog` | `cfg!(unix)` | `cfg!(unix)` | true | true | v2; 4 MiB frame; 256 events/batch |

The JSONL adapter has source-level locking, identity checks, torn-tail repair and
an `Unknown` outcome when a write cannot be confirmed. These are source facts and
static compilation evidence in this step, not a local durability or crash
recovery claim.

## Minimum Correlation Fixture

The smallest run/tool/approval/unknown fixture must be traceable through these
fields, with the owner of each field explicit:

| Link | Current field or record | Current limitation |
|---|---|---|
| ingress | `RequestContext.request_id` / `RequestId` | legacy handlers may emit related events without a formal causation link |
| command | `TransitionBatch.command_id`, `command_digest` | not every legacy append is a transition batch |
| run/session | `run.authorized` payload `run_id`, `session_id`, actor/project identity | older events can lack the authorization identity |
| turn | `run.prompt`/turn payload and event sequence | no universal typed `TurnId` contract yet |
| capability | `capability_request_id`, `capability.requested` and result events | coverage is enforced at selected pre-dispatch/result sites, not every legacy event |
| approval | `approval_id`, approval requested/decision/consumed events | direct and Run-bound continuation scopes remain distinct |
| event order | `event_id`, sequence, aggregate type/id, stream version | legacy records may omit stream metadata |
| retry/commit | `idempotency_key`, `CommandReceipt.commit_id`, first/cursor bounds | external effects still need a separate effect receipt/reconciliation authority |

The fixture is source-indexed for this step. It is not a locally executed
cassette; GitHub CI is the runtime evidence source under the project workflow.

## Failure and Cache Boundaries

| Condition | Current behavior | What it does not mean |
|---|---|---|
| EventStore explicitly does not support `read_all` | `ControlPlane` maps the documented sentinel to `None`; receipt/history may use their explicit compatibility path | it is not an empty ledger and not a successful receipt |
| EventStore returns an actual read failure | the error propagates; receipt/history/projection do not silently rebuild an empty success | no data is not a safe substitute for an I/O failure |
| Store is readable but empty | receipt returns `receipt_not_found`; run projection returns `run_not_found` | empty is not read failure, and no terminal result is invented |
| Missing or conflicting run terminal | receipt/projection returns `ResultUnknown` or a terminal-conflict error | a partial event stream cannot be upgraded to success |
| Effect exists without a committed result event | capability/run paths record or return `ResultUnknown` | the handler return value is not a durable receipt |
| Context/index cache missing or corrupt | index code rebuilds or reports cache recovery | cache files are never EventLog authority |
| Memory adapter selected | facts are process-local and `durable_commits=false` | an in-memory pass is not durable evidence |
| JSONL write cannot be confirmed | adapter returns an unknown outcome and requires confirmation | retrying blindly cannot establish exactly-once execution |

## Source Evidence Index

The source snapshot indexes 108 async tests in
`kiana-core/tests/control_plane.rs`, 74 in
`kiana-daemon/tests/daemon_host.rs`, and 2 in
`kiana-client/tests/client_methods.rs`; these counts are not runtime results.
Relevant named assertions include:

- `unknown_event_kind_does_not_break_history_folding`
- `history_read_failure_is_returned_not_treated_as_empty`
- `new_process_rebuilds_run_state_from_events_alone`
- `missing_or_unsupported_ledger_fails_run_projection`
- `receipt_without_a_terminal_event_is_result_unknown`
- `completed_side_effect_with_missing_result_event_is_result_unknown`
- `rebuild_does_not_replay_completed_capability`
- `disk_receipts_survive_restart_and_do_not_overwrite_the_first_run`
- `continue_unknown_session_does_not_start_a_new_run`

The event-literal source index contains 177 unique dotted strings across the
domain/core/daemon/eventlog source trees. It includes compatibility/action and
schema literals as well as event kinds, so it is an inventory aid rather than a
machine-readable registry. The principal families are request/command, run,
approval, capability/execution/invocation, model/usage, process/resource,
workspace, memory, hook/MCP, Company/workflow/swarm/cell/packet,
authority/session/connectors/extensions, failure/reconciliation, review and
human-inbox events.

## ER-00 Exit and Open Work

ER-00 is complete as a fact-boundary baseline: the snapshot, hashes, capabilities,
ownership map, minimum ID chain, cache distinction, failure classification and
source evidence index are recorded. No implementation status is promoted for
ER-01 or later.

Still open for later steps:

- a machine-readable event-kind/schema registry and migration policy;
- mandatory causation/correlation and typed turn/invocation contracts for legacy
  events;
- one atomic authority/approval/budget/lease/aggregate commit boundary;
- rebuildable Receipt fields for cost, files, evidence, owner and source cursor;
- durable restart fencing, effect receipts and external-result reconciliation;
- runtime denial/success/recovery evidence from GitHub CI.
