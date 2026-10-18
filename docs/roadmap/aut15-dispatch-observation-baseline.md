# AUT-15 dispatch and observation baseline

AUT-15 separates a claimed worker dispatch intent from its later observation. The intent binds
reservation, action digest, worker identity and authority epoch; observations must match those
facts. Succeeded/Failed become Observed, while uncertain transport remains Unknown and cannot be
converted to success.

## CI wiring gap closed (2026-09-24 text corrected)

This baseline previously stated that "GitHub CI runs the domain fixture and Core source guard in
`.github/workflows/aut15-dispatch-observation.yml`". That workflow file did not exist in the
repository, so the claim was an overstatement: the pre-existing `kiana-domain` fixture
(`3 tests`) and the Core marker guard had no dedicated CI wiring. The source slice itself was real
(`kiana-domain/src/automation_dispatch.rs`, `kiana-core/src/automation_dispatch.rs`). The manual
lane now exists; AUT-14 has the same correction in its own baseline.

## 2026-10-17 completion slice (CI-only)

The earlier slice stopped at value objects: a read-only Core facade over
`validate_automation_observation()`, no durable dispatch unit, no worker claim, no revision fence
and no daemon participation. This slice completes the card's source-level exit conditions in
`kiana-domain`, `kiana-ports`, `kiana-eventlog`, `kiana-core` and `kiana-daemon`. No local Cargo
test/build/check/clippy/smoke was run and no CI receipt is awaited; `proof_level` stays `source`.

**1. Re-fetchable command (`kiana-domain/src/automation_dispatch.rs`).**
`AutomationDispatchCommand` binds a `Prepared` intent to a `command_id`, an `idempotency_key` and a
monotonic `revision` (starting at 1), and carries the structured
`AutomationDispatchResult`/`AutomationDispatchFailure` once terminal. `validate()` fails closed on
schema drift, blank/bounded identity, revision 0, a result before terminal, a missing result in a
terminal state, a result digest that differs from `intent.observation_digest`, and a state/outcome
drift (Observed cannot record Unknown; Unknown cannot record a determined result). Transitions are
pure:

| transition | rule |
|---|---|
| `prepare_automation_dispatch_command` | only a valid `Prepared` intent becomes a command (revision 1) |
| `claim_automation_dispatch_command` | same worker+attempt on a `Claimed` command → `AlreadyClaimed` (safe re-fetch, revision-independent); different worker or attempt → `automation_dispatch_worker_conflict`; `Prepared` + stale `expected_revision` → `automation_dispatch_revision_conflict`; terminal → `Terminal` (never executes) |
| `automation_dispatch_result` | copies execution/reservation/action/worker/epoch **from the command**; the runner only supplies outcome, failure code and digests |
| `observe_automation_dispatch_command` | `Claimed` + current revision folds the result (Succeeded/Failed → `Observed`, Unknown → `Unknown`); terminal accepts only the **identical** result as a replay, otherwise revision/observation conflict |

Known failure is structured (`AutomationDispatchFailure { code, detail_digest, retryable }`);
`Succeeded`/`Unknown` carrying a failure are refused (`automation_dispatch_failure_unexpected`) and
`Failed` without one is refused (`automation_dispatch_failure_required`).

**2. Store boundary (`kiana-ports/src/lib.rs`, `kiana-eventlog/src/automation_dispatch.rs`).**
`AutomationDispatchStore` declares `commit_command` / `claim_command` / `observe_command` /
`fetch_command` / `fetch_command_by_idempotency_key` with fail-closed
`*_unsupported` defaults; `AutomationWorkerEffectRunner` has **no** default implementation, so a
missing runner cannot silently become a success. `MemoryAutomationDispatchStore` implements the
port with per-execution CAS:
an identical idempotency replay returns the **current** authoritative copy (`replayed=true`, stale
`expected_revision` ignored = crash-safe re-fetch), a changed payload under the same key or a second
command for one execution is refused, `claim_command` re-evaluates the domain decision under its
lock so exactly one worker/attempt wins, and `observe_command` never overwrites a terminal result.
The module states its CI semantics explicitly: no fsync, no crash recovery, no cross-process
fencing — it must not be cited as durable.

