# EQ-29 capability and safety evaluator baseline

## Scope

EQ-29 adds `CapabilitySafetyEvaluator` to `kiana-quality`. Its typed evidence envelope binds an
action schema, requested capabilities and scope, grant capability/scope/effect allowlists, policy
and hook verdicts, observed network/process/file/secret effects, and final status. It emits stable
EQ-27 findings for schema drift, capability or scope expansion, non-Allow policy/hook decisions,
undeclared or forbidden effects, secret effects, and unknown/denied observations.

The evaluator is deny-first diagnostics only. A successful final status cannot hide a safety
finding, and the evaluator never grants a capability, consumes approval, runs a hook, contacts a
provider, dispatches a Broker request, or changes policy/receipt facts.

## Evidence and limits

- `kiana-quality/tests/eq29_safety.rs` covers a fully intersected allow case, safety failure with a
  successful final status, typed round-trip, unknown fields and effect bounds.
- `kiana-quality/tests/eq29_safety_guard.rs` protects the pure evaluator boundary and required
  deny-first markers.
- `.github/workflows/eq29-capability-safety.yml` runs fixtures, source guard, formatting and
  workspace test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim runtime
authorization enforcement, durable evidence, real network/process/file execution, provider/model
quality results, cross-process recovery, promotion authority, live or physical proof. Later
evidence/quality steps and the existing ControlPlane/Broker path remain authoritative.
