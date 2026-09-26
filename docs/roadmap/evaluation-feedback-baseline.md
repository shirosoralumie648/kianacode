# EQ-45 quality feedback baseline

## Scope

EQ-45 adds a provider-independent `quality.feedback` evidence contract. A feedback record can
refer only to a server-resolved canonical target. The target carries the policy and receipt
digests that were actually evaluated; provenance and privacy scope are derived from that target
and the server context. Client supplied replacements fail closed.

The evaluator is diagnostic only. It does not edit a policy, receipt, route, grant, candidate or
gate, start an evaluator, contact a provider, or append an EventLog fact.

## Evidence and limits

- `kiana-domain/tests/quality_feedback.rs` covers a valid server-derived submission,
  provenance/privacy derivation, target binding, redaction and policy/receipt reference fences.
- `kiana-core/tests/quality_feedback_guard.rs` protects the trusted request adapter, canonical
  target, immutable policy/receipt and no-side-effect boundary.
- `.github/workflows/eq45-feedback.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=partial`, `proof_level=source`: it proves the typed validation
boundary only. It does not prove an authenticated principal, a durable feedback store,
cross-process replay, UI projection, reviewer decision, promotion, live provider quality or a
business outcome.
