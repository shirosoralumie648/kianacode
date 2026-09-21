# INT-03 Connector operation contract baseline

## Scope

INT-03 fixes operation-level input/output schema, risk, scope, data-class, retry, timeout and
idempotency metadata. Caller payloads cannot downgrade risk or silently create a retry policy.

## Implemented source slice

- `ConnectorOperationContract` binds effect to server risk, input/output schema digests, required
  scopes, data classes, bounded payload size and timeout.
- Retry and idempotency modes are typed; required idempotency cannot be paired with an implicit
  declared-idempotent retry path.
- Canonical contract digest and strict unknown-field serde form the stable operation identity for
  later registry/invocation/receipt steps.

## Evidence boundary

GitHub Actions is the test authority for the domain fixture and core source guard. Local tests are
intentionally not run. This step does not claim adapter execution, live provider/network effects,
or external business outcome correctness.
