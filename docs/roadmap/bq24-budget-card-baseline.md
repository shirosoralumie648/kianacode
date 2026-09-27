# BQ-24 read-only budget card, queue, overage reason and correction approval baseline

> Snapshot date: 2026-09-28. This slice adds one read-only budget card, one queue projection, one
> overage-reason vocabulary and one correction-approval card at the CLI/entrypoint surface, plus the
> `/budget` command that routes them to versioned commands. Local Cargo
> test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions owns the fixtures
> and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-24`](../roadmap.md#step-bq-24) |
| source snapshot | master plus this BQ-24 budget-card slice |
| feature_status | `partial` for source-level card/queue/reason/correction display contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | versioned billing command → server `BillingQuery` response → read-only card → four-surface render |

`BudgetCard` is a display projection of what the server already computed. It has exactly one
constructor, `from_response`, and it takes a BQ-23 `BillingQueryResponse` plus the **server-resolved**
actor. `render_budget_card` in `kiana-commands/src/budget.rs` is the only path from a response to a
card, and it contains no budget arithmetic. `kiana-entrypoints/src/budget_presenter.rs` is the one
render path all four entrypoints share; `BudgetCardParity` records each surface's actual output and
refuses a set that disagrees.

`remaining` is a **carried field, not a computed one**. The type deliberately has no `remaining()`
helper: a local `limit - spent` drifts from the authoritative BQ-20 rollup the moment one page
lags, and a user acting on the drifted number is worse off than a user who sees `unknown`.

The correction row is not an apply path. `BudgetCorrectionCard` names the existing versioned
`kiana_domain::COST_CORRECTION_COMMAND` (`cost.correction`), binds to the command digest and to the
approval the submitter already holds, and prints `pending_approval` when no approval is attached. It
never mints an approval and never decides `approve`/`deny`. `BudgetCorrectionIntent` is what a
surface emits; it carries no decision field at all.

## The card shows what the server said, not what it could derive

| Field | Source | What the card refuses |
|---|---|---|
| `spent` | BQ-23 `summary` rollup, folded by the server | a mixed-currency rollup fails closed rather than picking a currency (`budget_card_rollup_invalid`) |
| `remaining` | a server field, carried verbatim | there is no subtraction in the module at all; a `remaining` without a `limit` is rejected |
| `limit` | a server field | inventing a limit the server did not state |
| `queue_depth` / `queue_limit` / `queue[]` | server-reported | a depth below the listed rows, a depth over the limit, duplicate tickets |
| `unknown_cost_count` | the response's server-side unknown count | defaulting it to `0` |
| `freshness` | the response | rendering `unknown` freshness as a number |
| `actor_id` / `project_id` | the response + server-resolved actor | any caller-supplied override |

An optimistic charge that was never committed has no server-side amount, so it cannot appear in
`spent`. A card may report `pending_reservations`, but it must not let that count stand next to a
non-`WithinBudget` state, because that pairing reads as "the pending amount is already inside
`spent`" (`budget_card_pending_state_invalid`).

## What the card rejects, and how

| Rejected | How |
|---|---|
| **UI 本地计算剩余额度** | the card has no subtraction, no `remaining()` helper, and a source guard fails the build if `checked_sub` / `saturating_sub` / `micros - ` ever appears in the module. A `remaining` whose value the server did not send has no digest, so `validate` refuses it |
| **乐观扣费未 commit** | `spent` reads only the server rollup. A pending reservation is reported as a **count**, never as an amount, and `budget_card_pending_state_invalid` refuses a count shown beside an at/over-limit state |
| **隐藏 unknown** | `UNKNOWN_TOKEN` is the single literal for an absent amount, count, wait estimate or freshness. A `BudgetState::Unknown` card may not also carry a `remaining` (`budget_card_unknown_state_numeric`); an `OverLimit` card must name a reason and a non-`OverLimit` card may not (`budget_card_overage_reason_missing` / `_unexpected`). Wire decoding re-validates, so a hand-written `"remaining": {"micros": 600}` is refused on decode |
| **输入 actor/project 覆盖服务端** | `/budget card --actor` and `--project` are **refused by name**, not ignored — a silently dropped flag would leave the user believing the card showed their own scope. `BudgetCardIdentityOverride::reject` exists as a type whose only method refuses, and `budget_presenter::reject_identity_override` does the same at the render boundary. Daemon-side, BQ-23's `billing_query_unauthenticated` already pins the actor to the server's `AuthenticatedPrincipal` |

Correction-specific rejections: a card may not name its own command (it must be
`cost.correction`); a card claiming an approval the command does not carry is refused
(`budget_correction_command_binding_mismatch`); evidence references are redacted and then scanned, so
a secret-shaped or unredacted reference is refused before it can reach a screen; a nil approval id
is refused rather than rendered as a real-looking UUID.

Parity rejections: fewer than four frames (`budget_card_parity_surface_count`), a duplicated
surface, a missing surface, a frame bound to another card, and — the one that matters — a frame whose
**rendered text** differs from the card's own (`budget_card_parity_render_mismatch`). Comparing
labels alone would let a surface print a fabricated remainder while reporting the correct state.

## Failure-first fixture matrix

`kiana-commands/tests/bq24_budget_card.rs` (29 tests) and
`kiana-entrypoints/tests/bq24_budget_presenter.rs` (9 tests), one per rejection, plus 6 source
guards (4 in `bq24_budget_card_guard.rs`, 2 in `bq24_budget_presenter_guard.rs`):

| Fixture | Assertion |
|---|---|
| no local remainder | a server limit with no remainder renders `remaining: unknown`, never `600` |
| unpaired remainder | a `remaining` with no `limit` is rejected |
| forged digest | a hand-edited `remaining` without re-sealing is rejected |
| mixed currency | a USD/EUR rollup fails closed instead of summing |
| pending is not spent | 3 pending reservations contribute nothing to `spent` and produce no remainder |
| corrections net out | `measured 400 + correction -150` renders as `250` |
| unknown freshness | state, freshness and spent all print `unknown`; no line ends in `: 0` |
| unknown with a number | an `Unknown` state carrying a `remaining` is rejected |
| reason required / forbidden | `OverLimit` without a reason, and `WithinBudget` with one, are both rejected |
| unknown survives the wire | the unknown count round-trips; `remaining` stays `null` |
| unknown reason mapping | `RateCardMissing` → `unknown_cost`, `ResultUnknown` → `reconciliation_required`, `None` → `not_overage` |
| identity flags | `--actor`, `--project`, `--project=` are all refused; a bare `card` still routes |
| presenter identity | `reject_identity_override` refuses both fields |
| card/response binding | a card validated against a different response is refused |
| read-only request | a request with `read_only = false`, a bad digest, a zero limit or a zero cursor is refused |
| correction routing | `correction request` emits `cost.correction`, not a surface-local command |
| correction needs a reason | `correction request` with no reason is refused |
| intent has no decision | the intent JSON has no `approve`/`deny`/`decision` key; a hand-edited actor is refused |
| queue over limit | `queue_depth 9 > queue_limit 4` is rejected; a ticket of 0, a blank session and a zero timestamp are rejected |
| unknown wait | a row with no wait estimate prints `wait_ms=unknown`, not `0` |
| duplicate ticket | two rows sharing ticket 3 are rejected |
| four-surface parity | all four surfaces render identically; a frame with an extra `remaining: 999999` line is refused |
| missing surface | three frames for four surfaces is refused |
| frame/card mismatch | a frame carrying another card's digest is refused |
| decode re-validation | a tampered `actor_id` or an injected `remaining` fails decode |
| correction card | no approval prints `pending_approval` and never `approved`; a non-`cost.correction` command, a nil approval id and secret-shaped evidence are all refused |
| correction binding | the card binds to the draft and to the approved command, and a foreign approval id is refused |
| wire shape | both the card and the intent deny unknown fields; the card and query schemas are distinct versioned names |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate, which compiles and tests
`--workspace`. No separate workflow. Local Cargo tests, builds, checks, clippy and smoke commands are
intentionally not run, and CI results are not awaited.

### What this does NOT prove

- **No live numbers.** Every figure in the fixtures is hand-built. Nothing here shows that a real
  provider charge reaches the card, that a real invoice matches it, or that a real budget is
  enforced.
- **No daemon wiring.** `/budget` emits `budget.query.v1`, and the current DaemonHost has no handler
  for that command name — the BQ-23 `BillingQuery` body still returns
  `billing_query_projection_not_wired`. The card is proved against a synthetic response, not against
  a served one. Wiring `budget.query.v1` to the ControlPlane read surface is follow-up work.
- **`remaining` and `limit` are not yet populated.** Until the daemon supplies them, `render_budget_card`
  passes `None` and the card honestly prints `unknown`. That is the correct behaviour for an
  unpopulated field, but it means the "shows the remainder" half of the success criterion is
  **contracted, not demonstrated**. The daemon must compute them; no surface may.
- **Correction approval is not exercised end to end.** The card binds to a
  `CostCorrectionCommand` and its `CostCorrectionApproval` structurally, and refuses a foreign
  approval id. It does not show that submitting through the existing approval path actually settles
  a correction, and it does not touch the approval machinery.
- **Parity is proved for the presenter, not for four running surfaces.** `BudgetCardParity` proves
  that the shared render path is the only path and that a divergent frame is refused. It does not
  start four real entrypoints against one daemon and diff their screens; `contrib/desktop` is not
  wired to this presenter.
- **The source guard asserts source text, not behaviour.** It fails the build if a subtraction or a
  second budget state appears in these three files. It cannot prove a number on screen is correct,
  and it will need updating if the modules are legitimately refactored.
- **No secret scanning of rendered text.** Evidence references are redacted and scanned on the card;
  the rendered text itself is not passed through `scan_secret_sentinels` at this boundary. BQ-25 owns
  the receipt/telemetry separation check.
- **Server-side project authorization is out of scope.** `BudgetCard::from_response` trusts the
  response's `project_id` because the BQ-23 boundary check already bound it. Nothing here re-derives
  whether that actor may see that project; BQ-23's adapter and the ControlPlane own that.
