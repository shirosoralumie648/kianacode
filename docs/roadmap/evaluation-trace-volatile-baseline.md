# EQ-19 Volatile Trace Normalization Baseline

## Scope

EQ-19 adds an explicit volatile-value policy on top of the EQ-18 canonical trace. Rules name a
canonical event path and classify it as `timestamp`, `uuid`, `temp_path` or `actor`; matching
values become bounded tokens (`<TS>`, `<UUID>`, `<TEMP_PATH>`, `<ACTOR>`) and every replacement is
recorded with its path and kind. Array-member rules support an explicit `*` segment. A bounded
replacement budget prevents an unexpectedly large trace from being silently rewritten.

The normalizer also infers likely volatile values. If a UUID-like string, temporary path,
timestamp field or actor field is not covered by a rule, it returns a stable error instead of
normalizing it implicitly. Policies are validated before use: malformed paths, duplicate rules, one
path carrying two kinds (`volatile_rule_ambiguous`), an empty or oversized replacement budget and an
oversized rule list all fail closed. Source correlation/cursor fields remain provenance metadata;
canonical bytes are computed from the normalized event values, not raw source identity fields.

## Fixtures (named, deny first)

| Fixture | Proves |
|---|---|
| `undeclared_volatile_value_is_not_silently_normalized` | undeclared UUID/timestamp/temp-path/actor values each return `volatile_undeclared:<path>:<kind>` with the exact canonical path |
| `invalid_rule_and_policy_shapes_are_rejected` | malformed paths (`volatile_rule_invalid`), duplicate rules (`volatile_rule_duplicate`), one path with two kinds (`volatile_rule_ambiguous`), zero/oversized budget and oversized rule list (`volatile_policy_invalid`) |
| `declared_kind_must_match_the_value_type` | a declared path whose value type does not match its kind returns `volatile_value_type_mismatch` instead of being rewritten |
| `source_normalization_version_must_match_before_any_replacement` | a trace that is not the current EQ-18 canonical version is rejected before any replacement (`volatile_source_version_invalid`) |
| `wildcard_array_rule_does_not_normalize_undeclared_siblings` | a `*` rule covers exactly its declared path; the same field name elsewhere fails closed |
| `declared_timestamp_uuid_temp_path_and_actor_are_replaced_with_counts` | declared kinds become the four tokens and the total replacement count is recorded |
| `replacement_ledger_records_paths_kinds_and_per_kind_counts` | the ledger records each path/kind pair, `replacement_count_for` sums per kind, and canonical bytes carry the tokens with no raw volatile value |
| `wildcard_rules_cover_array_members_and_replacement_budget_is_bounded` | `*` rules replace each array member and a limited policy stops with `volatile_replacement_limit_exceeded` |

## Evidence and limits

- `kiana-quality/tests/eq19_volatile.rs` holds the fixtures above; they drive the pure
  `normalize_volatile`/`VolatilePolicy` API over EQ-18 `CanonicalEventTrace` values.
- `kiana-quality/tests/eq19_quality_guard.rs` protects the token/rule/no-I/O boundary and binds
  every fixture name to this baseline and the EQ-19 lane.
- `.github/workflows/eq19-trace-volatile.yml` runs the two targets, the named `--exact` fixtures and
  the `--no-run` compilations in GitHub Actions (manual `workflow_dispatch`); the unified
  `.github/workflows/ci.yml` `kiana-quality` shard runs the whole crate including both targets.
- Local tests are intentionally not executed in this CI-only session; their compilation and
  runtime correctness depend on the GitHub Actions receipt, which is not awaited.

This slice is `feature_status=implemented`, `proof_level=source`: explicit volatile normalization
and CI fixtures are present, while event/trace digest registration, diffing, capture and durable
quality evidence remain later EQ steps.