**3. Coordinator (`kiana-core/src/automation_dispatch.rs`).**
`dispatch_automation_worker_command(store, runner, command_id, worker_id, attempt)` re-fetches the
committed command instead of trusting a caller snapshot, claims it with the store's CAS, invokes the
runner exactly once and commits the observation. `AutomationWorkerDispatchOutcome` has three
variants: `Observed` (determined result committed), `Unknown` (effect attempted but the result is
Unknown or the commit is unconfirmed — the command is `Unknown` or still `Claimed`), and `Replayed`
(committed claim or terminal state; no effect ran). Denials, replay, a concurrent loser and an
uncertain commit return stable `PortError`s or `Unknown` — never `Observed`.

**4. Daemon (`kiana-daemon/src/automation_dispatch.rs`, `kiana-daemon/src/lib.rs`).**
`AutomationDispatchService` is one bounded (`AUTOMATION_DISPATCH_CHANNEL_CAPACITY = 64`) Tokio
command queue handled by a single worker, owned by `DaemonHost`
(`start_automation_dispatch_service` / `automation_dispatch_service()` / included in
`DaemonHost::shutdown()`). It exposes the phases as separate operations — `commit_command` (dispatch
intent fact), `dispatch` (re-fetch → claim → one effect → observation), `fetch_command` (reconciliation
re-read) — and its shutdown report counts `Observed` commands against `recovery_required_count`
(Claimed/Unknown), so an in-flight or unconfirmed attempt is never reported as done. The service only
uses the store/runner ports: it never holds the EventStore, the Broker, a harness or a model client.

## Fixtures and CI wiring

Deny-first fixtures (local run not performed):

| target | count | focus |
|---|---|---|
| `kiana-domain/tests/automation_dispatch.rs` | 3 → 8 | command/revision identity, one worker+attempt, structured failure, terminal resurrection/overwrite, unknown-field and drift closure |
| `kiana-core/tests/automation_dispatch_worker.rs` (new) | 6 | effect count is 0 or exactly 1 across denials, replays, concurrent losers and uncertain commits (`ObserveFailingStore`); structured failure and Unknown never become success |
| `kiana-core/tests/automation_dispatch_guard.rs` | 1 → 2 | boundary markers, rejection-code matrix, coordinator ordering (re-fetch → claim → run → observe), fixture names, manual lane and shard binding |
| `kiana-eventlog/tests/aut15_dispatch_store.rs` (new) | 4 | crash-safe idempotent re-fetch, one winning claim, observation revision race, terminal overwrite refusal |
| `kiana-daemon/tests/aut15_dispatch_observation.rs` (new) | 4 | separate commit/dispatch phases, restart replay without a second effect, Unknown/recovery counting, DaemonHost ownership and shutdown |
| `kiana-daemon/tests/aut15_dispatch_observation_guard.rs` (new) | 2 | bounded single-worker service markers, forbidden-boundary negative assertions, fixture binding |

The manual lane `.github/workflows/aut15-dispatch-observation.yml` (`workflow_dispatch` only,
`permissions: contents: read`) fetches locked dependencies, runs the fmt gate, runs all six targets
serially (`--test-threads=1`) and compiles each with `--no-run`. Automatic coverage: the unified
`ci.yml` shards — `automation_dispatch` in `kiana-domain-s1/4`, the new `automation_dispatch_worker`
plus `automation_dispatch_guard` in `kiana-core-s1/6`, and whole-crate discovery for
`kiana-eventlog` and `kiana-daemon`.

## Known limits

- The store and the service are in-process only (CI semantics): no fsync, no crash recovery, no
  cross-process fencing. Re-fetch after a crash is proven against the same process' store, not
  against a real power loss.
- No production ControlPlane path uses the new command yet: `handle_workflow_command` still commits
  and dispatches inline, and the module-map's `guard_workflow_capability` unknown-execution
  fail-closed gap is unchanged. A production wiring needs an explicit decision on where the
  command fact lives in the workflow aggregate.
- The runner is the only effect boundary in these fixtures; no real Broker/handler, provider,
  receipt or payment path is exercised, and nothing here is `durable`, `live` or `physical` evidence.
- Retry classification, attempt/lease/deadlines, cancellation fences and boot recovery remain
  AUT-16+/AUT-17+/AUT-21.
