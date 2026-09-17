# SC-04 SecurityContext 与入口身份边界基线

> 快照日期：2026-09-17。本页记录 ControlPlane source contract 与 GitHub CI-only fixture；不把
> server-owned DTO 误写成真实认证或完整跨入口安全证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-04`](security-compliance.md#step-sc-04) |
| feature_status | `implemented`（core SecurityContext、daemon preflight wiring、deny-first fixtures） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixture 执行 |
| authority | DaemonHost/ControlPlane server-owned principal、ProjectIdentity、trust 和 authority stream；wire metadata 只是 assertion |
| this step does | strict versioned SecurityContext、actor/project/role/department/trust assertion checks、authority/data epoch snapshot、effect trust gate、legacy RequestContext projection |
| this step does not | 不实现外部/OS/OAuth authentication、durable session/assignment recovery、PolicyBundle/Grant intersection、SecretStore、四入口完整 UAT 或外部 effect |

## 1. Context contract

`kiana-core/src/security_context.rs` 的 `SecurityContext` 只从 daemon 传入的
`AuthenticatedPrincipalRef`、`ProjectIdentity`、server-selected `RoleSpec`、ProjectTrust 和
ControlPlane authority revision/epoch 生成。它绑定 `SecurityContextId`、role descriptor/policy
digest、authority/data epoch 和 context digest，提供 strict serde、canonical bytes、
`validate_request_assertions`、`apply_to_request` 与 `require_trusted_for_effect`。

入口传来的 actor、session、project path、role、department 和 trust 不能扩大 server snapshot：
缺 actor、主体不一致、foreign project、role/department mismatch 或 caller 声称 trusted 而
server 不 trusted 都 fail-closed。DaemonHost 在进入现有 command/run/approval/query 分派前先
解析该 context；通过后才投影为兼容的 `RequestContext`，仍复用同一 ControlPlane→Broker→EventLog
脊柱。Context 解析不创建 grant、消费 approval 或执行 effect。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `security_context_is_server_owned_versioned_and_round_trips` | server snapshot 的 schema/digest/epoch、strict serde 和 legacy projection 稳定 |
| `caller_identity_project_role_and_trust_assertions_fail_closed` | actor/anonymous/project/role/forged trust 不能绕过 server context |
| `untrusted_server_snapshot_cannot_be_used_for_effects` | untrusted context 显式阻断 effect admission |
| `daemon_entry_builds_one_server_owned_context_before_control_plane_dispatch` | source guard 固定单一 preflight、无 caller actor overwrite shortcut、无第二执行循环 |

## 3. Proof ceiling and handoff

SC-04 证明上限为 `source`：可见的 preflight/context wiring 和 typed deny path 已登记，但
`AuthenticatedPrincipalRef::local` 仍是本地主体兼容实现，loopback/Host/Origin/instance token
不是 durable authentication；authority/session assignment 仍有进程/JSONL 局限，四入口 parity、
真实 OAuth/OS peer、跨进程恢复和 external/physical effect 未证明。SC-05 消费 context digest/epoch
做 policy decision trace；SC-06/07 继续补 Principal/session/authn 与 ProjectTrust/RoleAssignment。
