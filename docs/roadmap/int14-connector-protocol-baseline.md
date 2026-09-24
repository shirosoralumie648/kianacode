# INT-14 Connector protocol DTO and normalization baseline

## Scope

INT-14 gives `connector.manage`, `connector.invoke`, `connector.health` and the public
`connector.reconcile` command a single versioned wire DTO. Every surface encodes the same typed
request and the ControlPlane normalizes it once before binding lookup, policy, approval and the
existing Broker route. Reconciliation maps to the existing `connector.manage` handler with an
explicit `action=reconcile`; it does not create a second execution path.

## Implemented source slice

- `ConnectorCommandRequest` is strict and versioned. It bounds operation, payload, reason,
  idempotency and receipt evidence fields and rejects unknown fields.
- `actor_id`, `role_id`, `risk`, `endpoint` and a caller supplied `binding_snapshot` are explicit
  server-owned override attempts. They fail with `connector_server_owned_override` and the typed
  error always reports `broker_calls=0`.
- `normalize_connector_intent` checks ProjectTrust and authenticated operator context first,
  stamps actor/role/department and the project data boundary from `RequestContext`, and leaves
  binding snapshot, risk and endpoint resolution to the server. Untrusted or unauthenticated
  input cannot reach the Broker.
- Protocol and client helpers use `RequestEnvelope::connector_command`; the existing client
  connector health facade now uses this helper. Legacy untyped connector arguments remain
  readable in Core, but typed surfaces share the same normalizer and server-owned checks.
- `connector.reconcile` is a public typed discriminator that re-enters the existing management
  path, preserving idempotency, receipt and reconciliation ownership in ControlPlane/Broker.

## CI-only evidence

GitHub Actions runs the domain/protocol fixtures, Core source guard, entrypoint/client source
guard, formatting and targeted workspace test-target compilation. Local Cargo tests, build, check,
clippy and smoke commands are intentionally not run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The normalizer proves a typed source boundary and zero-Broker deny contract only. Binding lookup,
operation risk, policy/gate/approval, reservation/CAS, effect-time fencing, receipt persistence,
external transport, durable cross-process identity and live/physical connector effects remain
owned by later INT/CP/ER/PD steps. The current local fixture and stdio adapters remain the only
supported connector transports; arbitrary HTTPS endpoints and raw credentials are not admitted.
