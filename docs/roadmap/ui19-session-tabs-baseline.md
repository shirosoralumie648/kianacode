# UI-19 Web session ownership and multi-tab baseline

> Snapshot date: 2026-10-17 (completion slice; original narrow slice 2026-09-25). UI-19 keeps a
> bounded server lease projection and tab-local Web draft/submission state on the existing
> loopback Web → DaemonHost → ControlPlane path, and closes the card exit conditions with a typed
> tab/session relation graph, payload-digest-bound action idempotency with a replay marker, an
> encoded conflict-copy catalogue and real loopback runtime fixtures. GitHub Actions owns runtime
> verification; local Cargo tests, builds, checks, clippy and smoke commands are intentionally
> not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-19`](ui-entrypoints.md#step-ui-19) |
| source snapshot | master `eeccb1ca` (CO-12 merge point) plus this UI-19 completion slice (branch `step/ui-19-20261017-2`) |
| feature_status | `implemented` (bounded lease, owner checks, payload-bound idempotent replay, typed tab relations, conflict copy and runtime fixtures) |
| proof_level | `source`; CI is configured but its result is not awaited |
| canonical path | Browser tab → loopback Web → authenticated server principal → DaemonHost → ControlPlane → Broker |

## Ownership and tab contract

`kiana.ui-tab-session.v1` binds the server authenticated principal, session, tab, lease epoch and
token generation, and carries the disposition `owner`/`observer`/`revoked`/`closed` with the
`feed_only` invariant (`feed_only` is true for everything except `owner`). The first bootstrap tab
receives the mutation lease. Other tabs may attach the same owner-scoped read-only feed, but
`run`, `cancel`, approval, resume and command mutations require the exact owner tab. A tab id is a
scope label, never a credential or a permission grant.

`kiana.ui-tab-relations.v1` is the shared, id-only relation graph: one edge per tab with its
`owner`/`observer` kind and lease epoch, plus the server-owned `owner_tab_id`, lease epoch, token
generation, `max_tabs` and observer count. Validation rejects a bad header, duplicate tab ids, an
owner edge that does not match `owner_tab_id`, conflicting owners and a forged observer count; the
graph carries no draft, prompt, approval payload or credential. `/api/state` derives only the
requesting tab's own `tab_session`; the relation graph itself is shared and id-only.

Draft text and submission identity are kept in per-tab memory. A submission carries a stable
action id/idempotency key bound to the payload digest; a repeated click or lost HTTP response
reuses it and returns the original cached response plus the additive `web_action_replay` marker
(`schema` = `kiana.web-action-payload.v1`, `action_id`, `payload_digest`, `replayed: true`,
`retry: query_original`), while the same key with a different payload is refused with
`ui_action_payload_mismatch` (409) without consuming the key. A second tab, principal or target
for that key is rejected. The existing UI cursor CAS remains a server-side stale-card fence
before the handler reaches the ControlPlane.

## Conflict copy catalogue

`kiana.web-tab-conflicts.v1` (`WebTabConflictCopy` in `kiana-client`) is the bounded, display-only
catalogue of 16 stable refusal codes (including `web_auth_required`), with
`WEB_TAB_CONFLICT_MAX_REASONS = 8` reasons retained per tab. Every entry fixes its stage
(`session`/`submission`) and the only retry a tab may describe: query the original command,
refresh the snapshot first, switch to the owner tab, or do not retry. The catalogue can grant no
lease, retry no command by itself and never decides that an unknown result succeeded.

## Deny-first and recovery matrix

| Boundary | Contract |
|---|---|
| cross-tab mutation | observer tab can consume feed but `session_owner_required` denies action |
| stale card/CAS | every mutation requires the action envelope; server `claim_ui_headers` rejects missing/old epoch/cursor before dispatch |
| duplicate click | bounded submission registry returns the original response plus the `web_action_replay` marker; no second handler effect |
| changed payload | the same action key with a different payload digest is refused with `ui_action_payload_mismatch` (409) and does not consume the key |
| close tab | `beforeunload` only closes SSE; it never posts `/api/cancel` or beacon mutation |
| token rotation | server token generation is included in lease and relation metadata; old token is unauthorized (`web_auth_required`) |
| owner exit | lease loss does not synthesize cancellation; receipt remains authoritative |
| race | session owner and UI cursor checks are serialized before the existing execution spine |
| feed observation | every tab subscribes through `/api/events` with its own tab id; no shared mutable draft |
| bad projection | an invalid `tab_session`/`tab_relations` projection fails closed with `web_tab_projection_invalid` instead of painting a guess |

## Fixtures and CI evidence

`ui19-session-tabs.json` is a declarative `kiana.ui19-session-tabs.v1` matrix (13 deny-first +
8 success cases) that binds each case to one runtime fixture or guard function. The four runtime
fixtures drive a real loopback `DaemonHost` router over the embedded page and the versioned wire
envelopes: `web::tests::ui19_owner_and_observer_share_one_snapshot_without_leaking_drafts`,
`ui19_duplicate_click_replays_the_original_result_and_refuses_a_changed_payload`,
`ui19_missing_or_stale_action_envelopes_are_refused_without_consuming_the_key` and
`ui19_owner_exit_and_token_rotation_keep_observers_read_only`. `ui19_session_tabs.rs` binds the
fixture to the protocol/client/Web constants and the page catalogue; `ui19_session_tabs_guard.rs`
keeps tabs out of execution authority (`ui19_deny_first_guard_keeps_tabs_out_of_execution_authority`)
and binds the source and page to the tab lease contract
(`ui19_source_and_page_bind_the_tab_lease_contract`). The manual lane
`.github/workflows/ui19-session-tabs.yml` (`workflow_dispatch`-only, `permissions: contents: read`)
runs fetch, the fmt gate, both fixture targets with `--test-threads=1`, the
`--lib web::tests::ui19_` filter and a `--no-run` workspace test compile; the automatic shard stays
the whole-crate `kiana-entrypoints` shard of `ci.yml` (`scripts/ci/test-shards.json` unchanged).
Results are intentionally not awaited.

## Limitations

- Lease/submission maps and token generation are bounded Web process memory; restart, multi-process
  deployment and external browser authentication are not durable/live proof.
- Cached replay is the HTTP response projection; the ControlPlane/EventLog receipt remains the
  authority for actual effects and `result_unknown` reconciliation.
- No browser automation, real sleep/wake timing, external provider, network, OS or physical proof
  is claimed. Closing a tab is deliberately not treated as a cancellation command.

## Evidence block

```text
source_snapshot: master eeccb1ca (CO-12 merge point) plus this UI-19 completion slice (branch step/ui-19-20261017-2; source commit a3a6ac88)
worktree_status: isolated worktree /workspace/kianacode-worktrees/UI-19; protocol/client contracts, Web projections, page wiring, fixtures, guard tests, manual lane and docs only
command_argv: cargo fmt --all; cargo fmt --all -- --check; local static inspection (rg/python/node --check) only
cwd·environment: /workspace/kianacode-worktrees/UI-19; Linux/bash; no Cargo build/test/check/clippy/run, no smoke; no live provider
fixture·cassette: kiana-entrypoints/tests/fixtures/ui19-session-tabs.json plus web::tests::ui19_* over a real loopback DaemonHost router; no external effect
exit_code: local rustfmt and static checks only; GitHub Actions owns compile and behaviour verification and its result is not awaited
status change: UI-19 🔄 → ✅ (CI pending); typed tab/session relation graph, payload-digest-bound idempotent replay with replay marker, bounded conflict-copy catalogue, four real loopback runtime fixtures and the rewritten fixture/guard/lane bindings added
proof-level change: feature_status=implemented; proof_level=source only
limitations: no browser/runtime/durable/live/physical proof; bounded process-local leases and replay cache; CI receipt not awaited
reviewer: this session's source self-review against the card's deny-first/success list; independent GitHub CI reviewer pending
```
