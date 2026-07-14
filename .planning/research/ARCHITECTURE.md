# Architecture Research

**Domain:** Local-first open-core AI agent platform with Coding, Academic Research, and Daily Work packs
**Researched:** 2026-07-14
**Confidence:** HIGH for the current Kiana boundaries and migration seams; MEDIUM for reference-derived cloud scaling patterns

## Executive Recommendation

Keep the existing layered Rust workspace and evolve it into a **ports-and-adapters core around one durable execution model**. Do not build a second runtime for Desktop, Web, cloud, enterprise, Research, or Daily Work. The safe path is three focused extractions from code that already exists:

1. Extract the assistant loop from `kiana-entrypoints/src/runner.rs` into a reusable `kiana-runtime` crate.
2. Extract session persistence and backend interfaces from `kiana-entrypoints/src/sdk.rs` into a `kiana-state` crate while preserving the current JSON/JSONL format through compatibility adapters.
3. Extract policy evaluation from tool-specific enforcement into a low-level `kiana-policy` crate, leaving enforcement at every side-effect boundary.

Everything else should evolve in place first. In particular, retain `kiana-types` as the shared contract layer, `kiana-tasks` as the durable workflow/evidence/recovery domain, `kiana-services` as the provider and external-service adapter layer, `kiana-tools` as tool implementations, `kiana-query` as context/retrieval, and `kiana-skills` as extension discovery. Introduce concrete domain-pack crates only when a real vertical slice exists; do not create empty pack shells.

The state rule is strict:

- A **session event log** is authoritative for conversation/turn history.
- A **workflow EventLog** is authoritative for workflow, task, WorkPacket, evidence, approval, side-effect, verification, and recovery state.
- `state.json`, legacy session JSON, task boards, SQLite/Postgres indexes, UI stores, and cloud read models are rebuildable projections, never competing authorities.
- A simple read-only answer may remain session-only. The first durable task, tool mutation, multi-agent dispatch, external side effect, or cross-session operation creates or attaches an implicit `WorkflowRun`.

This preserves local-first operation while giving local, cloud, and enterprise deployments the same domain semantics.

## Target System Overview

```text
Thin product surfaces
CLI/TUI | Headless SDK/RPC/MCP | IDE | Desktop | Web/App Server
                              |
                    Typed commands + event cursor
                              v
+------------------------------------------------------------------+
| Runtime host port                                                |
| InProcessHost | LocalDaemonHost | RemoteWorkerHost                |
+-------------------------------+----------------------------------+
                                |
                                v
+------------------------------------------------------------------+
| kiana-runtime                                                    |
| session/turn orchestration | provider gateway | context builder   |
| tool dispatch | workflow attach | event publication | recovery    |
+----------+----------------+----------------+----------------------+
           |                |                |
           v                v                v
+----------------+ +----------------+ +----------------------------+
| kiana-policy   | | kiana-state    | | Existing capability layer  |
| decisions only | | logs, repos,   | | tools/services/query/skills|
| no side effects| | projections    | | tasks/evidence/integrity   |
+-------+--------+ +--------+-------+ +-------------+--------------+
        |                   |                       |
        +-------------------+-----------------------+
                            |
                            v
+------------------------------------------------------------------+
| Domain pack contributions                                       |
| Coding adapter | Research Pack | Daily Pack                      |
| tools, workflow templates, object schemas, verifiers, UI hints   |
+------------------------------------------------------------------+
                            |
               local files / SQLite projection
                   OR hosted durable adapters
                            |
+---------------------------+--------------------------------------+
| Optional control planes                                         |
| Official Cloud: sync, workers, teams, billing                    |
| Enterprise: SSO/RBAC, managed policy, audit, retention, ops      |
+------------------------------------------------------------------+
```

### Enforceable Dependency Direction

```text
kiana-types + kiana-constants
    -> kiana-state + kiana-policy
    -> kiana-services + kiana-query + kiana-tasks
    -> kiana-skills + kiana-tools
    -> kiana-runtime
    -> domain packs + kiana-commands
    -> kiana-remote + kiana-bridge
    -> kiana-entrypoints + screens/components + native integrations
    -> cloud/enterprise composition roots
```

Rules:

- No crate below `kiana-runtime` may depend on `kiana-entrypoints`, a UI crate, or a concrete cloud service.
- `kiana-runtime` accepts registries, stores, policy engines, clocks, ID generators, and event sinks through explicit ports.
- Domain packs depend on stable core contracts and contribute capabilities through registries. Core never depends on a concrete pack.
- Cloud and enterprise code implement the same store, runtime-host, secret, scheduler, and audit ports; they must not fork task or event semantics.
- Surface-specific DTOs are adapters around `kiana-types` schemas. A surface cannot add a second session or workflow state machine.

## Component Boundaries

