# EQ-39 baseline registry baseline

## Scope

EQ-39 adds a bounded baseline registry and compatibility comparison contract to `kiana-quality`.
Each baseline binds suite, case, target and evaluator digests with an owner, a bounded validity
window and refresh provenance; records and the registry are digest bound. Comparison rejects
expired, not-yet-valid, tampered or drifted baselines before producing a comparable result, so a
stale or incompatible baseline can never become a silent comparison base.

The evaluator is read-only evidence validation. It does not refresh a baseline, execute a target,
persist an EvalStore record, invoke a judge, alter a candidate or submit a gate decision.

## Completion slice (2026-10-17, CI-only)

The 2026-09-26 slice was a narrow record/registry pair with four fixtures. This slice closes the
card exit conditions in source and CI wiring:

- `BaselineRecord` is self-validating: schema drift (`baseline_record_schema_invalid`), empty /
  whitespace / unbounded `baseline_id`/`owner_id`/`refresh_provenance`
  (`baseline_record_identity_invalid`), a zero capture time, `expires <= created` or a window over
  `MAX_BASELINE_TTL_MS` = 180 days (`baseline_record_window_invalid`), non-canonical digests
  (`baseline_record_digest_invalid`, lowercase `sha256:` + 64 hex), a malformed or self-referencing
  `refreshed_from_baseline_digest`, and a stale `baseline_digest` after any field edit
  (`baseline_record_digest_mismatch`).
- `BaselineRegistry` is digest bound (`baseline_registry_digest_mismatch` /
  `baseline_registry_digest_invalid`), bounded (`baseline_registry_empty`,
  `baseline_registry_size_exceeded` at `MAX_BASELINE_ENTRIES`), schema checked, and refuses
  duplicate identities (`baseline_registry_duplicate`). Every entry is validated with the record
  contract before the registry is usable.
- Refresh provenance is load bearing: entries that bind the same suite/case/target/evaluator tuple
  must have disjoint windows (`baseline_registry_overlap`), the later entry must name the
  immediately preceding record as `refreshed_from_baseline_digest`
  (`baseline_registry_provenance_mismatch`) and must come from a new capture provenance
  (`baseline_registry_provenance_reused`), so a refresh extends a baseline instead of rewriting it.
- Comparison is reject-first: a comparison before `created_at_unix_ms` emits
  `baseline.not_yet_valid`, a comparison at or after `expires_at_unix_ms` emits `baseline.stale`,
  and each drifting suite/case/target/evaluator axis emits `baseline.incompatible` with the axis
  named in `expected`; only an empty finding list is comparable. A tampered record emits
  `baseline.record_invalid` with the record reason.
- Registry lookup (`baseline-registry-lookup` evaluator, `BaselineRegistryInput`) resolves the
  named baseline and then compares it exactly like a direct record comparison: an invalid registry
  emits `baseline.registry_invalid`, an absent identity emits `baseline.record_missing`, and a
  success path stays comparable. Both envelopes reject schema drift, unset comparison time and
  non-canonical expected digests, and both keep `deny_unknown_fields`.
- Findings cite the evidence section they came from (`baseline:input`, `baseline:registry`) and
  every value is bounded. Typed builders (`BaselineRecord::new`/`refreshed_from`,
  `BaselineRegistry::new`/`lookup`, `BaselineComparisonInput::new`/`as_value`,
  `BaselineRegistryInput::new`/`as_value`) and schema/limit constants
  (`BASELINE_RECORD_SCHEMA`, `BASELINE_REGISTRY_SCHEMA`, `BASELINE_INPUT_SCHEMA`,
  `BASELINE_REGISTRY_INPUT_SCHEMA`, `MAX_BASELINE_ENTRIES`, `MAX_BASELINE_TEXT_BYTES`,
  `MAX_BASELINE_TTL_MS`) are exported so fixtures and later steps share one contract.

## Evidence and limits

- `kiana-quality/tests/eq39_baseline.rs` expands the original four fixtures to 12 rejection-first
  fixtures: the card acceptance `stale_or_incompatible_baseline_cannot_compare` (stale plus each
  drifting axis plus combined stale-and-drift), the compatible success path, the tamper/identity/
  window/digest refusal matrix, exclusive expiry endpoints with the TTL bound, direct and lookup
  envelope refusal (schema, unset time, non-canonical digest, unknown fields, wrong types),
  registry digest/size/duplicate/entry refusal, the refresh-chain matrix, lookup resolution
  (success, missing identity, invalid registry, stale), deterministic `EvaluatorRegistry` wiring
  and typed round trips.
- `kiana-quality/tests/eq39_baseline_guard.rs` protects the pure expiry/digest/provenance boundary
  (no filesystem/network/process/store/broker dependency), the required markers and stable codes,
  the fixture matrix, the manual lane's exact steps and the automatic `kiana-quality` shard
  (`targets: null`).
- `.github/workflows/eq39-baseline.yml` runs `cargo fetch --locked`, the format gate, both fixture
  targets with `--test-threads=1` and a locked compile of the `kiana-quality` test targets in
  GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: no durable baseline store,
authenticated owner, refresh command, target execution, EvalStore record, gate decision, promotion
or live/physical evidence is claimed.

## Known limits

- The evaluator validates caller-supplied baseline evidence only; it cannot prove that a capture
  actually happened, that the owner really owns the baseline or that the version digests describe
  the real suite/case/target/evaluator.
- `owner_id` and `refresh_provenance` are bounded tokens, not authenticated authority; binding them
  to a Principal/approval is a later ControlPlane step.
- Baseline records and registries are not persisted or refreshed here. There is no durable
  EvalStore, no expiry sweep, no retention policy and no `eval capture` command.
- A refresh must link its immediate predecessor when both bind the same version tuple, but a
  pruned predecessor is tolerated: a `refreshed_from_baseline_digest` that no longer resolves
  inside the registry is not rejected, because retention is a later store concern.
- Window overlap and provenance reuse are checked inside one registry only; a caller that compares
  against a record outside a registry gets the record-level checks without the registry chain.
- Findings are diagnostics; mapping `baseline.*` into blocking rules, gate scoring or promotion
  belongs to EQ-41/EQ-42/EQ-43.
- The manual lane is `workflow_dispatch`-only; automatic coverage depends on the unified `ci.yml`
  `kiana-quality` shard (both targets are discovered because `targets` is null).
