# INT-07 Connector adapter and observation ports baseline

## Scope

INT-07 adds the narrow transport contracts required by the connector roadmap:
`ConnectorAdapter`, `EffectObserver`, `CredentialProbe` and `WebhookVerifier`.  The ports
consume server-owned admission material and return bounded result projections.  They do not own
authorization, approval consumption, EventLog writes or credential resolution.

## Implemented source slice

- `ConnectorPreparedPermit` binds connector/version, binding/account, operation, payload and
  idempotency digests, authority/policy revisions, credential generation and a bounded expiry.
- `CanonicalConnectorPayload` seals the canonical payload digest before an adapter sees it.
- `ConnectorAdapterCapabilities` is explicit and empty by default.  Checked methods reject
  missing capabilities instead of treating an unimplemented method as supported.
- `ConnectorAdapter` accepts only a prepared permit, an opaque one-shot `CredentialLease` and a
  canonical payload.  `ProviderReceipt` preserves `Succeeded`, `Failed` and `Unknown`; cancel
  returns the existing `StopReport` contract.
- `EffectObserver` returns only `EffectObservation`; `CredentialProbe` returns redacted health
  status/evidence; `WebhookVerifier` returns a `WorkflowEventOccurrence` projection through
  `VerifiedWebhook`.  None of these ports return raw credential material or provider bodies.
- The GitHub-only fixture injects known/unknown receipts, a confirmed stop report and a verified
  health result.  A source guard checks that the ports do not depend on EventStore/approval types.

## Evidence boundary

GitHub Actions is the test authority for the ports fixture, source guard and targeted compile.
Local tests, builds, checks, clippy and smoke commands are intentionally not run.  This source
slice does not prove a durable prepared-permit ledger, cross-process lease fencing, cryptographic
webhook verification, an external provider effect, or live/physical connector behavior.  The
fixture and health projection remain `feature_status=implemented`, `proof_level=source`; CI is
pending and is not being awaited.

## 2026-10-17 completion slice (CI-only)

The completion slice turns the source contracts into deny-first acceptance:

- All eight checked entry points (`discover`, `read_only_probe`, `invoke`, `cancel`,
  `capability_handshake`, `observe_receipt`, the probe port's `read_only_probe`, `webhook_verify`)
  deny with `connector_capability_missing:<capability>` before the implementation runs;
  call-counting `Sealed*` fakes prove the implementation was never entered.
- `PermissiveAdapter` registers every capability and still cannot execute: a permit bound to a stale
  binding revision fails `connector_prepared_permit_binding_mismatch`, and a probe lease pointing at
  a foreign `SecretRef` fails `connector_probe_credential_binding_mismatch` — both before the
  adapter call. Capability registration is descriptive, not authority.
- Receipts and observations are pinned to the permit: `connector_provider_receipt_binding_invalid`
  (wrong operation), `effect_observation_receipt_binding_mismatch` (observation derived from another
  receipt), `connector_observation_attempt_mismatch` (next attempt) and
  `connector_provider_receipt_invalid` when the receipt result echoes raw secret material (the error
  projection never carries the sentinel).
- Bounded projections have no raw-secret channel: probe result / effect observation / verified
  webhook reject unknown fields (`deny_unknown_fields`), health evidence is digest-or-failure, and
  the descriptor digest pins identity plus capability registration.
- Deny fixtures: `kiana-ports/tests/int07_connector_ports.rs` (`every_checked_port_entry_point_...`,
  `registered_capabilities_are_not_authority_over_the_prepared_permit`,
  `ports_reject_receipts_and_observations_not_bound_to_the_permit`,
  `port_results_have_no_raw_secret_channel_and_reject_unknown_fields`,
  `descriptor_and_capability_registration_are_pinned_and_default_to_deny`). Source guard:
  `kiana-core/tests/int07_connector_ports_guard.rs` (extended markers, forbidden authority/EventLog/
  dispatch markers, fixture-and-lane wiring). Manual lane:
  `.github/workflows/int07-connector-ports.yml` (`workflow_dispatch`-only; fetch, fmt gate, both
  targets, `--no-run`).
- Automatic coverage: the `kiana-ports` whole-crate shard in `ci.yml` runs the new targets, and
  `kiana-core-s4/6` lists `int07_connector_ports_guard`. No manifest/lockfile/package.json change,
  no new dependency, no second execution loop.

Evidence stays `feature_status=implemented`, `proof_level=source`; this slice runs no local
tests/build/check/clippy/smoke (only `cargo fmt --all`), GitHub Actions remains the test authority
and its receipt is not awaited. Still open: real adapter transports (HTTPS/MCP), cryptographic
webhook verification with a key store, durable prepared-permit/attempt ledgers, cross-process lease
fencing, provider receipt query/reconciliation and live/physical connector effects.
