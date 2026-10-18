# EQ-42 quality blocking rules baseline

## Scope

EQ-42 adds a fixed, closed blocking-rule matrix for `safety`, `evidence`, `replay`,
`forbidden_effect`, `fixture_integrity` and `infra`, and binds it to the EQ-41 `QualityGate`
contract so that a triggered rule can never be recorded as `Pass`.

This evaluator is read-only: it does not mutate QualityGate configuration, execute a target,
authorize a capability, persist a decision, or promote/rollback a candidate.

## Completion slice (2026-10-18)

The first slice (`rules.rs`, 2026-09-26) was a standalone finding evaluator over a
caller-supplied rule matrix. It never produced or inspected an EQ-41 `QualityGateDecision`, so a
decision could be built with a `Pass` verdict while a rule evidence object said `triggered`
whenever the separate `blocking_findings` list stayed empty. The completion slice closes that gap.

### Gate-bound binder (`kiana-quality/src/rules.rs`)

| Surface | Contract |
| --- | --- |
| `BlockingRuleDecisionInput` (`kiana.quality-blocking-rule-decision-input.v1`) | `deny_unknown_fields`; carries the exact `QualityGateConfig` version, decision id, candidate id/digest, rule rows, weighted score, threshold, proposed verdict, evidence ref, source cursor and decision time |
| `build_gate_blocking_decision` | returns a `QualityGateDecision` or a stable refusal code; never invents identity, score, cursor or time |
| `verify_gate_blocking_decision` | re-derives the decision from the evidence and refuses any supplied decision whose digest, verdict, blockers, regressions or binding axes disagree |
| `BLOCKING_RULE_KNOWN_RULES` | the closed six-rule vocabulary the evaluator implements |

Law under test — `blocking_rule_precedes_weighted_score`:

1. The **config decides the matrix**. `QualityGateConfig::blocking_rules` is validated against
   `BLOCKING_RULE_KNOWN_RULES`; the request must evaluate exactly the declared rules, and the
   declared set must cover all six required rules.
2. **Triggered always blocks.** `Pass` and `NeedsShadow` over a triggered rule are refused
   (`blocking_rule_triggered_forces_reject`). The only accepted verdicts are `Reject` or
   `Rollback`, and the decision is then built through `QualityGateDecision::new(...)` with a
   `blocking:<rule>` blocker and matching known regression, so the EQ-41 verdict matrix also
   holds.
3. **A high score cannot buy a Pass.** The safety-blocker fixture runs at the contract maximum
   score (`MAX_GATE_SCORE_MILLI`) and at `999_999` and still refuses.
4. **A low score cannot Pass.** Clean evidence below the threshold proposed as `Pass` is refused
   (`blocking_rule_score_below_threshold`); the same evidence may only be `NeedsShadow`. A
   reasonless `Reject` is refused by the EQ-41 matrix rather than given an invented blocker.
5. **The decision binds the exact config version** through `quality_gate_decision_binding`; an
   unbound build is refused (`blocking_rule_decision_unbound`) and a decision derived from a
   different version fails verification (`blocking_rule_decision_forged`).
6. **Missing / unknown / duplicate / drift fails closed** with distinct stable codes rather than a
   silent skip.

### Stable refusal codes

| Code | Meaning |
| --- | --- |
| `blocking_rule_config_invalid` | the gate configuration did not validate |
| `blocking_rule_config_rule_unknown` | the configuration declares a rule the evaluator does not know |
| `blocking_rule_config_matrix_incomplete` | reserved for a declared matrix that cannot cover EQ-42 |
| `blocking_rule_missing` | a declared (or required) rule has no evidence row |
| `blocking_rule_unknown` | evidence names a rule outside the evaluator vocabulary |
| `blocking_rule_duplicate` | the same rule is reported twice |
| `blocking_rule_config_drift` | declared and evidenced rule sets disagree |
| `blocking_rule_evidence_invalid` | a rule row has the wrong schema or an invalid rule token |
| `blocking_rule_input_invalid` | the decision envelope schema is wrong |
| `blocking_rule_finding_ref_missing` | a triggered rule has no finding references |
| `blocking_rule_finding_ref_invalid` | a finding reference is empty, padded, oversized or unbounded |
| `blocking_rule_score_out_of_bounds` | threshold is `0`/above the cap, or the score is above the cap |
| `blocking_rule_triggered_forces_reject` | a triggered rule was proposed as `Pass` or `NeedsShadow` |
| `blocking_rule_score_below_threshold` | clean evidence below the threshold was proposed as `Pass` |
| `blocking_rule_verdict_forged` | the proposed verdict is not reachable from the evidence |
| `blocking_rule_decision_invalid` | the EQ-41 decision could not be constructed |
| `blocking_rule_decision_unbound` | the built decision did not bind the exact config version |
| `blocking_rule_decision_forged` | a supplied decision disagrees with the derived decision |

