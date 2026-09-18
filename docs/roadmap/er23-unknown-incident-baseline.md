# ER-23 Unknown incident and RecoveryPlan baseline

ER-23 turns an uncertain runtime outcome into a typed, read-only incident with a
server-derived RecoveryPlan. Plans begin `proposed`, keep safe/forbidden actions
monotonic, and can only move through `approved` → `executing` → `verified`/`failed`/
`abandoned`. Every human transition is committed on the existing platform stream with
actor, source-event evidence, revision/CAS and an idempotency key; approval requires an
independent reviewer/sponsor and cannot self-approve the source actor.

`failure.incidents` projects journal failure/provider/MCP/disk/orphan/data-uncertainty
facts and Human Inbox exposes the same incident/recovery actions. `failure.recovery`
never retries, closes or mutates the original runtime outcome; unknown effects remain
reconciliation-bound. GitHub Actions runs P2-K6, CP-20 and ER-23 source guards plus
workspace target compilation. Local runtime tests and smoke commands are intentionally
not run.

This is source/static evidence only. It does not claim an external incident service,
cross-process durable queue, live provider reconciliation or physical proof.
