# DEP-25 external effect reconciliation baseline

> Snapshot date: 2026-09-28. Local Cargo **tests were not executed**; GitHub Actions owns fixtures
> and the workspace gate.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-25`](#step-dep-25) |
| code landing | `kiana-core/src/effect_reconciliation.rs`, registered by `kiana-core/src/lib.rs` |
| fixtures | `kiana-core/tests/dep25_effect_reconciliation.rs`, `kiana-core/tests/dep25_effect_reconciliation_guard.rs` |
| feature_status | `partial` — the decision is decidable in source; nothing performs a lookup or a compensation |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |

## The card in one sentence

A request went out and nobody knows whether it landed. Retrying might send it twice, declaring
failure might be a lie, declaring success might be a lie. This slice refuses to make that call and
instead decides **which of four honest moves is admissible**, given an observation somebody else
actually made.

```text
EffectObservation { ConfirmedSuccess | ConfirmedFailure | NoEffect | Unknown }
        ↓  reconcile_effect(request)
EffectResolution  { Reconciled | RetryWithoutEffect | Abandoned | Compensated }
        ↓  sealed receipt, re-derivable from the same request
```

## The three named failures, and the rule behind each

| Card failure | Refusal | Why |
|---|---|---|
| `result_unknown` 自动 retry | `effect_reconcile_unknown_auto_retry` | `NoEffect` is somebody having *proved* nothing happened; `Unknown` is nobody having proved anything. Retrying the second is how a timeout becomes a double charge, so it is refused at the point of decision rather than left to discipline. |
| 没有 external ref 伪造 success | `effect_reconcile_success_without_external_ref`, `effect_reconcile_observation_receipt_missing` | Success cannot be asserted locally. Two separate refusals, because "no reference at all" and "a reference over an observation the provider never receipted" are different forgeries. |
| 旧 approval/epoch 复用 | `effect_reconcile_approval_reused`, `effect_approval_epoch_required` | An approval is single-use: a second reconciliation under the same sign-off is how one human approval becomes two effects. An epoch of zero is not a weaker claim, it is no claim. |

## Why four resolutions rather than one "resolved"

Their preconditions genuinely differ, and collapsing them would lose the fact an auditor needs:
whether a **compensating action was taken** is not the same fact as "we stopped caring". Each
carries its own receipt and its own evidence requirements.

- `Reconciled` — the outside world told us; needs an external reference *and* a provider receipt
  on the observation.
- `RetryWithoutEffect` — nothing happened, so the same request may be sent again. Only from
  `NoEffect`.
- `Abandoned` — there will never be an effect. Only from `NoEffect`, with a reason, and with no
  external reference or compensation travelling alongside it.
- `Compensated` — something may have happened and a compensating action was taken. Needs a
  reference *and* evidence; refused outright from `NoEffect`, where a compensation would be a
  phantom action in the audit trail.

A field belonging to another resolution is refused rather than ignored, so a receipt can never say
two things at once — which is worse than saying the wrong thing, because the wrong thing is at
least checkable.

## Idempotency is a decision, not a retry

`check_idempotent_replay` requires the same key to carry the same request digest. A key reused with
different content is a second decision wearing a retry's name, and that is the shape a double
effect takes. An exact replay is accepted, which is what makes the key worth having.

## Reused, not forked

The observation vocabulary is `kiana_domain::EffectObservation` and its four states. This slice adds
no second effect-state enum, because a second one would be a second fact source about whether an
external effect happened.

## The command route

The reconciliation is reachable from a command: `effect.reconcile` in `ControlPlane::handle_command`
takes the observation, the proposed resolution and its supporting fields, and returns the sealed
receipt. Like the other routes in this batch it does **not** go through `authorize_and_execute` — the
decision answers "may this be recorded", and it executes no compensation, queries no provider and
issues no approval.

The operator requirement is the part with a real reason behind it. This decision is about **what an
already-sent request counts as** — retried, abandoned, or compensated — and each of those changes
consequences in the world. A Cell-internal worker taking part in ruling on what its own outbound
request came to is a witness for its own conduct.

One subtlety the route preserves: the request constructor seals and self-checks before the decision
runs, and when it refuses it hands back its own stable reason rather than a generic "invalid
request". A caller that sent a reused approval needs to be told *that*, because it is a different
problem from sending a malformed body.

## Honest limitations

This module **performs no lookup**. It does not query a provider, does not call a receipt-lookup
port, does not execute a compensation, does not append an event and does not issue an approval. The
observation and the external reference are both supplied by the caller, so a caller that hands over
a fabricated observation defeats this check exactly the way a lying `present_artifacts` inventory
defeated DEP-22. It decides whether a *proposed* resolution may be recorded; recording it still has
to go through `ControlPlane::handle_command` -- and the decision half of that is now reachable,
because `effect.reconcile` routes it. Recording the decision as a durable fact is still a
separate write that this slice does not perform. There is no
real provider, no real timeout and no real double charge anywhere in this slice, and no test was
executed locally.
