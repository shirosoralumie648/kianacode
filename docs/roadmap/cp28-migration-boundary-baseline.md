# CP-28 migration / compatibility / bypass baseline

## Scope

CP-28 keeps compatibility explicit and non-authorizing.  Versioned event/storage registries own
named migrations; legacy facts remain queryable but missing authority requires reauthorization;
JSONL old writers cannot mutate an upgraded journal; daemon/entrypoint/client constructors all
reuse the same ControlPlane spine and dependency-boundary rules.

## Evidence gate

- `cp_legacy_authority_records_require_reauthorization` checks event/storage upcasters, old
  Continue/identity/approval records and recovery read-only/reauthorization boundaries.
- `cp_old_writer_cannot_mutate_new_journal` checks JSONL/journal/memory writer-version fences,
  atomic-transition capability denial, migration dry-run/read-only limits and corruption handling.
- `cp_every_product_constructor_enforces_the_same_boundary` checks DaemonHost/harness/CLI/client
  constructors, architecture status and strict internal dependency/legacy-edge guards.

The GitHub workflow runs the existing schema, event-contract, JSONL, resume, dependency and
architecture fixtures, plus the migration preflight/rollback and legacy wire fixtures, then the
CP-28 source guard and workspace test-target compilation. Local runtime tests are not run.

## Migration matrix

Every compatibility surface has one named, versioned migration and one authority consequence.
Legacy facts stay queryable; none of them carries authorization forward.

| Surface | Legacy form | Current form | Migration / adapter | Authority consequence | Evidence |
|---|---|---|---|---|---|
| RuntimeEvent (`run.*`, `session.*`, `approval.*`) | schema `0` payloads | schema `1` | `legacy_run_event_v0_to_v1`, `legacy_session_event_v0_to_v1`, `legacy_approval_event_v0_to_v1` | upcast changes shape only; the record still passes policy/gate/approval before any effect | `er01_event_contract` |
| Storage records / memory records | legacy format digest | `StorageSchemaRegistry` format `1` | `upcast_storage_value`, `upcast_memory_record`, `SCHEMA_MIGRATIONS` | unknown major or non-migratable field → `storage_schema_unknown_major` / `storage_migration_non_migratable_field`; only same-major minor upcasts succeed | `pd02_schema` |
| Identity / local user | pre-migration principal | `IdentityMigration` metadata | `legacy_local_user_migration`, `migration_digest` | migration metadata is **not** an authentication assertion; the identity must be re-established (`principal_id`) | `ci04_ingress` |
| Approval record | standalone approval file without authority binding | journal approval bound to authority/epoch | `approval_legacy_reauthorization_required` | legacy approval is query-only; execution needs a fresh approval (`approval_clock_rollback`, `approval_payload_unrecoverable`) | `journal_approvals` |
| Continue / new turn | wire `v1` `Continue` (kept as-is) | `run.turn.v2` `new_turn`; `continue_run` retained | `check_schema_compatibility`, `PROTOCOL_SCHEMA` | an old daemon sees the v2 command name as unknown, never misreads it as continue | `h34_legacy_compat` |
| Run snapshot | epoch-less / stale snapshot | snapshot bound to authority + data epoch | `run_snapshot_stale`, `run_resume_authority_changed`, `run_resume_scope_changed` | stale or epoch-less snapshot → explicit not-resumable; recovery reads `read_all_events` and re-authorizes | `p0_g03_resume_guard`, `cp19_explicit_resume_guard` |
| Credential recovery | implicit admission | explicit re-admission binding | `explicit_re_admit_credential_recovery`, `credential_recovery_admission_binding_mismatch` | recovery requires an explicit re-admit; a mismatch is rejected | `ci11_credential_recovery_projection` |
| EventLog JSONL | record without `writer_version` | `JOURNAL_WRITER_VERSION` | `encode_legacy_record`, `writer_version` | a legacy writer that appends after upgrade → `eventlog_legacy_writer_after_upgrade` | `pd06_jsonl_guard` |
| Migration plan | ad-hoc migration steps | `MigrationRegistry` + `MigrationPreflightReport` + `MigrationRecord` | `preflight` (read-only), `apply`, `ordered_steps`, `signing_bytes` | unknown major / downgrade / missing migration fail-closed; the plane refuses `event_store_atomic_transitions_unsupported` instead of a partial apply | `dep27_migration_registry`, `dep28_migration_preflight` |

