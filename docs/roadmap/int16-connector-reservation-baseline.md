# INT-16 Connector invocation reservation and idempotency baseline

## Scope

INT-16 binds one connector invocation to a server-owned command digest, idempotency key digest,
binding/operation/owner/authority/policy/configuration/credential/data revisions and a short-lived
reservation lease. A committed reservation issues one permit for one attempt. The Broker and
daemon recheck the reservation at the existing effect boundary; the EventStore transition remains
the durable CAS authority and a replay returns the original receipt.

## Implemented source slice

- `ConnectorInvocationCommand` derives a canonical command digest from the immutable connector,
  operation, payload, idempotency and server-owned revision set. Same idempotency key with a
  different command digest is a typed conflict.
- `ConnectorReservationLease`, `ConnectorInvocationReservation` and
  `ConnectorInvocationPermit` bind lease ID, fence token, issue/expiry times, reservation digest,
  attempt and command digest. Uncommitted reservations, expired leases, stale revisions and old
  permits fail before an adapter effect.
- `ConnectorInvocationLedger` models reserve → CAS commit → one permit issue/consume → receipt
  apply. The same command replays the original reservation/receipt; a different command or
  receipt under the same key is rejected.
- `ControlPlaneConnectorReservation` serializes reservation facts through the existing EventStore
  transition/CAS path. Broker validates the optional server-owned envelope before its existing
  `ExecutionPermitVerifierPort` and handler boundary; the daemon rechecks it before the fixture
  adapter path and records the command digest in connector events.
- Domain fixtures and Core/Broker/Daemon source guards cover deny-first reservation/permit/CAS,
  revision races, idempotency conflicts, receipt replay and one-attempt consumption. No second
  execution loop, adapter authority or network transport was added.

## CI-only evidence

GitHub Actions runs the domain fixtures, Core/Broker/Daemon source guards, formatting and workspace
test-target compilation. Local Cargo test/build/check/clippy/smoke commands are intentionally not
run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The ledger is a source-level CAS model and the ControlPlane transition helper is not a cross
process recovery proof. Legacy connector requests without the additive reservation envelope remain
readable for compatibility until all callers emit committed reservation facts. Provider receipt
truth, durable lease settlement, crash recovery, external transports and live/physical connector
effects remain later INT-17..33 / ER / PD work; GitHub CI results are intentionally unobserved.
