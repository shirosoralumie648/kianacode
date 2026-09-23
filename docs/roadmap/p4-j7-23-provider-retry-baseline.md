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
  retry is prepared and admitted again.
- SDK implicit retries are disabled by `.retry(reqwest::retry::never())` in the shared provider
  client builder (present in target integration `origin/master` 2dcb410 from P4-J7-18). The source guard binds this
  integration prerequisite; the provider Gateway itself still sends one admitted call.

## GitHub CI fixtures

- `post_send_unknown_is_not_retried_automatically` checks one model dispatch and no completion.
- `retryable_429_then_success_records_two_attempts` checks two ModelTurn attempt records and one
  successful terminal result.
- `cancel_during_retry_backoff_prevents_next_attempt` checks cancellation interrupts backoff and
  prevents a second dispatch.
- `oversized_retry_after_does_not_retry_early` checks a Retry-After beyond the absolute deadline
  leaves the model request count at one.
- `observed_delta_prevents_retry_even_for_retryable_rejection` checks partial output is not replayed
  when a later error would otherwise be retryable.
- Provider unit fixtures cover injected-clock HTTP date parsing, oversized seconds and typed
  network-versus-unknown failure classification.
- The existing `cancelling_mid_stream_never_completes_or_emits_a_late_delta` daemon regression is
  retained in the dedicated workflow.

## Verification commands

GitHub Actions is the test authority. Local tests are intentionally not run. The dedicated
workflow runs:

```text
cargo fmt --all --check
cargo test -p kiana-provider --lib --locked -- --test-threads=1
cargo test -p kiana-runner --test p4_j7_23_retry --locked -- --test-threads=1
cargo test -p kiana-runner --lib --locked -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host cancelling_mid_stream_never_completes_or_emits_a_late_delta --locked -- --test-threads=1
cargo test -p kiana-core --test p4_j7_23_provider_retry_guard --locked -- --test-threads=1
```

Targeted `rustfmt --check` and `git diff --check` pass. The non-test command
`cargo check -p kiana-provider -p kiana-runner -p kiana-core --locked --offline` exits 101 before
reaching these crates' changed code: `kiana-domain/src/lib.rs` declares the missing
`memory_workbench` file and the shared domain baseline reports additional unrelated type/derive
errors. This does not verify that P4-J7-23 compiles.

## Evidence boundary

This is source plus remote-CI wiring only. No live provider request, durable attempt journal proof,
billed usage reconciliation, retry receipt projection or timing guarantee beyond the in-process
monotonic deadline is claimed. HTTP 429/503 rejection is treated as safe to retry only when no
response delta was exposed; provider-specific billing behavior still needs usage settlement in
P4-J7-24. The target integration contains the P4-J7-18 SDK retry-disable setting. GitHub Actions
currently stops at the workspace format step because
`kiana-domain/src/lib.rs` declares `memory_workbench` while that file is untracked in the primary
checkout and absent from `origin/master`; the missing file was not imported into this step. The
P4-J7-23 fixtures are therefore wired but not yet executed by CI.

Cancellation of a durable `reserve_prepared` commit has no release operation on the current
`ModelBudgetPort`; if cancellation races with that commit, this step prevents provider dispatch but
does not claim durable reservation reconciliation.
