# ControlPlane Entry Matrix

This is the CP-00 source baseline for the Kiana execution spine. It records the
current routing and the known denial/result boundaries; it is not a claim that
all paths already share one atomic transaction.

## Snapshot

```text
source_snapshot: f366436a0a232c2a9a31b3ab2968da4a6824aa90
worktree: master; clean before this documentation change
scope: kiana-daemon::DaemonHost, kiana-core::ControlPlane, kiana-runner, broker registrations, policy/gates/hooks, focused integration tests
```

## Public Request Routes

| Wire body / entry | DaemonHost route | ControlPlane owner | Execution boundary | Current evidence / gap |
|---|---|---|---|---|
| `Command` | `handle_command` | command-specific capability or Company handler | direct capability, Company state, or read-only projection | command rejection is recorded; command families do not all use the same transition helper |
| `Run` | `start_run_with_history` | `start_run_with_id` then `drive_run` | Runner emits `CapabilityRequested`; ControlPlane calls `broker_harness_capability` | Harness path rechecks policy/gate/hook before Broker; pending invocation is process-local |
| `Continue` | `continue_run` | `continue_run` then `drive_run` | same Runner path with existing Run/session binding | binding and role/profile checks exist; legacy Continue reuses RunId |
| `Resume` | `resume_run` | recovery projection and explicit continuation | only recovery material may rebuild a Runner continuation | missing or redacted material fails closed; full cross-process Runner restore is not complete |
| `ApprovalDecision` | `decide_approval_with_proof` | `decide_approval_with_proof` | direct pending uses `execute_authorized_request`; Run-bound pending uses `resume_approved_invocation` | hash/nonce/context/expiry are rechecked; direct and Run-bound continuation have different event scopes |
| `ListApprovals` | `list_pending_approvals` | recovery projection | read-only approval projection | does not execute or consume a pending action |
| `Cancel` | `cancel_run` | lifecycle cancellation and `cancel_pending_tools` | Runner cancel plus stop confirmation | cancellation is per Run; each in-flight effect still needs explicit stop/effect classification |
| `Receipt` | `read_receipt` | receipt projection | read-only EventStore query | foreign Run/request events are filtered; unsupported global scans fail closed |
| `Spawn` | `spawn_from_packet` | collaboration + CellRegistry + Runner | packet admission, resource reservation, then fresh Runner | packet/path/duplicate checks exist; resource and event commit are not one durable batch |
| `Symposium` | `convene_symposium` | collaboration/Company state | artifact and optional Runner path | role/chair/round limits are checked; no second model loop is created |
| `Review` / `Close` | `review_author_run` / `close_author_run` | collaboration and artifact handlers | fresh reviewer/closer session or artifact-only path | author/owner and artifact path checks exist; business lifecycle remains partial |

All routes enter through `DaemonHost::handle`; CLI, Web, Workbench, and client
helpers build the same `RequestEnvelope` family. The legacy CLI/SDK compatibility
surface is intentionally not counted as a second product execution spine.

## Capability Entry Paths

| Path | Preparation | Decision | Dispatch / finalization | Difference to close later |
|---|---|---|---|---|
| Direct command capability | `prepare_capability_action(..., from_runner=false)` | `authorize_capability_action` | `execute_authorized_request` -> `dispatch_capability_action` | no Run-bound continuation; result is returned to the command caller |
| Harness tool | `prepare_capability_action(..., from_runner=true)` | cancellable policy/gate/hook decision | `broker_harness_capability` -> Broker -> Runner `CapabilityResult` | pending approval pauses `drive_run`; cancellation is tied to Run and queued tool state |
| Approval resume | prepare the immutable stored request again | decision with approval requirements and current context | `resume_approved_invocation` or direct replay path | consumes an approval and must distinguish missing continuation from direct command replay |
| Company / packet action | command-specific normalization and Company guards | default policy plus Company invariants | Company/artifact/Cell adapter, then receipt events | extra business-state and owner checks are not yet represented as one generic action descriptor |

## Broker Registration Surface

