# INT-17 connector/account/project quota baseline

## Scope

INT-17 adds a server-owned connector quota contract on top of the INT-16 invocation reservation and
the BQ-16 provider capacity boundary. A canonical key binds connector/version, account, project,
normalized alias and credential generation. One reservation carries connector, account and project
rate dimensions, concurrency, budget upper bounds, authority/configuration/rate-card and capacity
lease digests. ControlPlane persists reservation, worker claim and settlement facts through the
existing EventStore CAS stream; Broker/provider adapters only revalidate those facts at the effect
boundary.

## Implemented source slice

- `ConnectorQuotaKey`, `ConnectorQuotaPolicy` and `ConnectorQuotaReservation` validate UTC window,
  connector/account/project limits, canonical alias, credential generation, owner/command digest,
  authority/configuration epoch, optional RateCard and BQ-16 capacity lease.
- `ConnectorQuotaLedger` models atomic reserve, one-winner worker claim, owner-bound release and
  replayable settlement. Over-limit, stale revision, old epoch, alias/credential mismatch and
  cross-owner release fail closed. Unknown usage remains a visible state and is never converted to
  success.
- `ControlPlaneConnectorQuota` writes reservation/claim/settlement facts with the existing
  `TransitionBatch`/EventStore CAS spine. Broker and provider helpers recheck quota, claim,
  credential generation and capacity lease before the existing permit/adapter boundary.
- `kiana-query` rebuilds a read-only projection from quota facts, preserving Unknown/Released
  states and source cursors. Domain/Core/Daemon/Provider/Query fixtures and source guards plus a
  GitHub-only workflow cover deny-first paths and replay shape.

## CI-only evidence

GitHub Actions runs `cargo fetch --locked`, formatting, INT-17 domain/provider/core/daemon/query
fixtures and workspace test-target compilation. Local Cargo test/build/check/clippy/smoke commands
are deliberately not run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The ledger and in-memory fixtures are reference contracts, not a cross-process durable quota store.
EventStore integration has no completed restart/crash or two-worker live proof; rate usage is
window-bound source state and no external provider billing truth is asserted. Settlement accepts an
explicit Unknown outcome and does not reconcile provider invoices, network effects or physical
actions. Legacy connector callers without the additive quota envelope remain readable until later
migration; live transports, restart recovery, and physical proof remain INT-18..33 / ER / PD work.

## 2026-10-17 completion slice (CI-only)

The card exit conditions are now covered end to end by executable fixtures, all deny-first:

- Domain (`kiana-domain/tests/int17_connector_quota.rs`, 3 -> 10): restart without a persisted
  ledger fails closed and a reopened ledger still charges the window's existing usage
  (`connector_quota_restart_state_missing`, `connector_quota_old_epoch`, then
  `connector_quota_connector_limit_exceeded`); per-scope (connector/account/project) over-limit
  codes for requests, tokens, budget and concurrency; forged policy/reservation/claim/settlement
  shapes; one-winner claim CAS with owner-bound release; exact settlement replay with Unknown
  kept visible; effect admission fences; idempotency replay/conflict/window codes.
- Source hardening: `ConnectorQuotaLimit::admits_without_usage` makes
  `connector_quota_effect_admission` re-apply the policy's absolute ceiling at the effect
  boundary, so a self-consistent but over-limit reservation (for example forged
  over-concurrency) cannot dispatch; `ConnectorQuotaSettlement::validate_identity_against` gives
  read models that keep the latest CAS snapshot an identity/command fence without weakening the
  write-path `validate_against`.
- Broker (`kiana-capability-broker/tests/int17_connector_quota.rs`): the real sealed
  `CapabilityBroker` with a recording handler; missing claim/policy, orphan claim/policy,
  tampered digests, stale claim revision, expired window, over-limit dimensions and policy fence
  drift leave the handler at zero calls, and the admitted envelope reaches it exactly once while
  legacy requests keep the compatibility path.
- Provider (`kiana-provider/tests/int17_connector_quota.rs`): `validate_connector_quota_lease`
  refuses expired reservations, policy fence drift, stale claims, foreign/missing capacity leases
  and tampered digests before transport and accepts the exact triple.
- Query (`kiana-query/tests/int17_connector_quota.rs`): rebuilt reserved/claimed/settled/released
  states, settlement preserved across a later release (the projection entry now binds a
  settlement by identity instead of the latest snapshot digest), Unknown usage visible without a
  numeric zero, tampered payloads/schema drift/duplicate or oversized projections refused,
  deterministic replay.
- Daemon (`kiana-daemon/tests/int17_connector_quota.rs`): `DaemonHost -> ControlPlane -> Broker ->
  EventStore`; a wire-supplied quota envelope is refused with zero connector facts, and a
  disk-backed restart proves the per-binding rate ceiling is rebuilt from the durable EventLog
  (third invocation still `connector_rate_limit_exceeded`, replay returns the original receipt and
  event id) rather than an in-memory counter resetting to zero.
- Guards (`kiana-core/tests/int17_connector_quota_guard.rs`,
  `kiana-daemon/tests/int17_connector_quota_guard.rs`) pin the refusal codes, fixture names, the
  manual lane steps and the automatic domain/core shards; the manual lane
  `.github/workflows/int17-connector-quota.yml` is `workflow_dispatch`-only with
  `permissions: contents: read`, a fetch + fmt gate, six serial targets and six `--no-run` steps.
  `scripts/ci/test-shards.json` is unchanged: `kiana-domain-s3/4` owns `int17_connector_quota`,
  `kiana-core-s4/6` owns `int17_connector_quota_guard`, and the broker/provider/query/daemon
  shards are whole-crate.

Known limits of this slice: no cross-process CAS/checkpoint proof, ControlPlane does not yet emit
quota facts for every caller (the additive envelope stays optional and readable for legacy
callers), no provider billing reconciliation and no live/physical connector effect. Local
build/test/check/clippy/smoke were not run in this session; CI receipt is not awaited and the
proof level stays `source`.