## Upgrade / downgrade handling

The upgrade path is fixed and observable: `detect → preflight → backup/manifest → acquire
migration lock → apply ordered idempotent steps → verify → record MigrationRecord → projector
catch-up`.

- **Dry-run first.** `MigrationRunnerPort::preflight` returns a `MigrationPreflightReport` with
  `read_only: true` and `effect_calls == 0`; it evaluates nine axes (store, schema, projection,
  workflow, provider, config, space, clock, lease) and blocks on
  `migration_store_digest_mismatch`, `migration_projection_lag`, `migration_old_writer_active`,
  `migration_runner_concurrent`, `migration_backup_unverified`, `migration_active_unknown` or
  `migration_clock_untrusted` before any write.
- **Original data is preserved.** Applying a migration requires a verified backup
  (`MigrationRecord::requires_verified_backup`, `backup_snapshot_id`) and the pinned
  `registry_digest`; the runner never deletes unverified data.
- **One authoritative writer.** Applying requires the migration lock and a single writer;
  `MigrationRollbackFacts::active_writer_count` blocks rollback with `rollback_writer_active`
  while any writer is live, so an old and a new daemon cannot mutate different fact sources at the
  same time.
- **Downgrade is denied.** `migration_downgrade_denied` and `migration_schema_major_mismatch` are
  fail-closed; the remediation is a forward migration or a verified compatible root, never an
  in-place reverse.
- **Failure is contained.** A blocked preflight, a checksum mismatch or a partial apply moves the
  record to `Quarantined` (`migration_failure_quarantined`) and the store falls back to restricted
  read-only queries; a `MigrationResumeToken` is required to resume, and it is validated against
  the quarantined state.
- **Old writers are fenced.** After JSONL upgrade, a writer that still lacks `writer_version`
  cannot append (`eventlog_legacy_writer_after_upgrade`), and `Memory`/journal adapters report
  `supports_atomic_transitions` so the control plane refuses a non-transactional path.

## Legacy bypass inventory

Product surfaces reach the executor only through the single spine
`DaemonHost → ControlPlane → CapabilityBroker → KianaHarness`. The remaining legacy dependency
edges are enumerated (not assumed) and may only shrink:

- `kiana-core/tests/dependency_boundaries.rs` measures the workspace graph via `cargo metadata`:
  `STRICT_ALLOWED` forbids every un-listed internal dependency, and `LEGACY_EDGES` lists the nine
  known `kiana-entrypoints`/`kiana-commands`/`kiana-tools` edges whose test
  `legacy_implementation_edges_can_only_decrease` fails if the set grows.
- `kiana-entrypoints/tests/cli_architecture.rs` runs the real binary and pins
  `control_plane == kiana-core`, `composition_root == kiana-daemon`, `capability_mode == brokered`,
  `legacy_prompt_loop == false` and `legacy_edges_remaining == 9`; its
  `product_surfaces_do_not_call_legacy_assistant_turn` scans `cli.rs`, `harness_run.rs`, `repl.rs`,
  `tui.rs`, `bg.rs`, `mcp.rs`, `sdk.rs`, `lib.rs` for the old runner loop.
- `kiana-daemon/tests/eq12_daemon_spine.rs` and the CP-28 guard assert every product constructor
  (`new`, `new_with_project_authority`, `with_harness_and_project_authority`, `local*`) takes
  `Arc<dyn EventStorePort>` and `ProjectTrustAuthority`, so a compatibility default method cannot
  masquerade as a durable, transaction-capable host; `harness_run.rs`/`cli.rs`/`client` must not
  build a `CapabilityBroker` or call `run_assistant_turn` directly.

Old compatibility branches are removed only when the call is provably unreachable; until then the
edges above are the authoritative, decreasing inventory rather than an unmeasured claim.

## Limits

This is source plus CI-fixture evidence only.  It does not claim a production upgrade rehearsal,
cross-version distributed writer race proof, physical backup/restore proof or removal of every
legacy compatibility crate; legacy edges remain explicitly measured until later cleanup gates.
