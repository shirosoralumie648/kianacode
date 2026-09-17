# P1-J2-02 tool schema/system prompt 预算基线

> 快照日期：2026-09-18。本页回填 TokenBudget 覆盖与 fail-closed 校验；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-J2-02`](../roadmap.md#step-p1-j2-02) |
| feature_status | `implemented`（domain/model/ports/runner source；CI-only budget fixtures） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | PreparedModelCall validation and server-owned request context determine budget admission; provider billing is separate evidence |
| this step does | TokenBudget explicitly accounts for message bytes, system prompt bytes, tool-schema bytes, reserved output and limit with conservative framing reserve; prepared calls validate nonzero limit and reject total overflow before provider invocation |
| this step does not | 不把 UTF-8 byte accounting 宣称成任意 provider tokenizer 硬上限，不在本步骤实现 compaction/cache、精确 billing 或第二预算事实源；真实 provider budget remains P4/CP/H21 work |

## 1. Contract

The same `ModelRequestContext::budget` is captured while preparing a model call, and
`PreparedModelCall::validate` rejects a changed or exhausted budget before sending. Tool schemas
and system prompt are separate measured components and both contribute to `total`; reserved output
is included in the same sum. Missing/zero limit and overflow are structured failures.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `tool_schemas_count_toward_the_budget` | adding tool schemas increases total and a one-token-short limit returns context_budget_exceeded |
| `budget_accounting_is_explicit_about_its_conservative_bound` | accounting mode and component framing reserves remain inspectable and deterministic |
| `prepared_model_calls_fail_closed_when_context_budget_is_exhausted` | model/ports/runner admission validates the same budget and includes system/tool components |

## 3. Proof ceiling and handoff

P1-J2-02 proof ceiling is `source` plus CI domain fixtures: system/tool schema coverage and
fail-closed prepared-call validation are explicit. Provider-specific tokenizer counts, wire framing,
cache prefix, compaction and billing/usage reconciliation remain P1-J2-03/04, H20/H21 and P4/CP work.
