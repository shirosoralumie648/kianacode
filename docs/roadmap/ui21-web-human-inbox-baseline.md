# UI-21 Web Human Inbox and approval action cards baseline

> Snapshot date: 2026-10-18 (base `e4611ee3`). UI-21 adds a typed Web Human Inbox projection and
> action intent boundary on top of the existing server-owned `human.inbox`/`human.resolve`
> contracts. This completion slice closes the card's remaining resolve-action, form-schema,
> duplicate-click and response-loss gaps. GitHub Actions owns fixture and compile verification;
> the focused local fixture/guard targets below were run and pass.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-21`](ui-entrypoints.md#step-ui-21) |
| source snapshot | `e4611ee3` (`fix: restore workspace compile on origin/master`) plus this UI-21 completion slice |
| feature_status | `implemented` (bounded Web card projection, typed form/intent validation, explicit unsupported-decision refusal, deny-first fixtures and CI wiring) |
| proof_level | `local_behavior` for the focused fixture/guard/unit targets that pass locally; the live loopback staging fixtures are blocked by a pre-existing baseline event-contract defect (below), so no live proof is claimed |
| canonical path | server `human.inbox` projection → typed Web action card → versioned intent → `DaemonHost` → `ControlPlane` → existing authority/Broker paths |

The browser receives reason, effect scope summary, expiry, expected revision, required fields,
allowed decisions and an opaque payload digest from the server. It can retain tab-local form data
and emit `UiHumanActionIntentV1`; it cannot choose an actor, add a scope path, manufacture an
approval, or execute a tool. The existing `human.resolve` authority remains responsible for
approval/company/failure/feedback dispatch and final policy/CAS checks.

## Resolve-action scope (what the backend can actually honor)

`ControlPlane::human.resolve` dispatches only: `approval` approve/deny, `company.business`,
`company`, and `failure.reconcile`/`failure.recovery`/`feedback.review`. `ApprovalDecision` has no
amend variant, so approval **edit** has no backend authority; workspace/checkpoint **restore** is a
separate `workspace.checkpoint.restore` command and run **continue** is the separate
`ControlPlane::continue_run`/`/api/resume` path. Rather than inventing an amend/restore/continue
intent that would silently collapse into `unmapped` (and then `human_action_unavailable`), the
server and client refuse those three ids by name with the stable code
`web_human_resolve_action_unsupported` plus the action id:

| action id | audit disposition | refusal reason |
|---|---|---|
| `edit` | `refused` | `no_approval_amend_authority` |
| `restore` | `refused` | `restore_is_a_separate_checkpoint_command` |
| `continue` | `refused` | `continue_is_a_separate_resume_command` |

The refusal table is a bounded `[WebHumanActionRefusal; 3]`, has no executor reference, and is
enforced after intent deserialization but before any form value is read. The audit mapping matches
the full `(command, action_id)` row; the intent wire shape carries no `command`, so the resolve
boundary refuses the three reserved ids by name. A refused card is never rendered as a submit
button, never gains `dangerous`/`effect_dispatch`, and never reaches `claim_action_submission`.

## Typed contracts

| Surface | Contract | Deny-first behavior |
|---|---|---|
| protocol | `UiHumanInboxV1`, `UiHumanActionCardV1`, `UiHumanActionIntentV1` | schema, digest, bounds, tuple-safe duplicate item/action identity and unknown top-level fields fail closed |
| ControlPlane | `human.resolve` payload/revision recheck | supplied card payload digest and expected revision are compared with the authoritative action before approval/company/failure dispatch |
| client | `WebHumanInbox::replace/prepare_intent` | duplicate card, observer/owner mismatch, expired/revoked card, stale inbox/card revision, missing/hidden/required field, illegal value and oversized form are rejected before an intent exists; `edit`/`restore`/`continue` are refused with the stable code |
| Web server | `annotate_human_inbox_response`, `validate_human_action_command`, `refuse_unsupported_resolve_action` | action metadata is additive/read-only; malformed or forged intent is rejected before idempotency claim and then forwarded only through the existing command path |
| browser | `humanActionCardFor`, `humanActionUnsupported`, `validateHumanActionFields`, `submitHumanAction` | form is driven by server card; payload digest and expected revision are preserved; unsupported ids are refused before the form is read; no actor/scope fields are accepted |
| result display | `renderHumanActionResult` | Applied/Accepted/Rejected/Unknown remain distinct; incident and limitations stay visible; Unknown instructs query-original and never auto-retries |

## Form schema and value refusals

Missing required fields produce `web_human_required_field_missing`; an explicit `null` on a
required field is the same refusal (it cannot clear a required field), while `null` on an optional
field still clears it. Illegal values fail closed per declared type: unknown declared type →
`web_human_field_type_unknown`, wrong JSON type → `web_human_field_value_type_invalid`, empty
required string → `web_human_field_value_empty`, value outside `allowed_values` →
`web_human_field_value_not_allowed`, over 64 list items →
`web_human_field_value_limit`, over 4 KiB string → `web_human_field_value_too_long`, and any
undeclared field → `web_human_field_denied` (a hidden field can never widen scope). None of these
paths produce an intent.

## Duplicate click and response loss

The browser action id is the single submission identity: `stableSubmissionId(...)` is computed
before `submitCommand('human.resolve', ...)` and passed as `idempotency_key`. A second click with
the same identity and payload hits the server's `WebActionClaim::Replay` branch and receives the
recorded original response (`replayed: true`, `retry: query_original`) instead of a second effect;
`claim_action_submission` still checks `payload_digest` first, and an in-flight duplicate is
refused with `ui_action_in_flight`. A lost or `unknown`/`result_unknown` response is reconciled only
by querying the original command with that same key — the result panel never retries or calls
`/api/run`/`/api/cancel`.

## Failure-first matrix

| Fixture / guard | Assertion |
|---|---|
| card/inbox fixture | server fields, card/intent schema and authority chain remain explicit |
| approval denial | expired/revoked cards and stale inbox/card revisions cannot produce an action |
| ownership denial | observer or wrong tab cannot submit; server validates before the replay/idempotency slot is claimed |
| payload/scope denial | unknown form fields, top-level actor/scope injection and digest mutation fail closed |
| unsupported decision | `edit`/`restore`/`continue` are refused with `web_human_resolve_action_unsupported:<id>`; the audit matrix records `refused` plus a reason and never `human_action_unavailable` |
| required/illegal values | missing required, required `null`, wrong type, empty string, disallowed value, list/string over limit and hidden field all refuse before an intent exists |
| duplicate click | same browser action id + payload replays the recorded result through `WebActionClaim::Replay`; the second submit never produces a second effect |
| response loss | `unknown`/`result_unknown` stays visible and is reconciled only by query-original with the same idempotency key |
| replay/recovery | stable idempotency key returns original command result; close/refresh does not cancel |
| result guard | incident and limitation metadata are text-only visible and do not trigger `/api/run`, `/api/cancel` or a retry |
| audit table size | `[WebHumanAuditRule; 14]` and `[WebHumanActionRefusal; 3]` stay in sync with their fixture rows |

## Evidence block

```text
source_snapshot: e4611ee3 (fix: restore workspace compile on origin/master) plus UI-21 completion slice
worktree_status: D:\code\kiana-steps\ui21 on step/ui-21-20261018-1; protocol/client/Core/Web contracts, browser card renderer, fixture, guards, workflow and baseline
command_argv:
  cargo test -p kiana-client --test ui21_human_inbox --locked --offline -- --test-threads=1            -> exit 0 (8 passed)
  cargo test -p kiana-entrypoints --test ui21_web_human_inbox --locked --offline -- --test-threads=1   -> exit 0 (6 passed)
  cargo test -p kiana-entrypoints --test ui21_web_human_inbox_guard --locked --offline -- --test-threads=1 -> exit 0 (8 passed)
  cargo test -p kiana-entrypoints --lib web::tests::ui21_ --locked --offline -- --test-threads=1       -> exit 101 (2 passed, 2 failed; baseline defect)
  rustfmt --edition 2021 on touched Rust files; git diff --check                                      -> exit 0