The DaemonHost composition root registers the following handler families in one
`CapabilityBroker`: context query/cache, shell/apply_patch, execution control,
stdio MCP, memory search/write/review, workspace checkpoint restore,
data-governance, connectors, and extensions. Model-visible tools remain the
fixed five (`shell`, `apply_patch`, `mcp`, `memory.search`, `memory.write`);
operator-only operations such as `memory.review` are not added to that catalog.

Before a handler can run, the ControlPlane records the request and capability
decision, applies server-owned risk checks, runs the pre-tool hook boundary, and
passes an `AuthorizedCapabilityRequest` to the broker. A broker call is never a
permission source.

## Failure Classification Baseline

| Class | Expected observable result | Broker/handler side effect | Source evidence |
|---|---|---|---|
| hard policy or scope denial | `Denied`/`Blocked` with stable reason and decision event | zero handler calls | role/department, trust, path, unknown-tool, MCP-risk, memory-ACL tests |
| approval required | `AwaitingApproval` with exact challenge | zero handler calls until proof | direct write approval and harness approval fixtures |
| approval denied/expired/cancelled | failed/denied/cancelled terminal with approval fact | zero handler calls | approval deny, cancel-after-awaiting, proof/expiry paths |
| pre-dispatch event read/authority failure | failed or blocked before dispatch | zero handler calls | EventStore read and authority failure fixtures |
| broker/provider failure | failed capability result, redacted and paired to call | handler may have started; effect certainty is not inferred | failing broker, pre-dispatch failure, result mismatch fixtures |
| missing terminal/result persistence | `ResultUnknown` and no success receipt | no automatic retry | result-event failure, missing terminal, receipt replay fixtures |
| cancellation before/while dispatch | cancelled only after stop is confirmed; otherwise `ResultUnknown` | queued calls drained; in-flight effect classified | mid-stream, shell timeout, in-flight cancel fixtures |
| foreign or stale identity | `ResultUnknown`/blocked, never foreign success | zero handler calls | foreign Runner events, session owner and role immutability fixtures |

## Source Test Index

The source index at this snapshot contains 108 async tests in
`kiana-core/tests/control_plane.rs`, 74 in `kiana-daemon/tests/daemon_host.rs`,
and 2 async tests in `kiana-client/tests/client_methods.rs`. Relevant named
coverage includes:

- Direct / approval: `write_capability_waits_for_approval_without_calling_broker`,
  `approval_resumes_the_stored_request_once_with_monotonic_events`,
  `approval_deny_returns_denied_without_broker_execution`.
- Harness / broker: `start_run_brokers_harness_tools`,
  `mismatched_harness_capability_result_is_unknown`,
  `malicious_unknown_tool_is_denied_before_the_broker`.
- Cancellation / recovery: `cancel_after_awaiting_approval_does_not_resume_the_pending_invocation`,
  `unconfirmed_cancel_result_is_unknown_without_cancelling_or_forgetting_the_run`,
  `rebuild_does_not_replay_completed_capability`.
- Hook / product path: `pre_tool_use_hook_blocks_apply_patch_before_broker_execute`,
  `pre_tool_use_hook_ask_fails_closed_before_broker_execute`,
  `review_author_run_uses_a_fresh_reviewer_session`.

These are source-index counts and names, not local runtime results. The current
tree was not tested locally by instruction; GitHub CI is the runtime evidence
source after the step is pushed.

## CP-04/CP-05 Handoff

The matrix exposes the remaining convergence work instead of hiding it:

1. Direct, Harness, and approval-resume paths should share one explicit
   prepare/authorize/dispatch/finalize contract while preserving their protocol
   and continuation scopes.
2. The negative baseline needs one shared fixture that injects a counting Broker,
   deny policy, expired approval, pre-cancelled token, and failing EventStore;
   each branch must prove handler calls remain zero where execution is denied.
3. A status of `Allowed` or an approval decision must not be treated as a
   durable dispatch permit until the EventStore, budget, lease, and cancellation
   checks are committed together.

This handoff is the CP-00 result; implementation belongs to CP-04/CP-05 and is
not silently claimed here.
