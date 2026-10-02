# CI-08 ProviderGateway 接线与 route admission 基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Admission contract

`PreparedModelCall` 现在冻结 `ModelRoute::digest()`、`configuration_revision`、
`credential_revision` 和 opaque `provider_account`。`JournalModelBudget::reserve_prepared`
把这些摘要以及 assignment 的 `authority_revision` 复制进 `ModelCallPermit`，并写入既有
`model.prepared` EventLog fact。所有 route 都必须绑定 route digest 与 configuration revision；
`Legacy` in-process adapter 可缺省 provider account 和 credential revision，非 Legacy route
仍必须在 effect 前携带完整 provider binding。

`ProviderGateway::complete_admitted` 的顺序是：准备请求校验 → connection route identity
核对 → permit route/config/authority/credential/account binding 核对 → 当前 SecretStore
revision 核对 → 既有 `ModelBudgetPort::consume_prepared`（原子 dispatch admission）→ transport。
任一 drift、过期 permit、非 Legacy 缺绑定或当前 credential 轮换都在网络请求前拒绝。transport 在取得
capacity 后重新 issue/consume CI-07 one-shot lease，并再次比较 credential revision，防止
检查与发送之间的 env 变更穿透。

Provider 不从 prompt、wire actor 或项目文本推断身份；它只生成由 provider/connection 派生的
opaque account digest，并将该 digest 作为受控 `x-kiana-provider-account` metadata 发送给
fake provider。认证 header 仍只在最后请求构造阶段出现，body、usage、EventLog/Receipt
projection 不含 secret。

## CI-only evidence

`.github/workflows/ci08-route-admission.yml` 执行 domain permit drift fixture、provider
loopback fake HTTP fixture 和 core source guard，并编译 workspace test targets。fake provider
只接受一条请求，断言 opaque account header 一次、Authorization 注入一次、body 无 sentinel，
并返回 bounded JSON reply；domain fixture 覆盖 route/config/authority/credential/account drift、
expiry，以及 Legacy 缺省 provider binding 时可继续而网络 route 仍拒绝缺省 binding。

2026-10-02 Legacy compatibility follow-up: GitHub run `37019474036` / CM-02 job `110879052235`
reached the intended daemon fixture but failed before candidate persistence with
`port_failed:model_route_admission_missing`. The default offline `ModelClient` route is `Legacy`
and omits provider account/credential revision; `ModelCallPermit::validate_for_prepared` had
required both for every protocol. The source correction narrows that requirement to non-Legacy
routes and adds the paired domain fixture.

Run `37025517103` provides an exact receipt for
`ci08_route_admission::legacy_route_may_omit_provider_identity_but_network_route_may_not`: domain
job `110899633747` passed it. The same run's provider job `110899633944` exposed a stale precondition
in `fake_provider_receives_one_opaque_account_binding_and_no_secret_in_body`: at
`kiana-provider/tests/ci08_route_admission.rs:161`, `ProviderGateway::prepare_call` returned
`model_server_assignment_required` because the fixture supplied `assignment: None`. The gateway
requires a server-owned assignment/profile before it can compile a network route
(`kiana-provider/src/lib.rs:106-111`). The fixture follow-up now supplies a validated PM assignment
with its role profile and trusted project context; it leaves production validation and the existing
opaque account, single Authorization, and secret-free body assertions intact. The post-fix provider
receipt is pending. CI-08 remains `partial` / `source`.

The same run's CM-02 daemon job `110899632784` passed the earlier Legacy route admission point but
then failed the target with `result_unknown:port_failed:execution_permit_already_consumed`. That
later permit lifecycle failure is outside the provider fixture correction and remains open.

Run `37025517103` / CM-02 job `110899632784` then advanced beyond Legacy model route admission but
failed the same daemon target at `kiana-daemon/tests/daemon_host.rs:3781` with
`result_unknown:port_failed:execution_permit_already_consumed`. This was not a second model permit
consume: the ControlPlane had written `invocation.executing` v2 before its first Broker call, while
`JournalPermitVerifier` only accepts the single `execution.prepared` record as pre-consume state.
The related CAP-05/CP-13 correction moves `invocation.dispatching` v2 and the effect-starting
`invocation.executing` v3 into the verifier's one atomic permit-consume CAS. CI-08 and CM-02 remain
partial/source; post-fix CI is pending and no complete memory candidate/evidence-denial flow is
claimed.

## Limitations

- provider account 目前是 provider+connection 派生 digest，不是外部账户认证或租户身份；真实
  ProviderReceipt、账号权限和远端 effect 仍未证明。
- route/authority admission 复用现有进程内 `JournalModelBudget`/EventStore；跨进程 durable
  permit recovery、动态配置 reload、rotation/revoke CAS、OAuth 与 host/DNS pinning 留后续
  CI-09/11、PD/ER/SC。
- HTTP client 仍是无 redirect、无 ambient proxy 的本地配置；fake loopback 成功不提升 live/
  physical proof，也不证明外部业务结果或计费正确。
- The Legacy exception only permits missing provider-account and credential-revision metadata for
  in-process compatibility adapters; it does not authorize a network route or remove route/config
  binding checks.
