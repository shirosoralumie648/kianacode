# INT-09 connector health and read-only probe baseline

## Scope

INT-09 adds one server-owned `connector.health` command and a redacted health fact/projection for
the local connector fixture path. The command resolves an active binding in the ControlPlane,
requires operator/project scope, fixes the risk to `ReadOnly`, and re-enters the existing
`DaemonHost → ControlPlane → Broker → EventStore` spine. It does not add a network transport,
destructive probe, raw credential resolution or a second execution loop.

## Implemented source slice

- `ConnectorHealthStatus` and `ConnectorHealthFact` are domain-owned, bounded and secret-free.
  Stable error codes map to `credential_invalid`, `scope_insufficient`, `endpoint_unreachable`,
  `provider_error` or `unsupported`; a verified status requires an evidence digest.
- `connector.health_checked` is registered as a connector event. The registry accepts a health
  fact only after its binding exists; the ControlPlane resolves the server-owned binding snapshot,
  revision and `read_only` probe kind instead of trusting caller supplied scope or risk.
- The daemon health handler checks operator authorization, project/binding identity, active
  revision and a declared read-only operation before loading a hash-pinned local fixture. A
  successful fixture read is reported as `connectivity_only` with `local_fixture_only` and
  `external_provider_not_probed` limitations. Fixture/path/hash failures are mapped to bounded
  statuses and only the redacted stable code is persisted.
- `kiana-query` projects the latest committed health fact per binding, marks revision drift as
  stale, retains source cursor/projection version/proof level/limitations and fails closed for
  malformed or foreign facts. Protocol/client/Workbench surfaces consume the nested typed fact
  and display only status, stale state and bounded limitations.

## CI-only evidence

GitHub Actions runs the domain classification/registry fixtures, query projection fixtures, Core
and Workbench source guards and a targeted compile. Local Cargo tests, builds, checks, clippy and
smoke commands are intentionally not run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The local fixture probe proves only a deterministic local observation and can report
`connectivity_only`; it does not prove an external provider was reached or that credentials are
valid. This step does not provide live HTTP/OAuth/MCP transport, provider receipts, durable
cross-process recovery, external effect truth or physical proof. Later INT-10–INT-13 own remote
transport, OAuth and redaction scans.
