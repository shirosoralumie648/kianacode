# PD-31 - Adapter conformance suite and capability matrix baseline

> Snapshot date: 2026-09-28. This slice owns the question "may this adapter be called, and what is
> it allowed to claim once it answers?". Local Cargo test/build/check/clippy/smoke commands are
> intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`PD-31`](../roadmap.md#step-pd-31) |
| source snapshot | master plus this PD-31 conformance slice |
| feature_status | `partial` for source-level declaration/ordering/refusal contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | adapter declaration -> shared suite -> conformance report -> capability matrix |

PD-05 asked the Memory and JSONL event stores to return the same *logical* commit outcomes. It did
that with a hand-written helper living in one test target. The gap PD-31 closes is that the
contract was **not a contract**: nothing obliged a third adapter, or a future edit to the first
two, to pass the same checks in the same order with the same error codes. So the suite moved out
of a test file and into `kiana-ports::adapter_conformance`, where every adapter reaches the same
function.

The second half is the capability matrix. A suite that only watched behaviour could not tell a
stub from a store, and a suite that only watched declarations would say nothing at all. The
declaration is therefore the thing under test: every adapter publishes an `AdapterConformance`, the
suite compares that declaration against the capabilities the adapter itself reports, and both must
agree before any behaviour is observed.

## The declaration is the thing under test

`AdapterConformance` carries the adapter's `EventStoreCapabilities`, the `ProofCeiling` it may
claim, the stable codes it returns for unsupported calls, and the single capability the suite will
probe for refusal. `AdapterCapabilityMatrix` collects one declaration per adapter so the
durable/local_behavior/physical boundary is stated once instead of re-derived per adapter in prose.

The `ProofCeiling` ladder is ordered and the suite refuses any row above what it can certify:

| Ceiling | What it would mean | Can this suite certify it |
|---|---|---|
| `source` | a declaration plus in-process outcomes | yes |
| `local_behavior` | the adapter's own process produced the outcome it declared | yes, for the checks listed below |
| `durable` | survives process restart and power loss | no |
| `physical` | survives the failure the deployment actually has | no |

`ProofCeiling::certifiable_by_source_suite` returns true only through `LocalBehavior`, and both
`AdapterConformance::validate` and `AdapterConformanceReport::validate` refuse anything higher. The
JSONL row therefore declares `durable_commits: true` (it really does write a file) while its
**ceiling** stays at `local_behavior` — writing a file is not surviving a power loss, and no
power-loss, torn-tail or disk-full case ran under this suite.

## What the card rejects, and how

The suite runs in a fixed order, and the order is part of the contract: a report's rows are sorted
by `ConformanceCheck::all()`, so two adapters cannot present their evidence in different orders.

**Declaration first, behaviour second, refusal last.** A lying declaration is reported as such
rather than being discovered halfway through, and the refusal probe runs after the behavioural
checks because probing a declaration that has already been contradicted would be meaningless.

| Rejected | How |
|---|---|
| adapter claims a capability and does not have it | `DeclarationMatchesAdapter` compares the published record with `EventStorePort::capabilities()` (`adapter_declaration_capabilities_mismatch`) |
| an atomic flag that disagrees with the capability record | `AtomicFlagAgreesWithCapabilities`; `supports_atomic_transitions()` and `capabilities().atomic_transitions` must be the same answer (`adapter_atomic_flag_disagrees_with_capabilities`) |
| a durable flag that disagrees with the capability record | `DurableFlagAgreesWithCapabilities` (`adapter_durable_flag_disagrees_with_capabilities`) |
| **Memory mislabelled durable** | `AdapterConformance::new` refuses to construct the row at all (`adapter_declaration_memory_cannot_be_durable`); `AdapterCapabilityMatrix` requires a Memory row and re-checks it (`adapter_capability_matrix_memory_row_missing`) |
| a ceiling the suite cannot certify | `ProofCeilingIsCertifiable` plus the same check in both `validate` methods (`adapter_declaration_proof_ceiling_not_certifiable`) |
| **a durable-file/SQLite row pretending to be implemented** | `AdapterKind::is_reserved` names the two kinds with no adapter in this checkout, and a reserved row may not carry a ceiling above `source` (`adapter_declaration_reserved_kind_not_implemented`) |
| **adapter declares unsupported, is called, and succeeds** | the probe must return `Err`; an `Ok` fails `UnsupportedCapabilityRefuses` (`adapter_unsupported_capability_succeeded`). The probe is caller-supplied so this module holds no reference to a concrete adapter |
| **refusal codes drifting between adapters** | the observed code must appear in that adapter's own declared `unsupported_codes` (`adapter_refusal_code_undeclared_<code>`). A declaration that does not list the code it expects its own probe to return is rejected at construction (`adapter_declaration_probe_code_not_declared`) |
| a probe on a capability the adapter declared it *has* | `AdapterConformance::validate` refuses a declaration whose probe is supported (`adapter_declaration_probe_capability_supported`), so a `true` capability can never manufacture an expected refusal |
| an unsupported read reported as an empty page | `UnsupportedReadAllIsNotEmptySuccess`; `Ok(vec![])` is a failure, and only the stable `event_store_read_all_unsupported` code is an acceptable `Err` (`adapter_read_all_empty_success`) |
| **event order drifting** | `CursorPagePreservesCommitOrder` requires strictly increasing `sequence` across a page and a cursor that covers the events returned; `StreamReadAgreesWithCommitOrder` requires the same for `read_stream` (`adapter_cursor_page_order_invalid`, `adapter_stream_order_invalid`) |
| replay/conflict drifting into something else | `SameCommandReplays` requires `Replayed` with a receipt whose command id *and* digest match; `StaleVersionConflicts` requires `Conflict`; a replay that committed again, or a conflict that returned `Committed`, is named (`adapter_replay_returned_*`, `adapter_stale_commit_returned_*`) |
| a conflict that left a receipt behind | `ConflictedCommandHasNoReceipt` (`adapter_conflicted_command_has_receipt`) |
| a suite that quietly stops checking something | `AdapterConformanceReport::validate` refuses a report that does not carry exactly one row per check, in order (`adapter_conformance_report_check_set_incomplete`, `adapter_conformance_report_check_order_invalid`) |

A failing row names the observed code rather than a boolean, so a report says *what* drifted.

## Failure-first fixture matrix

`kiana-eventlog/tests/pd31_adapter_conformance.rs`:

| Fixture | Assertion |
|---|---|
| `memory_and_jsonl_pass_the_same_contract_suite` | both shipped adapters pass every check and their reports validate |
| `a_shared_suite_produces_the_same_check_order_for_every_adapter` | the two reports carry the identical ordered check list and the identical refusal-code sequence |
| `the_capability_matrix_states_the_durable_local_behavior_physical_boundary` | only the JSONL row is durable; no row exceeds `local_behavior` |
| `a_stub_that_succeeds_on_an_unsupported_capability_is_refused` | a store that fabricates receipts and answers `read_all` with empty success fails all five behavioural checks by name |
| `a_probe_that_succeeds_is_refused_as_an_unsupported_capability` | an `Ok` probe fails both refusal checks |
| `a_memory_row_marked_durable_is_rejected_at_declaration_time` | the Memory+durable declaration is unconstructible |
| `a_matrix_whose_memory_row_claims_durability_is_rejected` | the matrix refuses it even when the declaration was assembled by hand |
| `a_declaration_whose_ceiling_outranks_the_suite_is_rejected` | a `durable` ceiling is unconstructible |
| `an_adapter_whose_event_order_differs_from_the_suite_is_refused` | a store that reverses its page is named `adapter_cursor_page_order_invalid` |
| `an_adapter_whose_refusal_code_was_never_declared_is_refused` | refusing with an undeclared string still fails |
| `a_declaration_whose_probe_code_is_missing_from_its_own_list_is_rejected` | a declaration cannot forget the code it expects |
| `a_declaration_whose_capabilities_drift_from_the_adapter_is_refused` | an adapter that quietly adds a capability fails the *first* check, before any behaviour |
| `an_adapter_whose_atomic_flag_disagrees_with_its_capabilities_is_refused` | the split flag is caught |
| `a_reserved_sqlite_row_may_not_claim_to_be_implemented` | a SQLite row above `source` is unconstructible |
| `a_reserved_durable_file_row_must_stay_at_source` | the reserved row is legitimate at `source` and the matrix keeps it |
| `the_matrix_requires_a_memory_row_and_rejects_duplicate_adapters` | a matrix without a Memory row, or with two, is refused |
| `unimplemented_storage_ports_refuse_rather_than_return_empty_success` | the five storage ports and both typed deletion methods refuse with their own codes |

`kiana-eventlog/tests/pd31_adapter_guard.rs` is the source guard: it asserts every stable code
above literally appears in `kiana-ports/src/adapter_conformance.rs`, that the module is reachable
from the crate root, that the ceiling and reserved-kind rules are present as written, that the
declaration check precedes the behavioural checks and the probe follows them, and that the module
contains no `std::fs`, `std::process`, `std::net`, `tokio::fs`, `rusqlite`, `sqlx`, `File::`,
`OpenOptions`, `PathBuf` or broker call.

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.
No local `cargo test`, build, check, clippy or smoke command was run; only static compilation of
the affected test targets and `rustfmt` on the four files this slice touched.

**This does not prove the following, and no claim here should be read as proving it:**

- **No durability.** Nothing restarted a process, dropped a page, killed a writer or ran a
  power-loss. `durable_commits: true` on the JSONL row is the adapter's own declaration, restated
  — it is not a durability result, and the row's ceiling stays at `local_behavior` for that reason.
- **No SQLite and no durable-file adapter exists.** `AdapterKind::DurableFile` and
  `AdapterKind::Sqlite` are reserved names. The matrix can hold a row for them, and the guard
  refuses to let such a row claim anything above `source`, but nothing was run against either.
- **The suite observes declarations, not implementations.** An adapter whose declaration *and*
  reported capabilities agree but whose behaviour is wrong would still be caught — that is what the
  behavioural checks are for — but an adapter that lies consistently in both places would need the
  behavioural checks to disagree with it, and a fresh store makes several of them easy to satisfy.
  Conformance is not a proof of correctness.
- **The probe is caller-supplied.** The suite calls a closure, so what "unsupported" means for a
  given adapter is chosen by the test author. The declaration must list the code it expects, which
  is the anti-drift guard, but the *choice* of which capability to probe is not itself checked.
- **No cross-process, no multi-writer, no projector.** The JSONL writer lock, `flock`, file
  identity, torn-tail repair, disk-full behaviour and checkpoint/restart replay remain PD-06,
  PD-07, PD-09 and ER-33/34. PD-31 does not restate or supersede them.
- **The capability matrix is not a runtime capability negotiation.** It is a published record of
  what each adapter claims and what each claim is worth. PD-32 remains where platform and
  filesystem negotiation is handled.

## Handoff

`PD-32` (platform/filesystem matrix) and `PD-33` (end-to-end backup→restore→upgrade→restart→delete
UAT) consume this. When a durable-file or SQLite adapter is written, it must publish an
`AdapterConformance` and pass `run_event_store_conformance` unchanged; a check added to
`ConformanceCheck::all()` applies to every adapter at once, which is the point of the suite living
in `kiana-ports` rather than in a test target.
