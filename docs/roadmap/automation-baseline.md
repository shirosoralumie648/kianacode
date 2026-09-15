# AUT-01 Automation / Workflow source baseline

> 快照日期：2026-09-16。本文是 `AUT-01` 的 source-only reconciliation，不是 durable
> scheduler、live timer 或 workflow runtime 的完成声明。本轮不在本地运行测试；
> `automation_baseline` 只由 GitHub Actions 执行。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`AUT-01`](../roadmap.md#step-aut-01) |
| source snapshot | `2d31b8d`（OA-28 已推送的基线） |
| proof ceiling | `source`；源码 guard/test-target 编译不提升 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical execution spine | `entrypoint → kiana-client/protocol → DaemonHost → ControlPlane → policy/gate/approval → workflow planner/commit → Broker/Runner → EventLog/Receipt` |
| this step does | inventory、旧兼容边界、唯一 scheduler 迁移护栏、fixture 命名和后续 AUT-02..24 handoff |
| this step does not | 不新增 ClockPort、scheduler worker、trigger、queue、workflow definition、lease/fence、第二执行循环或外部 timer |

## 2. Source hashes

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Workflow domain contracts | `kiana-domain/src/automation.rs` | `7839b8e114cbd68789908b01912ba3cc5788011545cc2ef06db19a1b3a09bd7b` |
| Workflow pure planner | `kiana-workflow/src/durable.rs` | `11691944613762c8a782f37b5ebce2ba9ce24d92a1a189f475f3acb776b7ac9c` |
| Workflow ControlPlane adapter | `kiana-core/src/automation.rs` | `806831b7077e11d918aa78de5e73ff453535a143fe917d63e1be522dde9bd853` |
| Daemon composition/routing | `kiana-daemon/src/lib.rs` | `bb458a97ab2c21645b295beb997cf0c3dfb5a42f4d64c4202040cc4f93bee9b2` |
| Protocol workflow route | `kiana-protocol/src/lib.rs` | `ab36f25cce3e6e52c617b6d845d6aa80a3e769900ee5997a1e98bd1c7aa16c2a` |
| Existing compatibility scheduler surface | `kiana-entrypoints/src/sdk.rs` | `be09a6c212f0e266cca9e3ef946fbc2f4eff60d1c0eb53aa52b8cfee6ba86e35` |
| Existing workflow tests | `kiana-workflow/tests/state_matrix.rs` | `1683696e7a43bfcf9d2781817d70f6c4683f6a54cdd3a4cc34f162c24dc04d95` |
| AUT-01 source guard/workflow | `kiana-core/tests/automation_baseline.rs`, `.github/workflows/aut01-baseline.yml` | `046733513525be2fd3b12d702cdcf52e170e581661275fab33780388c2ad2304`, `e3af1c5cf70d442696d7a46426eef4a59cb417c3afbbb76468b7ca0e30488cad` |

The hash rows for pre-existing files are a review anchor and must be refreshed whenever a later AUT
step touches the file. The AUT-01 guard/workflow row is bound to this commit's formatted source.

## 3. Current automation inventory

| 分类 | 当前 source facts | proof / limitation |
|---|---|---|
| Domain | `WorkflowDefinition`/`WorkflowNode`/`WorkflowInstance`/`TriggerDefinition`/`DurableTrigger` and typed `AutomationCommand` exist; schemas, role/project/budget/deadline/node and trigger bounds are represented | `source`；a type does not create a timer, queue claim or execution permit |
| Pure planner | `kiana-workflow::durable::plan_command` validates definitions, DAG, trigger schedule, dependency/parent state, approval/signal/lease/deadline and returns next state plus optional `WorkflowEffect` | `source`；planner has no I/O/random/current clock and cannot call Broker/Runner, but its outputs are not a durable command until ControlPlane commits |
| ControlPlane | `kiana-core::automation` loads `workflow` aggregate, checks actor/trust/role/revision/idempotency, commits `workflow.command_applied`, and only then routes `AgentTask` to Company StartRun or `Capability` to `authorize_and_execute`; observation/reconcile is separate | `source` / process-local；no dedicated scheduler service, ClockPort, durable queue/claim/fence or cross-process worker |
| Daemon | `DaemonHost` routes `workflow.command.v1` through the same `ControlPlane`; no `tokio` scheduler/worker is present in product composition | `source`；AUT-09 will add a bounded daemon service, never direct handler access |
| Existing tests | `kiana-workflow/tests/state_matrix.rs` covers the small generic state machine; current automation path has no dedicated cross-crate runtime baseline test before this guard | `source`；deny/expiry/replay/TOCTOU/unknown/restart/claim behavior remains AUT-02+ |
| Legacy scheduler | `kiana-entrypoints::sdk::watch_scheduled_tasks(dir)` only creates a directory and returns `ScheduledTasksHandle { dir }`; `CronTask`/`ScheduledTaskEvent` are compatibility DTOs and `build_missed_task_notification` formats text | `source`；not a timer, durable queue, EventLog occurrence or authority; must not be wired into DaemonHost or used as a second execution path |

## 4. Migration and safety guard

The only allowed migration is `legacy SDK scheduler surface → explicit AUT workflow/trigger command`
through the existing protocol and ControlPlane. A compatibility caller may retain its old DTOs, but
it must not infer a missed task, create a WorkflowInstance, call a capability, or claim a receipt.
Any future adapter must:

1. convert a user-approved schedule/event into a typed `AutomationCommand` with source, owner,
   project, definition version, idempotency/occurrence key and authority snapshot;
2. let ControlPlane validate trust, role, project, expiry, budget, policy, approval and CAS before
   committing an occurrence/instance fact;
3. let the pure planner return an opaque effect intent; only a committed reservation may reach the
   existing Broker/Runner or Company StartRun path;
4. persist Unknown/fence/reconcile facts when commit, dispatch, observation or shutdown is ambiguous;
5. preserve the old `watch_scheduled_tasks` API as compatibility-only until an explicit migration
   step supplies a versioned upcaster and evidence. No old event may be silently treated as a new
   `trigger.fired`/`workflow.started` fact.

The AUT-01 source guard rejects a second scheduler loop, direct Broker/handler references in the pure
planner, or removal/renaming of the legacy watcher without a migration record. It does not assert that
the target scheduler exists; that is AUT-02..AUT-24 work.

## 5. Fixture catalog and handoff

| Fixture | Purpose | Owner step |
|---|---|---|
| `automation_baseline` | source-only single-spine and legacy watcher guard | AUT-01 |
| `clock_rollback_denied` / `clock_untrusted_deadline` | zero/rollback/overflow cannot extend expiry | AUT-02 |
| `workflow_definition_cycle_denied` / `workflow_definition_version_immutable` | schema/DAG/role/project and version migration | AUT-03 |
| `trigger_occurrence_duplicate_denied` | source kind, occurrence key and project/owner binding | AUT-04/05 |
| `planner_replay_is_byte_stable` | pure planner determinism and no effect | AUT-06 |
| `queue_claim_old_fence_denied` / `worker_competition_single_winner` | durable claim/lease/fence | AUT-07/08 |
| `daemon_scheduler_shutdown_fences_claim` | bounded Tokio service and shutdown | AUT-09 |
| `unknown_effect_requires_reconcile` / `cancel_late_result_fenced` | Unknown/cancel/retry/compensation | AUT-15..20 |
| `workflow_restart_requires_explicit_resume` | boot recovery and no automatic dispatch | AUT-21 |
| `automation_four_entrypoint_parity` | CLI/Web/Workbench/Desktop same snapshot | AUT-22/23 |

All behavior fixtures run in GitHub Actions only. AUT-01 closes with a source inventory and explicit
gaps; it does not upgrade any feature or proof status and does not change the single DaemonHost spine.
