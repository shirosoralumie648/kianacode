# P1-J2-03 角色 prompt 接线基线

> 快照日期：2026-09-18。本页回填 RoleSpec.prompt→PromptBundle→provider system message；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-J2-03`](../roadmap.md#step-p1-j2-03) |
| feature_status | `implemented`（domain/core/runner/daemon/provider source；CI-only role-prompt guard） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | Product safety and role prompt sections are server-owned PromptBundle content; provider receives text but grants no capability authority |
| this step does | core builds PromptBundle from the assigned RoleSpec, Runner carries its encoded bundle, daemon/model client decodes the product system prompt and sends it in the provider `system` field, with safety section ordered before role content |
| this step does not | 不让角色 prompt 覆盖产品安全规则、不让环境追加指令改变 policy/gate/approval、不在本步骤实现 role-pack source or immutable provider snapshot；P1-J2-04/H20 handles source pinning |

## 1. Contract

Role assignment selects the role prompt and model profile once in the ControlPlane path. The prompt
bundle keeps product safety and role instructions as Product-authority sections, lower-trust context
as Context sections, and records provenance/hash. The provider adapter only maps the exact system
text to its request; it does not infer permissions from the prompt.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `role_prompt_reaches_the_provider_system_message` | RoleSpec/PromptBundle/Runner/daemon provider route carries role text into the provider system field after product safety |
| `role_prompt_cannot_replace_product_safety_prefix` | role prompt and operator additions remain subject to product policy and cannot become authority |

## 3. Proof ceiling and handoff

P1-J2-03 proof ceiling is `source`: role prompt routing, system-field mapping and safety ordering
are explicit. Role-pack loading, prompt source trust/invalidation, immutable StepContext, exact
provider framing and route/config snapshot remain P1-J2-04/H20/H21/EXT work.
