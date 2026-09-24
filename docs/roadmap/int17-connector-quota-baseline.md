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
