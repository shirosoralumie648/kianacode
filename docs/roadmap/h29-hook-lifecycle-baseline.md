# H29 有序、受约束的 Hooks / Skills 扩展点基线

> 快照日期：2026-09-19。本页记录 H29 的 source slice 与 CI-only 夹具；本地不运行测试，GitHub Actions 负责运行域与 Daemon source guard。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H29`](harness.md#step-h29) |
| feature_status | `implemented`（domain lifecycle contract + existing daemon boundary guard；CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| authority | Hook/Skill snapshot only describes bounded inputs and transformations; ProjectTrust, ControlPlane, Broker and existing pre-tool port remain the authority path |
| this step does | 新增六个 lifecycle point 的 deterministic order、Observer/Transformer role、read/write field allowlist、timeout/output/feedback budgets、failure policy、snapshot/source/binding digest；tool.arguments 变更强制 revalidation；existing pre-tool hook remains trust-gated, read-only/confined/cancellable and event-recorded, skill loading remains trust-aware/context-only |
| this step does not | 不把 hook 变成 grant/tool runner，不执行第二模型循环，不在本切片宣称所有 lifecycle points 的 live scheduler、真实 provider hook 或跨进程 hook projector 已完成 |

## 1. Contract

`HookLifecycleBinding` 绑定一个 immutable ExtensionSnapshot、source digest、lifecycle point、
sequence、execution role、可读/可写字段、failure policy 和 bounded budget。Observer 没有写集；
Transformer 只能修改 `context.*` 等白名单字段。若允许 `tool.arguments`，结果必须带
`requires_revalidation=true`，后续 Core 必须重新计算 schema/hash/risk/approval binding。

`HookLifecyclePlan` 对同一 snapshot 做稳定排序和重复身份拒绝；输入/输出只携带 digest 和
bounded fields，不携带 capability grant。Daemon 既有实现仍在 ProjectTrust 后加载 project
hooks，使用 read-only confined cancellable execution、deadline/output bounds，并把 `hook.decision`
事实写入 EventStore；Skills 只注入 Context，不扩张 allowed-tools。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `trusted_snapshot_orders_lifecycle_bindings_deterministically` | same snapshot bindings sort by point/sequence/hook identity and reject drift |
| `observer_cannot_write_and_transformer_writes_are_bounded` | observer is read-only; transformer writes only declared context fields |
| `tool_argument_change_requires_revalidation_and_foreign_fields_are_denied` | tool argument mutation is revalidation-bound and forged capability fields are rejected |
| `daemon_hooks_and_skills_keep_trust_snapshot_and_sandbox_boundaries` | existing daemon hook/skill path remains trust-before-load, bounded, confined and context-only |

## 3. Proof ceiling and handoff

H29 proof ceiling is `source`: ordered ABI-like contracts and deny-first field/revalidation rules are
defined and existing Daemon boundaries are source-guarded. Full multi-point live scheduler, hook
provider behavior, durable cross-process snapshot projector and performance/physical proof remain
open for later H32/PD/provider work.
