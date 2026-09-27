# INT-24 Webhook/A2A ingress baseline

> Snapshot date: 2026-09-27. This slice adds a bounded, signed Webhook/A2A occurrence contract and
> daemon verifier. Local Cargo test/build/check/clippy/smoke commands are intentionally not run;
> GitHub Actions owns the fixtures and target compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`INT-24`](../roadmap/integrations-connectors.md#step-int-24) |
| source snapshot | `d912e0b8` plus this INT-24 source slice |
| feature_status | `partial` for signed ingress authentication, tenant/payload policy, timestamp and dedupe contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | untrusted Webhook/A2A event → daemon source/key/HMAC verifier → typed occurrence → existing ControlPlane/workflow route |

`ConnectorIngressEvent` binds protocol, source/key, tenant/project, event identity, nonce,
payload digest and signature algorithm. `ConnectorIngressPolicy` checks allowlisted source/key,
tenant/project, event kind, clock skew and payload keys/size. The daemon keeps source keys private,
verifies HMAC-SHA256 over the canonical signing digest, stores only a signature digest in the
occurrence projection, and deduplicates by source plus event ID. Same-digest replay returns no new
occurrence; a different payload for the same identity is a conflict.

The verifier returns an occurrence for the existing routing boundary. It does not call a connector,
append EventLog facts, create a run, interpret payload fields as authority or bypass approval.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| auth | malformed/incorrect HMAC and unknown source/key fail before occurrence |
| tenant/scope | wrong tenant/project/protocol/event kind is denied |
| replay | same digest is idempotent; same source/event with another payload is conflict |
| time/nonce | clock skew, missing nonce and oversized payload fail closed |
| injection | payload keys outside server policy are rejected; raw payload is not copied into occurrence |
| boundary | source guard contains no Broker/connector invocation/EventLog append path |

## CI and limitations

GitHub Actions runs `.github/workflows/int24-ingress.yml` with domain/daemon fixtures, source guard
and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke commands are
intentionally not run, and CI results are not awaited.

Limitations: key provisioning/rotation, durable replay store, network HTTP/A2A listener, EventLog
append, occurrence-to-Workflow/Run command integration, rate limiting, external effect and live
provider evidence remain outside this source-only slice.
