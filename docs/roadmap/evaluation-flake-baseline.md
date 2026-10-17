# EQ-37 retry-once flake classifier baseline

## Scope

EQ-37 adds `FlakeClassifierEvaluator` with a bounded two-attempt contract. A first failure
followed by a pass is classified as flaky and must remain `NeedsReview`/`Quarantined` with an
expiring owner/evidence record; it is never a Pass. Infrastructure attempts require a known
runner/fixture/network/resource class and cannot be silently counted as a case result. Retrying
after a pass, malformed attempt order, missing quarantine or unknown infra classification fails
closed.

The evaluator only classifies caller-supplied attempt evidence. It does not rerun a case, quarantine
files, mutate a suite, change a gate or promote a result.

## Completion slice (2026-10-17, CI-only)

The original slice existed in source; this completion slice closes the card's exit conditions and
turns the first version into a full, reject-first retry-once contract:

- Envelope: a drifted schema, a missing attempt, more than one retry and an unset evaluation time
  fail closed in `FlakeInput::validate` (`flake_input_schema_invalid`,
  `flake_input_attempts_missing`, `flake_attempt_limit_exceeded`, `flake_input_time_invalid`).
  Per-attempt schema drift stays an explicit finding (`flake.attempt_schema_invalid`).
- Attempt evidence: attempt numbers must be sequential from one
  (`flake.attempt_order_invalid`), each attempt binds the envelope's bounded case identity
  (`flake.attempt_case_mismatch`), each attempt carries a strict `sha256:` evidence digest
  (`flake.attempt_evidence_invalid`) and a retry must supply fresh evidence instead of replaying
  the first attempt (`flake.attempt_evidence_repeated`).
- One fixed-seed retry: a second attempt requires `retry_once` (`flake.retry_policy_invalid`),
  must reuse the first attempt's fixed seed (`flake.seed_mismatch`) and is refused after an
  already passing attempt (`flake.retry_after_pass`). Two attempts are the hard maximum; a third
  attempt is a structural refusal.
- Classification: a first failure plus a pass is `flake`, the same failure twice is a real
  `failure`, a retry after a pass is `retry_after_pass`, and `infra`/`out_of_scope` attempts never
  produce a case result. The classification is derived from attempt evidence alone and never from
  the claimed `final_status`.
- Status claims: a flake may only stay `needs_review` or `quarantined` — a `pass` emits
  `flake.flaky_counted_as_pass` (the card acceptance `flake_is_never_counted_as_pass`) and any
  other case-result status emits `flake.flaky_counted_as_result`; an infra case may not be
  reported as `pass`/`fail` (`flake.infra_counted_as_result`); an out-of-scope case may not be
  reported as `pass`/`fail`/`infra` (`flake.out_of_scope_counted_as_result`); a real failure may
  not be reported as anything but `fail` (`flake.status_attempt_mismatch`).
- Infrastructure taxonomy: an infra attempt must name a known class and may not use `unknown`
  (`flake.infra_unclassified`), while a case result or skipped attempt may not carry an infra
  class at all (`flake.infra_class_unexpected`).
- Quarantine record: flakes and `quarantined` statuses require a record (`flake.quarantine_missing`)
  and stable outcomes may not carry one (`flake.quarantine_unexpected`); the record must match its
  schema (`flake.quarantine_schema_invalid`), carry bounded secret-free case/reason/owner text and
  a strict digest (`flake.quarantine_invalid`), bind the same case (`flake.quarantine_case_mismatch`)
  and the exact attempt evidence (`flake.quarantine_evidence_mismatch`, the canonical
  `flake_attempt_evidence_digest`), be live at the evaluation time (`flake.quarantine_expired`) and
  stay inside a 30-day lifetime (`flake.quarantine_expiry_unbounded`).
- Findings cite the evidence section they came from (`flake:input`, `flake:attempt:<i>`,
  `flake:status`, `flake:quarantine`) and every caller-supplied value goes through
  `Finding::redacted`; unbounded or secret-shaped identities are rendered as their shape instead of
  their raw text. Typed builders (`FlakeInput::new`/`with_retry_once`/`with_quarantine`,
  `FlakeAttempt::new`/`with_infra_class`, `FlakeQuarantine::new`) and `as_value()` were added.

## Evidence and limits

- `kiana-quality/tests/eq37_flake.rs` expands the original four fixtures to 19 rejection-first
  fixtures: the card acceptance `flake_is_never_counted_as_pass`, stable failure versus flake,
  fixed-seed/one-retry bounds, retry-after-pass, known/unknown infra classes, infra and
  out-of-scope exclusion from case results, quarantine bounds/binding/expiry, attempt schema and
  case bindings, digest strictness and replay, envelope schema/limit/time/unknown-field refusals,
  bounded redacted identities, classification independence from the claimed status, typed
  round-trip, deterministic registry digest wiring and the clean-success matrix.
- `kiana-quality/tests/eq37_flake_guard.rs` protects the pure retry-once/quarantine-first boundary
  (no filesystem/network/process/store/broker dependency), the required markers and stable codes,
  the fixture matrix, the manual lane's exact steps and the automatic `kiana-quality` shard
  (`targets: null`).
- `.github/workflows/eq37-flake.yml` runs `cargo fetch --locked`, the format gate, both fixture
  targets and two `--no-run` compile steps in GitHub Actions; local tests are intentionally not
  executed.

This slice is `feature_status=implemented`, `proof_level=source`: no real rerun, durable quarantine,
infra telemetry, statistical flake confidence, promotion authority, live or physical evidence is
claimed.

## Known limits

- The evaluator validates caller-supplied attempt evidence; it never reruns a case, samples a
  workload or recomputes an attempt result, so it cannot prove that a retry actually happened, that
  the claimed seed was used or that the two attempts came from the same environment.
- The quarantine record is checked for shape, identity binding, digest binding, liveness and a
  bounded lifetime. It is not persisted, not signed and not enforced against the suite; durable
  quarantine lifecycle belongs to a later EvalStore step.
- Infrastructure classes are a closed caller-supplied enum; no telemetry is sampled, so
  `runner`/`fixture`/`network`/`resource` cannot be verified against real infrastructure signals.
- `flake.flaky_counted_as_pass` and the other findings are diagnostics; mapping them into blocking
  rules and weighted scores belongs to EQ-42.
- No rerun scheduling, experiment execution, gate mutation, candidate advancement, live or
  physical evidence is added here.
- The manual lane is `workflow_dispatch`-only; automatic coverage depends on the unified `ci.yml`
  `kiana-quality` shard (both targets are discovered because `targets` is null).
