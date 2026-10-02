# SC-01 security threat register and fixture catalog

> 快照日期：2026-10-02。本页是 SC-01 的威胁与验证登记，不是安全认证或实现完成声明；运行时 fixture 只由 GitHub Actions 执行。

## 1. Authority and proof ceiling

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-01`](security-compliance.md#step-sc-01) |
| artifact_status | `implemented`（threat register and fixture catalog are present） |
| feature_status | `partial`（SC-01 roadmap step; source/documentation only, no runtime enforcement） |
| proof_level | `source`；登记、source guard 和 CI wiring 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| ID namespace | This file uses `SC01:T01–SC01:T12`; references here qualify the unchanged `security-compliance.md` section 3.4 `T01` through `T12` set as `COMPLIANCE-3.4:Tnn`. The qualified references are not aliases. |
| outcome boundary | This register provides no evidence of an external/physical outcome. |
| authority | Security Constitution + current source/CURRENT_STATUS; roadmap/reference are design inputs only |
| execution spine | `entrypoints → client/protocol → DaemonHost → ControlPlane → policy/gate/approval → Broker/Runner → handlers → EventLog → Receipt/projection` |
| this step does | threat IDs, assets, attacker assumptions, prevent/detect/recover controls, evidence ceilings and deny-first fixture catalog |
| this step does not | 不新增 SecurityContext/authn/SecretStore/Policy/adapter，不把 fixture 名称、类型、CI 文件或参考实现当作 enforcement，不创建第二执行循环 |

## 2. Threat register

The `SC01:Tnn` namespace is local to this register. The `Tnn` definitions in
`security-compliance.md` section 3.4 remain unchanged; references to that set are qualified here as
`COMPLIANCE-3.4:Tnn`. Section 2.1 records thematic relationships, not identifier aliases.

| SC-01 ID | Threat / abuse case | Impact if successful | Asset / boundary | Current control anchor | Evidence ceiling | Owner area / roadmap steps |
|---|---|---|---|---|---|---|
| SC01:T01 | wire actor/role impersonation | An untrusted caller can act under another identity, cross a project or role boundary, or corrupt attribution of later decisions. | Principal/Session/Assignment | Daemon server principal, ProjectIdentity, SessionAssignment | source/CI local fixture | Identity and session ownership: SC-04/06/07/08 |
| SC01:T02 | project trust bypass, indirect prompt injection, or supply-chain substitution of project resources/extensions | Untrusted instructions or substituted extension content can influence tool selection, load unreviewed code, expose data, or cause an unauthorized effect. | ProjectTrust, skills/plugins/MCP and extension inputs | trust/source resolver guards; SC-28/29 digest and provenance gates remain separately tracked | source/local slices | Project trust, input provenance, and supply chain: SC-04/06/24/25/26/28/29/39 |
| SC01:T03 | authority/policy/approval scope widening | A decision can authorize a different actor, operation, data set, or effect than the approved scope. | Grant/Approval/Policy/epoch | domain intersections, authority ledger, approval binding | source/CI | Policy, grant, and approval ownership: SC-05/08/09/10 |
| SC01:T04 | child delegation superset | A descendant Cell can exceed its parent's scope and access data or effects unavailable to the parent. | Cell/WorkPacket/Swarm | parent subset checks, typed graph/lineage | source/CI | Scope and delegation ownership: SC-09, SW-04/05 |
| SC01:T05 | path traversal/symlink/TOCTOU | A path race or escape can read or modify data outside the approved root or invalidate a pinned artifact reference. | workspace/artifact writes | lexical containment, no-follow helpers and path locks | local slices/source | Broker filesystem boundary: SC-13/14/20/22, PD |
| SC01:T06 | secret exfiltration through prompt/event/receipt/log/provider | Credentials or sensitive data can cross a trust boundary and persist in a prompt, event, receipt, log, or provider response. | SecretRef, EventLog, output | redaction/sentinel and opaque credential contracts | local/source; not arbitrary high entropy | Secret and data-boundary ownership: SC-18/19/20/21/23/24 |
| SC01:T07 | model/UI self-report approval/completion | A false completion or approval claim can mislead reviewers and downstream workflow as though it were a committed fact. | EventLog/Notification/Quality | server-owned event/source registry and ControlPlane facts | source/CI | Event-source and projection ownership: NM-03, EQ, SC-31/32 |
| SC01:T08 | duplicate/unknown/ambiguous side effect | A repeated or unresolved action can execute twice or be reported with a false terminal state, corrupting receipts and budgets. | Event/Receipt/Invocation | CAS/idempotency/ResultUnknown/fence | source/local slices | Command, receipt, and recovery ownership: CP/ER/PD/SW, SC-12/15/31 |
| SC01:T09 | cancellation race / stale worker | A late worker can commit or send an effect after cancellation, revocation, or a terminal decision. | Run/Cell/Lease | cancel token, terminal scope, authority/lease epoch | local slices/source | Runner lifecycle and cancellation ownership: SC-15/16, SW-09/10 |
| SC01:T10 | corrupt/torn/replayed storage or retention/deletion boundary violation | Facts may be altered or lost, retained data may outlive its allowed scope, or deletion may erase evidence needed for reconciliation. | facts/projections/artifacts/retention evidence | JSONL frame/CAS/health/schema contracts; SC-22/23 retention and tombstone propagation remain partial | source/local historical | Storage, retention, and recovery ownership: SC-22/23/31/32/42, PD/ER/DEP |
| SC01:T11 | resource exhaustion / retry storm | Unbounded work can exhaust CPU, disk, queues, provider budgets, or block unrelated runs. | budget/queue/provider/storage | bounded sizes, budgets, queue and retry ceilings | source/CI | Resource and cost ownership: SC-16/30/40, BQ/AUT/SW |
| SC01:T12 | external connector/physical action misuse | An unsupported or mis-scoped connector can trigger an unauthorized external API, payment, publication, or device action without a trustworthy receipt. | provider/MCP/webhook/payment/IoT | unsupported/local_fixture/stdio gates, no external authority | source only; no live/physical | Connector and effect-boundary ownership: SC-12/17/26/30, INT |

Owner areas identify the roadmap workstream responsible for follow-up, not a named individual;
their presence does not mean the corresponding control is complete.

### 2.1 Crosswalk to `security-compliance.md` section 3.4

The section 3.4 definitions are preserved. This crosswalk records overlap between two taxonomies;
one legacy row may map to several `SC01:` rows and a mapping does not make the IDs interchangeable.

| Normative source ID | Existing section 3.4 threat label | Related SC-01 register ID(s) | Relationship and remaining scope |
|---|---|---|---|
| COMPLIANCE-3.4:T01 | prompt injection / indirect injection | SC01:T02 | Partial thematic overlap; SC01:T02 also includes project trust and supply-chain substitution. |
| COMPLIANCE-3.4:T02 | confused deputy | SC01:T01, SC01:T03, SC01:T12 | Partial overlap across caller identity, authority scope, and external-account boundaries. |
| COMPLIANCE-3.4:T03 | privilege escalation | SC01:T01, SC01:T03, SC01:T04 | Partial overlap across identity, authority widening, and child delegation. |
| COMPLIANCE-3.4:T04 | secret exfiltration | SC01:T06 | Thematic overlap; SC01:T06 records the wider event/receipt/log/provider boundary. |
| COMPLIANCE-3.4:T05 | TOCTOU / path escape | SC01:T05 | Direct topic overlap; no claim that the fixture or controls are equivalent. |
| COMPLIANCE-3.4:T06 | replay / duplicate effect | SC01:T08, SC01:T09 | Partial overlap across duplicate/unknown effects and stale-worker races. |
| COMPLIANCE-3.4:T07 | SSRF / network boundary | SC01:T12 | Partial overlap only; SC01:T12 is broader and does not replace endpoint/DNS-specific coverage. |
| COMPLIANCE-3.4:T08 | resource exhaustion | SC01:T11 | Thematic overlap; budgets, queues, and retry behavior remain owned by their listed steps. |
| COMPLIANCE-3.4:T09 | fact tampering / hiding | SC01:T07, SC01:T08, SC01:T10 | Partial overlap across self-report, ambiguous effects, and storage integrity. |
| COMPLIANCE-3.4:T10 | supply-chain poisoning | SC01:T02 | Partial overlap for project extensions; dependency digest, license, SBOM, and provenance remain SC-28/29 work. |
| COMPLIANCE-3.4:T11 | privacy / retention violation | SC01:T06, SC01:T10 | Partial overlap across secret/data disclosure and retention/deletion boundaries. |
| COMPLIANCE-3.4:T12 | incident misrecovery | SC01:T08, SC01:T09, SC01:T10 | Partial overlap across unknown effects, stale workers, and storage/recovery facts. |

Threat assumptions: caller text, model output, UI state, transcript, cache, fixture, process exit and HTTP ACK are untrusted/non-authoritative; EventLog facts are authoritative only after committed CAS; unknown effects stay Unknown. An attacker may control wire fields, project-local resources, plugin/MCP manifests, fixture contents, timing and stale workers but cannot mint server secrets or bypass OS/filesystem reality merely by naming a role/path.

## 3. Control/evidence matrix

| Control family | Prevent | Detect | Recover / quarantine | Current status |
|---|---|---|---|---|
| Identity/trust | server principal, project root, role/department checks | mismatch/expiry/epoch events | revoke/session fence | partial/source; SC-04+ |
| Scope/capability | intersection, path/data locks, five-tool allow-list | permit/CAS/audit/reason codes | ResultUnknown/quarantine, no blind retry | partial/source/local slices |
| Secret/data | SecretRef, redaction, purpose/scope | sentinel/redaction drift, provider echo checks | redact/reconcile/retain fact, never expose raw | partial/source |
| Event/storage | schema/field/sequence/CAS/digest | health/checksum/cursor/integrity incident | named upcast/quarantine/rebuild | PD/ER partial/source |
| Messaging/quality | message authority=false, source registry, quality command registry | owner/source/unknown family checks | notification/review/quality facts remain non-authoritative | NM/EQ source |
| External/physical | deny-by-default unsupported adapters | connector receipt/unknown (future) | human confirmation/reconcile (future) | not_supported/source |

## 4. CI-only security fixture catalog

Fixture names below record required follow-up assertions; their presence in this catalog does not
mean the runtime control or a corresponding executable fixture is already implemented.

| Fixture | Threats | Required assertion | Existing executable target(s) | Coverage status | Owner steps |
|---|---|---|---|---|---|
| `security_baseline_covers_constitution_assets_and_spine` | all SC01:T01..T12 | constitution/assets/spine/feature-proof map remains explicit | No SC-37/39 target; SC-00 owns the baseline guard. | planned | SC-00 |
| `wire_actor_role_impersonation_is_denied` | SC01:T01/T03 | server actor/role overrides caller and no Broker effect | `kiana-core/tests/sc37_deny_matrix.rs::sc37_privilege_escalation_forged_role_is_denied_with_zero_dispatch` (forged-role deny only) | partial; session/project variants remain planned | SC-04/06 |
| `project_local_resource_is_untrusted` | SC01:T02 | project skills/plugins/MCP source cannot load before trust | Adjacent only: `kiana-core/tests/sc39_red_team_corpus.rs::sc39_malicious_plugin_manifest_cannot_self_authorize` checks self-authorization, not pre-load trust. | planned | SC-06/24/25 |
| `project_prompt_injection_is_untrusted_data` | SC01:T02 | instruction-like project/network content cannot alter trust, authority or cause Broker dispatch | `kiana-core/tests/sc39_red_team_corpus.rs::sc39_prompt_injection_cannot_become_product_authority`; `kiana-core/tests/sc39_red_team_corpus.rs::sc39_indirect_injection_from_repository_text_cannot_widen_a_grant` | partial; broader chains remain planned | SC-24/25/39 |
| `unverified_extension_dependency_is_quarantined` | SC01:T02 | changed/missing digest, signature or provenance fails closed before extension load or effect | Adjacent only: `kiana-core/tests/sc39_red_team_corpus.rs::sc39_malicious_plugin_manifest_cannot_self_authorize`; this does not verify dependency provenance. | planned | SC-28/29/33/41 |
| `grant_scope_intersection_never_widens` | SC01:T03/T04 | child scope is strict intersection across path/data/budget/epoch | No directly equivalent SC-37/39 target identified in this catalog. | planned | SC-09/SW-04 |
| `path_symlink_and_toctou_escape_is_denied` | SC01:T05 | lexical + effect-time no-follow checks block outside writes | Adjacent only: `kiana-core/tests/sc37_deny_matrix.rs::sc37_toctou_fence_scope_drift_is_denied_with_zero_dispatch` checks fence drift, not filesystem symlink replacement. | planned | SC-13/14/20 |
| `secret_never_reaches_event_receipt_provider` | SC01:T06 | raw token/bearer/sentinel absent at every output boundary | Partial targets: `kiana-core/tests/sc37_deny_matrix.rs::sc37_leakage_secret_sentinel_in_receipt_text_is_denied_with_zero_dispatch`; `kiana-core/tests/sc39_red_team_corpus.rs::sc39_secret_exfiltration_cannot_leave_the_broker` | partial; all output/provider boundaries remain planned | SC-18/19/23 |
| `model_or_ui_self_report_never_becomes_critical_fact` | SC01:T07 | source registry rejects model/UI approval/completion | No directly equivalent SC-37/39 target identified in this catalog. | planned | NM-03/SC-31 |
| `unknown_or_duplicate_effect_is_quarantined` | SC01:T08/T10 | gap/digest/idempotency/Unknown never dispatches twice | Adjacent only: `kiana-core/tests/sc37_deny_matrix.rs::sc37_replay_unbound_permit_is_denied_with_zero_dispatch` checks an unbound replay, not duplicate/Unknown recovery. | planned | ER/PD/SC-31 |
| `retention_and_tombstone_scope_violation_is_denied` | SC01:T10 | mis-scoped retention/hold/delete is denied; incomplete tombstone propagation is not reported complete | Partial targets: `kiana-core/tests/sc37_deny_matrix.rs::sc37_deletion_legal_hold_is_denied_with_zero_dispatch`; `kiana-core/tests/sc37_deny_matrix.rs::sc37_deletion_unknown_retention_is_denied_with_zero_dispatch` | partial; tombstone propagation assertion remains planned | SC-22/23/32/42 |
| `cancel_race_fences_stale_worker` | SC01:T09 | late worker cannot commit/release after cancel/epoch change | No directly equivalent SC-37/39 target identified in this catalog. | planned | SC-15/16/SW-10 |
| `resource_budget_and_retry_bounds_hold` | SC01:T11 | size/turn/token/effect/retry/queue caps reject safely | No directly equivalent SC-37/39 target identified in this catalog. | planned | SC-30/BQ/AUT |
| `external_and_physical_capabilities_stay_denied` | SC01:T12 | network/payment/IoT/publish/physical adapters have zero effect | No SC-37/39 runtime target; external/physical remain not_supported. | planned | SC-12/17/26/30 |

All fixtures must record source snapshot, exact command, environment/fixture digest, exit code and
limitations in `CURRENT_STATUS.md`; source/CI evidence cannot be promoted to durable/live/physical
without the corresponding cross-process or real-boundary proof. CI never stores real secrets.

## 5. Handoff

SC-01 supplies the `SC01:Tnn` IDs and fixture names to SC-02..43, CI/NM/EQ/PD/ER/SW. The register is
append-only evidence; discovering a new gap adds a row and a deny-first fixture rather than weakening
an assertion. Existing `security-compliance-baseline.md` remains the SC-00 current-state inventory.
