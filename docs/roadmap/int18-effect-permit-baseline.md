# INT-18 Effect-time connector permit and epoch fencing baseline

## Scope

INT-18 closes the admission-to-adapter TOCTOU window for connector reservations. A committed
INT-16 reservation still supplies the command identity, while `ConnectorEffectPermit` is a
short-lived server-owned permit that binds binding scope, command/payload/idempotency digests,
owner/config/policy revisions, lease/fence identity and authority/configuration/policy/credential/
data epochs. The Broker and local fixture adapter revalidate this envelope immediately before
handler/fixture dispatch.

## Implemented source slice

- `ConnectorEffectFence` is a typed current snapshot. Scope digest and all non-zero revocable
epochs are covered by a fence digest; a zero credential epoch is allowed only for bindings with
no credential.
- `ConnectorEffectPermit::issue` can only consume a committed INT-16 reservation and a matching
binding/fence snapshot. `validate_for_effect` rejects uncommitted, revoked, rotated, reconfigured,
policy-stale, credential-stale, data-stale, scope-mismatched and expired material.
- `kiana-capability-broker` performs the effect permit check before `ExecutionPermitVerifierPort`
and the handler. `kiana-daemon` repeats the same check before reading the connector fixture or
constructing a provider receipt. No adapter or second execution loop is introduced.
- Domain fixtures and Core/Broker/Daemon source guards cover stale epoch/scope/snapshot/expiry
denials and assert the deny boundary precedes adapter dispatch.

## CI-only evidence

GitHub Actions runs the domain fixture, source guards, formatting and workspace test-target
compilation. Local Cargo test/build/check/clippy/smoke commands are intentionally not run and CI
is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The current-fence values are server-owned inputs to the Broker boundary; this slice does not
claim a cross-process durable fence projector, provider receipt truth, cancellation settlement,
external/live/physical effect proof or GitHub CI results. Legacy connector requests without an
INT-16 reservation remain on the compatibility path and do not receive an effect permit.
