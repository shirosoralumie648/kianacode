# BQ-30 promotion gate, status ledger and module map baseline

> Snapshot date: 2026-09-28. Local Cargo **tests were not executed**; GitHub Actions owns fixtures
> and the workspace gate.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-30`](#step-bq-30) |
| code landing | `kiana-core/src/promotion_gate.rs`, registered by `kiana-core/src/lib.rs` |
| script | none added; the unified `ci.yml` remains the only automatic gate |
| fixtures | `kiana-core/tests/bq30_promotion_gate.rs`, `kiana-core/tests/bq30_promotion_gate_guard.rs` |
| documentation | `CURRENT_STATUS.md` (one evidence block per step) and `docs/module-map.md` (the new section "BQ-30 契约层回填") |
| feature_status | `partial` — the decision is decidable in source; no release is gated by it yet |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |

## The three failures the card names

| Card failure | Refusal |
|---|---|
| 只有类型/单测/估算不能宣称 billing live | `promotion_above_evidence_ceiling` — the claimed level is compared against the level the manifest's own proof level reaches |
| 无 provider receipt 不能宣称 measured | `promotion_live_requires_provider_receipt`, `promotion_measured_requires_provider_receipt` |
| 限制未记录阻断 Promote | `promotion_limitations_unrecorded` |

## Offline durable and opt-in live are two doors, not one

`ClaimedLevel` is an ordered ladder — `type_only < unit_test < estimated < offline_durable <
opt_in_live` — and the two upper rungs carry **different** preconditions on purpose. *Durable* asks
whether the fact survives a restart and needs a restart/replay reference. *Live* asks whether a real
external system was exercised under a typed opt-in and needs both that opt-in and the provider's own
receipt. A claim that satisfied one has said nothing about the other, which is why there is
deliberately no single `production` rung: collapsing them is how an offline rehearsal ends up
presented as a live run. The guard asserts that `"production"` appears nowhere in the module.

## The gate consumes the evidence contract

The input is SC-35's `EvidenceManifest`, and the ladder is derived from its `proof_level` by
`evidence_reaches`, so the two can never disagree about what the evidence supports. A second
evidence contract or a second proof ceiling would be a second answer to the question this gate
exists to ask, so the guard asserts neither `Bq30ProofLevel`, `Bq30Evidence` nor
`BQ30_PROOF_CEILING` exists.

The evidence manifest is validated **first**, before the gate interprets anything about it. That
ordering matters: a record the evidence layer already rejected must not reach the promotion layer to
be interpreted there, and the fixture for the limitations rule shows this by asserting only that the
request is refused, not by asserting a particular code.

## Honest limitations

This module decides and promotes nothing. It does not run a release, read a status file, edit the
roadmap, touch `CURRENT_STATUS.md` or the module map, and it is not wired into `ci.yml`, a release
workflow or `ControlPlane::handle_command`. The manifest it consumes is supplied by the caller, so a
caller that hands over a manifest at `live` with a plausible receipt defeats this check the way a
lying `present_artifacts` inventory defeated DEP-22. `Physical` has no rung on this ladder and is
mapped to the top rather than rounded down silently, which is a limitation of the ladder rather than
a decision the module is entitled to make. The card's wider asks — every historical step carrying an
evidence block, and the `P1-K5`/`CP-11`/`P4-J7-24`/`ER-12`/`OA-08`/`CO-45` touchpoints being backfilled
— are documentation work that this slice starts rather than completes: it adds the gate, one
evidence block and one module-map section, and says so rather than implying the ledger is finished.