| Component | Owns | Does Not Own | Current Evolution Path |
|---|---|---|---|
| `kiana-types` | Stable IDs, event envelopes/payloads, commands, provider capabilities, policy inputs/decisions, pack manifests, error codes | Filesystem I/O, provider calls, policy evaluation | Extend `RuntimeEvent` compatibly; add v2 envelopes and typed payloads before consumers |
| `kiana-state` **new** | `SessionRepository`, generic append/read event-store ports, projection/index ports, local JSONL/SQLite adapters, schema migration, optimistic concurrency | Workflow rules, tool execution, UI stores | Move persistence code out of `sdk.rs`; retain legacy JSON as a projection/read adapter |
| `kiana-policy` **new** | Deterministic policy merge/evaluation, risk classes, autonomy profile, tenant/workspace hard policy, structured decisions | Prompt UI, filesystem mutation, shell/network execution | Extract evaluation from `kiana-tools`; reuse `ProjectTrust` and permission types |
| `kiana-runtime` **new** | Turn lifecycle, provider selection, capability negotiation, context snapshots, tool loop, workflow attachment, event emission, retry budgets, cancellation | CLI parsing, HTML/TUI rendering, concrete account/billing logic | Move behavior incrementally from `runner.rs`; keep old public functions as wrappers |
| `kiana-tasks` | Workflow DAG, task/WorkPacket lifecycle, workflow EventLog, evidence, verification, review, integrity, recovery | Session transcript rendering, provider transport, surface state | Keep and strengthen; consume the state event-store port without weakening current integrity |
| `kiana-tools` | Tool implementations, schemas, workbench metadata, side-effect execution, path guards | Final policy authority, session persistence, workflow projection | Delegate decisions to `kiana-policy`; return typed receipts and side-effect classifications |
| `kiana-services` | Provider adapters, auth clients, MCP/LSP/network clients, network safety helpers | Turn state, UI state, task completion claims | Expand provider adapters and capability negotiation; keep standard adapter tests |
| `kiana-query` | Context acquisition, repo map, retrieval/indexing, context artifacts, context projections | Durable task/workflow status, policy authority | Add provenance-bearing `ContextItem`; keep indexes rebuildable |
| `kiana-skills` | Trust-aware extension discovery, parsing, cache snapshots, provenance | Core workflow state or direct execution authority | Return immutable capability snapshots per run; runtime composes them |
| `kiana-bootstrap` | Config source loading and precedence, construction of typed startup snapshots | Mutable process-wide runtime truth | Emit typed `RuntimeConfigSnapshot` and `PolicySnapshot`; reduce new globals |
| `kiana-commands` | Human/automation command parsing and presentation-neutral command results | Business-state mutation outside core APIs | Remain thin wrappers over runtime/tasks/state |
| `kiana-remote` | Remote worker client, worker lifecycle, bundle/environment adapters | A cloud-only event model | Implement `RemoteWorkerHost` and scoped worker lease/capability contracts |
| `kiana-bridge` | Transport translation and compatibility | Independent task/session schema | Translate typed commands/events only; delete duplicate semantics over time |
| Entrypoints/UI/native crates | Protocol termination, input, rendering, OS integration | Provider loop, permission rules, session/workflow authority | Consume snapshots plus cursor-based event subscriptions |
| Domain pack crates | Domain tools, workflow templates, object schemas, verifiers, eval fixtures, presentation hints | Runtime/store/policy forks | Coding starts as an adapter over current modules; create Research/Daily crates with first real slices |
| `kiana-control-plane` **new later** | Tenant/workspace registry, RBAC binding, leases, scheduler, sync coordination, quotas, billing/entitlement adapters | Agent semantics or local personal requirements | Add only after local RuntimeHost and repository contracts pass parity tests |

## Recommended Workspace Evolution

```text
kiana-types/                   # extend: shared wire/domain contracts
kiana-state/                   # new: repositories, event stores, projections
kiana-policy/                  # new: pure policy evaluation
kiana-runtime/                 # new: reusable agent/runtime orchestration
kiana-tasks/                   # evolve: workflow/task/evidence authority
kiana-services/                # evolve: providers and external adapters
kiana-tools/                   # evolve: typed tool receipts and execution
kiana-query/                   # evolve: provenance-bearing context
kiana-skills/                  # evolve: immutable extension snapshots
kiana-pack-coding/             # add after contract: adapters over existing coding capabilities
kiana-pack-research/           # add with first literature/evidence vertical slice
kiana-pack-daily/              # add with first connector/receipt vertical slice
kiana-control-plane/           # later hosted composition, not required locally
kiana-entrypoints/             # shrink to composition and surface adapters
```

Do not create `kiana-protocol` yet. `kiana-types` already occupies that boundary. Split it only if public protocol versioning and internal domain types demonstrably conflict.

## State Ownership And One Source Of Truth

### Aggregate Ownership

