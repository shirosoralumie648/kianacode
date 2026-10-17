# EQ-32 context and memory evaluator baseline

## Scope

EQ-32 adds `ContextMemoryEvaluator` to `kiana-quality`. It verifies that ACL admission happens
before ranking/selection, selected memory hits retain bounded source/provenance digests, freshness
and evidence status are explicit, stale/unknown material is not silently presented, compaction
preserves the latest goal/pending pairs/verifiable completed actions, and token/wire budgets are
enforced without overflow.

The evaluator consumes retrieval/compaction/budget evidence only. It does not query Memory or an
index, perform ranking, compile a ContextPlan, call a model, compact a transcript, or alter ACL,
data epoch, policy or budget authority.

## Completion slice (2026-10-17, CI-only)

The original slice existed in source; this completion slice closes the card's exit conditions and
turns the first version into a full, reject-first context/memory contract:

- ACL-before-ranking: a report that admits ranking before ACL taints every ranked/selected hit
  (`context.acl_order_invalid`); an allowing decision without a recorded check is incoherent
  (`context.acl_evidence_incoherent`); ranked and selected hits without admission are separate
  `context.unauthorized_memory_hit` findings (`acl_allowed_before_ranking`,
  `selected_hit_is_acl_admitted`) bound to the offending hit. An unranked, unselected denied hit
  stays silent: denial alone is not a quality violation.
- Provenance: every hit keeps a bounded identity (`context.hit_identity_invalid`, duplicates stay
  visible under distinct `context:hit:<index>` references), a bounded source revision and strict
  `sha256:` source/provenance digests (`context.provenance_invalid`), and must belong to the
  queried scope and data epoch (`context.scope_mismatch`, `context.epoch_mismatch`). The query
  itself needs strict digests and a positive epoch/generation (`context.query_provenance_invalid`)
  plus a positive `top_k` within the hit limit (`context.selection_bound_invalid`).
- Ranking/selection: ranked hits carry dense, unique one-based ranks (`context.rank_invalid`,
  `context.rank_duplicate`, `context.rank_gap`), the presented set is the top-ranked prefix
  (`context.selection_order_invalid`) and never exceeds `top_k` (`context.selection_over_top_k`).
- Freshness/evidence: a selected stale hit needs an explicit acknowledgement
  (`context.stale_not_acknowledged`), a selected unknown hit is never silent
  (`context.freshness_unknown`), a selected hit needs verified/attributed evidence
  (`context.evidence_missing`), and an acknowledgement outside stale material is incoherent
  (`context.stale_acknowledgement_unexpected`).
- Budget: positive token/wire limits (`context.budget_invalid`), an estimate that covers the
  observed usage (`context.budget_estimate_invalid`), no overflow against the token limit, the
  used-plus-reserved window or the wire limit (`context.budget_overflow`), and both budgets
  actually enforced (`context.budget_unenforced`).
- Compaction: an attempt demands a present, digest-bound summary
  (`context.compaction_summary_missing`, `context.compaction_summary_unbound`), a positive ordered
  source range (`context.compaction_cursor_invalid`), preserved latest goal/pending
  pairs/verifiable completed actions (`context.compaction_integrity_invalid`) and a rejected stale
  commit (`context.compaction_stale_commit_unfenced`). Summary/cursor/digest evidence without an
  attempt is `context.compaction_unexpected`.
- Hard input bounds live in `ContextMemoryInput::validate`
  (`context_memory_input_schema_invalid`, `context_memory_hit_limit_exceeded`) so structural drift
  fails closed before any finding; entry-shape problems remain findings.
- Hits are ordered by a stable identity key before evaluation, so the finding set does not depend
  on the caller's array order while duplicate evidence stays visible. Findings carry per-section
  evidence references (`context:query`, `context:acl`, `context:ranking`, `context:hit:<index>`,
  `context:budget`, `context:compaction`) and every caller-supplied value goes through
  `Finding::redacted`.

## Evidence and limits

- `kiana-quality/tests/eq32_context.rs` (25 rejection-first fixtures) covers the card's acceptance
  scenario `unauthorized_memory_hit_is_blocking`, a fully admitted report with no findings, ACL
  evidence coherence and denial, rank uniqueness/denseness, the `top_k` bound and rank-prefix
  selection, hit identity/provenance/scope/epoch, stale acknowledgement and unknown freshness,
  selected evidence status, query provenance, token/wire budget overflow/enforcement/estimate
  bounds, compaction summary/cursor/preservation/stale-commit/unexpected evidence, schema drift on
  every section, unknown-field/envelope drift, bounded references, oversized input rejection,
  order independence with duplicate evidence, the redaction boundary over caller values, typed
  round-trip, and deterministic evidence-bound registry output.
- `kiana-quality/tests/eq32_context_guard.rs` protects the pure ACL-first boundary (no
  filesystem/network/process/provider/store/broker dependency), the required markers and stable
  codes, the fixture matrix, the manual lane's exact steps and the automatic `kiana-quality` shard
  (`targets: null`).
- `.github/workflows/eq32-context-memory.yml` runs `cargo fetch --locked`, the format gate, both
  fixture targets and two `--no-run` compile steps in GitHub Actions; local tests are
  intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim semantic
retrieval quality, durable Memory/index state, real tokenizer/provider behavior, cross-process
compaction recovery, deletion/retention proof, promotion authority, live or physical evidence.

## Known limits

- Findings are diagnostics over caller-supplied values only; they never search Memory or an index,
  rank a record, compile a ContextPlan, compact a transcript, call a model, or change ACL, data
  epoch, policy or budget authority. Mapping them into blocking rules belongs to EQ-42.
- `acl_checked`, `acl_allowed`, the ACL order flag, ranks and the selection flags are
  caller-asserted evidence. The evaluator can refuse an incoherent or unauthorized shape, but it
  cannot prove that a real ACL check happened or that the recorded ranking is the ranking the
  product performed.
- Provenance digests are caller-supplied; the evaluator binds and bounds them but never recomputes
  a source digest or verifies a memory revision against durable state.
- The token/wire numbers are reported estimates/usage; no real tokenizer or provider is contacted,
  so the evaluator cannot prove the estimate matches a model's own accounting.
- References are restricted to a bounded ASCII identity alphabet; unknown shapes are rejected as
  invalid rather than normalized.
- `Finding::redacted` covers known secret shapes; unmarked high-entropy values may still pass.
- The manual lane is `workflow_dispatch`-only; automatic coverage depends on the unified `ci.yml`
  `kiana-quality` shard (both targets are discovered because `targets` is null).
