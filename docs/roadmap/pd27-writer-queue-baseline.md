# PD-27 - Bounded writer queue, backpressure and shutdown baseline

> Snapshot date: 2026-09-27. This slice owns the *decision* half of the bounded multi-writer
> queue: what a writer may enqueue, what a refusal looks like, when a closed queue stops
> accepting, and what a cancelled or hard-killed writer must hand over. Local Cargo
> test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions owns fixtures
> and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`PD-27`](../roadmap.md#step-pd-27) |
| source snapshot | master plus this PD-27 writer queue slice |
| feature_status | `partial` for source-level writer-queue admission/shutdown contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | writer lease registry + bounded policy -> admission/takeover report -> shutdown record |

The card names five things: multi-process writers, backpressure, flush/shutdown, cancellation and
resource caps. Two of them were already built by the steps this card depends on, and this slice
deliberately does **not** rebuild them:

| Already exists | Where | What it does |
|---|---|---|
| a bounded writer worker pool that refuses rather than waiting | `kiana-eventlog/src/jsonl.rs` (`MAX_STORAGE_WORKERS`, `try_acquire_owned`) | saturates to a stable `PortError::Unavailable` instead of blocking without a bound |
| observable `flush` / `close` acknowledgements | `EventStorePort::flush` / `::close`, ER-06 | a successful close is a distinct acknowledgement, never inferred from task drop |

This slice adds the contract those two leave open. `JsonlEventLog` reports *that* it saturated;
nothing states *which* append was refused, *whether the queue was open at all*, *whether a second
process may write*, or *what a writer must hand back when it stops*. A guard test
(`pd27_jsonl_still_refuses_observably_rather_than_waiting_or_dropping`) pins that the existing
bounded queue was not quietly replaced by an unbounded wait or a silent drop while this slice was
added.

## The two rules the whole card rests on

### A full queue refuses observably; it never drops silently

A silently dropped event is the worst failure this store can have, because it leaves no trace.
The caller believes it committed, the store never receives it, and the two diverge with no error
anywhere to notice. So `WriterAdmissionReport::evaluate` returns one of exactly two statuses, and
a **refused** report carries `accepted_depth_after: 0` and `accepted_bytes_after: 0`. A refusal
can therefore never be read as a successful append. Forging those two numbers onto a refusal and
re-sealing the digest fails closed at `writer_report_admitted_depth_leak`.

### After shutdown returns, no writer may still be appending

An append that arrives after `close` returned is refused (`writer_queue_closed`), not parked in a
drain that has already happened. From the other side, `WriterShutdown` refuses a record that
claims shutdown returned while a writer was still appending
(`writer_shutdown_outstanding_writer`) or while frames were still queued
(`writer_shutdown_pending_remainder`).

## What the card rejects, and how

| Rejected | How |
|---|---|
| **queue full, event dropped silently** | depth and byte bounds are separate rules with separate codes (`writer_queue_full`, `writer_queue_bytes_exceeded`); a refusal reports zero admitted depth and bytes, and a forged non-zero value fails closed |
| **write after shutdown returned** | `WriterQueueState::Closed` refuses with `writer_queue_closed`; a shutdown record that still lists a writer or a pending remainder is rejected |
| **cancel leaves a lock or lease behind** | `WriterShutdownMode::Cancelled` counts as claiming post-stop effects, so it must report `lock_released` and an empty lease list (`writer_shutdown_lease_residue`, `writer_shutdown_lock_not_released`). A lease left behind is ownership nobody can revoke |
| a second process writing silently | only the `WriterRegistry` holder may append (`writer_instance_not_registry_holder`); another instance must take the lease over explicitly |
| displacing a live writer | `WriterTakeoverReport` refuses while the previous lease has not lapsed (`writer_takeover_live_holder`) |
| a resurrected writer passing its own token | a takeover must advance the data epoch and use a fresh fence token (`writer_takeover_epoch_not_advanced`, `writer_takeover_fence_token_reused`) |
| reasoning about the wrong registry | the takeover's bound digest and lock must match the registry on disk; checked **first**, before liveness |
| a cap that can never bind | a per-writer cap above the queue cap is rejected (`writer_queue_policy_pending_exceeds_queue`) rather than looking enforced |
| an empty lease window | a lease that expires at or before it was acquired never bounded anything (`writer_registry_lease_window_invalid`) |
| a dead writer claiming effects | `HardKill` may not report a lock release or newly durable bytes, because a kill-9 runs no code (`writer_shutdown_dead_writer_released_lock`, `writer_shutdown_dead_writer_cannot_flush`) |

## Decision order

Both reducers apply a fixed rule order, so the reported reason is the first violated rule and is
therefore deterministic for the same facts.

Admission: `unknown` -> `closed` -> `draining` -> holder identity -> epoch -> frame size -> batch
size -> per-writer cap -> queue depth -> queue bytes -> admitted.

`Unknown` is checked first, before `Closed` and before every capacity rule, because an
unestablished state cannot be diagnosed and must not be reported as a specific refusal.

Takeover: registry digest -> lock/holder identity -> lease liveness -> epoch monotonicity ->
fence-token freshness -> accepted. Identity comes first because a takeover bound to a digest that
is not on disk is not reasoning about the right state at all.

Shutdown validation: header -> unknown state -> already closed -> pending remainder -> outstanding
writer -> cursor claims -> lease list -> mode-specific truth.

## Reuse, not a second vocabulary

DEP-17 already registered `CapacityEnvelope` and the bounded-queue budget vocabulary. This slice
consumes it rather than declaring numbers of its own:
`WriterQueuePolicy::from_capacity_envelope` reads the queue depth from
`observability_queue_capacity`, the per-writer cap from `max_batch_events` and the frame cap from
`max_event_bytes`. The envelope's own `validate` runs first, so a policy cannot be built from an
envelope that has dropped its `backpressure_preserves_facts` guard — which the fixture asserts by
feeding an unguarded envelope in and getting `performance_safety_guard_missing` back.

## Failure-first fixture matrix

`kiana-eventlog/tests/pd27_writer_queue.rs`, one test per "先拒绝的" column of the card:

| Fixture | Assertion |
|---|---|
| open queue below every bound | the append is admitted and reports the depth and bytes it actually added |
| **queue full** | refused with `writer_queue_full` and **zero** admitted depth and bytes |
| forged admitted depth | a refusal re-sealed to claim it queued the frame fails closed (`writer_report_admitted_depth_leak`) |
| byte bound | the byte rule is a separate bound with its own code, not a restatement of the depth rule |
| per-writer cap | a writer already holding its pending cap cannot add another frame even with an almost empty queue |
| batch / frame caps | an oversize frame and an oversize batch are each refused with their own code |
| **write after shutdown returned** | `Closed` -> `writer_queue_closed`, `Draining` -> `writer_queue_draining`, `Unknown` -> `writer_queue_state_unknown`, all with zero admitted depth |
| unknown outranks closed | an unestablished state reports `unknown` even when the occupancy would also be over the bound |
| second instance | a non-holder instance is refused and may not append |
| stale epoch | a stale data epoch is refused before any capacity rule |
| **cancel leaves a lease** | `writer_shutdown_lease_residue`; the same writer having released everything is accepted |
| cancel keeps the lock | `writer_shutdown_lock_not_released` |
| **shutdown returns with a writer still appending** | `writer_shutdown_outstanding_writer`; buffered frames and bytes both give `writer_shutdown_pending_remainder` |
| cursor claims | a shutdown claiming more durable than it flushed, or a regressing cursor, is refused |
| hard kill | a dead writer may claim neither a release nor new durability; the honest kill-9 record is accepted, which is what lets the next process take over |
| unknown / already closed queue | the shutdown record is refused for both |
| live holder | a live writer may not be displaced |
| **takeover after a kill** | an expired lease is inherited with an advanced epoch and a fresh fence; the successor then appends as the holder |
| reused epoch or fence | each is refused with its own code |
| foreign registry digest | refused before liveness is even considered |
| policy without bounds | a zero bound, a zero writer cap and an inert per-writer cap are each rejected |
| capacity reuse | bounds are derived from the registered envelope; an envelope missing its backpressure guard is refused |
| empty lease window | a lease that expires at or before acquisition is refused |
| writer label hygiene | an unredacted or oversize writer label is refused, as any other receipt text is |
| tamper / determinism | a forged acceptance, a stale digest and a repeated decision are all checked |

## CI

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. This slice adds no
separate workflow and no new workflow file.

## Limitations — what this does NOT prove

This is a source-only contract over facts an adapter supplies. Concretely, it does **not** prove:

- **That any queue ever actually rejected anything.** The depth, byte and per-writer numbers are
  reported by a caller. An adapter that under-reports its own pending depth produces a confident
  "admitted" for a queue that was in fact full.
- **That the decision was honoured.** `WriterAdmissionReport` is evidence that an admission was
  refused. Nothing here makes the adapter honour that refusal, and nothing here bounds the real
  `Semaphore` in `jsonl.rs`, which is still a separate, independently tunable resource.
- **That no writer is still appending after `close` returns.** The shutdown record states that no
  writer was outstanding when it was produced. Proving it requires driving real concurrent
  appends against a closing store and observing what lands afterwards.
- **That a hard-killed writer can be taken over.** The takeover is a decision over a registry and
  a lease-expiry fact. Whether a `flock` left behind by a `kill -9` is actually released, and
  whether the successor's first append is genuinely fenced, needs a real two-process fixture.
  That is the card's success path and it is still open.
- **Any durability or RPO/RTO property.** `EventStoreHealth` read here is only used to show that
  a flush/close acknowledgement remains an acknowledgement; reading one does not establish that
  the queue is bounded, only that a call returned. Fault injection (kill-9, disk full, lock
  contention) belongs to PD-30, and the platform/filesystem matrix to PD-32.
- **Anything about `kiana-daemon`.** This slice did not need a daemon change: the contracts are
  adapter-side, and the daemon already delegates `flush`/`close`/`last_durable_cursor` to the
  store. Wiring a live writer registry into `DaemonHost` is not done here.
