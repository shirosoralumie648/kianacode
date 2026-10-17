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
  Only the separately authorized `connector.manage` `bind` action can set this reference; bind
  requires the operator mutation fields and is assigned `ExternalSideEffect` risk before normal
  authorization. A `connector.invoke` request accepts no `SecretRef`, `credential_ref`, lease, or
  credential override field. Other backends stay unsupported until their adapters are explicitly
  implemented.
- Issuance requires an active binding, a registered operation with its required scope, a valid
  opaque reference with the connector invocation purpose/audience, and a bounded lease lifetime.
- Daemon invokes the Broker consume helper against the current EventStore binding after fixture
  payload matching and before committing the invocation fact. The helper revalidates the full
  server-owned binding and invocation values, then consumes the lease once. Safe evidence contains
  only reference/lease/binding digests and non-secret binding metadata.
- Core rejects invocation-level SecretRef/lease fields. The separately authorized operator bind
  action accepts and validates the opaque reference as account-binding configuration. Existing
  local-fixture invocation does not resolve a secret or make an external request.

## Deny and success fixtures

GitHub Actions runs domain and Broker fixtures for exact binding, operation/key/target drift,
wrong purpose, a non-reference environment key, expiry, mutable non-one-shot/overlong lease
rejection and repeated consumption, plus a Core/Daemon source guard. The guard asserts the invoke
allowlist excludes credential fields and bind validates a typed `AccountBinding` behind
`ExternalSideEffect` risk. Local tests are intentionally not run.

## Evidence boundary

This is a source contract and CI wiring slice. The local fixture adapter does not resolve secret
material; actual connector adapters and effect-time resolution are deferred to INT-07/INT-11.
CI-07's mutable in-process lease object only demonstrates same-object repeated-consumption
rejection; it does not prove durable or cross-process replay prevention. Workspace format checks on
the pre-CM-36 source base were blocked by its missing `memory_workbench.rs`; the current integration
base includes the CM-36 fix. This workflow's test results remain pending until it runs on GitHub.

## 2026-10-17 completion slice (CI-only)

Adds the Broker-side one-shot fence the earlier slice was missing and closes the four-channel
(Core/Runner/Event/UI) denial evidence.

- `kiana-capability-broker/src/lib.rs`: `ConnectorCredentialLeaseFence` — a server-owned ledger of
  `lease_id` → `lease_digest` only. `consume()` refuses a lease identity already admitted
  (`credential_lease_replayed`, denied copy untouched), otherwise delegates to
  `consume_connector_credential_invocation` so every binding/operation/idempotency/effect-target/
  purpose/audience/expiry check still runs before the single admission is recorded.
- `kiana-daemon/src/connectors.rs`: `ConnectorRegistry` owns
  `credential_fence: std::sync::Mutex<ConnectorCredentialLeaseFence>` and consumes through
  `fence.consume(...)` at the adapter effect boundary (the lock is released before the adapter
  call). The daemon no longer calls the bare helper, so a clone cannot be admitted twice.
- New deny fixtures: Broker `fence_admits_one_lease_identity_and_refuses_every_clone`,
  `fence_refuses_a_deserialized_replay_of_one_lease`,
  `fence_refuses_a_recomputed_digest_forgery_of_one_lease`; domain
  `consumed_invocation_cannot_be_replayed_after_serialization`,
  `event_projection_keeps_digests_and_ref_metadata_only` (evidence field set fixed at 13
  digest/ref-metadata keys, event payload free of `key`/`store`/`secret_value`/`credential_ref`);
  Runner `kiana-runner/tests/int06_connector_credential_guard.rs`
  (`runner_mapping_drops_model_supplied_credential_fields`,
  `runner_tool_surface_declares_no_credential_parameter`,
  `runner_sources_never_resolve_connector_credentials`).
- `kiana-core/tests/int06_connector_credential_guard.rs` now requires the daemon to go through
  `ConnectorCredentialLeaseFence`/`fence.consume(` and forbids the bare
  `consume_connector_credential_invocation(` call there.
- CI wiring: manual `.github/workflows/int06-connector-credential.yml` (CI-07 lease fixtures plus
  the four INT-06 targets, fmt gate, `--no-run` compile steps). Automatic coverage stays with
  `ci.yml`: `kiana-domain-s3/4`, `kiana-core-s4/6`, and the whole-crate `kiana-capability-broker`
  and `kiana-runner` shards.

Limitations: the fence is in-process metadata, so it does not survive a daemon restart, does not
prove cross-process replay prevention, and does not constitute durable/live evidence; effect-time
secret resolution and real connector transports remain deferred to INT-07/INT-11. Local tests were
not run in this session; GitHub Actions is the test authority and no receipt has been awaited.
