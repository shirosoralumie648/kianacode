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

## CI-only evidence

GitHub Actions runs the domain receipt/observation fixtures, the shared source guard, formatting
and workspace test-target compilation. Local Cargo test/build/check/clippy/smoke commands are
intentionally not run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: partial
proof_level: source
```

The contract proves only typed, caller/adapter-supplied evidence and redaction fences. It does not
prove a provider's real-world outcome, cross-process durable receipt storage, query reconciliation,
retry policy, live transport or physical effect. Those remain later connector steps.
