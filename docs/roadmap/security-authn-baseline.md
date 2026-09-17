# SC-06 Principal、Session 与 authn adapter 基线

> 快照日期：2026-09-17。本页记录 domain/daemon source contracts 与 GitHub CI-only fixtures；
> 外部认证、跨进程 session durability 和生产密钥均不在本步证明范围。

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-06`](security-compliance.md#step-sc-06) |
| feature_status | `implemented`（session assertion、local opaque adapter、daemon preflight） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责运行 fixtures |
| authority | daemon-owned AuthenticatedPrincipalRef/SessionAssertion 与既有 ControlPlane SecurityContext；wire fields 只是 assertion |
| this step does | strict Principal/session lifecycle、assurance/status/expiry/revoke、opaque local adapter、protected-session check、duplicate/replay/missing guards |
| this step does not | 不实现 OAuth/tenant/OS peer authentication、SecretStore、durable session projector、role assignment/Grant、跨进程 recovery 或 live/external effect |

## 1. Contract

`kiana-domain/src/session_contracts.rs` 新增 strict `SessionAssertion`、
`AuthenticationAssurance` 和 `SessionStatus`。assertion 绑定 opaque principal、session、
assurance、issued/expiry、credential generation、authority epoch 和 digest；匿名 active、
过期/吊销/非单调状态转移、unknown field、无效时间或 digest 全部 fail-closed。既有
`Principal` 继续提供 active-at/expiry 与 authentication identity binding。

`kiana-daemon/src/authn.rs` 新增 `LocalAuthnAdapter`：只保存 opaque
`AuthenticatedPrincipalRef` 与进程内 SessionAssertion map，支持开 session、active/expiry/revoke
校验、duplicate replay 拒绝和 protected-local missing deny。它不保存 bearer/API/token 值、不
分配角色、不调用 Broker；未知 legacy session 仅在未声明 protected-local 时保留兼容路径。
DaemonHost 在既有 SecurityContext/ControlPlane 之前调用该检查，成功后仍走原执行脊柱。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `session_assertion_is_versioned_bounded_and_round_trips` | strict schema、expiry、opaque principal 和 unknown raw token 字段拒绝 |
| `session_status_is_monotonic_and_expiry_or_revoke_is_terminal` | suspended/revoked/expired 不 resurrect，anonymous active 拒绝 |
| `principal_snapshot_expiry_and_identity_binding_are_explicit` | Principal identity/expiry 校验稳定 |
| `local_authn_adapter_issues_and_validates_opaque_sessions` | session 生成、active validation、duplicate replay 拒绝 |
| `local_authn_adapter_denies_expired_revoked_and_missing_protected_sessions` | expiry/revoke/missing protected deny，legacy missing 只读兼容 |
| `daemon_authn_adapter_is_opaque_and_precedes_existing_control_plane_path` | source guard 固定 adapter 无 secret/执行依赖且先于 ControlPlane |

## 3. Proof ceiling and handoff

SC-06 的证明上限为 `source`：本地 adapter 和 strict lifecycle 已固化，但 sessions map 是进程内
兼容实现，重启后不会恢复；`AuthenticatedPrincipalRef::local` 仍是 local-user 兼容身份，不等于
OAuth、Unix peer 或 tenant authentication。SC-07 继续把 ProjectTrust/RoleAssignment/Department
快照绑定到该主体，SC-08 负责 epoch/fence refresh；真实认证、SecretStore、provider/外部 effect
和 durable/live/physical 证据仍未宣称。