cwd·environment: D:\code\kiana-steps\ui21; Windows 11, rustc stable; CARGO_TARGET_DIR=D:/code/kiana-steps/target-ui21; CARGO_BUILD_JOBS=4; RUST_MIN_STACK=33554432 for lib tests
fixture·cassette: kiana-entrypoints/tests/fixtures/ui21-web-human-inbox.json; duplicate/expiry/revocation/CAS/owner/hidden-field/required-value/payload/idempotency/duplicate-click/response-loss/unsupported-decision cases; no provider, filesystem or external effect contacted
exit_code: 0 for the three focused targets; 101 for the live staging filter (see limitations)
status_change: UI-21 card remains 🔄; completion slice adds the explicit unsupported-decision refusal table, required/illegal-value refusal, duplicate-click/response-loss proofs and baseline/workflow updates
proof-level change: proof_level local_behavior for the focused client/entrypoints fixture and guard targets; no live loopback proof
limitations: the two live `web::tests::ui21_*` staging fixtures (and the unrelated live `loopback_health_and_cassette_write_go_through_daemon_host`) fail before the approval stage with `port_failed:event_contract_event_payload_unknown_field`: `kiana-core/src/capabilities.rs` writes `input_digest` into `run.tool_call` but `kiana-domain/src/event_contracts.rs` `RUN_TOOL_CALL_FIELDS` was not extended, a base-commit (`e4611ee3`, from CAP-25) defect outside this card's scope. Also absent: browser E2E, screenshot/golden, real HTTP/session lease race, durable cross-process inbox, provider/Broker effect, approval identity authentication, incident reconciliation and live/physical proof; the UI remains a projection and final authorization is server-owned.
reviewer: UI-21 completion source review; checked server-owned card fields, no actor/scope expansion, expiry/revocation/revision/owner/idempotency fences, unsupported-decision refusal, required/illegal value refusal, Unknown/incident/limitation visibility and no second execution loop
```
