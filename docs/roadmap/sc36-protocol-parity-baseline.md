# SC-36 protocol parity across CLI, Workbench, Web, Desktop and daemon

> Snapshot date: 2026-09-28. Local Cargo **tests were not executed**; GitHub Actions owns fixtures
> and the workspace gate.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SC-36`](security-compliance.md#step-sc-36) |
| code landing | `kiana-client/src/protocol_parity.rs`, registered by `kiana-client/src/lib.rs` |
| fixtures | `kiana-client/tests/sc36_protocol_parity.rs`, `kiana-client/tests/sc36_protocol_parity_guard.rs` |
| feature_status | `partial` — the comparison is decidable in source; nothing invokes it from a gate |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |

## What UI-31 already had, and what this card adds

UI-31 compares four UI surfaces on command identity, cursor, disposition, retry and receipt
digest. That catches disagreement. It cannot catch **a surface that is right for the wrong reason**,
and that is the class of bug SC-36 names:

| Card failure | Why UI-31 cannot see it | Refusal here |
|---|---|---|
| 入口显示不同状态 | UI-31 does see a state mismatch | `protocol_parity_mismatch:{disposition,retry,cursor,revision,receipt,error_code}` |
| 局部 approve | A locally applied approval does not change any compared field | `protocol_parity_approval_applied_locally` |
| 不同 redaction | Redaction never entered the trace | `protocol_parity_redaction_profile_mismatch` |
| 本地计算权限 | A surface that computes the right answer looks identical to one that was told | `protocol_parity_locally_computed_decision` |

Two more are added because the same reasoning applies: an approval shown as decided without naming
the server-issued reference (`protocol_parity_approval_reference_missing`,
`protocol_parity_approval_reference_mismatch`), and an `unknown` disposition rendered as settled
(`protocol_parity_unknown_rendered_as_settled`). Agreeing that "it was approved" is not agreement
unless the surfaces also agree on *which* approval; and `unknown` rendered as finished is an
invention the user will act on.

## Why a new enum instead of a fifth variant on `ParitySurface`

`ParitySurface` has four variants and a fixed `REQUIRED_SURFACES` array. Adding `Daemon` to it
would silently widen what UI-31 accepts, and a parity check that changes meaning when nobody is
looking is precisely the failure this card is about. The daemon is also not a peer: it is the
surface that *issued* the decision the others render. So `ProtocolSurface` has five variants and
`compare_protocol_parity` takes the daemon observation as the **reference**, recording
`reference_surface` in the report so the direction of the comparison is never ambiguous.

## The check order is deliberate

Authority is decided **per surface, before anything is compared**. A surface that computed its own
decision may well agree on every field; the point is that agreement is not the property being
tested. Validating first means such a surface is refused for the real reason rather than passing
because it happens to match, and it means the failure names the bug instead of a downstream
symptom.

## Honest limitations

This is a comparator over supplied observations. It opens no transport, starts no process, runs no
command, appends no event and reads no file — the guard asserts the absence of each of those tokens.
It is not wired into a release gate or into any entrypoint, so no surface is actually forced
through it yet.

**Why there is no `ControlPlane::handle_command` route, and why that is not an oversight.** The two
sibling routes this batch added — `security.incident.evaluate` and `promotion.check` — are both
reachable from `handle_command` because their types live in `kiana-core`. This comparator's types
live in `kiana-client`, and `kiana-core` does not depend on `kiana-client` (nor the reverse): the
two are siblings under `kiana-domain`. A route would therefore require adding a dependency edge
between them purely so that a command handler could name a type.

That edge is the wrong trade. Parity is a property of the client protocol — it compares what the
surfaces *said* — so the comparator belongs where the protocol DTOs are, and pulling it into the
control plane would make the control plane aware of surfaces it does not own. The honest statement
is therefore the structural one: this gate is reachable from wherever `kiana-client` surfaces are
driven, and closing that would be a different decision about where parity is enforced rather than a
missing branch. It compares what a surface *reports*; a surface that
reports faithfully and behaves differently is not caught here, and neither is a surface that never
produces an observation at all — a missing surface is `protocol_parity_surfaces_missing`, but a
surface that is silently absent from the product is invisible to this check. No real browser, PTY,
Electron or ACP host was involved; no golden trace was replayed end to end; nothing here proves two
real surfaces agree, only that two supplied records can be held to the same rule.
