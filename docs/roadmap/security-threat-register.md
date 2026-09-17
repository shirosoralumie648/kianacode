# SC-01 security threat register and fixture catalog

> 快照日期：2026-09-17。本页是 SC-01 的威胁与验证登记，不是安全认证或实现完成声明；运行时 fixture 只由 GitHub Actions 执行。

## 1. Authority and proof ceiling

| 项目 | 记录 |
|---|---|
| roadmap card | [`SC-01`](security-compliance.md#step-sc-01) |
| feature_status | `implemented`（threat register, asset/control mapping and fixture catalog） |
| proof_level | `source`；登记、source guard 和 CI wiring 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| authority | Security Constitution + current source/CURRENT_STATUS; roadmap/reference are design inputs only |
| execution spine | `entrypoints → client/protocol → DaemonHost → ControlPlane → policy/gate/approval → Broker/Runner → handlers → EventLog → Receipt/projection` |
| this step does | threat IDs, assets, attacker assumptions, prevent/detect/recover controls, evidence ceilings and deny-first fixture catalog |
| this step does not | 不新增 SecurityContext/authn/SecretStore/Policy/adapter，不把 fixture 名称、类型、CI 文件或参考实现当作 enforcement，不创建第二执行循环 |

## 2. Threat register

| ID | Threat / abuse case | Asset / boundary | Current control anchor | Evidence ceiling | Follow-up |
|---|---|---|---|---|---|
| T01 | wire actor/role impersonation | Principal/Session/Assignment | Daemon server principal, ProjectIdentity, SessionAssignment | source/CI local fixture | SC-04/06/07/08 |
| T02 | project trust bypass / untrusted resource injection | ProjectTrust, skills/plugins/MCP | trust checks and source resolver guards | source/local slices | SC-04/06/24/25 |
| T03 | authority/policy/approval scope widening | Grant/Approval/Policy/epoch | domain intersections, authority ledger, approval binding | source/CI | SC-05/08/09/10 |
| T04 | child delegation superset | Cell/WorkPacket/Swarm | parent subset checks, typed graph/lineage | source/CI | SC-09, SW-04/05 |
| T05 | path traversal/symlink/TOCTOU | workspace/artifact writes | lexical containment, no-follow helpers and path locks | local slices/source | SC-14/20/22, PD |
| T06 | secret exfiltration through prompt/event/receipt/log/provider | SecretRef, EventLog, output | redaction/sentinel and opaque credential contracts | local/source; not arbitrary high entropy | SC-18/19/21/23/24 |
| T07 | model/UI self-report approval/completion | EventLog/Notification/Quality | server-owned event/source registry and ControlPlane facts | source/CI | NM-03, EQ |
| T08 | duplicate/unknown/ambiguous side effect | Event/Receipt/Invocation | CAS/idempotency/ResultUnknown/fence | source/local slices | CP/ER/PD/SW |
| T09 | cancellation race / stale worker | Run/Cell/Lease | cancel token, terminal scope, authority/lease epoch | local slices/source | SC-15/16, SW-09/10 |
| T10 | corrupt/torn/replayed storage | facts/projections/artifacts | JSONL frame/CAS/health/schema contracts; recovery partial | source/local historical | PD/ER/DEP |
| T11 | resource exhaustion / retry storm | budget/queue/provider/storage | bounded sizes, budgets, queue and retry ceilings | source/CI | SC-30, BQ/AUT/SW |
| T12 | external connector/physical action misuse | provider/MCP/webhook/payment/IoT | unsupported/local_fixture/stdio gates, no external authority | source only; no live/physical | SC-12/17/26/30, INT |

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

| Fixture | Threats | Required assertion | Owner steps |
|---|---|---|---|
| `security_baseline_covers_constitution_assets_and_spine` | all | constitution/assets/spine/feature-proof map remains explicit | SC-00 |
| `wire_actor_role_impersonation_is_denied` | T01/T03 | server actor/role overrides caller and no Broker effect | SC-04/06 |
| `project_local_resource_is_untrusted` | T02 | project skills/plugins/MCP source cannot load before trust | SC-06/24/25 |
| `grant_scope_intersection_never_widens` | T03/T04 | child scope is strict intersection across path/data/budget/epoch | SC-09/SW-04 |
| `path_symlink_and_toctou_escape_is_denied` | T05 | lexical + effect-time no-follow checks block outside writes | SC-14/20 |
| `secret_never_reaches_event_receipt_provider` | T06 | raw token/bearer/sentinel absent at every output boundary | SC-18/19/23 |
| `model_or_ui_self_report_never_becomes_critical_fact` | T07 | source registry rejects model/UI approval/completion | NM-03/SC-31 |
| `unknown_or_duplicate_effect_is_quarantined` | T08/T10 | gap/digest/idempotency/Unknown never dispatches twice | ER/PD/SC-31 |
| `cancel_race_fences_stale_worker` | T09 | late worker cannot commit/release after cancel/epoch change | SC-15/16/SW-10 |
| `resource_budget_and_retry_bounds_hold` | T11 | size/turn/token/effect/retry/queue caps reject safely | SC-30/BQ/AUT |
| `external_and_physical_capabilities_stay_denied` | T12 | network/payment/IoT/publish/physical adapters have zero effect | SC-12/17/26/30 |

All fixtures must record source snapshot, exact command, environment/fixture digest, exit code and
limitations in `CURRENT_STATUS.md`; source/CI evidence cannot be promoted to durable/live/physical
without the corresponding cross-process or real-boundary proof. CI never stores real secrets.

## 5. Handoff

SC-01 supplies the threat IDs and fixture names to SC-02..43, CI/NM/EQ/PD/ER/SW. The register is
append-only evidence; discovering a new gap adds a row and a deny-first fixture rather than weakening
an assertion. Existing `security-compliance-baseline.md` remains the SC-00 current-state inventory.
