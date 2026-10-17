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

## 2026-10-17 completion slice (CI-only)

- Scope classification now wins over credential classification (`forbidden`, `permission_denied`,
  `insufficient_scope`, `scope_denied`, ...). A 403 is never reported as an invalid credential.
- Probe errors are reduced by `redacted_connector_probe_error_code`: non-JSON bodies, HTML,
  multiline text, Authorization headers, `sk-` tokens and over-long strings collapse to
  `connector_probe_failed`; fixture evidence failures map to `endpoint_unreachable`.
- A health fact cannot mix `verified`/`connectivity_only` with an error code, cannot carry a
  secret-sentinel limitation, and must match both the binding id and the connector id.
- The daemon only probes when the definition declares a read-only operation and the binding
  grants its scope. A write-only connector is `unsupported` with
  `connector_read_only_probe_unsupported`, never a destructive probe or a credential error.
- Retrying the same server request id returns the original health fact and event id without a
  second append. A failed probe leaves the binding active and does not touch the credential ref.
- Query projections keep all seven health classes distinct; protocol/Workbench projections reject
  a row cursor beyond the projection cursor and duplicate binding rows, and render only the
  typed status/stale/limitations fields.

Fixtures:

- domain `scope_errors_are_never_reported_as_invalid_credentials`,
  `probe_failures_collapse_non_json_text_and_never_echo_the_body`,
  `health_fact_rejects_conflicting_status_and_secret_bearing_limitations`, plus the registry
  connector-relabel denial.
- query `projection_keeps_every_health_class_distinct_and_redacted`,
  `projection_rejects_a_health_fact_that_relabels_the_connector`.
- daemon `read_only_health_is_connectivity_only_redacted_and_replays_the_original_fact`,
  `health_refuses_destructive_probe_foreign_scope_and_revoked_binding`,
  `health_separates_missing_scope_from_unsupported_destructive_connectors`.
- entrypoints `workbench_health_renders_every_class_without_provider_material`,
  `workbench_health_rejects_unknown_oversized_and_duplicate_projection_rows`; Core source guard
  keeps the handler free of deletion/revocation and external transport markers.

## CI-only evidence

GitHub Actions runs the domain classification/registry fixtures, query projection fixtures, Core
and Workbench source guards, the daemon `DaemonHost → ControlPlane → Broker → EventStore` health
behaviour fixtures and targeted compiles through `.github/workflows/int09-connector-health.yml`.
Local Cargo tests, builds, checks, clippy and smoke commands are intentionally not run and CI is
not awaited.

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
