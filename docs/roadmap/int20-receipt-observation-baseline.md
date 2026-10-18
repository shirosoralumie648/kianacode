# INT-20 ProviderReceipt / EffectObservation baseline

## Scope

INT-20 makes the provider receipt and external effect observation a single, typed evidence
contract. Receipt identity, operation, idempotency key and final payload hash are validated before
projection; observation evidence binds that receipt's payload hash, idempotency digest, provider
receipt ID and outcome while keeping owner and audience checks explicit.

## Implemented source slice

- `ProviderReceipt::validate` enforces the versioned schema, bounded identifiers/key/result,
  compatible payload hash spellings, provider receipt/source identity and the receipt secret scan.
  Raw provider response/secret-shaped values are rejected before EventLog or receipt projection.
- `EffectObservation` carries the optional payload hash for compatibility with older no-effect
  constructors, while observations created from a `ProviderReceipt` always carry it. The new
  `validate_for_receipt` check binds payload hash, idempotency key, provider receipt ID, outcome,
  owner and audience in one fail-closed validation.
- Connector fixture, daemon projection/reconciliation and `ConnectorAdapter` checked receipt
  validation use the same domain contract. Succeeded, failed and unknown outcomes remain distinct;
  no raw response is returned as evidence.

## 2026-10-17 completion slice (CI-only)

The card exit conditions are now fully covered at source level and wired to CI:

- `ProviderReceiptBinding` (`kiana.provider-receipt-binding.v1`) is the server-owned expectation:
  connector, binding, account, operation, invocation, attempt, idempotency key digest, optional
  canonical payload hash and optional owner/audience scope, plus a digest self-proof.
  `ProviderReceipt::validate_for_binding` refuses a receipt from another binding, account,
  operation, idempotency key or payload with the distinct stable codes
  `provider_receipt_binding_mismatch`, `provider_receipt_idempotency_mismatch` and
  `provider_receipt_payload_mismatch`; `normalize_provider_payload_hash` keeps the historical
  unprefixed and `sha256:` spellings from evading the comparison.
- `ProviderReceipt::validate` additionally rejects non-object results
  (`provider_receipt_result_invalid`), raw response keys (including nested ones), secret-shaped
  header fields and payloads, and oversized results
  (`provider_receipt_raw_response_forbidden`, `provider_receipt_result_too_large`). Its `Debug`
  implementation publishes only the result digest.
- `ProviderReceiptProjection` (`kiana.provider-receipt-projection.v1`) is the digest-only released
  view: identity, idempotency digest, canonical payload hash, provider receipt ID, outcome, source,
  result digest and projection digest; the raw result never leaves `projection()`.
- `EffectObservation::validate_for_receipt_binding` binds the receipt to the server-owned
  expectation plus observation owner/audience/invocation/attempt/idempotency-key; the
  self-asserted `validate_for_receipt` form stays for compatibility. Evidence refs must be strictly
  sorted and unique (`effect_observation_evidence_refs_unordered`), and
  `EffectObservationProjection` (`kiana.effect-observation-projection.v1`) is the canonical,
  digest-only released view. `from_provider_receipt_for_binding` builds the observation from the
  expectation rather than from caller-supplied scope.
- Ports and daemon consume the same contract: `EffectObservationRequest::binding` derives the
  expectation from the prepared permit plus admitted owner/audience, `observe_checked` re-validates
  the observation against it, `validate_receipt_for_permit` compares the receipt against a
  permit-built expectation, and the daemon observer receives a server-built
  `ProviderReceiptBinding` (binding snapshot/payload at dispatch, reconciliation case at
  reconcile). The domain, ports and daemon paths share one validation implementation.

Fixture matrix (deny-first):

| Fixture | Proves |
|---|---|
| `receipt_from_another_binding_account_operation_or_key_is_refused` | foreign connector/binding/account/operation/key/payload refusal, canonical payload spelling, identity-only expectation semantics, forged binding header |
| `observation_scope_attempt_and_receipt_binding_fail_closed` | owner/audience, invocation/attempt, provider receipt ID, swapped key, missing payload and scope-free expectation refusals |
| `succeeded_failed_and_unknown_project_distinct_stable_evidence` | three outcomes stay distinct and project byte-stably; evidence refs canonicalise |
| `unordered_duplicate_or_forged_evidence_refs_are_refused` | re-ordered/duplicated/over-limit/bad evidence refs and forged projection digests |
| `raw_provider_response_shapes_never_leave_the_receipt` | string/array results, raw and nested response keys, secret-shaped result/key/source, oversized results, `Debug` redaction |
| `receipt_projection_is_canonical_and_key_order_independent` | result key order independence, payload spelling normalisation, forged projection digest/schema/unknown fields |
| `receipt_binding_digest_covers_every_identity_and_scope_field` | eleven binding variants change the digest; nil invocation, attempt 0 and bad payload hash fail closed |
| `receipt_headers_and_unknown_outcomes_fail_closed` | schema/identity/key/hash/receipt-id bounds, unknown fields and unknown outcomes |
| `existing_receipt_observation_projection_contract_still_holds` | the pre-existing receipt/observation binding and JSON round-trip keep working |
| ports `observation_request_derives_its_binding_from_the_permit_only` | permit-derived expectation, foreign receipt/result refusals |
| ports `observation_projection_binds_attempt_scope_and_ordered_evidence` | projection binding, wrong scope/late attempt/unordered evidence refusals |

## CI-only evidence

GitHub Actions runs the domain receipt/observation fixtures, the new ports fixture, the shared
source guard, formatting and workspace test-target compilation. The manual lane is
`.github/workflows/int20-receipt-observation.yml` (`workflow_dispatch`-only, `contents: read`,
`cargo fetch --locked` + fmt gate + three targets + three `--no-run`). Automatic coverage:
`int20_receipt_observation` stays in `kiana-domain-s3/4`, `int20_receipt_observation_guard` stays in
`kiana-core-s4/6`, and the ports fixture is picked up by the whole-crate `kiana-ports` shard.
Local Cargo test/build/check/clippy/smoke commands are intentionally not run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The contract proves only typed, caller/adapter-supplied evidence and redaction fences. It does not
prove a provider's real-world outcome, cross-process durable receipt storage, query reconciliation,
retry policy, live transport or physical effect. Those remain later connector steps (INT-21/22/23
and the live gates). The daemon reconcile path still trusts the case/receipt digests it validates;
no provider query is performed.
