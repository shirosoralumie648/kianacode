# INT-16 Connector invocation reservation and idempotency baseline

## Scope

INT-16 binds one connector invocation to a server-owned command digest, idempotency key digest,
binding/operation/owner/authority/policy/configuration/credential/data revisions and a short-lived
reservation lease. A committed reservation issues one permit for one attempt. The Broker and
daemon recheck the reservation at the existing effect boundary; the EventStore transition remains
the durable CAS authority and a replay returns the original receipt.

## Implemented source slice

- `ConnectorInvocationCommand` derives a canonical command digest from the immutable connector,
  operation, payload, idempotency and server-owned revision set. Same idempotency key with a
  different command digest is a typed conflict.
- `ConnectorReservationLease`, `ConnectorInvocationReservation` and
  `ConnectorInvocationPermit` bind lease ID, fence token, issue/expiry times, reservation digest,
  attempt and command digest. Uncommitted reservations, expired leases, stale revisions and old
  permits fail before an adapter effect.
- `ConnectorInvocationLedger` models reserve → CAS commit → one permit issue/consume → receipt
  apply. The same command replays the original reservation/receipt; a different command or
  receipt under the same key is rejected.
- `ControlPlaneConnectorReservation` serializes reservation facts through the existing EventStore
  transition/CAS path. Broker validates the optional server-owned envelope before its existing
  `ExecutionPermitVerifierPort` and handler boundary; the daemon rechecks it before the fixture
  adapter path and records the command digest in connector events.
- Domain fixtures and Core/Broker/Daemon source guards cover deny-first reservation/permit/CAS,
  revision races, idempotency conflicts, receipt replay and one-attempt consumption. No second
  execution loop, adapter authority or network transport was added.

## CI-only evidence

GitHub Actions runs the domain fixtures, Core/Broker/Daemon source guards, formatting and workspace
test-target compilation. Local Cargo test/build/check/clippy/smoke commands are intentionally not
run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The ledger is a source-level CAS model and the ControlPlane transition helper is not a cross
process recovery proof. Legacy connector requests without the additive reservation envelope remain
readable for compatibility until all callers emit committed reservation facts. Provider receipt
truth, durable lease settlement, crash recovery, external transports and live/physical connector
effects remain later INT-17..33 / ER / PD work; GitHub CI results are intentionally unobserved.

## 2026-10-17 completion slice (CI-only)

The 2026-10-17 slice turns the existing source model into targets that CI can actually exercise
and closes three gaps found while pinning it. No new execution loop, dependency or transport was
added, and no local build/test was run.

- `kiana-domain/tests/int16_connector_reservation.rs` grows from 3 to 8 fixtures:
  `command_digest_covers_every_server_owned_field_and_refuses_tampering` (the canonical digest
  separates key and payload, a binding revision race is visible before any adapter call, an
  overwritten digest without re-deriving it is refused as `connector_invocation_command_invalid`,
  and the digest survives a serialization round-trip),
  `same_command_replays_the_first_reservation_and_cas_races_are_denied` (an identical command
  replays the original reservation instead of minting a second ID, a different key reusing a
  reservation ID is `connector_reservation_id_conflict`, stale/ambiguous expected state revisions
  are `connector_reservation_cas_conflict`, a repeated exact commit is `Replayed`),
  `one_attempt_consumes_exactly_one_permit_and_uncommitted_work_cannot_consume` (an uncommitted
  reservation can neither issue nor consume, a second issue is
  `connector_attempt_permit_already_issued`, a second consume - including a deserialized replay -
  is `connector_permit_already_consumed`, a permit from another attempt is
  `connector_permit_reservation_mismatch`), `stale_leases_permits_and_epochs_never_reach_the_adapter`
  (before/after the lease window the permit is `connector_permit_expired`, and a renewed lease does
  not revive the old permit), and `receipt_conflicts_and_effects_before_consumption_are_refused`
  (a receipt before consumption is `connector_reservation_effect_not_admitted`, a different
  terminal result under the same reservation is `connector_idempotency_receipt_conflict`, and
  `replay_receipt` only returns the original receipt for the same key and command digest). The
  earlier weak assertion "a second consume is an error" is tightened to the exact code.
