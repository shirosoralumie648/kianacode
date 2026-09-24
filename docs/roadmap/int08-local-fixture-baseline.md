# INT-08 local fixture baseline

## Scope

INT-08 makes the existing local connector fixture contract explicit and domain-owned. It keeps the
fixture adapter behind the existing `DaemonHost → ControlPlane → Broker → EventStore` path and does
not add a network transport, a credential store, or a second execution loop.

## Implemented source slice

- `ConnectorFixture` and `ConnectorFixtureCase` implement strict
  `kiana.connector-fixture.v1` decoding with a 1 MiB byte bound, at most 32 operations and at most
  128 cases per operation. Unknown JSON fields, malformed identifiers and empty case lists fail
  closed.
- Fixture bytes are checked against the binding's SHA-256 before decoding. The adapter accepts the
  historical `sha256:<hex>` spelling for compatibility but emits and compares the canonical raw
  64-character digest. Replacing the file or supplying a different digest returns
  `connector_fixture_hash_mismatch`.
- Payload matching canonicalizes object keys and rejects duplicate canonical payloads. An operation
  absent from the fixture and a payload absent from its operation have separate stable errors.
- A fixture or case marked `external_effect=true` is rejected. Successful, failed and unknown cases
  are projected to deterministic `ProviderReceipt` values with canonical payload hashes,
  `source=local_fixture`, redacted result data and no external effect claim.
- Existing daemon bind, invoke, idempotent replay and explicit unknown reconciliation continue to
  append through EventStore CAS; the daemon only loads and validates the domain fixture contract.

## CI-only evidence

GitHub Actions runs the domain deny/success fixtures, the core/daemon source boundary guard and a
targeted compile for the local fixture path. Local cargo tests, builds, checks, clippy and smoke
commands are intentionally not run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The fixture and receipt are deterministic local observations. They do not prove an external provider
received an effect, exactly-once behavior across processes, durable connector registry recovery, or
live/physical business outcomes. HTTP/OAuth/MCP remote transports, provider query receipts and
cross-process lease fencing remain later INT steps.
