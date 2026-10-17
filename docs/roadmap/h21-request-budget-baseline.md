# H21 real request budget and stable prefix baseline

## Delivered source slice

- `WireBudgetInput` accounts for system, history/message, tool schema, attachment and provider
  framing bytes plus one shared output reservation/cap.
- `TokenAccounting::Exact` and explicit `ConservativeUtf8` are distinct; the latter carries its
  bytes-per-token and safety margin instead of pretending to be arbitrary-provider tokenizer
  truth. Unicode, emoji and long-schema boundaries are covered by CI fixtures.
- `StablePrefix` fixes ordered stable segments and keeps dynamic suffix digest out of the prefix
  and cache key. Profile, prompt bundle, catalog and data epoch all participate in cache identity,
  so revocation/data changes cannot reuse a stale prefix.
- Existing runner budget/compaction source is guarded for shared output reservation, prompt source
  provenance and product-owned stable prefix behavior.
- `kiana-runner/src/step_context.rs` exposes one shared reuse decision
  (`decide_stable_prefix_reuse(previous, current)`) that both the harness and the fixtures call, so
  the recorded explanation and the actual cache decision cannot drift apart.
- `compile_wire_budget_with_accounting(prepared, accounting)` is the runner-side entry point for
  adapters that can supply an exact count or a tokenizer digest; `compile_wire_budget` keeps the
  explicit `CONSERVATIVE_WIRE_ACCOUNTING` fallback. Either way the output reservation and output cap
  come from the same effective `TokenBudget` (`reserved_output` / `limit`).

## Behaviour fixtures (2026-10-17 completion slice, CI-only)

`kiana-runner/tests/h21_request_budget_behavior.rs` drives the real `KianaHarness` with a capturing
fake `ModelClient` (no network, no tool execution, no DaemonHost) and feeds tool results back
through the protocol `CapabilityResult` command. Deny first, then success:

- `h21_huge_schema_or_system_prompt_exhausts_request_budget` (deny): a ~86 KiB product section plus
  an adapter window that leaves only 512 tokens of slack fails with
  `step_context_wire_budget:wire_budget_exceeded`; the rejected request never reaches the provider
  and exactly one frozen `PreparedModelCall` was produced before the refusal.
- `h21_cache_hit_cannot_bypass_data_revocation` (deny): path A narrows the tools the adapter
  compiles for step 2 of the same run, so the effective catalog digest moves and the recorded
  decision is `stable_prefix_authority_revoked` (visible segments and the task are unchanged, the
  cache key changes, and the narrowed step still runs - only reuse is refused); path B compares two
  real compiled prefixes whose visible stable segments are identical but whose prompt bundle digest
  differs (deleted/changed source) and gets `stable_prefix_data_epoch_changed`. The positive
  control recompiles the same inputs and gets `reused: true` with the same cache key.
- `h21_context_fits_actual_wire_budget_and_preserves_stable_prefix` (success): two steps stay under
  the same effective limit, the five budget parts plus provider framing partition exactly the wire
  body that was sent, coverage items are the fixed seven, stable segments keep the declared order
  and one cache key while `dynamic_suffix_digest` moves with history, step 2 records `reused: true`,
  the sent tool order equals the fixed catalog on both steps, usage deltas are `Unknown` when the
  adapter reports nothing and otherwise equal `actual - estimated` item by item, and the
  `ExactTokenizer` accounting path rebuilds the same frozen request while refusing an over-limit
  exact count.

`kiana-domain/tests/h21_request_budget.rs` adds the boundary/contract fixtures: the conservative
formula is pinned for CJK, code and emoji samples; a long tool schema exhausts the shared budget and
passes once it fits; exact/`ExactTokenizer` counts are declared and digest-bound (bad digest, zero
count and over-limit all fail closed); coverage items are the fixed seven and one input yields one
budget digest; `cache_hit_cannot_bypass_data_epoch_revocation` checks the data-epoch refusal before
the authority-epoch refusal; stable prefix segments sort by declared priority and duplicate names
are rejected; the fixed tool order is deterministic and pinned by `tool_catalog_hash`.

`kiana-runner/tests/h21_budget_prefix_guard.rs` keeps the source markers (now including
`kiana-runner/src/step_context.rs`) and adds
`h21_behaviour_fixtures_are_wired_into_the_diagnostic_lane`, which binds the three fixture names to
the behaviour file, this baseline and the focused workflow, requires the deny fixtures to precede
the success fixture, and requires the fmt step, three targets, per-fixture `--exact`,
`--test-threads=1` and three `--no-run` compiles.

## Boundary and proof ceiling

The exact tokenizer/provider adapter remains optional and external; local conservative estimation
is an explicit fallback. `ExactTokenizer` counts are only exercised through the runner API with a
fixture-supplied digest: no real provider tokenizer is wired into the harness, so this step does not
claim a provider-specific billing/tokenizer result or accuracy, a cache hit rate, live provider
behaviour, cross-process cache durability, or any savings ratio.

The manual focused diagnostic workflow [`h21-request-budget-diagnostic.yml`](../../.github/workflows/h21-request-budget-diagnostic.yml)
rebuilt into a single job: `cargo fmt -p kiana-domain -p kiana-runner --check`, the domain budget
target, the runner guard target, the runner behaviour target, a per-fixture `--exact` loop over the
three card-named fixtures, and `--no-run` compiles for all three targets. An unawaited remote run
does not raise the proof level.
