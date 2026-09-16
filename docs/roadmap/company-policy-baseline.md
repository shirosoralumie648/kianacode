# CO-05 Company command policy and human-decision baseline

> 快照日期：2026-09-16。本文记录 CompanyCommand 的统一 policy、DecisionPurpose/HumanTask/HumanDecision 合同和 ControlPlane 入口 guard；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CO-05`](companyos.md#step-co-05) |
| source snapshot | `a7f4bbb`（CO-04 parent）加本步源码；最终 commit 记录在 git history |
| feature_status | `implemented`（domain policy/decision contracts + core admission source） |
| proof_level | `source`；静态编译与远程 fixtures 不提升为 local_behavior/durable/live/physical |
| authority path | RequestContext(server actor/role/cell) → CompanyCommandPolicy → server-generated HumanDecision in CompanyProof → existing CompanyState transition/EventLog CAS |
| this step does | 为每个 CompanyCommand 暴露显式 allowed roles、actor kinds、assignment/evidence 标记和 DecisionPurpose；HumanTask/Decision 固定目标 revision/digest、scope、选项、有效期、decider 和 authority epoch；ControlPlane 在解析命令后先做 policy admission，拒绝 agent/SponsorProxy 自批并把 decision 记录到同一 Company event |
| this step does not | 不把工具 approval 当 Charter/Acceptance/Delivery 决定；不新增审批存储或第二执行循环；现有历史 CompanyEvent replay 对缺失的新 decision 字段保持兼容，新的写入口仍必须经过 core policy |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain policy/decision contract | `kiana-domain/src/company_policy.rs`, `kiana-domain/src/company.rs`, `kiana-domain/src/company_business.rs`, `kiana-domain/src/lib.rs`, `kiana-domain/src/contracts.rs` | `71f96e97c89297f1eec642420d9254b92878ec3ca850f96d5b281ef89ec909a6`, `34444282dbafd9f1b696288dc0c6e466f1f52babe301a9060e16a095b635580f`, `0190af710f60fb71768afaad0961beda4e0803d3ac21f6258e54843f7f05141e`, `d4920006e3aa4a20b5175365054627e632c0a63744e088383389be16383a4e61`, `116b9b7c2eb22ab38b1ab4a1865a1d16b64f040ffb221113fd973757c65245cc` |
| Core admission/proof | `kiana-core/src/company.rs`, `kiana-core/src/company_business.rs` | `1d417b108d41e858c0f2def8a53a01515379da2c9f7ad168255b451035dd02c0`, `0885fdff073d65f4565ab7fc8c54712d0da8135c14d69fe4a22655bf7c825aaf` |
| Protocol/export | `kiana-protocol/src/lib.rs` | `96a2c4ab571fa150a8998f34971869942c08025a377ef0df3283a92d4e0dfef6` |
| Fixtures and CI | `kiana-domain/tests/co05_policy.rs`, `kiana-core/tests/co05_policy_guard.rs`, `.github/workflows/co05-company-policy.yml` | `9e3da767af5c63e016a290c14e99653d4f20b02dcf5cd9f570a8858061d4138b`, `7e7e46f6a0810e0639622f98d0f0489bcc0eb46b07275f594312d2c58f5eef44`, `20b19cca4444c63d31ad00fd56627caf024487474b68ca484d14ad1179a58702` |

## 2. Command policy contract

`CompanyCommandPolicy::for_command` 为每个现有命令生成稳定 `kiana.company-command-policy.v1` 快照。角色集合从 command/action 的 server catalog 派生，当前命令缺少显式条目的 fallback 是空集合，`authorize_context` 直接 `company_role_denied`；新增变体不会继承权限并必须显式补矩阵。`assignment_required`、`evidence_required` 和 policy digest 随快照保存，不能从模型/UI payload 覆盖。

高影响命令映射到 `DecisionPurpose`：Objective/Project/Budget、Acceptance、Delivery、Change、ProjectControl 和 BusinessCloseout。Packet approval 保留目的标签但不强制人类，因为规划角色可在其已授权 packet path 产出建议；真正的人类决定只接受 `DecisionActorKind::Human`。`DecisionActorKind::from_context` 将有 cell 的受控模型运行识别为 Agent，将明确 `service:`/`system:` actor 识别为 Service，缺失身份为 Unknown。

## 3. HumanTask / HumanDecision

`HumanDecision` 绑定 decider principal、role/session、command name、purpose、option、target revision/digest、scope digest、有效期和 authority epoch。它由 core 根据已认证 `RequestContext` 与当前 Company revision 生成，拒绝 agent、无 actor、过期、未知选项、digest 篡改和 scope drift；command `RejectProject`/`DecideAcceptance(Reject)` 等记录对应选项，不把所有结果伪装成 Approve。

`HumanTask` 是面向 inbox/恢复的可序列化投影，固定 target、允许选项、期限、状态和可选 decision。它不拥有执行权；审批消费仍复用现有 ApprovalStore，工具批准不能替代业务 Charter/Acceptance/Delivery 决定。

## 4. Core integration and compatibility

`ControlPlane::handle_company_command` 在读取 CompanyState 前调用 `request.command.policy().authorize_context(&context)`，因此 agent/SponsorProxy 或 Builder 无法绕过角色矩阵提交 Sponsor-only decision。`company_proof` 将当前 actor kind 写入 BusinessProof，并生成 HumanDecision；CompanyEvent 仍是唯一业务事实，CAS/idempotency/StartRun→Harness 路径不变。旧 event 缺失 `human_decision` 时可被读取/重放，新的命令写入不允许跳过 policy。

## 5. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `policy_separates_human_decision_from_agent_and_records_purpose` | Acceptance decision 绑定 purpose/option/target，agent cell 和过期 decision fail-closed |
| `human_task_binds_options_target_digest_and_scope` | HumanTask 只能使用 purpose 允许选项，unknown fields 拒绝 |
| `company_commands_use_one_policy_and_human_decision_boundary` | Company command policy、core admission、BusinessProof actor kind 仍在单一 ControlPlane 路径 |

`.github/workflows/co05-company-policy.yml` 在 GitHub runner 执行 domain policy fixtures、core source guard、fmt 和 domain/protocol/core/daemon test-target compile；本地只做格式、静态编译和 diff 检查。

## 6. 限制与交接

- 当前 HumanTask/Decision 由 CompanyProof 伴随事件生成，尚无独立 durable Human Inbox、跨进程 assignment CAS、通知投递或人工 UI action journal。
- 业务命令的完整 object-state/assignment/project scope 矩阵仍要在 CO-06/CO-07 结合不可变 Artifact/Evidence 与稳定 receipt 收口；本步不声称所有旧 Company 命令已具备最终业务权限。
- Actor kind 目前由 server RequestContext 的 cell/service 标记推导，真实多主体身份、SponsorProxy assignment 和组织成员持久化依赖 CO-03/后续身份切片。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