- Domain source fix: `ConnectorInvocationLedger::consume_permit` now reports the already consumed
  one-shot permit with the documented `connector_permit_already_consumed`. The Committed-only
  `validate_for_reservation` check previously ran first and masked the CAS fact as
  `connector_permit_reservation_mismatch`. No state transition or rejection strength changed; the
  early return still requires the permit itself to be valid and current.
- New Broker fixture `kiana-capability-broker/tests/int16_connector_reservation.rs` drives a real
  sealed `CapabilityBroker`, a recording `connector.invoke` handler and an
  `ExecutionPermitVerifierPort`:
  `committed_reservation_reaches_the_handler_exactly_once_with_its_command_digest` proves the
  complete server-owned envelope reaches the handler exactly once (call count 1) and that a legacy
  request without the additive envelope still follows the compatibility path;
  `uncommitted_stale_and_tampered_reservations_never_reach_the_handler` proves zero handler calls
  for a missing permit (`connector_permit_required`), an orphan permit
  (`connector_reservation_required`), an uncommitted reservation
  (`connector_reservation_not_committed`), an expired permit (`connector_permit_expired`), a
  cross-attempt permit (`connector_permit_reservation_mismatch`), a tampered reservation digest
  (`connector_invocation_reservation_invalid`) and a tampered or malformed permit
  (`connector_invocation_permit_invalid`).
- Broker source tightening: an orphan `connector_permit` without the server-owned
  `connector_reservation` is now refused as `connector_reservation_required` instead of silently
  falling back to the legacy path. The daemon already behaved this way; no producer emits a lone
  permit.
- New daemon fixture `kiana-daemon/tests/int16_connector_reservation.rs` drives the production
  `DaemonHost → ControlPlane → Broker → EventStore` path over a hash-pinned `local_fixture`
  binding: `same_key_with_a_different_command_digest_is_denied_before_any_effect` (the same key
  over a different registered payload is `connector_idempotency_command_digest_conflict` and the
  connector stream length, receipt set and registry version stay unchanged; a different key over
  the same payload is a new attempt with another event ID),
  `replayed_command_returns_the_original_receipt_and_keeps_one_attempt` (`replayed = true` with the
  original event ID, receipt, dispatch lifecycle and effect observation, and no stream growth), and
  `wire_supplied_reservation_envelope_is_refused_before_any_effect` (a caller cannot smuggle
  reservation/permit/effect-permit material through the wire; the request is
  `connector_arguments_invalid` with zero connector facts).
- `kiana-core/tests/int16_connector_reservation_guard.rs` pins the new fixtures, the manual lane
  steps and the absence of automatic triggers; the Broker guard pins `connector_reservation_required`
  and the daemon guard keeps its existing markers.
- New manual lane `.github/workflows/int16-connector-reservation.yml` (`workflow_dispatch`-only,
  `permissions: contents: read`) runs `cargo fetch --locked`, `cargo fmt --all --check`, the four
  targets with `--test-threads=1` and four `--no-run` compile steps. Automatic coverage stays the
  unified `ci.yml` shards: `kiana-domain-s3/4` (`int16_connector_reservation`),
  `kiana-core-s4/6` (`int16_connector_reservation_guard`) and the whole-crate
  `kiana-capability-broker` / `kiana-daemon` shards, so `scripts/ci/test-shards.json` is unchanged.

Proof boundary for this slice: `feature_status: implemented`, `proof_level: source`; local
rustfmt only, no local build/test, CI receipt not awaited. A committed reservation is still not
emitted by ControlPlane for every caller, the broker/daemon effect-time path is dormant until it
is, and durable EventStore CAS receipts, cross-process lease settlement, provider receipt
reconciliation and live/physical effects remain later work.
