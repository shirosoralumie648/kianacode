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

## 2026-10-17 completion slice (CI-only)

The four public surfaces now share one encoder instead of each assembling JSON:

- `kiana-domain/src/connector_protocol.rs` adds `ConnectorSurfaceCommand`
  (`kiana.connector-surface-command.v1`) and `normalize_surface_connector_arguments`.
  `ConnectorSurfaceCommand::encode` validates the typed request first, so every forged
  server-owned field fails with `connector_server_owned_override` before a surface sends
  anything; `validate` pins `wire_name`/`route`/`arguments` and requires the arguments to decode
  strictly back into the same command. The `surface` label is correlation/audit metadata and
  never authority.
- `kiana-entrypoints/src/connector_command.rs` is the shared surface encoder
  (`SurfaceCommandRoute`, `surface_command_route`, `connector_health_route`). It re-encodes only
  arguments carrying the `kiana.connector-command.v1` schema and passes legacy untyped arguments
  through unchanged, so the compatibility path in Core keeps working.
- Workbench `/connector-health`, the Web `/api/command` route and the CLI `kiana command` path
  all call the shared encoder. The Web boundary re-encodes before claiming the replay key, so a
  forged command cannot consume an idempotency slot.
- The legacy MCP tool server must not mint or normalize connector commands locally (guard
  assertion); MCP-originated connector commands enter through the same `DaemonHost` path as the
  other surfaces.

Fixtures (deny first):

- `kiana-domain/tests/int14_connector_protocol.rs`: `surface_commands_share_one_surface_independent_encoding`,
  `surface_encoding_refuses_forged_authority_before_any_wire_use` (actor/role/risk/endpoint/
  `binding_snapshot` × four surfaces), `surface_command_validation_pins_route_wire_name_and_arguments`,
  `surface_normalization_only_accepts_matching_typed_arguments`,
  `manage_and_invoke_commands_are_validated_and_share_the_surface_encoding` (invoke/manage
  list/bind/revoke field-set refusals plus valid encodings that normalize to the public route).
- `kiana-entrypoints/tests/int14_connector_surfaces.rs`: `every_surface_gets_the_same_normalized_health_intent`,
  `generic_surface_paths_reencode_typed_arguments_to_the_same_shape`,
  `reconcile_keeps_the_public_name_but_enters_the_existing_manage_route`,
  `forged_authority_fields_are_refused_per_surface_with_zero_broker_calls`,
  `untrusted_unauthenticated_and_cell_surfaces_never_reach_the_broker`,
  `legacy_untyped_surface_arguments_stay_on_the_compatibility_path`.
- Guards: `kiana-core/tests/int14_connector_protocol_guard.rs` and
  `kiana-entrypoints/tests/int14_connector_protocol_guard.rs` extend the marker/negative-assertion
  matrix (no Broker/EventStore/approval execution inside the surface encoder; the legacy MCP
  tool server cannot normalize connector commands locally).

## CI-only evidence

GitHub Actions runs the domain/protocol fixtures, Core source guard, entrypoint/client source
guard, formatting and targeted workspace test-target compilation. Local Cargo tests, build, check,
clippy and smoke commands are intentionally not run and CI is not awaited. Automatic coverage:
`kiana-domain-s3/4` (`int14_connector_protocol`), `kiana-core-s4/6`
(`int14_connector_protocol_guard`) and the whole-crate `kiana-entrypoints` / `kiana-protocol` /
`kiana-client` shards (`targets: null`, new tests targets are discovered automatically).

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
The MCP surface parity is a negative source assertion (no local normalization/execution), not a
wired MCP connector command route; the read-only surface UI is INT-28 work.
