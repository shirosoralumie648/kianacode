# BQ-26 fault injection baseline: concurrency, crash, disk full, network EOF, provider 429/5xx, clock fault

> Snapshot date: 2026-09-28. This slice adds one **driving** fault-injection harness
> (`kiana-core/src/bq26_fault_harness.rs`), one narrow public failure classifier
> (`kiana_core::classify_failure_summary`, extracted from the existing private incident arm so a
> fixture can drive the same code production uses), an adapter-backed seam in
> `kiana-core/tests/support/bq26_adapter_seam.rs`, 24 failure-first fixtures and 9 source guards.
> Local `cargo test` execution is forbidden by the user for this session; GitHub Actions owns the
> fixtures and target compilation. Only `cargo check` and `cargo clippy` were run locally.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-26`](../roadmap.md#step-bq-26) |
| source snapshot | master plus this BQ-26 fault-harness slice |
| feature_status | `partial` for source-level driven-fault coverage of all six card families |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | injected fault → real existing seam → the value that seam actually returned → `Bq26FaultObservation` → `Bq26FaultHarnessRun` |

### The one idea this slice is built on

**A fault that is merely documented is not tested.** This repository already contains three
*replay-only* fault matrices — `kiana-domain/src/fault.rs` + `kiana-core/src/fault_injection.rs`
(8 injection points), `kiana-domain/src/er31_fault_matrix.rs` (12 crash points) and
`kiana-domain/src/notification_faults.rs` (9 scenarios). Every one of them is a **table**: a struct
literal records which safety result a human expects at a named boundary, and `validate` checks the
table is internally consistent. None of them can fail, because none of them runs anything. A
reviewer can read ER-31 and believe the crash windows are handled; the table proves only that the
`duplicate_effect` and `false_success` columns are `false`.

BQ-26 deliberately does **not** add a fourth table. Every case here is a function that calls a
seam and compares against its real return value:

| Case | Family | Seam actually driven | What the seam returned |
|---|---|---|---|
| `cas_race` | concurrency | `MemoryEventLog::commit_transition` (real `plan_transition` read-set check) | `CommitOutcome::Conflict` naming the winning version; the stream still holds exactly one fact |
| `lease_leak` | concurrency | `ProviderCapacityController::release` | `provider_capacity_lease_owner_mismatch` / `provider_capacity_lease_unknown`; counter never decremented for a permit nobody held |
| `over_budget_dispatch` | concurrency | `ProviderCapacityController::admit` + `dispatch_next` | `Delay`/`Reject`/`Queue`; never a lease, and releasing the slot does **not** refund the RPM budget |
| `flush_failure` | crash | `ModelAttemptLifecycleEvent::validate` | `model_attempt_dispatch_requires_prepared_flush` |
| `settlement_loss` | crash | `SettlementFoldLedger::release_unused` | `settlement_not_consumed`; the held reservation stays `Reserved` |
| `disk_full` | disk full | `kiana_core::classify_failure_summary` | `FailureClass::DiskFull` for all three ENOSPC spellings; a cancelled run is not laundered into it |
| `partial_frame` | network EOF | `kiana_provider::replay_stream_fixture` → production `Framer` | `provider_frame_truncated` |
| `network_eof` | network EOF | same fixture, clean frame boundary | `provider_stream_incomplete` with `side_effect_state: Unknown` |
| `unknown_auto_retry` | provider HTTP | `RetryPolicy::classify` on a real `ModelError` | deny `side_effect_unknown` |
| `provider_429` | provider HTTP | `RetryPolicy::classify` on a real `ModelError` | bounded retry, backoff ≥ `Retry-After`; denied once the request budget is spent |
| `provider_5xx` | provider HTTP | `RetryPolicy::classify` on a real `ModelError` | deny `side_effect_unknown` — the side-effect fence is checked *before* the retry class |
| `clock_rollback` | clock | `ClockObservation::observe` / `require_trusted` / `QuotaWindow::from_clock` | `ClockTrust::Rollback`; `clock_untrusted`; no window, no extended deadline |

### Where the drivers live, and why that is a feature

`kiana-eventlog` and `kiana-provider` are **dev**-dependencies of `kiana-core`. Making them
library dependencies to hold three fixture functions would pull reqwest and rustls into every
`kiana-core` consumer, so the harness is split along a real seam rather than a convenient one:

- **Contract + nine library drivers** in `kiana-core/src/bq26_fault_harness.rs` (flush failure,
  settlement loss, unknown auto-retry, 429, 5xx, over-budget dispatch, clock rollback, lease
  leak, disk full). These drive BQ-11 lifecycle validation, the BQ-12 settlement fold, BQ-17
  retry classification, BQ-16 capacity, the clock contract and the platform classifier.
- **Three adapter drivers** in `kiana-core/tests/support/bq26_adapter_seam.rs`, behind a
  `Bq26FaultSeam` trait: the CAS race (in-memory EventStore) and the partial frame / network EOF
  (provider `Framer`).
- `Bq26FaultHarnessRun::evaluate_with_default` **refuses** those three cases with
  `bq26_case_requires_adapter_seam` rather than faking them. A library-only run therefore cannot
  claim to have driven an adapter seam it could not reach, and a source guard asserts both halves
  of that property.

Two structural properties make the report non-forgeable:

- **`evaluate_with_seam` is the only full constructor.** It iterates `Bq26FaultCase::ALL`, and
  there is no constructor anywhere that accepts caller-chosen cases, so a report cannot claim
  coverage it did not perform. `seal` refuses anything that is not the full 12-case matrix, and
  `validate` independently re-checks that all 12 cases and all 6 families are present
  (`bq26_fault_case_missing`, `bq26_fault_family_incomplete`, `bq26_fault_harness_header_invalid`).
- **The negative assertion is a field, not a default.** `Bq26FaultRefusal` carries `fired` and
  `returned_ok`. `fired` is required to be `true` (`bq26_fault_did_not_fire`), and
  `forbidden_value` names the specific wrong outcome the seam must *not* have produced — a parsed
  `ModelReply`, a granted `retry`, a `CommitOutcome::Committed`, a
  `ProviderCapacityOutcomeKind::Accept`. A seam that returned `Ok` under an injected fault is
  visible as `returned_ok: true` rather than silently counted as a pass.

## What the card rejects, and how

| Rejected (card's rejected-first column) | How it is refused |
|---|---|
| **CAS race** | two writers present the same expected `AggregateVersion`; the in-memory store's real read-set check returns `CommitOutcome::Conflict` and the harness asserts the aggregate stream still holds **exactly one** event (`bq26_cas_race_duplicate_fact`). A torn write would be caught here, not in prose |
| **partial frame** | a stream that stops mid-`data:`-line goes through the provider's production `Framer`; `finish()` returns `provider_frame_truncated`. The fixture asserts the forbidden value is a parsed `ModelReply` |
| **flush 失败** | a `Dispatching` fact with `prepared_flushed: false` is refused by the BQ-11 validator. The fixture also asserts `reconciliation_required` — a dispatch that was never flushed must be reconcilable, never settled |
| **settlement 丢失** | an attempt whose settlement never lands is still `Reserved`; `release_unused` returns `settlement_not_consumed`. The fixture asserts `leaked_reservations == 1`: the hold is **correct**, and releasing it would be the double charge |
| **未知自动重试** | `ModelError::transport(..., request_sent: true)` carries `side_effect_state: Unknown`; `RetryPolicy::classify` denies with `side_effect_unknown` before it ever looks at the retry class. This is the double-charge guard, and it is the one ordering in the whole file that matters most |
| **超额继续 dispatch** | with `requests_per_minute: 1`, the second `admit` returns no lease and a non-dispatchable kind; `dispatch_next` is then driven to prove the queue is not a back door; and after the first lease is released the third `admit` **still** fails, because releasing a concurrency slot does not refund an RPM budget |

`flush_failure`, `settlement_loss` and `unknown_auto_retry` are the three cases where a
non-zero `leaked_reservations` is the *passing* result — the reservation is deliberately held
until an explicit reconciliation. `validate` encodes that distinction per case rather than
globally, so it is not possible to make a genuine leak look like a correct hold (or vice versa).

## Failure-first fixture matrix

`kiana-core/tests/bq26_fault_injection.rs` (24 tests), one named test per card rejection first:

| Fixture | Assertion |
|---|---|
| `cas_race_second_writer_conflicts_and_writes_no_duplicate_fact` | conflict fired, not `Ok`, forbidden value named, zero leaks |
| `partial_frame_is_never_parsed_as_a_complete_reply` | `provider_frame_truncated`, forbidden value is a `ModelReply` |
| `network_eof_after_a_clean_frame_boundary_is_still_incomplete` | `provider_stream_incomplete`, and explicitly **not** the truncation code — a clean boundary is not a complete answer |
| `flush_failure_refuses_dispatch_and_holds_the_reservation` | dispatch refused and the attempt stays reconcilable |
| `settlement_loss_holds_the_reservation_and_refuses_release` | hold retained, release refused, incident has an action |
| `unknown_auto_retry_is_denied_because_the_side_effect_is_unknown` | deny reason is the side-effect fence, not the retry class |
| `over_budget_request_is_never_dispatched_and_the_queue_is_not_a_back_door` | no lease, and the queue cannot hand one out |
| `disk_full_is_classified_and_never_treated_as_transient` | all three ENOSPC spellings classify; not laundered to transient or to cancel |
| `provider_429_retries_only_because_it_was_refused_before_any_charge` | bounded retry with `Retry-After` respected |
| `provider_5xx_after_send_is_never_auto_retried` | denied at the side-effect fence |
| `clock_rollback_cannot_build_a_window_or_extend_a_deadline` | rollback untrusted; no quota window; no deadline extension |
| `leaked_capacity_lease_is_never_recycled_to_another_owner` | foreign owner and unknown lease both refused; counter intact |
| `run_executes_every_case_in_the_matrix_exactly_once` | 12 cases, no duplicate |
| `run_covers_all_six_card_families` | `families_covered == Bq26FaultClass::ALL` |
| `every_case_fired_its_fault_and_refused_it` | no `fired: false`, no `returned_ok: true`, no empty action, all replayable |
| `report_states_what_it_does_not_prove` | the five honesty clauses are present and digest-bound |
| `forged_digest_and_missing_case_are_both_refused` | forged digest, dropped case, duplicated case all rejected |
| `observation_leak_and_marker_fields_cannot_be_forged` | an edited leak count or cleared reconciliation marker fails validation |
| `run_is_bound_to_its_source_cursor_and_event_ids` | `validate_against` refuses a moved cursor |
| `report_is_deterministic_for_the_same_seed_and_source` | same seed ⇒ same digest |
| `invalid_headers_are_refused_before_any_fault_runs` | zero seed/cursor, empty and duplicated source ids |
| `fault_classes_and_cases_map_to_the_card_vocabulary` | family mapping matches the card's six families |
| `wire_decoding_revalidates_so_a_hand_edited_report_is_refused` | a report edited outside the constructor fails decode-then-validate |
| `card_six_rejections_are_each_covered_by_a_refusal_code` | all six rejected-first items map to their exact refusal code |

`kiana-core/tests/bq26_fault_harness_guard.rs` (9 tests), in the style of
`kiana-core/tests/bq17_retry_policy_guard.rs`:

| Guard | Assertion |
|---|---|
| `bq26_harness_drives_the_named_seams_instead_of_describing_them` | 22 markers, each an actual **call site** rather than a doc comment — this is what stops the module decaying back into a table |
| `bq26_adapter_cases_are_not_faked_in_the_library_harness` | the library refuses the three adapter cases, the adapter delegates the other nine back, and the library links neither adapter |
| `bq26_harness_is_deterministic_and_never_sleeps_on_wall_time` | no `sleep`/`SystemTime::now`/`Instant::now`; all time from the injected fixture |
| `bq26_harness_is_not_a_second_execution_loop` | no `tokio::spawn`, `std::process`, `libc::kill`, `std::fs`, `reqwest::`, `ProviderGateway`, `ControlPlane::new`, `CapabilityBroker` |
| `bq26_harness_does_not_invent_a_second_failure_classifier` | one `classify_failure_summary`, used by both the production incident arm and the fixture; `FailureClass::DiskFull` appears exactly once in `platform.rs` |
| `bq26_harness_reuses_existing_vocabularies_instead_of_new_ones` | BQ-17 retry, BQ-12 settlement, BQ-16 capacity, journal-CAS and clock types are all imported; no parallel BQ-26 struct replacing any of them |
| `bq26_report_cannot_claim_coverage_it_did_not_run` | `evaluate` is the only constructor, the completeness checks exist, and the negative assertion is a required field |
| `bq26_registration_lines_are_present` | the four `lib.rs` lines are wired |
| `bq26_does_not_shadow_the_er31_and_notification_fault_contracts` | no ER-31 / notification / generic fault-matrix schema or type is restated |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate, which compiles and
tests `--workspace`. No separate workflow. Local `cargo test` is forbidden by the user for this
session and was not run; only `cargo check` and `cargo clippy` were run locally.

### What this does NOT prove

- **No process is killed.** The `crash` family is modelled by an uncommitted or unacknowledged
  fact: a `Dispatching` event whose `prepared_flushed` is false, and a settlement fold that never
  receives its terminal event. Nothing here survives an actual `SIGKILL`, and nothing here
  demonstrates a journal replay after one. ER-31 remains a table; BQ-26 does not upgrade it.
- **No filesystem is filled.** `disk_full` drives the *classifier*: it proves the string
  `"No space left on device"` reaches `FailureClass::DiskFull` in the same function production
  incidents use. It does not create an `ENOSPC`, does not show the JSONL writer's behaviour under
  one, and does not show that a partially written frame is recovered.
- **No socket is opened.** `partial_frame` and `network_eof` push byte slices through
  `replay_stream_fixture`, which runs the production `Framer` and `Accumulator`. That is the real
  parser, but it is not the real transport: the `provider_read_idle_timeout` and
  `provider_headers_timeout` arms in `kiana-provider/src/transport.rs` are still untested, as is
  any behaviour that depends on a genuine half-closed TCP connection.
- **No provider is called.** The 429 and 5xx cases construct a `ModelError` and hand it to
  `RetryPolicy::classify`. The classification half is proved; the transport half — that a real
  503 sets `ModelRetryClass::Never`, that the circuit breaker opens, that a half-open probe is
  claimed and abandoned — is not exercised here.
- **The CAS race is deterministic, not concurrent.** The two writers are interleaved by `await`
  order against one in-memory store, which reproduces the stale-read conflict exactly but does not
  reproduce a genuine data race, thread interleaving or lock contention. `JsonlEventLog`'s
  `flock`-based cross-process behaviour is untouched.
- **Nothing is durable.** `MemoryEventLog` and the capacity controller are in-process. This slice
  does not show that a reservation, lease or queue entry survives a restart, nor that a restarted
  process rebuilds the same state — that is BQ-21 and ER-29's job.
- **`leaked_reservations` is a declared count, not a measurement.** The harness records the
  number it deliberately left held, and `validate` checks the count against a per-case
  expectation. It does not instrument the domain types to prove nothing *else* holds a resource;
  a new holding path added to `SettlementFoldLedger` would not be caught by this slice.
- **The source guard asserts source text, not behaviour.**
  `bq26_harness_drives_the_named_seams` fails the build if a call site is renamed or deleted,
  which is what stops the module silently becoming a table. It cannot prove the seam behaved
  correctly, and it will need updating if the drivers are legitimately refactored. Two of its
  assertions are exact-substring matches on a `match` arm
  (`Bq26FaultCase::CasRace | Bq26FaultCase::PartialFrame | Bq26FaultCase::NetworkEof =>`) and on
  `controller.admit(request(500)?)?`; a rustfmt reflow that breaks one of those across lines will
  fail the build even though behaviour is unchanged.
- **Two cases are modelled, not observed.** `flush_failure` asserts that the BQ-11 validator
  refuses an unflushed dispatch fact, and `settlement_loss` asserts that the fold refuses to
  release a reservation that never reached a terminal fact. Neither drives the *producer* side —
  nothing here shows that a real writer under a real fsync failure emits exactly the unflushed
  fact the validator is handed, or that a real crash produces exactly a `Reserved` fold with no
  terminal event. The validators are pinned; the crash windows upstream of them are not.
- **The harness adds no admission or dispatch capability.** It is a fixture surface in
  `kiana-core`; it grants no permit, consumes no quota and starts no attempt. A caller can build a
  report, but a report is evidence of a fixture run, not of a production incident.
- **`classify_failure_summary` is a public widening.** It is a pure function of one string plus
  two booleans, and the production arm now delegates to it, so behaviour is unchanged — but it is
  a new public item in `kiana-core` and a source guard (`FailureClass::DiskFull` appearing exactly
  once in `platform.rs`) is what keeps it from being forked a second time.
