# INT-02 Connector contract baseline

## Scope

INT-02 fixes the typed domain boundary for Connector definitions, account bindings, binding
snapshots, invocation/result receipts and recovery evidence. The existing contracts are metadata
only and re-enter the normal ControlPlane/Broker path for effects.

## Implemented source slice

- `ConnectorDefinition`, `ConnectorOperation`, `AccountBinding` and
  `ConnectorBindingSnapshot` are strict serde contracts with unknown-field rejection.
- Binding snapshots preserve project scope, revision and fixture/content digest; receipts carry
  provider outcome, payload hash and effect observation without raw secret material.
- Daemon connector management/invoke/reconcile resolves server-owned binding state and uses the
  existing EventLog/ControlPlane path; no connector adapter becomes an authority source.

## Evidence boundary

GitHub Actions is the test authority for the source guard. Local tests are intentionally not run.
This step does not claim durable external provider effects, live OAuth, remote A2A/MCP or business
outcome correctness.
