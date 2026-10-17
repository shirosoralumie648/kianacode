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

## 2026-10-17 completion slice (CI-only)

The effect fence already existed in the domain/Core/Broker/daemon sources; this slice turns it into
a reviewable, CI-wired step with deny-first fixtures and two source hardenings.

Source hardening (`kiana-domain/src/connector_effect_permit.rs`):

- A binding with a `credential_ref` can no longer be fenced by a zero credential epoch: `issue` and
  `validate_for_effect` both reject it with the new stable code
  `connector_effect_credential_epoch_required`, so the documented "zero only without a credential"
  rule is enforced where the binding snapshot is known.
- `validate_for_effect` validates the caller-supplied fence first (`current.validate()`); a fence
  whose recorded digest no longer matches its own fields is refused as
  `connector_effect_fence_invalid` instead of being silently re-derived.

Fixtures (deny-first, success last):

- `kiana-domain/tests/int18_effect_permit.rs` 2 -> 6:
  `effect_permit_binds_scope_digests_epochs_and_lease_expiry`,
  `stale_authority_configuration_policy_credential_and_data_epochs_never_reach_the_adapter`,
  `revoked_rotated_or_reconfigured_bindings_and_old_snapshots_are_refused`,
  `permit_cannot_be_minted_from_uncommitted_or_mismatched_snapshot`,
  `tampered_or_replayed_effect_material_is_refused_before_any_effect`,
  `fence_and_permit_digests_cover_scope_digests_and_every_revocable_epoch`.
- `kiana-capability-broker/tests/int18_effect_permit.rs` (new): the real sealed Broker with a
  recording handler and a recording one-shot `ExecutionPermitVerifierPort`; thirteen refusals
  (missing, tampered, expired, five stale epochs, missing epochs, missing binding snapshot, revoked,
  revision drift, foreign snapshot, scope drift, cross-attempt permit) leave both counters at zero,
  and the exact envelope dispatches once.
- `kiana-daemon/tests/int18_effect_permit.rs` (new): `DaemonHost → ControlPlane → Broker →
  EventStore`; wire-supplied `connector_effect_permit`/`connector_effect_fence`/reservation+permit
  envelopes and fence epochs are refused with `connector_arguments_invalid` while connector fact
  count, connector-stream length and registry version stay unchanged; the compatibility command
  still reaches the fixture adapter exactly once; the invocation output and the persisted connector
  facts never carry permit/fence material.
- `kiana-core/tests/int18_effect_permit_guard.rs` (3 guards): markers and rejection codes, the
  ordering proof (Broker fence before `verify_and_consume`, daemon fence before `load_fixture`),
  the ten fixture names, the manual lane command set and the shard ownership parsed from
  `scripts/ci/test-shards.json` (`int18_effect_permit` in `kiana-domain-s3/4`,
  `int18_effect_permit_guard` in `kiana-core-s4/6`, whole-crate shards for
  `kiana-capability-broker`/`kiana-daemon`).

CI wiring: manual lane `.github/workflows/int18-effect-permit.yml` (`workflow_dispatch` only,
`permissions: contents: read`, `cargo fetch --locked`, `cargo fmt --all --check`, four
`--test-threads=1` targets and four `--no-run` compiles). The automatic lane stays the unified
`ci.yml` shards; `scripts/ci/test-shards.json` is unchanged.

Evidence boundary: `feature_status: implemented`, `proof_level: source`; local tests/builds were
not run and the CI receipt is not awaited. The fence values are still server-owned inputs to the
Broker boundary, ControlPlane does not emit reservation/effect-permit facts for every caller yet,
and the daemon/Broker branch remains "deny when the envelope is present, legacy compatibility path
when it is absent".
