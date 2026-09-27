# PD-34 capacity, throughput, latency and degradation budget baseline

> Snapshot date: 2026-09-28. This slice adds *declared* per-subject budgets on top of the existing
> DEP-17 `CapacityEnvelope`, each with its measurement method recorded. Local Cargo
> test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions owns fixtures
> and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`PD-34`](persistence-data-layer.md#step-pd-34) |
| source snapshot | master plus this PD-34 declared-budget slice |
| feature_status | `partial` for source-level declared-budget/refusal contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | `CapacityEnvelope` -> `DeclaredBudget` per subject -> `StorageDegradationReport` |
| source | `kiana-ports/src/storage_capacity_budget.rs` |
| fixture | `kiana-ports/tests/pd34_storage_capacity_budget.rs` |
| guard | `kiana-ports/tests/pd30_32_34_storage_matrices_guard.rs` |

The card asks for 容量、吞吐、延迟和退化预算 over 事件帧、Artifact、索引、backup/prune, rejecting
无界 frame/batch/queue、长 reader 阻塞 writer、maintenance 挤占用户命令, and delivering 上限、
P95/P99、背压和降级证明 with 超过容量进入可诊断拒绝.

**No pressure run, stress test, throughput measurement, artifact staging, index rebuild, backup or
prune was performed for this slice.** Every number here is a *declared* budget: a cap read from the
existing `CapacityEnvelope`, paired with a recorded `BudgetMeasurement`. There is no field a source
contract can fill to claim a measured latency number, and `StorageDegradationReport::anything_measured`
answers `false` for a report assembled from declared budgets.

## This slice does not invent a second capacity model

| Reused | From | How PD-34 uses it |
|---|---|---|
| `CapacityEnvelope` | DEP-17 `performance.rs` | **The** bounds. `StorageCapacitySubject::envelope_field` / `envelope_bound` read the envelope, so a budget cannot restate a number |
| `PersistenceCapacityBudget` (p95/p99, queue, rejection, maintenance share) | DEP-17 `persistence_capacity.rs` | Carried as `DeclaredBudget::latency_budget`, unchanged |
| `BenchmarkSummary` | DEP-17 `performance.rs` | Carried as `StorageDegradationReport::summaries` for a future real run |
| `StorageErrorClass` | `kiana-domain::storage_health` | A capacity refusal must be `Unavailable` or `Conflict`; `ResultUnknown` is the PD-30 fault shape, not a capacity one |
| `WriterQueuePolicy` caps | PD-27 `kiana-eventlog` | The origin recorded for the `writer_queue` subject |
| `MAX_MAINTENANCE_SHARE_BPS` | PD-27 | The basis-point vocabulary for the maintenance share |

`StorageCapacitySubject` is the card's four things — `event_frame`, `artifact`, `index`,
`backup_prune` — plus `writer_queue`, because PD-27's `WriterQueuePolicy` already bounds a queue
and a queue with no subject would be invisible to the report.

## Declared budgets and their measurement methods

`BudgetMeasurement::for_subject` is fixed per subject, so two subjects cannot be measured by two
different means without the difference showing in the report.

| Subject | Envelope field | Origin | Measurement method |
|---|---|---|---|
| `event_frame` | `max_frame_bytes` | `capacity_envelope` | `sustained_append` — a sustained append of bounded frames, latency as a percentile |
| `artifact` | `max_artifact_bytes` | `capacity_envelope` | `artifact_stage_commit` — stage then commit a maximum-size artifact |
| `index` | `max_page_events` | `capacity_envelope` | `rebuild_under_read` — rebuild a representative snapshot under concurrent reads |
| `backup_prune` | `journal_max_bytes` | `persistence_budget` | `maintenance_under_load` — backup + prune with user admission running |
| `writer_queue` | `max_batch_events` | `writer_queue_policy` | `queue_contention` — fill the queue to its cap while writers contend |

`BudgetOrigin::DeclaredByReview` exists for a number a reviewer chose with no derivation. Every
budget records one; the module refuses a budget whose origin string is empty.

## The numbers are declared, not measured

This is the honesty requirement, and it is structural rather than a promise in prose:

- A `DeclaredBudget` is `Bounded` — not `Measured` — until `measurement_receipt_digest` is `Some`.
  A receipt added after the budget was sealed fails `capacity_budget_digest_mismatch`.
- `StorageDegradationReport::measured_subjects` is derived from the receipts, not from a claim. A
  report whose `measured_subjects` names a budget without a receipt is refused
  (`capacity_budget_receipt_missing`, `storage_degradation_measured_subject_unknown`).
- A report assembled with no summaries and no receipts answers `false` to `anything_measured()`.
- The p95/p99 values the fixture constructs are **declared caps** from DEP-17's
  `PersistenceCapacityBudget`, exercised on a budget *structure*. They are not observations and no
  fixture asserts a measured latency.

## What the card rejects, and how

The decision order in `derive` is fixed, so the reported reason is the first violated rule.

| Rejected | How |
|---|---|
| **an unbounded frame / batch / queue** | every budget's bound is read from the `CapacityEnvelope` for its subject (`capacity_budget_envelope_field_mismatch`, `capacity_budget_envelope_bound_mismatch`), and a zero bound is named (`capacity_subject_not_bounded`). `CapacityEnvelope::validate` refuses a zero field first, so a validating envelope cannot leave a subject unbounded |
| **a long reader blocking the writer** | `CapacityRefusalObservation::reader_held_writer` is its own decision (`capacity_reader_blocked_writer`), checked after the maintenance share and before the receipt rule |
| **maintenance squeezing out user commands** | the share is basis points bounded by `MAX_MAINTENANCE_SHARE_BPS`, and exceeding any budget's share is named on that budget's subject (`capacity_maintenance_budget_exceeded`, remediation: defer maintenance) |
| **backpressure that is not fact-preserving** | every `CapacityBackpressure` strategy declares `preserves_accepted_facts`; an observation that lost a committed fact is refused at construction and by the report (`capacity_backpressure_dropped_facts`) |
| a refusal an operator cannot act on | a rejection with no stable code is refused (`capacity_refusal_observation_invalid`), and a rejection filed under a PD-30 reconciling class is refused for the same reason — capacity and corruption are not the same thing |
| a second number for one subject | one declared budget per subject (`capacity_budget_duplicate`) |
| a budget pointed at a field it does not read | `capacity_budget_envelope_field_mismatch` |
| a budget with no recorded method | `capacity_budget_measurement_mismatch` at the case, `capacity_budget_measurement_missing` at the report |
| a refusal reported for an undeclared subject | `capacity_observation_subject_undeclared` |
| a malformed benchmark summary silently dropped | `storage_degradation_summary_invalid` — a report may not look measured with fewer samples than it claims |
| a tampered report | `validate` re-derives the whole decision (`storage_degradation_report_binding_invalid`, `storage_degradation_report_digest_mismatch`) |

## Failure-first fixture matrix

`kiana-ports/tests/pd34_storage_capacity_budget.rs`:

| Fixture | Assertion |
|---|---|
| `every_subject_has_a_declared_bound_a_measurement_method_and_a_backpressure` | the card's four subjects plus the queue are bounded from the envelope, each with its fixed method, each reporting `Bounded` and not `Measured`; `anything_measured()` is `false` |
| `a_diagnosable_refusal_is_preserved_as_evidence_not_as_a_drop` | a refusal with a stable code and a capacity class is kept, and the report still reads `Refused` |
| `a_measured_subject_requires_a_receipt_on_its_own_budget` | a budget with a receipt is reported `Measured` and the subject list is derived from the receipts |
| `an_unbounded_subject_is_refused_rather_than_reported_ready` | a stripped bound is refused at validation, and a zero envelope field is refused by `CapacityEnvelope` |
| `a_duplicate_budget_for_one_subject_is_refused` | the duplicate is named with its subject |
| `a_budget_with_the_wrong_envelope_field_is_refused` | a budget pointing at `journal_max_bytes` for the frame subject is refused |
| `a_budget_measured_without_a_receipt_is_refused` | a receipt added after the seal is refused |
| `a_long_reader_holding_the_writer_is_refused` | `reader_held_writer` is named on the `index` subject |
| `maintenance_above_its_share_is_refused` | a 4000 bps share against a 2500 bps budget is named on `backup_prune`, and the remediation names deferring maintenance |
| `a_maintenance_share_above_one_hundred_percent_is_refused` | a share over 10 000 bps is refused |
| `a_refusal_with_no_code_is_not_a_diagnosable_refusal` | an anonymous rejection, and one filed under `ResultUnknown`, are both refused |
| `a_refusal_that_lost_a_committed_fact_is_refused` | a lossy rejection is refused, and all three strategies declare that they preserve accepted facts |
| `a_refusal_for_an_undeclared_subject_is_refused` | a refusal with no matching budget is named |
| `a_tampered_report_is_refused` | an edited status and a hand-set `measured_subjects` are both refused |
| `a_malformed_benchmark_summary_is_refused_rather_than_dropped` | a summary whose digest no longer matches cannot be ignored |
| `the_budgets_reuse_the_existing_capacity_vocabulary_rather_than_a_second_model` | each subject's envelope field is read from the envelope, and the measurement methods are one-to-one with the subjects |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.
No local `cargo test`, build, check, clippy or smoke command was run; only static compilation of
the affected targets and `rustfmt` on the files this slice touched.

**This does NOT prove the following, and no claim here should be read as proving it:**

- **No latency was measured.** There is no p95, p99, throughput or queueing figure in this slice
  that came from a stopwatch. Every percentile is a declared cap from DEP-17's
  `PersistenceCapacityBudget`, and no fixture asserts a measured value. The card's 给出上限、P95/P99
  is satisfied as *declarations with a recorded measurement method*, not as results.
- **No stress test was run.** Nothing filled a frame, a batch, a page, a queue, an artifact or a
  journal to its cap. `CapacityRefusalObservation` rows in the fixture are declared shapes; zero
  requests were presented and zero were rejected.
- **No backpressure was applied to a real store.** `CapacityBackpressure` names three strategies;
  none was exercised against `JsonlEventLog` or `MemoryEventLog`. The PD-27 writer queue's own
  saturation code (`eventlog_worker_queue_full`) is what actually bounds a live queue, and this
  slice only names it.
- **No index rebuild, backup or prune ran.** The `index` and `backup_prune` subjects are declared
  against envelope fields; neither workload was performed. Whether maintenance actually stays inside
  25% of runtime under load is unmeasured.
- **The maintenance share is a declared policy, not an observation.** 2500 bps for backup/prune is
  a number somebody chose. Whether user admission stays observable above it is PD-29/PD-33
  territory and was not checked.
- **The report's envelope is reconstructed from its budgets.** `StorageDegradationReport::evaluate`
  builds `capacity` from the declared bounds so a report and its budgets cannot disagree. That is a
  consistency guarantee, not a second source of truth; DEP-17's envelope remains authoritative and
  is not written by this slice.
- **`StorageDegradationReport` is `PartialEq`, not `Eq`.** `CapacityEnvelope` and `BenchmarkSummary`
  are not `Eq`, and adding a second capacity model to make this `Eq` would be the drift the card
  rejects. The reason is recorded in the struct's doc comment.
- **PD-34 does not replace the existing `PersistenceCapacityReport`.** That DEP-17 report remains the
  authority for deciding whether a real `BenchmarkSummary` is within a real budget. This slice adds
  the *declaration* layer beneath it: what is bounded, by what, measured how, and refused how when
  the bound is reached.
- **Nothing here ran on Linux's filesystem specifically.** The bounds in the fixture come from a
  `CapacityEnvelope` value, not from a probe of the mount the tests happened to execute on. The
  PD-32 matrix is what records how the same subjects behave on Linux/ext4, Linux/tmpfs, a network
  filesystem, Windows and macOS; this slice reads the envelope only and is silent on all of them.
