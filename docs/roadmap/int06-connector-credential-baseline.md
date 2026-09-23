# INT-06 Connector credential binding baseline

## Scope

INT-06 composes the existing CI-07 opaque `SecretRef` and `CredentialLease` contracts with the
server-resolved connector binding and one invocation. It does not introduce a credential store,
transport, or second execution path.

## Implemented source slice

- `ConnectorCredentialInvocation` binds connector/version, binding id/revision/full binding digest, account, operation,
  invocation id, idempotency-key digest, SecretRef digest/generation, effect-target digest and the
  CI-07 lease.
- `AccountBinding` may carry an optional strict opaque environment `SecretRef`; its key must be an
  environment-variable identifier, and its purpose/audience are checked against the connector.
  The operator-authorized `connector.manage bind` path may set this reference as binding
  configuration; a `connector.invoke` request cannot add or replace it. Other backends stay
  unsupported until their adapters are explicitly implemented.
- Issuance requires an active binding, a registered operation with its required scope, a valid
  opaque reference with the connector invocation purpose/audience, and a bounded lease lifetime.
- Daemon invokes the Broker consume helper against the current EventStore binding after fixture
  payload matching and before committing the invocation fact. The helper revalidates the full
  server-owned binding and invocation values, then consumes the lease once. Safe evidence contains
  only reference/lease/binding digests and non-secret binding metadata.
- Core does not accept a client-supplied lease or invocation-level SecretRef. Only an
  operator-authorized `connector.manage bind` command can set the opaque binding reference.
  Existing local-fixture invocation does not resolve a secret or make an external request.

## Deny and success fixtures

GitHub Actions runs domain and Broker fixtures for exact binding, operation/key/target drift,
wrong purpose, a non-reference environment key, expiry, mutable non-one-shot/overlong lease
rejection and repeated consumption, plus a Core/Daemon source guard for the operator binding and
server-owned invocation boundary. Local tests are intentionally not run.

## Evidence boundary

This is a source contract and CI wiring slice. The local fixture adapter does not resolve secret
material; actual connector adapters and effect-time resolution are deferred to INT-07/INT-11.
CI-07's mutable in-process lease object only demonstrates same-object repeated-consumption
rejection; it does not prove durable or cross-process replay prevention. Workspace format checks on
the pre-CM-36 source base were blocked by its missing `memory_workbench.rs`; the current integration
base includes the CM-36 fix. This workflow's test results remain pending until it runs on GitHub.
