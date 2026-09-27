# PD-30 storage fault-injection matrix baseline

> Snapshot date: 2026-09-28. This slice owns the *classification* of a storage fault and the
> decision that no fault may produce a completed command, a duplicate effect or a recovery past
> the adapter's authority. Local Cargo test/build/check/clippy/smoke commands are intentionally not
> run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`PD-30`](persistence-data-layer.md#step-pd-30) |
| source snapshot | master plus this PD-30 fault-matrix slice |
| feature_status | `partial` for source-level classification/decision contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | adapter-reported `StorageError` -> `StorageFaultCase` -> ordered `StorageFaultMatrix` |
| source | `kiana-ports/src/storage_fault_matrix.rs` |
| fixture | `kiana-eventlog/tests/pd30_storage_fault_matrix.rs` |
| guard | `kiana-ports/tests/pd30_32_34_storage_matrices_guard.rs` |

The card names seven faults — kill -9, disk full, permissions, lock contention, a corrupt frame,
crash timing and a network Unknown — and one invariant for all of them: 每种故障都不能生成
Completed、重复 effect 或越权恢复. The delivery column is 每个故障有状态分类、恢复动作、fixture、
退出码和限制证据.

**This matrix is built from an adapter-reported `StorageError` and nothing else.** It does not
`kill` a process, fill a disk, change a permission, take a lock, corrupt a frame, reopen a store or
open a socket. The guard asserts that: none of the three new modules contains `std::fs`,
`std::process`, `std::net`, `std::thread`, `std::path`, `std::time`, `rusqlite`, `sqlx`,
`CapabilityBroker` or `execute_capability`, and a bare `fsync(` / `rename(` / `flock(` /
`statfs(` / `sync_all(` spelling is refused too.

## Vocabulary is reused, not reinvented

| Reused | Where it comes from | How PD-30 uses it |
|---|---|---|
| `StorageErrorClass` | `kiana-domain::storage_health` | **The** state classification. A case reads it off its `StorageError` (`StorageFaultCase::class`), so there is no parallel enum that could drift |
| `StorageError` | `kiana-domain::storage_health` | The adapter's own error record, carried whole |
| `FaultInjectionPoint` | `kiana-domain::fault` | The existing eight-point crash timeline. PD-30 binds crash timing to it rather than naming new crash points |
| `AdapterKind` | `kiana-ports::adapter_conformance` (PD-31) | A fault is stated against an adapter that already publishes a conformance declaration, so there is no second adapter registry |
| `StorageFaultExit` | new, here | A **declared** exit vocabulary, not an observed one. See the honesty note below |

## The seven faults, in the card's order

`StorageFaultKind::ALL` is exactly `KillNine, DiskFull, PermissionDenied, LockContention,
CorruptFrame, CrashTiming, NetworkUnknown`, and the guard pins each variant to its own `ALL` block
so a reordering is visible in the diff. Each fault has exactly one legal state classification
(`expected_class`), taken from `StorageErrorClass`:

| Fault | Class | Recovery | Exit |
|---|---|---|---|
| `kill_nine` | `Unknown` | `reconcile_from_facts` | `unknown_outcome` (3) |
| `disk_full` | `Unavailable` | `refuse_without_effect` | `refused` (2) |
| `permission_denied` | `Unavailable` | `refuse_without_effect` | `refused` (2) |
| `lock_contention` | `Conflict` | `fence_then_reconcile` | `refused` (2) |
| `corrupt_frame` | `Corrupt` | `quarantine_for_operator` | `quarantined` (4) |
| `crash_timing` | `ResultUnknown` | `restart_from_committed_prefix` | `unknown_outcome` (3) |
| `network_unknown` | `ResultUnknown` | `reconcile_from_facts` | `unknown_outcome` (3) |

## What the card rejects, and how

The decision order in `StorageFaultMatrix::evaluate` is fixed, so the reported reason is the first
violated rule and therefore the same for the same facts.

| Rejected | How |
|---|---|
| **a fault that produces `Completed`** | `completion_claimed` is refused at construction (`storage_fault_case_unsafe`) and again by the matrix (`storage_fault_completion_claimed:<fault>`); an unreconciled outcome may not be reported as confirmed either (`storage_fault_unknown_outcome_not_fenced:<fault>`) |
| **a duplicate effect** | `duplicate_effect` is refused at construction and again by the matrix (`storage_fault_duplicate_effect:<fault>`); a `ResultUnknown` answered with `refuse_without_effect` is unconstructible, because "nothing happened" is the one answer an unknown outcome cannot take |
| **a recovery past the adapter's authority** | `recovery_beyond_authority` is refused at construction and again by the matrix (`storage_fault_recovery_beyond_authority:<fault>`); a corrupt frame may only be quarantined, never "recovered" by truncating its tail (`storage_fault_corrupt_not_quarantined`) |
| a restart that hands back a cursor the fault never established | `admit_fault_restart` refuses a zero durable cursor for a case that started an effect (`storage_fault_restart_cursor_not_established`) and refuses an unfenced reconciling case outright (`storage_fault_restart_not_fenced`) |
| a fault filed under a class it cannot produce | a disk full reported as `Conflict` is refused (`storage_fault_class_mismatch`), because it invites a retry a full disk cannot satisfy |
| a crash point on the wrong fault, or a crash-timing case with none | `expects_crash_point` is true for `CrashTiming` alone, so both directions are refused (`storage_fault_crash_point_invalid`) |
| a faulted store that exits zero | `StorageFaultExit::for_fault` derives the code from the classification, and a case carrying anything else is refused (`storage_fault_exit_code_invalid`) |
| a matrix that quietly drops a fault this host cannot produce | coverage is checked before any other rule, and a missing fault is named (`storage_fault_kind_missing:<fault>`). This is the rule that makes "we cannot produce a network Unknown here" a *decision* rather than an absence |
| two claims for one fault | `storage_fault_kind_duplicate`, and a case that fails its own seal is named first (`storage_fault_case_invalid:<fault>`) |
| a classification with nothing to disclose | `limitations` is required and non-empty (`storage_fault_limitation_required`), bounded, redacted and secret-scanned |
| a tampered report | `validate` re-derives the whole decision and binds the baseline digest (`storage_fault_matrix_binding_invalid`, `storage_fault_matrix_header_invalid`) |

## Failure-first fixture matrix

`kiana-eventlog/tests/pd30_storage_fault_matrix.rs`:

| Fixture | Assertion |
|---|---|
| `the_matrix_classifies_all_seven_named_faults_with_exit_codes_and_evidence` | all seven are present in `ALL` order, each with a class, a recovery, a non-zero exit and non-empty limitations |
| `the_state_classification_is_reused_from_the_adapter_error_not_restated` | `case.class() == case.error.class == kind.expected_class()`; a mismatched class is refused |
| `a_fault_claimed_as_completed_is_refused` | `completion_claimed` fails at the case seal and the matrix names `kill_nine` |
| `a_confirmed_effect_on_an_unreconciled_fault_is_refused` | a confirmed `Unknown` is refused, whichever way it arrives |
| `a_restart_may_not_hand_back_a_cursor_the_fault_never_established` | `admit_fault_restart(case, 0)` is refused for a started effect; a known cursor is admitted |
| `a_fault_that_duplicated_its_effect_is_refused` | `duplicate_effect` is named on `corrupt_frame` |
| `an_unreconciled_effect_may_not_be_replayed_without_reconciling_first` | a network `Unknown` answered with `refuse_without_effect` is unconstructible |
| `a_recovery_beyond_the_adapters_authority_is_refused` | `recovery_beyond_authority` is named on `network_unknown` |
| `a_corrupt_frame_may_not_be_recovered_from_it_is_quarantined` | only `quarantine_for_operator` is accepted for `Corrupt` |
| `a_matrix_missing_a_named_fault_is_refused_by_name` | the missing fault is named, not counted |
| `a_matrix_with_two_cases_for_one_fault_is_refused` | a duplicate is refused |
| `a_fault_that_exits_zero_is_refused` | each class has one fixed exit code, and `Clean` is not among the legal values for a fault |
| `a_case_with_no_limitation_evidence_is_refused` | empty `limitations` is refused |
| `a_crash_timing_case_must_name_its_point_and_no_other_fault_may` | both directions of the crash-point rule |
| `a_tampered_matrix_is_refused` | an edited status, an edited reason and a broken baseline digest are each refused |
| `the_matrix_reuses_the_pd31_adapter_kinds_and_the_existing_crash_points` | the matrix is bound to `AdapterKind` and to `FaultInjectionPoint::Commit` |
| `the_matrix_does_not_claim_a_capability_the_existing_store_does_not_publish` | a missing root still fails to open a store, and the `StorageHealth`/`StorageCapabilities` shape is unchanged |

`kiana-ports/tests/pd30_32_34_storage_matrices_guard.rs` is the source guard: every stable code
above literally appears in `storage_fault_matrix.rs`, the seven `ALL` variants appear in the card's
order inside their own block, the three rejection codes are unique and decided in a fixed order,
the reused vocabulary is imported rather than redefined, and the effect-boundary forbidden list is
absent.

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.
No local `cargo test`, build, check, clippy or smoke command was run; only static compilation of
the affected targets and `rustfmt` on the files this slice touched.

**This does NOT prove the following, and no claim here should be read as proving it:**

- **No fault was injected.** Nothing was killed, filled, chmodded, locked, corrupted or disconnected.
  Every case in the fixture is a `StorageError` a fixture *wrote by hand*. A green run means the
  classification and the decision order are coherent; it says nothing about whether a real kill -9
  produces the class the matrix expects.
- **No exit code was observed.** `StorageFaultExit` is a *declared* vocabulary with fixed integers.
  No process was started, so nothing here shows that a real `kiana` run exits 3 after a kill -9.
  The numbers are a proposal the entrypoint owner still has to wire.
- **No adapter was exercised under fault.** The matrix is adapter-agnostic: it does not know whether
  `JsonlEventLog` actually returns `eventlog_disk_limit` on ENOSPC, or whether the Memory adapter
  refuses rather than drops. PD-06, PD-08 and ER-33 own the adapter-side behaviour.
- **A classification is a claim, not evidence.** `StorageFaultCase` is a record of what an adapter
  said. This slice cannot tell a truthful adapter from a lying one; it can only enforce that a
  claim about an unknown outcome is never reported as a confirmation.
- **Recovery actions are named, not performed.** `RefuseWithoutEffect` through
  `QuarantineForOperator` are declared actions. No fence was taken, no prefix was restarted from,
  no store was quarantined, and the takeover rules themselves remain PD-27.
- **The seven faults are not equally reachable from this host.** A Linux process can be killed and
  can hit ENOSPC/EACCES, but nothing here exercised even those two. The network `Unknown` row in
  particular has no producing adapter in this checkout at all.
- **PD-30 does not cover the ER-31 crash-point timeline.** ER-31's twelve effect boundaries remain
  the authoritative crash-timing vocabulary for the Event/Receipt/Recovery path; PD-30 only binds
  one of them to a storage fault classification.
