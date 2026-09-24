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