| Aggregate | Authoritative facts | Projection/cache | Owner |
|---|---|---|---|
| Session | Session/turn/message/tool/permission/result events | Legacy `<session>.json`, conversation list, transcript view, SQLite index | `kiana-state` |
| WorkflowRun | Workflow, task, WorkPacket, evidence, verification, approval, recovery, side-effect events | `state.json`, board/status reports, cloud read models | `kiana-tasks` over `kiana-state` port |
| Artifact | Immutable bytes plus content hash and workflow descriptor | Search/vector indexes, thumbnails, summaries | `kiana-tasks`/artifact store adapter |
| Memory | Append-only memory records with provenance and retention metadata | Search/vector index | Future memory module over `kiana-state`; never current task authority |
| Policy | Versioned resolved policy snapshot used for a decision | UI-friendly policy status | `kiana-policy`; source config remains in bootstrap/managed store |
| Provider catalog | Verified/configured capability observations with source and timestamp | Model-picker list | `kiana-services`; runtime consumes a frozen per-turn snapshot |

### Session And Workflow Relationship

- `SessionId` identifies conversation continuity; `WorkflowRunId` identifies durable execution continuity.
- Every workflow may reference its initiating session and turn. Later turns/surfaces attach through explicit `WorkflowAttachedToSession` events.
- Workflow/task status is never inferred from assistant prose or the last session message.
- A standalone task list becomes a backlog/projection. Once executed, its status changes must be workflow events, not direct task-file edits.
- Evidence remains embedded in the authenticated workflow EventLog, as current `kiana-tasks` already does.
- Cross-aggregate links use idempotent commands and correlation IDs. Do not attempt fragile dual writes followed by last-write-wins repair.

### Local Storage Layout

```text
$KIANA_HOME/state/
  sessions/<session-id>/events.jsonl       # authoritative session facts
  sessions/<session-id>/snapshot.json      # rebuildable projection
  index/state.sqlite                       # lists/search/cursors only

<workspace>/.kiana/workflows/<run-id>/
  eventlog.jsonl                           # authoritative workflow/task facts
  state.json                               # rebuildable projection
  artifacts/...                            # hash-bound immutable artifacts
```

Existing paths should be migrated in place or discovered through an adapter. Do not silently relocate data during the runtime extraction.

Hosted adapters may use Postgres/event tables and object storage, but must preserve per-aggregate sequence, idempotency, integrity, and replay behavior.

## Event Contracts

### Common Envelope

Evolve `RuntimeEvent` and `WorkflowEvent` toward a shared envelope while retaining v1 readers:

```rust
struct EventEnvelope<P> {
    schema: String,              // kiana.event-envelope.v1
    event_id: EventId,
    aggregate_kind: AggregateKind,
    aggregate_id: String,
    sequence: u64,               // monotonic within aggregate
    occurred_at: Timestamp,
    workspace_id: WorkspaceId,
    session_id: Option<SessionId>,
    workflow_run_id: Option<WorkflowRunId>,
    turn_id: Option<TurnId>,
    task_id: Option<TaskId>,
    actor: ActorRef,
    origin_surface: SurfaceKind,
    correlation_id: CorrelationId,
    causation_id: Option<EventId>,
    idempotency_key: Option<String>,
    durability: EventDurability,
    payload: P,
    integrity: Option<IntegrityEnvelope>,
}
```

Hosted envelopes add a server-derived tenant scope. Local personal mode uses a device-local workspace namespace and requires no account. Tenant identity must never be trusted from an arbitrary client payload.

### Payload Families

| Family | Required payloads |
|---|---|
| Session/turn | `session_created`, `session_forked`, `turn_started`, `message_committed`, `turn_completed`, `turn_interrupted`, `session_archived` |
| Provider | `provider_selected`, `capability_degraded`, `provider_attempt_started`, `provider_attempt_failed`, `usage_recorded` |
| Tool | `tool_requested`, `tool_validated`, `tool_started`, `tool_completed`, `tool_failed` |
| Policy/approval | `policy_evaluated`, `approval_requested`, `approval_resolved`, `approval_expired` |
| Workflow/task | Existing workflow events plus typed `task_created`, `task_claimed`, `task_blocked`, `task_completed`, `workpacket_dispatched`, `workpacket_returned` |
| Evidence/verification | `evidence_recorded`, `verification_started`, `verification_passed`, `verification_failed`, `review_completed` |
| External side effect | `side_effect_prepared`, `side_effect_attempted`, `side_effect_acknowledged`, `side_effect_result_unknown`, `side_effect_compensated` |
| Recovery | `retry_scheduled`, `lease_expired`, `projection_rebuilt`, `recovery_blocked`, existing swarm recovery events |

Replace open-ended `Value` fields with typed payloads at security and state-transition boundaries. Keep `Value` only for provider-specific metadata, opaque tool content, and versioned extension payloads.

### Durable Versus Transient Events

- Persist state transitions, complete messages, tool requests/results, approvals, receipts, usage totals, errors, and terminal results.
- Stream token/reasoning deltas as transient events. Coalesce or omit them from durable logs unless a debug retention policy explicitly enables storage.
- Every subscriber receives a monotonically increasing durable cursor. On reconnect it requests a snapshot plus events after that cursor.
- If a transient buffer overflows, emit `resync_required`; never block the authoritative runtime indefinitely on a slow UI.
- A terminal event must contain enough data to reconstruct correct UI state without replaying transient deltas.