The first slice's evaluator (`BlockingRuleEvaluator`, `kiana.quality-blocking-rule-input.v1`) is
retained unchanged in behavior and still reports `blocking.rule_precedes_score`,
`blocking.rule_missing`, `blocking.finding_ref_missing` and `blocking.score_threshold_invalid`.

## Acceptance fixtures (`kiana-quality/tests/eq42_rules.rs`)

Deny paths first; the success fixture is last except for the standalone first-slice matrix.

| fixture | law under test |
| --- | --- |
| `high_score_cannot_override_a_triggered_safety_rule` | card acceptance: maximum/999999 score + safety blocker + `Pass`/`NeedsShadow` is refused |
| `below_threshold_score_cannot_be_a_pass_decision` | below threshold + `Pass` refused; shadow accepted; reasonless reject refused by EQ-41 |
| `missing_rule_evidence_fails_closed` | a removed evidence row is refused |
| `unknown_rule_evidence_fails_closed` | evidence for an unknown rule is refused |
| `duplicate_rule_evidence_fails_closed` | a repeated rule row is refused |
| `config_declaring_a_narrower_matrix_fails_closed` | a config declaring a subset cannot produce a decision |
| `evidence_for_an_undeclared_rule_is_config_drift` | evidence outside the declared set is drift |
| `config_declaring_an_unknown_rule_fails_closed` | an unknown declared rule is refused (EQ-41 refuses the config first) |
| `triggered_rule_without_finding_refs_fails_closed` | triggered rows need bounded finding refs |
| `malformed_headers_scores_and_schemas_fail_closed` | envelope schema, score bounds and rule schema |
| `forged_decision_and_unbound_version_are_refused` | hand-edited verdict/blockers and a decision built under another version are refused |
| `triggered_rule_forces_reject_or_rollback_never_pass` | triggered → `Reject`/`Rollback` with stable blockers, EQ-41 matrix satisfied |
| `clean_matrix_below_threshold_cannot_be_reported_as_pass` | standalone evaluator agrees with the binder below threshold |
| `clean_untriggered_evidence_with_passing_score_builds_a_bound_pass` | success: full untriggered matrix + passing score → `Pass` bound to config id/version/digest/suite digest, deterministic and JSON round-tripping |
| `standalone_rule_evaluator_keeps_its_first_slice_matrix` | first-slice evaluator behavior is unchanged |

`kiana-quality/tests/eq42_rules_guard.rs` keeps the boundary honest: no I/O, provider, authority,
event-store or promotion import; every refusal code is a public constant; every fixture is present
and no test is disabled; every deny fixture precedes the success fixture; the manual lane is
`workflow_dispatch`-only with `contents: read`, locked fetch, formatting gate, both targets serially
and a `--no-run` compile; and `kiana-quality` stays a `targets: null` automatic shard.

## Evidence and limits

- `.github/workflows/eq42-blocking-rules.yml` is manual-only and read-only; it runs
  `cargo fetch --locked`, `cargo fmt --all --check`, both EQ-42 targets with `--test-threads=1`
  and `cargo test -p kiana-quality --no-run --locked`.
- Local focused runs on 2026-10-18 (Windows, Git Bash, `CARGO_TARGET_DIR=D:/code/kiana-steps/target-eq42`):
  `eq42_rules` 15/15 passed, `eq42_rules_guard` 2/2 passed.
- `eq41_gate` was also run: 12/14 passed, with two **pre-existing** failures
  (`gate_config_and_decision_are_separate_value_contracts` and
  `gate_decision_digest_and_header_tamper_is_refused`) that reproduce unmodified on the base commit
  with this card's changes stashed. They are not caused by and not fixed by EQ-42; they are
  reported here rather than silently adopted into this card's green claims, and the lane does not
  run `eq41_gate` as a passing step.
- This slice is `feature_status=implemented`, `proof_level=local_behavior` for the focused fixture
  targets only: it does not claim a persisted GateDecision, ControlPlane promotion authority, a
  durable EvalStore, live or physical proof.

## Known limits

- The binder is a pure value function over caller-supplied rule evidence; it does not recompute
  the weighted score, read an EvalStore or prove that the evidence was produced by a real run.
- The configuration's per-dimension `thresholds` are not re-used by the binder; the request carries
  the weighted score and the weighted floor explicitly, because mapping named dimensions to one
  weighted scalar is EQ-43 territory.
- `blocking_rule_config_matrix_incomplete` is defined but currently unreachable through the binder:
  a narrower declared matrix reports `blocking_rule_missing`, and duplicates are refused by EQ-41
  at the config header. Both facts are asserted in the fixtures.
- No persistence, no route mutation, no approval, no receipt, no execution and no second execution
  loop.
