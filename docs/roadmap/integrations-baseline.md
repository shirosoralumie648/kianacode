# INT-00 Integrations / Connectors source baseline

> 快照日期：2026-09-16。本文是 `INT-00` 的 source-only inventory，不是外部 connector、OAuth、
> HTTP MCP、Webhook/A2A、真实 provider 账户、业务 delivery 或 physical effect 的完成声明。本轮不在
> 本地运行测试；`integrations_baseline` 只由 GitHub Actions 执行。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`INT-00`](../roadmap.md#step-int-00) |
| source snapshot | `e41d69e`（PD-00 已推送的干净基线） |
| proof ceiling | `source`；source guard/test-target 编译不提升 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | `entrypoint/workflow/verified webhook → DaemonHost → ControlPlane → Policy/Gate/Approval → Broker → adapter → EventLog/Receipt/Reconcile` |
| this step does | Provider/Connector/MCP/A2A/Notification 术语和边界、local_fixture/stdio 现状、owner/scope/receipt gap、INT-01..33 fixture catalog |
| this step does not | 不新增 connector/credential/HTTP/OAuth/Webhook/A2A adapter、网络调用、第二 Broker/runner loop 或外部账户写入 |

## 2. Source hashes

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Connector domain contracts | `kiana-domain/src/connectors.rs` | `34ff7a3d4f73be7339109d56ba017d4b7cde8e90d158aa7486ab2d67d753a6e1` |
| Connector core normalize | `kiana-core/src/connectors.rs` | `0379b1197d104f4e6b9e2a0e70ca281141de78664b3a6b134934e6d1def395c8` |
| Connector local_fixture adapter | `kiana-daemon/src/connectors.rs` | `7dbacc7a1e362f34cd0a7fcd6477ae6233a44e30362ea2b82037dd601a31f954` |
| MCP stdio adapter | `kiana-daemon/src/mcp_stdio.rs` | `e2bb6713f4b28ceab1b7f0fd189abd4f647dcf161a9d8407af1c38b60dc0f739` |
| Protocol/client route | `kiana-protocol/src/lib.rs`, `kiana-client/src/lib.rs` | `ab36f25cce3e6e52c617b6d845d6aa80a3e769900ee5997a1e98bd1c7aa16c2a`, `e1e9c48be7e99e686eeabca54b987ae7576eb9eb4f298b5b34b4cfaaeea40ff2` |
| INT-00 source guard/workflow | `kiana-core/tests/integrations_baseline.rs`, `.github/workflows/int00-baseline.yml` | `c3431c24285e44e059734c1444e23666409bd40fb2a910e53b4da4c06b82b9cb`, `0f9efe166a759981d6fcf4200b209383467c5c3e3a990087abe8578c7c8b5b4a` |

Later INT steps touching these files must refresh the corresponding hash. Hashes are source anchors,
not external connectivity or business-effect evidence.

## 3. Terminology and boundary matrix

| 对象 | Current source / owner | Can prove | Cannot replace |
|---|---|---|---|
| Provider | `kiana-provider` model endpoint/config/usage; daemon model client | model request/stream/usage parsing boundary | Kiana Principal、Connector account、project authorization or business outcome |
| Connector | domain `ConnectorDefinition`/`ConnectorOperation`/`AccountBinding`/`ProviderReceipt`; core normalize; daemon registry | local fixture bind/invoke/reconcile shape, scope/idempotency/risk checks | Provider identity, MCP transport, Company Acceptance, external confirmation |
| MCP | stdio frame/handshake/tool adapter in daemon; model-facing catalog is separately bounded | local stdio lifecycle and schema checks | connector account scope, approval, idempotency or external outcome; HTTP MCP remains `not_supported` |
| A2A | no product adapter currently; roadmap target | terminology/design only | Kiana approval, capability permit, durable task/effect receipt or webhook authenticity |
| Notification | HumanInbox/RunStream/SSE projections from committed facts | display/action reference and gap/terminal hint | EventLog, delivery confirmation, unread/read authority or business outcome |
| Browser/Search | read-only input/adapter surfaces | bounded source content | scope expansion, account authority or write effect |

## 4. Current connector path

`kiana-domain/src/connectors.rs` validates `ConnectorBindingSnapshot` only for
`adapter=local_fixture` and `data_processing=local_only`, requires definition/binding schema,
fixture path/hash, bounded operation/scope, idempotency and reconciliation. `ProviderReceipt` carries
connector/binding/account/operation/idempotency/payload hash/provider receipt ID/outcome/source/result;
it does not contain raw credential material.

`kiana-core/src/connectors.rs` rejects missing actor, untrusted project, cell callers, malformed
binding/operation/payload, scope mismatch, missing idempotency or overlarge input; it resolves the
server-owned binding snapshot and sends the normalized CapabilityRequest through
`authorize_and_execute`. The adapter cannot lower risk or choose another account.

`kiana-daemon/src/connectors.rs` reads a project-local, hash-pinned fixture, enforces registry CAS,
payload matching, rate limits, idempotency replay and receipt/reconcile binding. Its output explicitly
sets `external_effect_performed=false` and `source=local_fixture`; unknown fixture outcomes remain
Unknown and require a later reconciliation observation. No direct network, external account mutation,
or external provider confirmation exists.

## 5. Current MCP / A2A / Notification boundaries

- MCP product transport is stdio-only. Server/tool descriptions are untrusted input; handshake,
  frame, timeout, output and cancellation errors are bounded and still return through ControlPlane.
  HTTP/remote MCP is explicitly unsupported; a tool list never grants connector scope.
- No A2A/webhook ingress exists in the product path. Future inbound events must validate source,
  signature/auth audience, timestamp, nonce, schema, account/project scope and idempotency before
  writing a typed occurrence; `AUTH_REQUIRED` or push ACK cannot authorize a capability.
- Notification is currently `HumanInboxItem`/RunStream/SSE (see NM-00), not an external channel.
  Slack/Email/Webhook/A2A send would be a connector effect requiring provider receipt, retry/Unknown,
  retention and cleanup evidence; no direct Broker or channel path may be added.

## 6. Missing capabilities and migration guard

Missing or target-only components include versioned ConnectorRegistry/Adapter/CredentialProbe/
EffectObserver/WebhookVerifier ports, ProviderAccount/SecretRef/CredentialLease binding, durable
definition/account/project scope registry, external OAuth/PKCE, HTTP/SSRF policy, A2A task/push
occurrence, external receipt/query reconciliation, connector-specific rate/concurrency ledger,
retention/data-epoch propagation, and CLI/Web/Workbench/MCP shared connector projection.

The only valid migration is `local_fixture/stdio source shape → typed connector invocation` through
the existing DaemonHost/ControlPlane/Broker path. A future adapter must:

1. derive owner/project/data boundary, risk, scope intersection, config/authority/data epoch and
   operation contract on the server; client/model/MCP payload cannot override them;
2. keep raw secret at the final CredentialStore/Broker/transport boundary and expose only SecretRef,
   generation, expiry and digest in Core/Runner/Event/UI/Receipt;
3. commit reservation/action/payload/idempotency facts before effect, then re-check binding/lease/
   approval/epoch at dispatch and append known receipt or fenced Unknown;
4. require independent provider receipt/query and a new authorized attempt for any reconcile/retry;
5. leave HTTP MCP, payments, transport to arbitrary URLs, remote workers, enterprise tenants and
   physical effects disabled until target-specific evidence blocks exist.

Any direct Broker/handler call from an adapter, raw secret in a receipt/error, cross-project scope,
unknown schema/signature/replay, or `external_effect_performed=true` in a fixture is a release blocker.

## 7. Fixture catalog and handoff

| Fixture | Purpose | Owner step |
|---|---|---|
| `integrations_baseline` | source-only terminology/path/scope guard | INT-00 |
| `connector_definition_unknown_schema` | schema/operation/adapter fail-closed | INT-01..04 |
| `connector_binding_scope_intersection` | owner/project/data scope and revoke | INT-05 |
| `raw_credential_never_enters_receipt` | SecretRef/lease/redaction | INT-06 |
| `local_fixture_payload_and_receipt_replay` | deterministic bind/invoke/idempotency/Unknown | INT-08 |
| `http_mcp_and_ssrf_denied` | unsupported transport and endpoint safety | INT-10/11 |
| `connector_provider_receipt_binding` | receipt owner/account/operation/payload hash | INT-19/20 |
| `connector_unknown_requires_reconcile` | effect unknown, fence and new attempt | INT-21..23 |
| `webhook_a2a_auth_replay_denied` | signature/nonce/schema/source occurrence | INT-24 |
| `connector_revocation_invalidates_derived_views` | data epoch/cache/index/retention | INT-26 |
| `connector_four_entrypoint_parity` | CLI/Web/Workbench/MCP shared projection | INT-28/30 |
| `connector_live_target_requires_independent_evidence` | per-account live/physical handoff | INT-31..33 |

INT-00 closes only the source inventory and migration guard. All behavior fixtures and external
connectivity evidence run in GitHub Actions/approved target environments only.
