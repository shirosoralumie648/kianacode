# BQ-29 golden trace and cross-entrypoint end-to-end chain baseline

> Snapshot date: 2026-09-28. Local Cargo **tests were not executed**; GitHub Actions owns fixtures
> and the workspace gate.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-29`](#step-bq-29) |
| code landing | `kiana-core/src/golden_trace_chain.rs`, registered by `kiana-core/src/lib.rs` |
| fixtures | `kiana-core/tests/bq29_golden_trace_chain.rs`, `kiana-core/tests/bq29_golden_trace_chain_guard.rs` |
| feature_status | `partial` — the chain is verifiable in source; nothing is replayed and no golden trace is loaded |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |

## The chain

```text
model → tool → event → receipt → invoice correction
```

A `GoldenTrace` records that chain so it can be replayed. Recording it is the easy half. This
module checks that a chain somebody *reports* actually is that chain, which is harder than it
sounds: a list of stage names is trivial to produce and says almost nothing.

## The three ways it lies

| Card failure | Refusal | Why |
|---|---|---|
| 任一入口绕过 DaemonHost | `golden_chain_stage_route_bypass` | Every stage must name the one route. The route is what makes the stage's contents trustworthy, so a stage that arrived another way is not a slightly worse stage, it is an unverified one. |
| provider/handler 调用数与 reservation 不符 | `golden_chain_call_count_mismatch`, `golden_chain_reservation_mismatch` | A provider called twice against one reservation is a double charge wearing a receipt; a stage that ran with no reservation is an effect nobody charged for. |
| Receipt 把 runtime success 写 business outcome | `golden_chain_business_outcome_without_runtime`, `..._evidence_missing` | A 200 is a fact about the transport. Whether the business succeeded is a separate claim that needs its own evidence, and conflating them is how a failed workflow gets invoiced as a success. |

The business rule is symmetric on purpose: evidence travelling without a claim is refused as
`golden_chain_business_evidence_without_outcome`, because that is the same confusion in reverse —
a claim that was meant and not written down.

## The check order is the argument

1. **Route, per stage, first.** Everything else in a stage is only as trustworthy as how the stage
   was reached.
2. **Stage completeness and order.** A missing stage is a hole in the audit, not a shorter trace.
3. **Counts against expectations, then reservations.** A reservation that outlives every stage is a
   leak; a stage that ran with none is an unaccounted effect. Both are accounting failures and both
   are refused.
4. **The business-outcome rule last**, because it only means anything once everything above holds.

## Reused, not forked

The route is the existing `ENTRYPOINT_ROUTE` (`daemonhost.controlplane`) from the ER-27 entrypoint
parity module, and the chain binds to a `GoldenTrace` through its digest rather than redefining a
trace type. The guard asserts both that the existing route is imported and that no
`GOLDEN_CHAIN_ROUTE` or `GoldenStage` was invented — a second route string would be a second answer
to "how does an entry reach the control plane".

## Binding to the trace it claims to reproduce

`GoldenTraceChain` carries a `golden_trace_digest`, and until `bind_golden_trace` existed nothing
ever compared it to an actual `GoldenTrace`. **A digest nobody checks is a string.** The binding
closes that, in the order the argument runs:

1. the digest does not match, so the chain is not reproducing that trace at all;
2. the trace cannot reproduce anything -- no normalized events, an inverted cursor range, no source
   snapshot;
3. it has expired, because a past baseline cannot prove today's behaviour;
4. it was never human-accepted, because nobody signed for it and it is only a machine output;
5. the chain claims it reached `Receipt` but the trace carries no receipt hash -- then the chain
   proves "it got somewhere", not "it finished".

The binding requires a non-zero binding time, because otherwise "now" is unanswerable and the
expiry check would be theatre.

What it still does **not** do is replay anything. It checks that the trace the chain names is
worthy of it; comparing the trace's normalized events against what the chain actually produced is a
different action, and this module neither performs it nor pretends to.

## Honest limitations

This module **verifies and replays nothing**. Beyond the binding it does not replay a `GoldenTrace`, does not run a
model, a tool, a provider or a handler, does not append an event, and does not produce a receipt. The
chain, the counts, the routes and the business claims are all supplied by the caller, so a caller
that reports a flattering chain defeats this check the way a lying `present_artifacts` inventory
defeated DEP-22; the report's own always-non-empty `limitations` says this outright. The binding
to a `GoldenTrace` is a real comparison of identity and admissibility, not a replay: no fake
provider or local executor was driven, no synthetic stream was parsed, and nothing is wired into
`ControlPlane::handle_command` or into the harness.