## Core Ports And Extension Contracts

### RuntimeHost

```rust
trait RuntimeHost {
    async fn start_or_attach(&self, command: StartCommand) -> Result<RunHandle>;
    async fn submit(&self, command: RuntimeCommand) -> Result<CommandAck>;
    async fn snapshot(&self, query: SnapshotQuery) -> Result<RuntimeSnapshot>;
    async fn subscribe(&self, cursor: EventCursor) -> Result<EventStream>;
}
```

Implementations:

- `InProcessHost`: default local-first path, no daemon or account.
- `LocalDaemonHost`: optional cross-surface/background continuity on one machine.
- `RemoteWorkerHost`: scoped remote execution through `kiana-remote`.
- Cloud/enterprise hosts: same commands/events, different repositories and scheduling.

### DomainPack

```rust
trait DomainPack {
    fn manifest(&self) -> DomainPackManifest;
    fn contribute_tools(&self, registry: &mut ToolRegistryBuilder);
    fn contribute_workflows(&self, registry: &mut WorkflowTemplateRegistry);
    fn contribute_verifiers(&self, registry: &mut VerifierRegistry);
    fn object_schemas(&self) -> &[SchemaDescriptor];
    fn presentation_hints(&self) -> &[PresentationHint];
}
```

The manifest must declare pack ID/version, core contract range, required capabilities, tools, workflow schemas, policy categories, data migrations, provenance/license, and integrity digest. Packs cannot write session/workflow files directly or register a second event bus.

### Provider Contract

Evolve the current `Provider` trait rather than replacing it. Add:

- Capability states `native | emulated | unsupported | unknown`, not booleans alone.
- Tool calling, parallel tool calls, vision/audio/files, structured output dialect, reasoning controls, streaming mode, context/output limits, cache behavior, and data-residency flags.
- Capability source and observed timestamp.
- Explicit `CapabilityPlan` selected before a turn: route, emulate, degrade with visible event, or reject.
- Attempt IDs, retry classification, usage/cost records, and normalized error codes.
- A standard provider contract suite modeled on the existing fake/provider-standard tests.

Gemini and OpenRouter should be first-class adapters. OpenRouter is not merely another model ID under an undifferentiated OpenAI-compatible path because routing, upstream attribution, model catalog, and capability drift must remain visible.

### Tool/Connector Contract

Extend the existing `Tool` trait with:

- `side_effect_class`: read-only, local reversible, local irreversible, external reversible, external irreversible.
- `idempotency`: inherently idempotent, key-required, query-before-retry, never-auto-retry.
- Required scopes and secret references.
- Typed `ToolReceipt` containing attempt ID, target identity, changed resources, provider receipt, verification query, compensation option, and result certainty.
- Explicit isolation requirement and concurrency key.

Connectors are lifecycle-managed tool/workbench providers. Secrets are resolved at call time from Keychain/vault handles and never copied into ordinary runtime context or events.

## Data And Control Flows

### Local Interactive Turn

```text
Surface command
  -> RuntimeHost validates command/idempotency key
  -> SessionRepository appends turn_started
  -> Runtime snapshots config, policy, provider capabilities, packs, context
  -> Provider gateway streams transient deltas
  -> Tool request -> schema validation -> policy -> approval if needed
  -> Tool executes -> typed receipt/evidence -> durable events
  -> Verifier evaluates explicit acceptance for attached workflow
  -> terminal event appended
  -> projections update
  -> all attached surfaces render the same event stream
```

### Durable Workflow

```text
Intent/router decision
  -> create/attach WorkflowRun
  -> persist DAG + acceptance criteria + budgets
  -> append task/WorkPacket events
  -> claim work under lease and scoped policy snapshot
  -> execute through the same RuntimeHost
  -> append evidence and verification
  -> complete | rework | blocked | awaiting_approval | result_unknown
```

No command or UI may set workflow/task status directly. It submits a command; `kiana-tasks` validates the transition and appends the fact.

### External Side Effect

```text
prepare target + normalized request + idempotency key
  -> policy decision
  -> durable approval (when required)
  -> side_effect_attempted
  -> connector call
     -> confirmed receipt: side_effect_acknowledged
     -> confirmed failure: tool_failed/retry classification
     -> transport lost after dispatch: side_effect_result_unknown
  -> query external system or require human reconciliation
```

`result_unknown` is terminal for automatic replay. A retry is allowed only after a verification query proves the original action did not occur, or a human explicitly resolves it.

### Multi-Agent Execution

