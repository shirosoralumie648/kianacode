# P1-J2-01 类型化区段与 provenance 基线

> 快照日期：2026-09-18。本页回填 PromptSection/render/provenance 基础；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-J2-01`](../roadmap.md#step-p1-j2-01) |
| feature_status | `implemented`（domain/core/daemon prompt source；CI-only section fixtures） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | Product/Context PromptAuthority is explicit; prompt text and provenance never grant capability permission |
| this step does | typed `PromptSection{name,order,text,source,authority}`, deterministic order/name/source rendering, per-section prompt hash provenance, and PromptBundle product/context partition are used by core/daemon harness assembly |
| this step does not | 不把字符串 transcript、retrieved memory 或 skill 声明当权限，不在本步骤宣称完整 ContextPlan/immutable wire snapshot/tokenizer budget；这些留 P1-J2-02+、H20/CM |

## 1. Contract

Sections are sorted by `(order,name,source)` before rendering, so insertion order cannot change the
prompt. Provenance records source, authority and a hash of the exact section text. Product safety and
role sections remain distinct from lower-trust context sections; downstream policy and capability
authorization never consume the rendered text as authority.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `context_sections_render_with_provenance` | typed sections render in deterministic order and expose source/authority/text hash provenance |
| `prompt_render_order_is_deterministic_for_equal_orders` | equal-order insertion differences do not change the rendered prompt |
| `context_sections_stay_typed_and_provenance_bound` | core/daemon harness assembly preserves typed sections and provenance markers |

## 3. Proof ceiling and handoff

P1-J2-01 proof ceiling is `source` plus CI domain fixtures: typed sections, deterministic rendering,
provenance and product/context separation are explicit. Budget coverage, role prompt provider wire,
role-pack loading, immutable StepContext and retrieval/omission explanation remain P1-J2-02/03/04,
H20/CM work.
