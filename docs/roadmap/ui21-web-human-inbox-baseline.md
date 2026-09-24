# UI-21 Web Human Inbox and approval action cards baseline

> Snapshot date: 2026-09-25. UI-21 adds a typed Web Human Inbox projection and action intent
> boundary on top of the existing server-owned `human.inbox`/`human.resolve` contracts. GitHub
> Actions owns fixture and compile verification; local Cargo tests, builds, checks, clippy and
> smoke commands are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-21`](ui-entrypoints.md#step-ui-21) |
| source snapshot | `e44434bf` (UI-20/INT-13/BQ-17) plus this UI-21 source slice |
| feature_status | `implemented` (bounded Web card projection, typed form/intent validation, deny-first fixtures and CI wiring) |
| proof_level | `source`; CI is configured but its result is not awaited |
| canonical path | server `human.inbox` projection → typed Web action card → versioned intent → `DaemonHost` → `ControlPlane` → existing authority/Broker paths |

The browser receives reason, effect scope summary, expiry, expected revision, required fields,
allowed decisions and an opaque payload digest from the server. It can retain tab-local form data
and emit `UiHumanActionIntentV1`; it cannot choose an actor, add a scope path, manufacture an
approval, or execute a tool. The existing `human.resolve` authority remains responsible for
approval/company/failure/feedback dispatch and final policy/CAS checks.

## Typed contracts

| Surface | Contract | Deny-first behavior |
|---|---|---|
| protocol | `UiHumanInboxV1`, `UiHumanActionCardV1`, `UiHumanActionIntentV1` | schema, digest, bounds, duplicate item/action and unknown top-level fields fail closed |
| ControlPlane | `human.resolve` payload/revision recheck | supplied card payload digest and expected revision are compared with the authoritative action before approval/company/failure dispatch |
| client | `WebHumanInbox::replace/prepare_intent` | duplicate card, observer/owner mismatch, expired/revoked card, stale inbox/card revision, missing/hidden field and oversized form are rejected before an intent exists |
| Web server | `annotate_human_inbox_response`, `validate_human_action_command` | action metadata is additive/read-only; malformed or forged intent is rejected before idempotency claim and then forwarded only through existing command path |
| browser | `humanActionCardFor`, `validateHumanActionFields`, `submitHumanAction` | form is driven by server card; payload digest and expected revision are preserved; no actor/scope fields are accepted |
| result display | `renderHumanActionResult` | Applied/Accepted/Rejected/Unknown remain distinct; incident and limitations stay visible; Unknown instructs query-original and never auto-retries |

## Failure-first matrix

| Fixture / guard | Assertion |
|---|---|
| card/inbox fixture | server fields, card/intent schema and authority chain remain explicit |
| approval denial | expired/revoked cards and stale inbox/card revisions cannot produce an action |
| ownership denial | observer or wrong tab cannot submit; server validates before the replay/idempotency slot is claimed |
| payload/scope denial | unknown form fields, top-level actor/scope injection and digest mutation fail closed |
| replay/recovery | stable idempotency key returns original command result; close/refresh does not cancel; response Unknown is visible and query-original only |
| result guard | incident and limitation metadata are text-only visible and do not trigger `/api/run`, `/api/cancel` or a retry |

## Evidence block

```text
source_snapshot: e44434bf (UI-20/INT-13/BQ-17) plus UI-21 source slice
worktree_status: isolated /tmp/kiana-step-ui21; protocol/client/Core/Web contracts, browser card renderer, fixture, guards, workflow and baseline
command_argv: rustfmt --edition 2021 on target UI-21 Rust files; git diff --check; GitHub Actions will run cargo fetch --locked, cargo fmt --all --check, UI-21 fixtures/guards and cargo check --workspace --tests --locked
cwd·environment: /tmp/kiana-step-ui21; Linux/bash; local Cargo test/build/check/clippy/smoke deliberately not run; GitHub Actions is the test authority and is not awaited
fixture·cassette: kiana-entrypoints/tests/fixtures/ui21-web-human-inbox.json; duplicate/expiry/revocation/CAS/owner/hidden-field/payload/idempotency/Unknown/incident/limitation/close-tab cases; no provider, filesystem or external effect contacted
exit_code: target-only rustfmt and git diff --check only; remote fixtures, workspace compile and CI exit codes are pending/unobserved
status_change: UI-21 ⏳ → 🔄; typed protocol/client/server card and intent contracts, Web renderer, deny-first source guards and GitHub workflow added
proof-level change: feature_status=implemented for bounded Web source contracts; proof_level=source only
limitations: no browser E2E, screenshot/golden, real HTTP/session lease race, durable cross-process inbox, EventLog append/replay, provider/Broker effect, approval identity authentication, incident reconciliation or live/physical proof; UI remains a projection and final authorization is server-owned
reviewer: Codex UI-21 source review; checked server-owned card fields, no actor/scope expansion, expiry/revocation/revision/owner/idempotency fences, Unknown/incident/limitation visibility and no second execution loop; no local runtime test reviewer
```