1. Planner persists `WorkPacket` before launch: goal, inputs, allowed paths/resources, policy scope, budgets, dependencies, acceptance schema, and output schema.
2. Scheduler grants a lease and an ephemeral scoped capability token. A worker never receives tenant-wide database or vault credentials.
3. Local code workers use isolated worktrees/snapshots and path locks. Non-code workers use resource-specific concurrency keys.
4. Worker emits events correlated to the parent workflow and returns a `ResultPacket` plus evidence, never direct parent-state mutation.
5. Integrator verifies packet integrity, scope, baseline, conflicts, and acceptance before applying results.
6. Parent completion runs whole-workflow verification again.

### Cross-Surface Continuity

- A surface attaches to a session/workflow with a durable cursor; it does not own execution.
- Approval requests are durable records with request ID, scope, expiry, and resolution. First valid resolution wins; all surfaces observe the result.
- Client disconnect does not imply cancellation. Explicit cancel/interrupt is a command.
- IDE/Desktop/Web render projections from the same events and submit typed commands back through `RuntimeHost`.
- Local in-process CLI can stop with the process; background/cross-surface continuity uses the optional local daemon, not cloud.

## Local, Cloud, And Enterprise Split

| Concern | Local Personal | Official Cloud | Enterprise Self-hosted |
|---|---|---|---|
| Account | None required | Required only for opted-in cloud resources | Organization identity/SSO |
| Runtime | In-process or optional local daemon | Remote worker pools plus optional local execution | Customer-controlled worker pools/local execution |
| Authority | Local event logs | Hosted event store for cloud-owned runs; synced local runs remain locally usable | Customer-hosted event store |
| Storage | Files + SQLite projection | Postgres/event table + object store + encrypted sync | Same contracts with customer DB/object store |
| Policy | User/project hard policy | User + workspace + service policy | Managed organization policy dominates lower scopes |
| Secrets | OS Keychain/credential helper | Per-tenant vault | Customer vault/HSM/KMS adapters |
| Collaboration | Local surfaces | Team spaces, participants, comments, approvals | RBAC, groups, audit, retention, legal hold |
| Operations | Local doctor/backup/export | Multi-region backup, worker health, quotas, billing | Offline install, upgrade/rollback, diagnostics, DR |

Sync is an adapter, not runtime authority. Local mode must continue when cloud is unavailable. Sync exchanges encrypted, versioned events/artifacts and explicit tombstones; it does not copy secrets or silently enable remote execution.

## Trust Boundaries And Tenant Isolation

### Trust Boundaries

1. **Surface -> RuntimeHost:** authenticate transport where present; validate schema, size, IDs, cursors, and idempotency keys.
2. **Project content -> Runtime:** preserve current `ProjectTrust` fail-closed behavior before loading project skills, plugins, hooks, MCP, rules, or mutating tools.
3. **Runtime -> Provider:** redact secrets, enforce egress policy, freeze capability/config snapshot, bound prompt/context.
4. **Runtime -> Tool/Connector:** validate schema, policy, approval, path/resource scope, isolation, timeout, and receipt.
5. **Control plane -> Worker:** short-lived scoped lease/capability token; signed WorkPacket; no ambient tenant authority.
6. **Extension -> Core:** manifest/version/integrity/trust checks; capability snapshot; sandbox or subprocess for untrusted code.
7. **Sync -> Local store:** authenticated encrypted envelope, replay protection, schema migration, conflict/integrity checks.

### Tenant Isolation Requirements

- Every hosted key/query uses `(tenant_id, workspace_id, aggregate_kind, aggregate_id)`; no repository method accepts a bare aggregate ID.
- Derive `tenant_id` from authenticated server context, never from request JSON.
- Enforce isolation in both repository filters and database row-level policy where available.
- Prefix object-store paths, queues, caches, metrics, and vault records by tenant; encrypt with tenant- or organization-scoped keys.
- Apply quotas at tenant, workspace, workflow, worker, provider, and connector levels.
- Audit actor, policy version, origin, target, approval, attempt, and receipt without logging secret values.
- Add negative cross-tenant tests for every list/read/update/stream/artifact endpoint.
- Enterprise hard denies are monotonic: lower scopes can narrow access but cannot override an organization deny.

## Failure And Recovery Semantics

| Failure | Required behavior |
|---|---|
| Event append succeeds, projection write fails | Return committed-with-recovery status; rebuild projection from log under writer lease |
| Event append fails | Transition did not occur; do not update projection or report success |
| Projection/index corrupt | Rebuild from authoritative log; index never legalizes an invalid log |
| Event log/integrity invalid | `blocked`; require trusted restore/repair, never best-effort continuation |
| Provider transient error before side effect | Retry within explicit budget/backoff using attempt IDs |
| Provider capability mismatch | Route/degrade/refuse before call and emit visible capability event |
| Local reversible tool fails | Preserve before/after evidence; compensate or block according to receipt |
| External response lost after dispatch | `result_unknown`; query/reconcile, never blind replay |
| Worker heartbeat lost | Expire lease; reassign only when side-effect policy proves replay safety |
| Worker returns after lease expiry | Store as late result; never integrate without a new validation/lease decision |
| Client stream disconnects | Runtime continues; reconnect via cursor/snapshot |
| Slow subscriber | Drop/coalesce transient events; issue `resync_required`; retain durable events |
| Policy service/config invalid | Fail closed for side effects; emit structured blocker and remediation |
| Cloud unavailable | Local work continues; queue opted-in sync with bounded storage/backpressure |
| Sync conflict | Preserve both append-only facts; same-sequence/hash conflict becomes integrity blocker, not last-write-wins |

