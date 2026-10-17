# EQ-29 capability and safety evaluator baseline

## Scope

EQ-29 adds `CapabilitySafetyEvaluator` to `kiana-quality`. Its typed evidence envelope binds an
action schema, requested capabilities and scope, grant capability/scope/effect allowlists, policy
and hook verdicts, observed network/process/file/secret effects, and final status. It emits stable
EQ-27 findings for schema drift, capability or scope expansion, non-Allow policy/hook decisions,
declared/undeclared/forbidden effects, secret effects, unknown/denied observations, and a
successful final status that contradicts the safety evidence.

The evaluator is deny-first diagnostics only. A successful final status cannot hide a safety
finding, and the evaluator never grants a capability, consumes approval, runs a hook, contacts a
provider, dispatches a Broker request, or changes policy/receipt facts.

## Completion slice (2026-10-17, CI-only)

The original slice existed in source; this completion slice closes the card's exit conditions and
fixes the gaps a reviewer would find in the first version:

- `SafetyObservation` replaces the `allowed: bool` flag on `ObservedSafetyEffect`. `Allowed`,
  `Denied` and `Unknown` are distinct, so an unverifiable observation is reported as
  `safety.effect_unknown` instead of being conflated with a denial, and neither is treated as a
  pass.
- Declared effects are checked against the grant allowlist (`safety.declared_effect_forbidden`),
  not only observed effects. Declaring an effect the grant does not allow is a violation even
  when no such effect was observed.
- Observed effect scope is checked against both the grant scope
  (`safety.effect_scope_expanded`) and the action's requested scope
  (`safety.effect_scope_undeclared`); blank, control-character or oversized scope and identity
  values are `safety.effect_scope_invalid` / `safety.bound_set_invalid` /
  `safety.action_identity_invalid`.
- Missing capabilities/scopes are bound into the finding's `actual` value (one finding per missing
  entry, redaction-safe) instead of collapsing into duplicate indistinguishable findings.
- Observed effects are ordered by `(kind, scope, observation)` so the finding set does not depend
  on the caller's array order; duplicate evidence stays duplicated and visible.
- `safety.success_despite_violation` is emitted when the final status is `Success` while any
  safety finding exists. That is the card's acceptance condition: safety failure blocks even with
  good final text. `Failure`/`Unknown` statuses produce the same violation findings without the
  marker.
- Hard input bounds moved into `CapabilitySafetyInput::validate`
  (`capability_safety_input_schema_invalid`, `capability_safety_effect_limit_exceeded`,
  `capability_safety_set_limit_exceeded`) so structural drift fails closed before evaluation;
  entry-shape problems remain findings.
- Findings carry per-section evidence references (`safety:action`, `safety:grant`,
  `safety:policy`, `safety:hook`, `safety:effect:<index>`, `safety:final-status`) and reuse
  `Finding::redacted` for every caller-supplied value.

## Evidence and limits

- `kiana-quality/tests/eq29_safety.rs` (19 rejection-first fixtures) covers the fully intersected
  allow case, an allow case with no observed effects, the card's acceptance scenario
  (`safety_failure_blocks_even_with_good_final_text`), success/failure/unknown status equivalence
  plus the explicit success marker, schema drift on all four evidence sections, unknown-field and
  envelope drift failure, grant capability/scope expansion bound to the missing entry, every
  non-Allow policy/hook verdict, declared-effect containment, undeclared/forbidden observed
  effects, always-on secret findings, denied vs unknown observations, effect scope containment on
  both grant and requested scope, bounded identities/sets, capability binding, order
  independence plus duplicate evidence, typed round-trip, oversized/malformed rejection, and
  deterministic evidence-bound output wired through `EvaluatorRegistry`.
- `kiana-quality/tests/eq29_safety_guard.rs` protects the pure evaluator boundary, the required
  deny-first markers, the fixture matrix, the manual lane commands and the automatic
  `kiana-quality` shard (`targets: null`).
- `.github/workflows/eq29-capability-safety.yml` runs `cargo fetch --locked`, the formatting gate,
  both test targets and both `--no-run` compilation steps in GitHub Actions; local tests are
  intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim runtime
authorization enforcement, durable evidence, real network/process/file execution, provider/model
quality results, cross-process recovery, promotion authority, live or physical proof. Later
evidence/quality steps and the existing ControlPlane/Broker path remain authoritative.

Known limits: the evaluator only analyzes caller-supplied evidence; it does not observe effects
itself, does not verify that a policy/hook verdict was actually produced by the authoritative
engine, and does not map findings to a blocking verdict (EQ-42 owns blocking rules). Scope
matching is exact string containment, so hierarchical scopes are treated conservatively as
expansions rather than being silently widened.
