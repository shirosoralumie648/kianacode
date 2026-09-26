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

## Evidence and limits

- `kiana-quality/tests/eq32_context.rs` covers a fully admitted current hit, ACL-before-ranking
  denial, provenance/freshness/evidence gaps, budget overflow/enforcement and compaction drift.
- `kiana-quality/tests/eq32_context_guard.rs` protects the pure ACL-first boundary and required
  provenance/freshness/compaction/budget markers.
- `.github/workflows/eq32-context-memory.yml` runs fixtures, source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim semantic
retrieval quality, durable Memory/index state, real tokenizer/provider behavior, cross-process
compaction recovery, deletion/retention proof, promotion authority, live or physical evidence.
