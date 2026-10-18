# AUT-21 boot recovery baseline

AUT-21 makes restart recovery an explicit, operator-gated step instead of an implicit
"resume everything" on boot. The slice is local-only and in-memory: `feature_status=partial`,
`proof_level=local_behavior` for the three focused targets below.

## Scope

### Domain contract (`kiana-domain/src/automation_boot_recovery.rs`)

`AutomationBootRecoveryFact` binds `source_cursor`, `projection_cursor`,
`projection_generation`, `authority_epoch` plus the freshly observed
`observed_authority_epoch`, `pending_unknown_count`, `stale_index`, a typed
`AutomationBootLease`, a typed `AutomationBootJournal`, the declared state and a list of typed
`AutomationReconcileCase`s, all covered by `recovery_digest`.

States are `Rebuilding` (clean restart default), `ReconcileRequired` (Unknown / stale index /
open lease), `Ready` (only after explicit operator resume) and `Blocked` (evidence container
that can never dispatch). `Ready` is refused unless:

1. `projection_cursor == source_cursor` (`automation_boot_recovery_projection_lagging`);
2. `pending_unknown_count == 0` (`automation_boot_recovery_unknown_pending`);
3. `reconcile_cases` is empty (`automation_boot_recovery_reconcile_case_open`);
4. `stale_index == false` (`automation_boot_recovery_index_stale`);
5. the lease is absent (`_stale_lease` / `_lease_active`);
6. the journal is healthy with the current writer schema (`_journal_corrupt` /
   `_journal_schema_mismatch`);
7. `authority_epoch == observed_authority_epoch` (`_authority_epoch_stale` ahead of an older
   snapshot, `_authority_epoch_ahead` for a fabricated future epoch).

The old vacuous `reconcile_refs: Vec<String>` success fixture was replaced: `reconcile_refs` is
gone, and the fixture named `lagging_projection_is_refused_even_when_everything_else_is_clean`
now proves that an otherwise perfect fact with `projection_cursor = source_cursor - 1` is
refused.

### Core facade (`kiana-core/src/automation_boot_recovery.rs`)

`validate_boot_recovery` delegates to the domain rules. `derive_boot_recovery_resume` refuses
`Ready` input (`automation_boot_recovery_resume_already_ready`), `Blocked`
(`automation_boot_recovery_blocked`) and `ReconcileRequired`
(`automation_boot_recovery_reconcile_required`); it only derives `Ready` from a `Rebuilding`
fact whose barriers all hold, re-seals the digest and re-validates.

`ControlPlane::handle_boot_recovery_resume` is routed from `commands.rs` under
`automation.boot_recovery.resume.v1` and enforces, in order: operator context (no `cell_id`,
non-empty `actor_id`, object arguments), `project_root` match, present/deserializable `fact`,
domain validation, state derivation, then a **live** `observed_authority_epoch` comparison so a
fact observed before an epoch advance cannot resume.

### Daemon consumer (`kiana-daemon/src/automation_boot_recovery.rs`)

`DaemonHost::observe_automation_boot_recovery(project_root)` reads the event-store cursor
snapshot, persisted events, event-store health and the ledger authority epoch, then calls
`ProjectionDriver::rebuild()` and replays the committed events through `apply()`, taking
`projection_cursor` from the rebuilt checkpoint. This closes the earlier dangling claim: the
boot path really does call the existing rebuild, it does not merely assert that a projection is
current. The resulting fact is `Blocked` on observation failure / corrupt journal, otherwise
`ReconcileRequired` when pending Unknown or an open lease exists, otherwise `Rebuilding`. The
host records the fact and fences dispatch.

`DaemonHost::resume_automation_after_recovery(context, project_root)` requires a prior
observation (`port_conflict:automation_boot_recovery_observation_required`), routes through
`ControlPlane::handle_command` (no second loop), and only lifts the fence after the Core-derived
`Ready` fact validates. Neither the observation nor the resume starts
`automation_dispatch_service` or `workflow_queue_service`; the daemon fixtures assert both still
return `port_unavailable:*_not_started` afterwards.

The fence is also the precondition of the two daemon start methods:
`start_automation_dispatch_service` and `start_workflow_queue_service` refuse with
`port_conflict:automation_boot_recovery_resume_required:<service>` while
`automation_dispatch_fenced()` is true. A restarted host therefore cannot start an effect worker
before the operator resume, even through the existing AUT-15/AUT-09 entry points.

## Fixtures

- `kiana-domain/tests/automation_boot_recovery.rs` — 8 tests, deny-first: lagging projection,
  pending Unknown + open cases, stale/active leases, journal corruption/schema drift,
  old/ahead authority epochs, stale index + structural damage, blocked evidence; success
  (`Ready` / `ReconcileRequired` visibility) last.
- `kiana-core/tests/automation_boot_recovery_guard.rs` — exactly 2 tests: a source guard that
  the facade has no second execution loop and that `commands.rs` routes the resume constant, and
  a behaviour guard that refuses self-certified `Ready`, `ReconcileRequired`, `Blocked`, lagging
  projection, stale lease and journal drift before the success case.
- `kiana-daemon/tests/aut21_boot_recovery.rs` — 5 tests driving a real `DaemonHost` over
  `MemoryEventLog` with a zero-call runner: fenced restart + operator-only resume, pending
  Unknown forcing `ReconcileRequired`, no service auto-start, service starts refused until
  resume, and resume-without-observation.

## Evidence

Local Windows/Git Bash, `CARGO_TARGET_DIR=D:/code/kiana-steps/target-aut21`, `--offline`,
`--test-threads=1`:

| target | result |
|---|---|
| `cargo test -p kiana-domain --test automation_boot_recovery` | exit 0, 8 passed |
| `cargo test -p kiana-core --test automation_boot_recovery_guard` | exit 0, 2 passed |
| `cargo test -p kiana-daemon --test aut21_boot_recovery` | exit 0, 5 passed |

`.github/workflows/aut21-boot-recovery.yml` is a manual (`workflow_dispatch`) lane with
`permissions: contents: read`; its receipt was not awaited, and `cargo test --workspace` was not
run. The stale `scripts/ci/doc-reference-exemptions.txt` line for this workflow was deleted.

## Limitations

- `pending_unknown_count` is a heuristic over committed events: a run whose last event kind ends
  with `.result_unknown` is counted as one pending Unknown. It never queries a provider or
  decides whether the effect actually succeeded.
- `stale_index` cannot be observed yet, so the daemon always reports `false`; the domain refusal
  path is tested but has no producer.
- The projection rebuild is in-memory only. No checkpoint is persisted, so the rebuilt projection
  and `projection_generation = 1` are re-derived on every restart.
- `MemoryEventLog` does not implement `event_store_health`; the daemon treats the specific
  `event_store_health_unsupported` error as "journal shape unknown but healthy", not as a crash.
  Durable journal corruption is only exercised by domain fixtures.
- Resume does not write a recovery event, consume an approval or reconcile the open cases; the
  unknown cases must be settled through existing reconciliation paths before a fresh
  observation can become `Rebuilding`.
- `automation_dispatch_fenced()` guards the two daemon service start methods; individual
  in-service calls after a service was started before observation are not re-checked, and no
  `kiana run`/web/workbench entry point observes boot recovery yet.
- No live/provider, durable, cross-process or physical evidence; the two daemon entry points are
  not yet wired into `kiana run`/web/workbench.
