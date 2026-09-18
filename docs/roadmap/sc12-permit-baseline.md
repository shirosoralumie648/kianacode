# SC-12 Pending Invocation / Permit Baseline

## Scope

SC-12 closes a source/CI evidence slice around the existing single execution spine. `PendingInvocation`
retains approval/request/context/sandbox/event sequence and optional continuation binding; strict
`DispatchPermit` binds execution/invocation/request IDs, action digest, project identity, authority
versions, approval, expiry and permit digest. Core consumes the committed `execution_permit` stream
with CAS, rejects stale authority epoch or reused permit, and records execution facts only after
permit admission.

The DTOs are not authority by themselves: EventLog commit/CAS and the ControlPlane/Broker path are
still required. This step does not add a second permit verifier or execution loop.

## Evidence and limits

- `kiana-core/tests/sc12_permit_guard.rs` pins PendingInvocation/DispatchPermit identity, digest,
  authority-version, idempotency and no-bypass markers in CI.
- GitHub Actions runs the source guard and workspace compile; local tests are intentionally not
  executed.

This slice is `feature_status=implemented`, `proof_level=source`: full cross-process durable permit
projector, TOCTOU path fence, secret/egress enforcement and external/live/physical effect proof
remain SC-13+ and PD/ER work.
