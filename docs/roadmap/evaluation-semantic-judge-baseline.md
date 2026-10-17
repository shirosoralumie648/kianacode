# EQ-35 semantic judge contract baseline

## Scope

EQ-35 adds a strict quality-side semantic-judge evidence contract around the existing
`kiana-ports::Judge` boundary. It fixes provider/model/prompt/temperature/configuration identity,
binds input/reference/output digests and result digest, and distinguishes Available, Unavailable
and NotConfigured. A required unavailable judge, or a forged Pass while unavailable, is blocking;
judge output remains a quality diagnostic and never changes authorization or promotion.

No real model, account, network or judge adapter is invoked by this slice. The contract is ready
for a later explicitly configured adapter while preserving the no-judge-is-pass boundary.

## Completion slice (2026-10-17, CI-only)

The original slice existed in source; this completion slice closes the card's exit conditions and
turns the first version into a full, reject-first semantic-judge contract:

- Envelope: structural drift and an oversized unavailable reason fail closed in
  `SemanticJudgeInput::validate` (`semantic_judge_input_schema_invalid`,
  `semantic_judge_reason_limit_exceeded`); per-section schema drift stays an explicit finding
  (`judge.config_schema_invalid`, `judge.request_schema_invalid`, `judge.result_schema_invalid`).
- Availability: `Available` without all three evidence sections is refused
  (`judge.evidence_missing`) and an available judge must not carry an unavailable reason
  (`judge.unavailable_reason_unexpected`). `Unavailable` and `NotConfigured` are distinct: a
  required judge blocks (`judge.unavailable` / `judge.not_configured`), an optional judge stays a
  visible `judge.not_evaluated` diagnostic instead of being silently counted as success, and an
  unavailable judge can never present a result (`judge.unavailable_not_pass` for a forged pass,
  `judge.evidence_unexpected` for any other result). Every unavailable/unconfigured input needs a
  bounded single-line reason (`judge.unavailable_reason_missing`).
- Fixed identity: provider, model version, prompt version and the strict `sha256:` prompt and
  configuration digests are bounded and versioned (`judge.config_invalid`); the request must bind
  the same configuration and prompt (`judge.config_drift`, `judge.prompt_drift`) and carries strict
  `sha256:` input/reference/output digests (`judge.request_invalid`).
- Result: the score is bounded and the digest must be a strict `sha256:` (`judge.result_invalid`);
  a pass verdict with a zero score is contradictory (`judge.result_inconsistent`); a failing or
  review verdict from an available judge stays visible (`judge.verdict_failed`,
  `judge.verdict_needs_review`) instead of collapsing into success.
- Findings cite the evidence section they came from (`judge:availability`, `judge:config`,
  `judge:request`, `judge:result`) and every caller-supplied value goes through
  `Finding::redacted`.

## Evidence and limits

- `kiana-quality/tests/eq35_judge.rs` (19 rejection-first fixtures) covers the card's acceptance
  scenario `judge_unavailable_is_not_a_success`, the bounded success path, optional/required
  unavailable and not-configured judges, forged and non-pass results from an unavailable judge,
  bounded reasons (including the structural reason limit), missing evidence sections,
  configuration/prompt drift, fixed-identity bounds, request/result digest and score bounds,
  failing and review verdicts, the zero-score pass contradiction, envelope/section schema drift,
  strict unknown-field rejection, typed round-trip, the redaction boundary over caller values, and
  deterministic evidence-bound registry output.
- `kiana-quality/tests/eq35_judge_guard.rs` protects the optional/no-authority boundary (no
  filesystem/network/process/provider/store/broker/gate dependency), the required markers and
  stable codes, the fixture matrix, the manual lane's exact steps and the automatic
  `kiana-quality` shard (`targets: null`).
- `.github/workflows/eq35-semantic-judge.yml` runs `cargo fetch --locked`, the format gate, both
  fixture targets and two `--no-run` compile steps in GitHub Actions; local tests are intentionally
  not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no semantic quality claim,
external judge result, durable EvalStore record, promotion, live or physical evidence is made.

## Known limits

- The evaluator decides whether judge evidence is usable; it never calls a judge, a model, a
  provider or an account, and it cannot prove that a real judge produced the verdict, that the
  prompt digest matches the executed prompt bytes, or that a score reflects semantic quality.
- Digests are bounded and format-checked but never recomputed against a prompt store, model
  registry or trace.
- `judge.not_evaluated` and the verdict findings are diagnostics; mapping them into blocking rules
  and weighted scores belongs to EQ-42, and dimension aggregation to EQ-36.
- The temperature bound and the identity alphabet are shape checks, not sampling reproducibility or
  provider capability checks.
- No adapter, durable EvalStore record, promotion path, live or physical evidence is added here;
  the optional port stays compile-only (`kiana-ports::Judge`) until a later explicitly configured
  adapter lands.
- The manual lane is `workflow_dispatch`-only; automatic coverage depends on the unified `ci.yml`
  `kiana-quality` shard (both targets are discovered because `targets` is null).
