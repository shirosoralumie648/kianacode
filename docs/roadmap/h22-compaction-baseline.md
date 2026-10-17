# H22 working-state compaction baseline

## Delivered source slice

- Versioned `CompactSummary` carries goal, pending work, next action, bounded constraints and
  evidence-only decision/completed/verification references. Plain prose cannot assert a completed
  tool or approval.
- Compaction is split into **plan → generate → validate**:
  `kiana-runner::plan_compaction` is a pure function that first externalizes oversized tool
  observations into `result:<digest>` references, deduplicates repeated observations, then selects
  the older complete groups to fold while always retaining system/role messages, the newest complete
  user/assistant/tool group, the latest goal and any pending assistant/tool pair.
- The summary itself comes from an admitted `ModelClient` request with `ModelPurpose::Compaction`,
  an empty tool surface (domain route admission denies `compaction_tools_denied`), a bounded prompt
  (`COMPACT_SUMMARY_PROMPT_MAX_TOKENS`) and a separate bounded budget
  (`COMPACT_SUMMARY_BUDGET_TOKENS`). Provider transport retries stay inside the existing bounded
  attempt driver; a summary that fails validation is **not** retried — the run keeps the original
  view and pauses with `compact_summary_unavailable:<reason>`.
- `parse_compaction_summary` parses a size-bounded JSON object, stamps the server-computed digest,
  discards every model-supplied evidence entry and resolves references against the authoritative
  `CompactSummaryEvidence` projection bound per run (`KianaHarness::bind_compaction_evidence`).
  A reference that is missing or not `confirmed` rejects the whole summary, so a summary cannot forge
  a completed tool, an approval, a decision or a verification fact.
- The view builder never invents a local placeholder summary: with nothing to fold it performs only
  bounded truncation (and keeps a previous summary as the working-state carrier), and an overlarge
  latest group still fails closed with `compact_latest_group_exceeds_budget` before any model call.
- `KianaHarness::compaction_explanation` exposes the compacted counts, externalized/duplicated
  references and summary digest as a display/audit object; it is not an EventLog fact and does not
  change Invocation/approval/task state.

## Named behaviour fixtures

The deny path is proven first; all three run in
[`kiana-runner/tests/h22_compaction_behavior.rs`](../../kiana-runner/tests/h22_compaction_behavior.rs)
with a purpose-aware fake `ModelClient` (no network, no tool execution, no DaemonHost):

- `h22_summary_cannot_forge_completed_tool_or_approval` (deny): a summary referencing an unknown
  `event:` completion or an unconfirmed approval is rejected; the run pauses, the view is unchanged
  and no task request follows.
- `h22_compaction_keeps_pending_pairs_and_latest_goal`: the newest group (including a pending
  assistant/tool pair) and the latest goal stay verbatim, oversized older results are externalized
  to `result:<digest>`, repeats are elided and the summary prompt never re-reads externalized bodies.
- `h22_agent_continues_task_after_two_compactions_without_repeating_completed_edit`: after two
  model-summary compactions the agent still sees that the edit already happened (confirmed
  `file:REPORT.md` evidence in the summary) and never re-requests the completed `apply_patch`.

## Boundary and proof ceiling

The authoritative evidence projection is bound by the caller; deriving it durably from the event
projection, persisting the summary as its own event and cross-process recovery remain H23/H24 work.
The runner does not upgrade model prose, local observations or its own estimates into authoritative
facts, and `compaction_explanation` is not a receipt. Token numbers are local conservative estimates
(≈4 bytes/token); they are neither a provider tokenizer nor a billing proof.

The manual focused diagnostic workflow [`h22-compaction-diagnostic.yml`](../../.github/workflows/h22-compaction-diagnostic.yml)
checks formatting and runs the domain summary fixtures, the runner source guard and the behavior
fixtures (plus `--exact` reruns and `--no-run` compiles). An unawaited remote run does not raise the
proof level.
