# CM-15 ContextPlan selection and omission baseline

> Snapshot date: 2026-09-20. This source slice is verified by GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-15`](context-memory.md#step-cm-15) |
| feature_status | `implemented` (typed material classes, fixed ordering, source budgets and explainable omission) |
| proof_level | `source`; local checks are limited to formatting/diff hygiene, focused fixtures run in GitHub Actions and are not awaited |
| canonical path | PromptBundle/retrieval candidate → ContextCandidate(material type + authority) → ContextPlan(source budgets + omission reasons) |
| authority | only ProductSystem/Role from verified Prompt sources may be Product; all task/packet/workspace/history/Memory/repo/live material is Context |

## Contract and behavior

`ContextMaterialType` covers Product/system, role, task, packet, workspace snapshot, history,
Memory, repo map and live results. The type fixes selection order and allowed source kinds;
`ContextCandidate` rejects Product authority for non-Prompt material and rejects a Product label
without verified Prompt evidence.

`ContextPlan` records one budget for every material type, selects deterministically by authority,
material order, caller priority and name, and marks every omitted item with either
`context_source_budget_exceeded` or `context_budget_exceeded`. The source-budget usage is
validated against the included items and participates in the plan digest.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `context_plan_selection_is_explainable` | fixed Product/Role-first order, per-material budget usage and Memory omission reason are visible |
| `untrusted_text_cannot_enter_product_section` | a workspace source cannot be relabeled as Product |
| `context_plan_selection_keeps_material_and_authority_boundaries` | Core source guard keeps the selection contract in the domain boundary without a second execution path |

## Proof ceiling and handoff

The CM-15 ceiling is `source` plus remote CI wiring. Provider tokenizer/wire accounting belongs to
CM-16, and immutable provider submission/resolution remains CM-17. This slice does not claim
retrieval quality, live freshness, durable index recovery or external authority.
