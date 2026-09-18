# AUT-05 Automation Event / CAS Baseline

## Scope

AUT-05 makes the existing workflow EventLog boundary explicit.  A committed workflow command
fact carries a strict `kiana.workflow-event-envelope.v1` with aggregate identity, stream version
and cursor, request/idempotency key, command digest, payload digest and envelope digest.  Control
Plane stamps the envelope immediately before the existing protected EventStore CAS commit and
revalidates it during replay; legacy events without the additive envelope remain readable for
migration but do not upgrade proof.

`EventStorePort::read_stream_after` adds a bounded cursor query without turning a read into a claim.
Existing `append_idempotent_expected`/`commit_transition` and `with_stream_metadata` remain the
only commit/CAS boundary; same idempotency key with changed command/payload is a conflict, and a
stale stream version cannot produce a committed effect.

## Evidence and limits

- Domain fixtures cover round-trip, unknown-field, cursor/command/payload binding and tamper
  rejection.
- Core source guard covers envelope stamping/replay validation, aggregate stream metadata and
  idempotency/CAS; GitHub Actions runs it and compiles the workspace.
- Legacy workflow facts are accepted without the additive envelope during migration.  Durable
  indexed cursor/projector queries, multi-scheduler contention and torn-tail recovery remain
  PD/ER/AUT-08+ work.

This slice is `feature_status=implemented`, `proof_level=source`; no local tests, live scheduler,
external effect or physical proof is claimed.

