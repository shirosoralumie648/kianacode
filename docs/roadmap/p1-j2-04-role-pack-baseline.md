# P1-J2-04 提示词来源与角色包加载基线

> 快照日期：2026-09-18。本页回填 built-in role packs、trust-aware context extensions 与 prompt hash 复现；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-J2-04`](../roadmap.md#step-p1-j2-04) |
| feature_status | `implemented`（domain/core/daemon/skills source；CI-only role-pack fixtures） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | RoleSpec/role-pack snapshot and server-derived prompt_hash are authoritative; project skills/extensions are lower-trust context after ProjectTrust |
| this step does | nine RoleSpec factories load fixed role-pack files, derive stable prompt_hash, PromptBundle validates the exact hash, and lifecycle/receipt/resume facts carry the hash; trusted project skills are appended only as Context sections after trust resolution |
| this step does not | 不让项目 skill/plugin 覆盖 Product safety/role sections、不把环境 prompt 变成 role-pack authority、不声称 hot reload/signature/remote role catalog；这些留 EXT/SC/DEP/H20 |

## 1. Contract

Role packs are compile-time bundled product assets in this slice. The assigned role's prompt and
hash are captured in RoleSpec, PromptBundle, ModelAssignment, RunSnapshot and receipts; a mismatch
on resume fails closed. Project/user skills and extension context use the existing trust-aware
loader, remain Context-authority, are bounded and cannot alter the five-tool/policy catalog.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `role_prompt_loads_from_the_role_pack` | all nine role prompts equal their bundled role-pack content and validate their stable hash/bundle metadata |
| `role_pack_hash_is_stable_across_bundle_round_trip` | PromptBundle encode/decode preserves the role hash and section provenance |
| `role_pack_hash_reaches_receipt_and_resume_fences` | lifecycle/receipt/resume paths carry and recheck role_prompt_hash; project extensions remain trust-gated context |

## 3. Proof ceiling and handoff

P1-J2-04 proof ceiling is `source` plus CI role-pack fixtures: bundled source, hash provenance,
resume drift fence and trust-aware context extension are explicit. Signed/hot-updatable role packs,
immutable StepContext/wire snapshot, provider framing/cache and cross-process catalog recovery remain
EXT/SC/DEP/H20/H21 work.