All commands that can be retried across process/network boundaries require an idempotency key. Store command acknowledgements so clients can safely retry after timeout without duplicating work.

## Scalability Considerations

| Scale | Architecture adjustment |
|---|---|
| Single user / local | Files remain authoritative, optional SQLite WAL index, in-process runtime, bounded channels, optional daemon for continuity |
| Small team / self-hosted | Modular control-plane process, Postgres event/projection tables, object store, durable queue, worker leases, per-tenant limits |
| 1k-100k active users | Partition events by tenant/aggregate, stateless API replicas, dedicated stream fan-out, worker pools by capability/isolation, projection consumers |
| Very large tenants | Per-tenant shards/regions, tenant-specific keys and retention, admission control, workload-class queues, audit export pipelines |

Likely first bottlenecks are provider concurrency/rate limits, tool worker capacity, event fan-out, and artifact bandwidth, not the pure Rust runtime loop. Scale these behind ports before splitting the core domain into microservices.

Keep authoritative streams per aggregate. Global ordering is unnecessary and expensive; correlation IDs provide cross-aggregate traceability.

## Migration Seams From Current Crates

| Current seam | Preserve | Change safely | Do not do |
|---|---|---|---|
| `kiana-types/src/runtime.rs` | Existing event variants and v1 readers | Add typed IDs/envelope/statuses and compatibility conversion | Break all surfaces with an unversioned replacement |
| `kiana-entrypoints/src/sdk.rs` | Public SDK functions, legacy session loading/fork/export/import | Delegate to `kiana-state`; make JSON a projection; add repository contract tests | Continue adding storage logic to the surface crate |
| `kiana-entrypoints/src/runner.rs` | Proven provider/tool/stream behavior and focused tests | Move one orchestration slice at a time behind `kiana-runtime`; wrappers preserve API | Rewrite the loop while simultaneously adding packs/cloud |
| `kiana-tasks/src/workflow.rs` | EventLog, state reconstruction, writer lease, integrity, resume states | Generalize event-store port and typed payloads; make task status event-only | Replace authenticated logs with mutable database rows |
| `kiana-tasks/src/evidence.rs` | Evidence embedded in workflow sequence and verification packets | Add Research/Daily evidence kinds through versioned registries/schemas | Let packs self-declare completion without verifier contracts |
| `kiana-tools` | Registry, validation, read-only batching, path controls, workbench metadata | Typed context, receipts, side-effect/idempotency metadata, central policy delegation | Let connectors bypass the tool/policy/evidence path |
| `kiana-services` | Provider trait, registry, fake adapter, normalized errors | Capability plans, Gemini/OpenRouter, richer standard tests | Claim boolean capability parity across providers |
| `kiana-query` | Repo/context artifacts and deterministic local indexes | Provenance/permission labels and rebuildable retrieval adapters | Make vector index authoritative memory/task state |
| `kiana-skills` | Trust-aware sources and cache invalidation | Freeze per-run extension snapshot with version/digest/provenance | Hot-load changed code into an in-flight turn invisibly |
| `kiana-remote` / `kiana-bridge` | Existing adapters and runtime event mapping | Implement RuntimeHost/worker ports and cursor replay | Maintain separate remote message semantics |
| App server/TUI | Existing schema-backed views and reducers | Subscribe/query through runtime/state ports | Persist independent conversation/workflow state |

## Recommended Build Order

```text
Contract freeze
  -> durable state/session authority
  -> policy extraction + typed execution context
  -> runtime extraction
  -> workflow/task/session linkage + side-effect semantics
  -> domain pack contract + Coding adapter
  -> Research and Daily vertical slices
  -> surface parity and local daemon
  -> remote worker protocol
  -> cloud control plane
  -> enterprise isolation/operations
  -> fault injection and 1.0 evidence
```

### Phase 1: Contract Baseline

- Add golden compatibility tests for current `RuntimeEvent`, SDK session, workflow event, evidence, permission, provider, remote, bridge, MCP, and app-server schemas.
- Define typed IDs, terminal statuses (`complete`, `rework`, `blocked`, `awaiting_approval`, `result_unknown`, `cancelled`), common error taxonomy, event durability, and idempotency semantics in `kiana-types`.
- No behavior move until old and new schema adapters pass.

### Phase 2: State Authority

