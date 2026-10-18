# NM-19 external notification/webhook contract baseline

> Snapshot date: 2026-09-26. Tests are GitHub Actions-only; this step does not run local tests,
> build, check, clippy or smoke commands and does not wait for CI.

NM-19 adds a default-off, source-only external notification contract. `ExternalNotificationPolicy`
uses exact canonical HTTPS origins, a server-owned signing-key reference, authority/data epochs and
an explicit enable bit. `ExternalNotificationEnvelope` binds transport, recipient, delivery ID,
nonce, expiry, payload/signature digests and source event IDs. The core gate checks policy,
allowlist, nonce, epochs and expiry but returns only `NotSupported` or an explicit
`ReadyForExplicitConnector` handoff with `direct_effect=false`.

Provider receipts are observation DTOs only. A binding-matched `Acknowledged` receipt is observed;
`Unknown` becomes `ReconcileRequired`; drift or invalid receipt is rejected. No HTTP/A2A client,
socket, secret signer, connector dispatch or receipt application was added, and the default policy
remains disabled.

`feature_status=implemented`; `proof_level=source`.

Known limits: no network transport, provider signature verification, nonce store, durable receipt or
live/physical external delivery evidence is claimed. External connector enablement remains a later
explicitly approved scope.

## Completion slice (2026-10-18)

The 2026-09-26 slice had five real gaps: the core gate took a caller-supplied
`ExternalNotificationPolicy` and `validate()` only recomputed that policy's own digest, so any caller
could fabricate an enabled policy with an arbitrary origin and key id (`ExternalNotificationPolicy::disabled()`
had zero production callers); the envelope had no key id and `signature_digest` was shape-checked
only; `canonical_origin` accepted `https://127.0.0.1` and `https://169.254.169.254`; the nonce was
compared only against a caller-passed `expected_nonce` with no store and no TTL bound; and the four
schemas were not in `SCHEMA_CONTRACTS`. The deny fixture matrix also asserted only three strings of
eleven, and the guard `include_str!`-ed exactly two files, so a new module could evade it.

This slice closes all six, still as pure contracts with `direct_effect` hardcoded false and no
transport:

- **Server-owned policy.** `ExternalNotificationPolicySnapshot`
  (`kiana.external-notification-policy-snapshot.v1`) pins `revision` + `issued_at_unix_ms` + the
  policy + a snapshot digest; `ExternalNotificationPolicySnapshot::disabled()` is the explicit
  server default. `kiana_core::ExternalNotificationPolicyRegistry` installs one snapshot and only
  exposes `install`/`publish`/`admit`. `publish` refuses an equal or older revision
  (`external_notification_policy_revision_rollback`) and a new revision carrying the previous
  policy digest (`external_notification_policy_replay`). `admit` compares the caller's
  `ExternalNotificationAdmissionContext::policy_digest` with the pinned digest and refuses anything
  else with `external_notification_policy_not_pinned` before looking at the envelope, so a request
  can never supply, enable, widen or re-key the policy. `signing_key_id` stays inside
  `policy_digest`, and the admission carries `policy_revision`/`policy_digest`.
- **SSRF.** `is_forbidden_address` in `kiana-domain/src/connector_https.rs` is now `pub(crate)` and
  reused by `canonical_origin` for literal IPv4/IPv6 origins; single-label names, `localhost`,
  `.localhost`, `.local`, `.internal` and `.home.arpa` are refused too. A denied origin returns
  `external_notification_origin_ssrf_denied` (envelope destinations reuse the same code path). The
  `int11_https_connector_guard` marker set is unchanged.
- **Signature binding.** `ExternalNotificationEnvelope` gained `signing_key_id`;
  `bind_signing_key(&policy)` is a pure binding check (`external_notification_signature_key_missing`,
  `external_notification_signature_binding_mismatch`,
  `external_notification_signature_policy_key_missing`).
  `kiana_core::check_external_notification_signature` returns a `ExternalNotificationSignatureCheck`
  whose `verification` is always `ExternalNotificationSignatureVerification::NotSupported` with
  reason `external_notification_signature_verification_not_supported`; nothing is claimed verified.
- **Replay.** `ExternalNotificationReplayWindow` (bounded `max_entries` ≤ 4096 and `max_ttl_ms` ≤ 24 h)
  plus `ExternalNotificationReplayStore` admit each nonce and delivery id exactly once
  (`external_notification_nonce_replayed`, `external_notification_delivery_replayed`), refuse an
  expired or over-TTL envelope (`external_notification_replay_window_invalid`) and a full window
  (`external_notification_replay_capacity_exceeded`) after purging expired entries. Refusals happen
  before mutation, so a denied delivery never burns a fresh identifier. The store is an explicit
  owned value: no global state, no clock read, no persistence claim.
- **Schema registration.** `kiana.external-notification-policy.v1`,
  `…-policy-snapshot.v1` and `…-envelope.v1` are `SchemaLayer::Domain`, `…-receipt.v1` is
  `SchemaLayer::Projection`; all four are `owner_crate=kiana-domain`, `Breaking`, and reject unknown
  fields. `kiana.external-notification-admission.v1` is produced by `kiana-core` and stays outside
  the domain registry.
- **Deny-first fixtures + widened guard.** New `kiana-domain/tests/nm19_external_notification.rs`
  (13 fixtures: SSRF/metadata/private origins, non-HTTPS/userinfo/path/query, envelope destination
  SSRF, signature binding, nonce/delivery replay, TTL/capacity bounds, policy enablement bounds,
  snapshot digest/revision, ACK ref, schema registration, then disabled default / enabled snapshot /
  envelope+receipt round trip). `kiana-core/tests/nm19_external_notification.rs` rewrote the 5 old
  fixtures into 10 deny-first ones (caller-fabricated policy, revision rollback/replay, disabled
  not-supported, replay/expiry/epoch, allowlist+signature binding, NotSupported signature check,
  receipt drift) followed by the handoff, receipt-observation and fixture-matrix cases.
  `nm19_external_notification_guard.rs` now walks every `notification*.rs` in both
  `kiana-core/src` and `kiana-domain/src` (`std::fs` scan, ≥ 20 files asserted) instead of two
  `include_str!` files, forbids network/broker/event-write markers in each of them, and checks the
  fixture/reason/lane/shard/baseline wiring.

Lane `.github/workflows/nm19-external-notification.yml` (manual, `workflow_dispatch`-only,
`permissions: contents: read`, fetch/fmt gate, three serial targets plus three `--no-run` compiles).
Automatic coverage is the unified `ci.yml` shard `kiana-core-s4/6` (already lists
`nm19_external_notification` and `nm19_external_notification_guard`); the new domain target was
added to `kiana-domain-s3/4`.

Proof: the focused targets were run locally (see the `CURRENT_STATUS.md` NM-19 block):
`kiana-domain --test nm19_external_notification` 13 passed, `kiana-core --test nm19_external_notification`
10 passed, `kiana-core --test nm19_external_notification_guard` 2 passed, all exit 0.
`proof_level=local_behavior` for those focused targets only.

Known limits after the completion slice: there is still no transport, no provider signature
verification (explicitly `NotSupported`), no durable/committed nonce or receipt store (the replay
store is in-memory and caller-owned), no delivery timeout/attempt contract, no connector dispatch,
no DaemonHost wiring, and no live or physical external delivery evidence. A real external channel
remains `not_supported` until a separately approved connector step; nothing here enables delivery.
