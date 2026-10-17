# P4-J7-23 provider retry, absolute deadline and cancellation baseline

## Implemented source slice

- `ModelClient` describes exactly one admitted attempt. `KianaHarness::invoke_model` is the only
  retry driver, uses at most three model attempts, and creates fresh prepared call/permit identity
  for each retry.
- Typed transport classification permits retry only for recognized pre-send network connect
  failures or an explicit HTTP 429/503 rejection. TLS/unknown connect failures, auth and other
  statuses, response timeouts, stream failures, any observed delta, and post-send unknown outcomes
  are terminal.
- Explicitly rejected responses carry `side_effect_state=None`; that typed evidence allows the
  Runner to schedule a retry without treating arbitrary post-send failures as safe.
- Exponential backoff with bounded jitter never shortens `Retry-After`. Delta seconds are
  overflow-saturated; HTTP dates use the injected `SystemTime` parameter. A delay that cannot fit
  within the original wall-clock deadline exits without an early request.
- The same wall-clock budget covers provider admission wait, each model attempt and retry backoff.
  Cancellation is checked before an attempt and races with admission, transport and backoff; every
  retry is prepared and admitted again. A cancellation that wins during the admission wait releases
  the local attempt reservation without consuming or settling a permit, and the run reports exactly
  one terminal state.
- SDK implicit retries are disabled by `.retry(reqwest::retry::never())` in the shared provider
  client builder (present in target integration `origin/master` 2dcb410 from P4-J7-18). The source guard binds this
  integration prerequisite; the provider Gateway itself still sends one admitted call.

## GitHub CI fixtures

- `post_send_unknown_is_not_retried_automatically` checks one model dispatch and no completion.
- `retryable_429_then_success_records_two_attempts` checks two ModelTurn attempt records and one
  successful terminal result.
- `provider_retries_do_not_consume_the_separate_repair_budget` checks that a 429→503→success
  sequence can use three separately identified attempts with a repair limit of one while the
  repair counter remains zero; the Core source guard prevents transport retry from charging it.
- `cancel_during_retry_backoff_prevents_next_attempt` checks cancellation interrupts backoff and
  prevents a second dispatch.
- `oversized_retry_after_does_not_retry_early` checks a Retry-After beyond the absolute deadline
  leaves the model request count at one.
- `observed_delta_prevents_retry_even_for_retryable_rejection` checks partial output is not replayed
  when a later error would otherwise be retryable.
- `cancel_during_model_admission_wait_prevents_dispatch` holds a fake `ModelBudgetPort` in the
  admission queue and checks that cancellation ends the run with exactly one terminal state, with
  zero model dispatches, zero permit consumption and zero settlements.
- `each_retry_is_admitted_again_with_a_fresh_permit` checks that a 429→success sequence calls
  `reserve_prepared` and `consume_prepared` twice, that the admitted attempt ids equal the dispatched
  attempt ids, and that each attempt carries its own admission lease digest.
- `attempt_reservations_settle_or_mark_unknown_and_never_dangle` checks that a sent 429 rejection is
  recorded conservatively as `unknown`, the recovered attempt as `settled`, the in-memory ledger
  releases the whole reservation (`reserved_tokens == 0`) while conservatively charging unreported
  usage, and that a post-send unknown model outcome stays a model-attempt fact with no capability
  request, tool cancellation or completion invented from it.
- `cancel_while_the_model_turn_is_pending_emits_one_terminal_state` checks that cancelling a pending
  model turn produces exactly one terminal state, keeps the cancellation error, and emits no late
  delta.
- `cancel_racing_with_model_completion_keeps_one_terminal_state_per_observer` races start and cancel
  eight times and checks per observation that the run ends with exactly one terminal state, never
  both completed and cancelled, and that the cancellation response never reports a completion.
- Provider unit fixtures cover injected-clock HTTP date parsing, oversized seconds and typed
  network-versus-unknown failure classification.
- The existing `cancelling_mid_stream_never_completes_or_emits_a_late_delta` daemon regression is
  retained in the dedicated workflow and still checks that the durable event log holds exactly one
  terminal event.

## Verification commands

GitHub Actions is the test authority. Local tests are intentionally not run. The dedicated
workflow runs:

```text
cargo fetch --locked
cargo fmt --all --check
cargo test -p kiana-provider --lib --locked -- --test-threads=1
cargo test -p kiana-runner --test p4_j7_23_retry --locked -- --test-threads=1
cargo test -p kiana-runner --lib --locked -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host cancelling_mid_stream_never_completes_or_emits_a_late_delta --locked -- --test-threads=1
cargo test -p kiana-core --test p4_j7_23_provider_retry_guard --locked -- --test-threads=1
```

On integrated snapshot `855aa099`, targeted `rustfmt --check`, `cargo fmt --all --check` and
`git diff --check` pass. The workflow path filter now includes the current CM-36
`kiana-domain/src/memory_workbench.rs` module and requests a fresh remote run after the historical
fmt blocker; that result is pending/unobserved. Before CM-36 was merged, the non-test command
`cargo check -p kiana-provider -p kiana-runner -p kiana-core --locked --offline` on base d85a4a5
exited 101 before reaching these crates' changed code: `kiana-domain/src/lib.rs` declared a missing
`memory_workbench` file and the shared domain baseline also reported unrelated type/derive errors.
That historical failure does not verify the build on integrated snapshot `855aa099`; no check/build
or tests were run after integration. P4-J7-23 CI fixtures have not been awaited.

The 2026-10-17 completion slice (queue-stage cancellation, per-attempt admission re-entry,
reservation release and the completion-versus-cancel terminal race) was written in a CI-only
session: only `rustfmt` was run locally on the touched Rust files, and both the manual lane and the
unified shard wiring were extended (`cargo fetch --locked` plus `cargo fmt --all --check` now gate
the manual lane; `kiana-provider` and `kiana-runner` remain whole-crate shards and
`p4_j7_23_provider_retry_guard` stays in the `kiana-core-s5/6` shard list). No build, check or test
was executed locally, so the new fixtures are source plus CI wiring only and their results are
pending.

## Evidence boundary

This is source plus remote-CI wiring only. No live provider request, durable attempt journal proof,
billed usage reconciliation, retry receipt projection or timing guarantee beyond the in-process
monotonic deadline is claimed. HTTP 429/503 rejection is treated as safe to retry only when no
response delta was exposed; provider-specific billing behavior still needs usage settlement in
P4-J7-24. The target integration contains the P4-J7-18 SDK retry-disable setting. The earlier
workflow attempt stopped at workspace formatting before CM-36 landed its `memory_workbench` module;
the integrated snapshot now passes `cargo fmt --all --check`. CI fixture results for P4-J7-23 remain
unverified until the fresh run completes.

Cancellation of a durable `reserve_prepared` commit has no release operation on the current
`ModelBudgetPort`; if cancellation races with that commit, this step prevents provider dispatch but
does not claim durable reservation reconciliation.