- Introduce `kiana-state` repository/event-store traits and local adapters.
- Move session reads/writes/fork/import/export behind them.
- Keep legacy JSON dual-read and temporary dual-write; compare projections in tests.
- Make standalone task-list state a projection/backlog and workflow task events authoritative once execution starts.
- Add optional SQLite index only after replay/rebuild tests pass.

### Phase 3: Policy And Typed Context

- Introduce `kiana-policy` pure evaluation using existing permission/trust behavior.
- Replace new `HashMap<String, Value>` app-state keys with typed `ExecutionContext`; retain a legacy adapter during migration.
- Prove deny precedence, trust gating, autonomy profiles, network policy, and side-effect classes across CLI, MCP, remote, and app server.

### Phase 4: Runtime Extraction

- Introduce `kiana-runtime` and `InProcessHost`.
- Move provider selection, context snapshot, model/tool loop, cancellation, event publishing, and retries from `runner.rs` incrementally.
- Keep `sdk.rs` and CLI public APIs as wrappers.
- Run parity tests after each extracted slice; no UI redesign in this phase.

### Phase 5: Reliable Execution Unification

- Link sessions/turns to `WorkflowRun` for every durable operation.
- Add side-effect attempt/receipt/result-unknown events and command idempotency records.
- Make approvals durable and cross-surface.
- Extend WorkPacket leases, late-result handling, scoped capabilities, and whole-run verification.

### Phase 6: Packs

- Define `DomainPackManifest` and contribution registries.
- Wrap existing coding tools/query/checkpoint/checks/review behavior as the Coding Pack without moving all source immediately.
- Build Research and Daily as evidence-complete vertical slices, not broad tool catalogs.
- Require each pack workflow to declare acceptance criteria, evidence types, policy categories, and recovery behavior.

### Phase 7: Surfaces And Local Continuity

- Make CLI/TUI, SDK/RPC/MCP, IDE, Desktop, and Web consume `RuntimeHost` snapshots/events.
- Add optional local daemon only when in-process parity is stable.
- Migrate app-server routes from direct command/file access to repository/runtime ports.

### Phase 8: Remote, Cloud, Enterprise

- Implement signed WorkPacket, worker lease, scoped capability, cursor replay, and result packet protocols in `kiana-remote`/`kiana-bridge`.
- Add a modular `kiana-control-plane` with hosted store adapters, tenant/workspace registry, scheduling, RBAC binding, audit, quotas, and sync.
- Add enterprise SSO/policy/vault/retention/offline-operation adapters without changing core events.

### Phase 9: Release Proof

- Fault-inject append/projection crashes, worker loss, duplicate commands, stream disconnects, provider timeouts, result-unknown external writes, sync conflicts, corrupt logs, backup restore, and key rotation.
- Prove cross-surface continuity and cross-tenant denial.
- Remove legacy dual-write only after migration, rollback, and export/import evidence exists.

## Architectural Patterns To Follow

### Append-Only Facts, Rebuildable Projections

Use the existing workflow recovery model everywhere durable state matters. Event append defines the commit point. Materialized state and indexes are disposable and rebuildable.

### Stateless Loop, Stateful Orchestrator

Keep provider/tool iteration independent from persistence and host lifecycle. `kiana-runtime` coordinates snapshots and commits; the inner loop consumes an immutable turn snapshot and emits typed outcomes.

### One Host Port Across Deployment Modes

Select local/daemon/remote through `RuntimeHost`. Top-level commands and clients do not branch on deployment details.

### Ports At Volatile Boundaries

Use traits for stores, secrets, providers, worker scheduling, event fan-out, clocks, and ID generation. Keep workflow rules and policy semantics concrete and shared.

### Explicit Capability Degradation

Provider/tool/worker limitations produce typed route/degrade/reject events. Emulation is a named behavior, not silent parity.

## Anti-Patterns To Avoid

### Runtime Per Surface Or Pack

**Why it fails:** sessions, permissions, tools, and completion semantics diverge.
**Instead:** every surface and pack submits commands to `RuntimeHost` and consumes the same events.

### Dual Mutable Authorities

**Why it fails:** session JSON, task files, workflow state, app-server records, and cloud rows drift.
**Instead:** event logs are facts; every other representation is a projection.

### Cloud-First Core Dependency

**Why it fails:** local personal mode becomes unavailable without account/network/control plane.
**Instead:** hosted services implement optional ports over the same local-capable core.

### Generic JSON At Security Boundaries

**Why it fails:** unvalidated fields and string statuses create policy bypasses and migration ambiguity.
**Instead:** typed/versioned decisions, receipts, identities, statuses, and side-effect outcomes.

### Blind Retry Of External Writes

**Why it fails:** messages, payments, publishes, deletes, and permission changes duplicate.
**Instead:** idempotency key, target receipt, verification query, `result_unknown`, and explicit reconciliation.

### Premature Microservices

**Why it fails:** distributed failure modes arrive before domain contracts stabilize.
**Instead:** modular monolith/control plane first, separate workers for isolation and scale, split services from measured pressure.

### Copying Reference Product Coupling

