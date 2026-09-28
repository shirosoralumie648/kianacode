# SC-40 capacity and resource fault injection baseline

> Snapshot date: 2026-09-28. Local Cargo **tests were not executed**; GitHub Actions owns fixtures
> and the workspace gate.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SC-40`](security-compliance.md#step-sc-40) |
| code landing | `kiana-core/src/capacity_fault_envelope.rs`, registered by `kiana-core/src/lib.rs` |
| fixtures | `kiana-core/tests/sc40_capacity_fault_envelope.rs`, `kiana-core/tests/sc40_capacity_fault_envelope_guard.rs` |
| feature_status | `partial` — the envelope is decidable in source; nothing is injected or measured here |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |

## What this adds on top of BQ-26

BQ-26 injects the faults and reports what the seams did, including whether reservations leaked.
SC-40 asks the next question: **while that was happening, did the system stay bounded?** An
operation that eventually returns an error can still have run away on the way there — unbounded
queue, unbounded output bytes, unbounded latency, a provider that kept spending after a 429.

```text
fault happened
     ↓  CapacityFaultSample (what was observed while it happened)
CapacityBudget (the bounds the system claims to hold)
     ↓  evaluate_capacity_fault
CapacityFaultReport — inside the envelope, or the first bound that was broken
```

## The bounds are inputs, not constants

A bound nobody wrote down is not a bound. `CapacityBudget` is supplied, sealed with its own digest
and carried into the report, so a report can always answer "bounded by what, exactly". A budget with
a zero in any ceiling field is refused (`capacity_fault_budget_unbounded`) rather than read as
"unbounded", because the difference between "I claim no bound" and "my bound is zero" is exactly
the difference a reader needs.

## The check order is the argument

1. **Clock rollback first.** Every remaining check compares a number, and a number measured against
   a clock that went backwards is not evidence of anything. So the rollback is decided before any
   number is read.
2. **Queue, then output bytes, then latency** — the order a reader asks about them.
3. **Backpressure.** A disk that reported full and then "recovered" without ever pushing back did
   not recover; it absorbed the load somewhere the sample cannot see. This is refused even by a
   budget that did not require backpressure.
4. **Reservation leaks**, and only then recovery.

## Recovery is checked on its own terms

A system can hold every bound *during* a fault and still leak afterwards: the queue drains, the
latency returns to normal, and the reservation from the attempt that got a 429 is never released.
That is the leak the card names and it is invisible to any sample taken while the fault was still
running. So `reservations_after_recovery` is a separate mandatory field, a non-zero value is refused
(`capacity_fault_reservation_leak_after_recovery`), and a sample that never established recovery at
all cannot claim the envelope (`capacity_fault_recovery_not_established`).

A provider that kept spending after a 429 or 5xx is refused under its own reason so the cause is
named, and a `ClockFault` family sample that did not actually observe a rollback is refused as
`capacity_fault_clock_rollback_unproven` — reporting a clock fault as observed when the clock never
moved is the same mistake as quoting a number measured against a clock nobody trusts.

## Reused, not forked

`Bq26FaultCase` and `Bq26FaultClass` come from the BQ-26 harness. The guard asserts both that they
are used and that this module defines no `Sc40Fault`, `CapacityLimits` or `CapacityBound` of its
own, because a second fault taxonomy is a second answer to "what went wrong".

## Honest limitations

This module **computes**. It injects no fault, starts no timer, reads no clock, opens no socket,
allocates no queue and dispatches nothing — the guard asserts the absence of each of those tokens.
Every latency, queue depth and byte count in a report was supplied by the caller, and so was the
budget; a caller that reports flattering numbers defeats this check exactly the way a lying
`present_artifacts` inventory defeated DEP-22. The report says so in its own `limitations`, which
is never empty. No queue was resized, no reservation was actually released or leaked, and nothing
here is wired into `ControlPlane::handle_command`.
