# Durable scheduling command API

These source contracts use `RequestEnvelope::command` through `DaemonHost`. They add no
model loop, transport, model-visible tool or direct artifact execution route. `CoreResponse.status`
describes applying the command; inspect the returned instance/project/swarm status for the business
outcome. This implementation has been compiled without adding or running tests.

## Packet scheduling and handoff

`company.snapshot.v1` and its `company.next.v1` alias return the Company state plus
`scheduling[project_id] = {ready, blocked, expired_claims}`. The domain `ready_packets` and
`validate_dependency_dag` functions are the single source used by this projection and `StartRun`.
Dependencies require success, failures propagate, and cycle output is deterministic. Graphs are
bounded to 4,096 packets.

Company commands use `kiana.company-command.v1`, `expected_revision` and `idempotency_key`
as described in `company-command-api.md`. Additional command types:

- `claim_packet {packet_id}` reserves a server-generated Cell ID for the current Builder session.
- `renew_packet_claim {packet_id}` requires that same session and a live lease.
- `reclaim_packet_claim {packet_id}` records expiry before releasing it. Undispatched packets
  become ready; dispatched work must have a terminal runtime observation. Unknown results retain
  an Incident and cannot be dispatched again under the same packet ID.
- `company.reclaim_expired.v1` is a deterministic scan command, with optional `project_id` in
  its arguments. It processes at most 128 expired claims and returns per-packet decisions.

Claims expire after five minutes (or the earlier packet deadline). Dispatch also claims unclaimed
work. Continue and capability admission renew live claims at 15-second intervals; an expired
claim is never silently resurrected. The scan is explicit; an external caller can schedule it through
the existing daemon command route.

`approve_packet` and `rework_packet` create `PacketHandoff` records for planning → executing.
`acknowledge_handoff {handoff_id, accept, reason}` requires the recipient role and a session separate
from the sender. Handoff IDs are `packet:<packet_id>`. `start_run` also records a typed positive ACK
when the receiving Builder dispatches the frozen packet. A negative ACK blocks dispatch.

`review_packet {packet_id, review_id, criterion_results, evidence_refs}` records a review of a
completed packet with the exact frozen criteria and persisted runtime evidence. Its Reviewer
session must differ from the Builder. Packet reviews support swarm merge; final project Acceptance
still has its separate frozen criteria, review and closing guards.

## Budgets

Sponsor can submit `configure_budget {project_id, policy}` before any project Run is reserved.
The project ID here must be a UUID and match `policy.project.project_id`; the quota scope must
be the same project ID. Example policy:

```json
{
  "project": {"project_id":"72739e1a-176d-4d2f-8a8b-a94f0ff77b3d","max_runs":8,"max_tokens":524288},
  "runtime": {"max_model_calls":16,"max_tokens":65536,"max_wall_time_ms":300000},
  "quota": {"scope":"72739e1a-176d-4d2f-8a8b-a94f0ff77b3d","model_calls":128,"tokens":524288,"concurrency":4}
}
```

Company CAS reserves maximum per-run allowances, including failed or uncertain dispatches. They
are reservations, not a financial spend estimate. `ProjectBudget` and `Quota` enforce project
admission; `RuntimeBudget` feeds the actual Cell `BudgetLease`, model step limit and grant deadline.
Unconfigured ordinary packet runs retain the existing template limits. Swarms require an explicit
budget approval. Runtime token/call ceilings do not imply complete provider billing measurements.

## Workflow

`workflow.command.v1` accepts:

```json
{
  "schema":"kiana.workflow-command.v1",
  "expected_revision":0,
  "idempotency_key":"register-summary-v1",
  "command":{
    "type":"register_definition",
    "definition":{
      "definition_id":"summary","version":1,
      "input_keys":[],"output_keys":["summary"],"allowed_roles":["builder"],
      "max_duration_ms":300000,"max_steps":8,
      "nodes":{
        "summarize":{
          "dependencies":[],"timeout_ms":300000,"retry_limit":0,"compensation":null,
          "kind":{"type":"literal","values":{"summary":"Recorded deterministic output"}}
        }
      },
      "artifacts":{}
    }
  }
}
```

Definitions are registered by PM/Sponsor/Architect and are immutable by `(definition_id, version)`.
`start {instance_id, definition_id, version, inputs}` creates an instance under one of its allowed
roles. `advance {instance_id}` chooses a ready node deterministically and persists its reservation
before execution. Nodes are bounded by graph size, instance duration, step count, attempts and
lease. Each node records input fingerprint, execution ID, session, output, errors and evidence.

Supported node kinds are `literal`, `copy_input`, `gate`, `agent_task`, `capability`, `approval`,
`wait_signal`, `fan_out`, `fan_in` and `sub_workflow`. `AgentTask` contains a frozen Company
`project_id`/`packet_id` and uses Company `StartRun`. `Capability` contains an ordinary
`CapabilityRequest`; its request ID is rebound by the core and it passes through the existing
policy/gate/approval/Broker path. A workflow template never gives a PM the Builder role.