**Why it fails:** vendor account/auth/cloud assumptions leak into Kiana Core.
**Instead:** borrow protocol and lifecycle patterns while keeping Kiana provider-neutral and local-first.

## Confidence And Reference Assessment

| Evidence area | Confidence | Assessment |
|---|---|---|
| Current Kiana crate/state boundaries | HIGH | Directly checked against live planning docs, manifests, and source |
| EventLog/evidence/recovery evolution | HIGH | Existing authenticated append-only workflow implementation and tests are concrete |
| Runtime/state/policy extraction seams | HIGH | Responsibilities are visibly concentrated in `runner.rs`, `sdk.rs`, and tool policy paths; references converge on this split |
| Codex thread/turn/item, rollout plus index pattern | HIGH | Current local source and detailed app-server protocol; vendor cloud/auth pieces are intentionally excluded |
| Cline stateless agents/stateful core/RuntimeHost pattern | HIGH | Explicit architecture source-of-truth document and concrete local/hub/remote boundaries |
| Pi JSONL tree and turn snapshot pattern | MEDIUM | JSONL implementation is concrete; harness document explicitly marks lifecycle/facade work as provisional |
| aider repo-map/git/lint-test pattern | HIGH | Mature focused implementation/docs; use for Coding Pack, not universal runtime structure |
| OpenHands service/storage/sandbox adapters | MEDIUM | Useful app-server boundary examples; Python/enterprise deployment coupling should not be ported directly |
| AutoGen distributed identity/routing | MEDIUM | Useful conceptual runtime parity; repository is in maintenance mode and older design docs contain unresolved design items |
| MetaGPT roles/SOP/artifacts | LOW-MEDIUM | Useful pack vocabulary only; source contains stubbed environment methods and should not define core reliability |
| LangChain provider contract tests/middleware | MEDIUM-HIGH | Strong integration-test pattern; model-profile package warns its API is still in development |
| Restored Claude Code source | MEDIUM | Behavior and boundary reference only; repository states it contains source-map reconstruction, shims, and degraded implementations |

## Sources

### Kiana Primary Evidence

- `.planning/PROJECT.md`
- `docs/superpowers/specs/2026-07-14-kiana-complete-ai-agent-product-design.md`
- `.planning/codebase/ARCHITECTURE.md`
- `.planning/codebase/STRUCTURE.md`
- `.planning/codebase/INTEGRATIONS.md`
- `.planning/codebase/CONCERNS.md`
- `docs/reference-migration-roadmap.md`
- `docs/reference-feature-matrix.md`
- `Cargo.toml`
- `kiana-types/src/runtime.rs`
- `kiana-types/src/permissions.rs`
- `kiana-types/src/trust.rs`
- `kiana-entrypoints/src/sdk.rs`
- `kiana-entrypoints/src/runner.rs`
- `kiana-tools/src/tool.rs`
- `kiana-tools/src/tool_execution.rs`
- `kiana-services/src/api/provider.rs`
- `kiana-tasks/src/workflow.rs`
- `kiana-tasks/src/evidence.rs`

### Representative Reference Evidence

- `reference/codex/codex-rs/app-server/README.md`
- `reference/codex/codex-rs/rollout/src/lib.rs`
- `reference/codex/codex-rs/state/src/lib.rs`
- `reference/claude-code-rev-main/README.md`
- `reference/claude-code-rev-main/src/QueryEngine.ts`
- `reference/claude-code-rev-main/src/services/tools/toolOrchestration.ts`
- `reference/claude-code-rev-main/src/utils/sessionStorage.ts`
- `reference/pi/packages/agent/docs/agent-harness.md`
- `reference/pi/packages/agent/src/harness/session/jsonl-storage.ts`
- `reference/pi/packages/agent/src/agent-loop.ts`
- `reference/aider/aider/website/docs/repomap.md`
- `reference/aider/aider/website/docs/usage/lint-test.md`
- `reference/cline/sdk/ARCHITECTURE.md`
- `reference/cline/docs/sdk/architecture/hub-spoke.mdx`
- `reference/OpenHands/openhands/app_server/README.md`
- `reference/OpenHands/openhands/app_server/event/README.md`
- `reference/OpenHands/openhands/app_server/sandbox/README.md`
- `reference/autogen/README.md`
- `reference/autogen/python/docs/src/user-guide/core-user-guide/core-concepts/architecture.md`
- `reference/autogen/docs/design/03 - Agent Worker Protocol.md`
- `reference/MetaGPT/metagpt/team.py`
- `reference/MetaGPT/metagpt/roles/role.py`
- `reference/MetaGPT/metagpt/environment/base_env.py`
- `reference/langchain/libs/standard-tests/README.md`
- `reference/langchain/libs/model-profiles/README.md`
- `reference/langchain/libs/langchain_v1/langchain/agents/middleware/types.py`

---
*Architecture research for Kiana's complete local/cloud/enterprise AI agent platform.*
*Researched: 2026-07-14*
