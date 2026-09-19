# CM-13 Repo map 任务相关排序和依赖证据基线

> 快照日期：2026-09-20。本页记录 task-lens source slice；夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-13`](context-memory.md#step-cm-13) |
| feature_status | `implemented`（domain task selection/evidence contract + query adapter + CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| canonical path | existing read-only RepoMap → task terms/path/symbol score → stable budget selection → heuristic/declared evidence |
| authority | repo map remains derived context evidence; heuristic symbols/edges are never compiler or permission facts |

## Contract and behavior

`RepoMapTaskCandidate` binds content digest, symbols, dependency edges, task score and token
estimate. `RepoMapSymbol`/`RepoMapDependencyEdge` carry explicit Heuristic or Declared evidence;
there is no compiler-fact claim. `RepoMapTaskSelection` sorts score/path/digest deterministically,
keeps selected estimated tokens within budget and records omitted paths and a selection digest.
The query adapter reuses the existing scanner and does not create a second filesystem/index loop.

## CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `repo_map_budget_is_stable` | candidate order does not change deterministic score/path budget selection |
| `heuristic_symbol_is_not_reported_as_compiler_fact` | symbol evidence remains explicitly heuristic |
| `task_lens_reuses_repo_map_and_keeps_budget` | query adapter consumes existing RepoMap and preserves evidence/budget |
| `repo_map_task_lens_keeps_heuristic_and_budget_boundaries` | source guard prevents authority/compiler/second-loop claims |

## Proof ceiling and handoff

CM-13 proof ceiling is `source` plus remote CI wiring. Optional tree-sitter/reference graph,
compiler facts, incremental dependency extraction, durable index integration and production task
quality remain later CM/PD work; no semantic compiler truth is claimed.