Within one instance effects are advanced one at a time. Fan-out/fan-in are graph dependency nodes;
bounded concurrent Cell execution uses the swarm route below. Subworkflows reference previously
registered definitions, so definition recursion cannot be introduced. Their instances are advanced
through the same command route and their parent reads the result with `reconcile`.

Additional commands:

- `reconcile {instance_id, node_id}` reads persisted runtime or child-workflow facts, without
  repeating execution. An expired or unobserved effect becomes `result_unknown`.
- `decide {instance_id, node_id, approve, evidence_ref}` resolves a workflow Approval node with
  the designated role, independent session, unexpired node and owned event reference. Capability
  approvals continue to use the existing approval API and request proof.
- `signal {instance_id, signal, value, evidence_ref}` supplies one declared signal exactly once.
- `pause`, `resume`, `cancel {instance_id, reason}` gate future execution. Cancellation propagates
  to subworkflows, cancels active Company runs and denies staged capability approvals. Nodes still
  require runtime reconciliation before the instance can claim they stopped.
- `retry {instance_id, node_id}` permits only declared, bounded retries of pure failed nodes.
  Effects and unknown outcomes require a new reviewed contract.
- `compensate {instance_id, compensation_instance_id}` creates a separate instance containing
  the declared compensation nodes for successful nodes of a known failed/cancelled instance.
  The original result remains in history, and every compensation effect is authorized again.

`artifacts` is an optional artifact graph: each artifact declares `generated_by`, `requires` and
`output_key`. Graph validation requires matching node dependency edges, and successful completion
requires the declared output keys. This is an output contract, not proof that a physical file exists.
`workflow.snapshot.v1` reconstructs state from the EventStore. Replaying a command ID never repeats
an effect; after a process stops between reservation and dispatch, use reconciliation.

## Triggers

Workflow commands also support `register_trigger`, `disable_trigger`, `fire` and `tick`.
A trigger fixes the definition version, inputs, owner, execution role, expiry, maximum firings,
approval reference and schedule. Registration requires a Company project approval or a positive
workflow approval event owned by the same principal/workspace. Firing requires the frozen execution
role; a scheduler cannot change roles to satisfy a template.

Schedules are `manual`, `event {kind}` and `interval {every_ms, first_at}`. Intervals have a minimum
of one second. Event firing requires an owned persisted event of the configured kind. A stable
`firing_key` makes manual/event firings idempotent.

Concurrency policies are `reject`, `queue`, `replace` and `coalesce`. Replace requests cancellation
and waits for the old instance's terminal facts before starting a successor. Interval missed-run
policies are `skip`, `fire_once` and `catch_up`; catch-up is bounded to 32 firings per tick and queues
are bounded to 128. Definitions, next due time, queues, firing keys and budget use are durable facts.
`tick {trigger_id}` is explicit through DaemonHost, and creates workflow instances; those instances
advance through the same workflow API. This slice does not introduce a background principal or
an autonomous provider session.

## Bounded swarm

`swarm.command.v1` uses `schema: "kiana.swarm-command.v1"`, `expected_revision`, an
`idempotency_key`, and a typed command. `swarm.snapshot.v1` reconstructs the aggregate.

1. PM/Sponsor calls `create {plan}`. Plan fields are `swarm_id`, `project_id`, `packet_ids`,
   `reason_code`, `max_concurrency`, `max_depth`, `expires_at`, `max_tokens`, `max_model_calls`,
   `merge_strategy: "receipt_only"`, and `approval_ref` pointing to the project's budget approval.
   Limits are eight distinct packets, eight simultaneous children, depth exactly one, and five minutes.
2. Builder calls `start_child {swarm_id, packet_id, sandbox}` for each ready partition. The core
   generates the child session and reuses Company dispatch. Concurrent callers are serialized by CAS
   for admission; the configured maximum still bounds active children.
3. `reconcile {swarm_id}` folds persisted child outcomes. Failures have typed `ChildFailureReport`
   entries. Failed/unknown children block new siblings, and active sibling cancellation is requested.
4. Independent Reviewers record `review_packet` decisions through `company.command.v1`.
   Reviewer/Closer calls `merge {swarm_id, decisions}` with one accepted/rejected decision per
   partition, explicit reasons and review IDs for accepted outputs. Unknown outputs cannot be accepted.
5. `cancel {swarm_id, reason}` requests cancellation; reconciliation must confirm the result.

The controller is a Cell resource reservation with no model session and no owned write paths.
Its fixed `swarm-controller.v1` template permits only one delegation level. Children use fixed
`swarm-child.v1`, cannot delegate, share the controller BudgetLease, and pass the existing grant
containment and path-lock admission. Partitions must have disjoint write sets. Merge records output
acceptance; it never runs a Git merge or modifies artifacts outside those already executed by the
children. Registry snapshot/restore integration remains the existing runtime recovery responsibility.
