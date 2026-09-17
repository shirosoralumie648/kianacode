# CP-09 approval subject 与 execution material 基线

> 快照日期：2026-09-17。本页记录 domain/daemon/core approval-material source contract 与
> GitHub CI-only fixture；不把 challenge/preview 写成执行许可或 durable human authentication。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-09`](control-plane.md#step-cp-09) |
| feature_status | `implemented`（ApprovalExecutionMaterial、journal subject/preview guard、existing approval recheck） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | EventLog-backed JournalApprovalStore + ControlPlane; display preview never authority |
| this step does | exact action/identity/version/target/scope/expiry subject, independent nonce, redacted preview, protected volatile payload handle, material digest and recheck |
| this step does not | 不执行 provider/handler、不把 nonce 当身份、不支持 standing approval、SecretStore 或跨进程 durable payload archive |

## 1. Contract

`kiana-domain/src/approval_journal.rs` 新增 strict `ApprovalExecutionMaterial` 与
`ApprovalMaterialState::{InlineRedacted, VolatileProtected}`。它只保存 payload/preview digest、
state、expiry 和 material digest；不序列化原始 request、preview、secret 或 provider 错误，
`matches_payloads` 可在执行前复核 exact material。

`kiana-daemon/src/journal_approvals.rs` 的 Subject 现在带 material：stage 时先生成 redacted
preview 和 volatile protected payload，再落 `approval.staged`；fold/material 会校验 material
schema/digest/expiry/payload，材料丢失返回 `approval_payload_unrecoverable`，redaction/nonce/size
失败不发布 challenge。既有 authority version、policy version、role prompt hash、request hash
和 nonce 校验继续生效；Approved/Consumed 仍由后续 CP-10/13 事务控制。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `approval_material_binds_payload_preview_state_and_expiry_without_raw_content` | material digest/state/expiry、payload/preview exact hash，JSON 无 raw content |
| `approval_material_rejects_unknown_or_bad_digest_fields` | strict unknown/raw/bad digest fail-closed |
| `cp09_approval_material_keeps_preview_non_executable_and_rechecks_authority` | source guard 固定 material/preview/volatile/authority/continuation boundary |

## 3. Proof ceiling and handoff

CP-09 proof ceiling 为 `source`：subject/material 边界和失败原因已固定，但 volatile payload
仍是进程内，旧 approval records 缺 material 会进入显式 corrupt/reauthorization；Human Inbox、
external human identity、durable pending/decision/consume CAS 和 Broker effect 仍由 CP-10/13、
SC-10/12 继续完成。Preview 只用于展示，任何恢复都必须重新验证 request/action/policy/epoch。
