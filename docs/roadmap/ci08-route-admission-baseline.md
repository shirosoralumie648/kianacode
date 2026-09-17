# CI-08 ProviderGateway 接线与 route admission 基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Admission contract

`PreparedModelCall` 现在冻结 `ModelRoute::digest()`、`configuration_revision`、
`credential_revision` 和 opaque `provider_account`。`JournalModelBudget::reserve_prepared`
把这些摘要以及 assignment 的 `authority_revision` 复制进 `ModelCallPermit`，并写入既有
`model.prepared` EventLog fact；可选字段保持旧 cassette/legacy reader 的 JSON 兼容，但网络
provider 在 effect 前要求完整绑定。

`ProviderGateway::complete_admitted` 的顺序是：准备请求校验 → connection route identity
核对 → permit route/config/authority/credential/account binding 核对 → 当前 SecretStore
revision 核对 → 既有 `ModelBudgetPort::consume_prepared`（原子 dispatch admission）→ transport。
任一 drift、过期 permit、缺绑定或当前 credential 轮换都在网络请求前拒绝。transport 在取得
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
并返回 bounded JSON reply；domain fixture 覆盖 route/config/authority/credential/account drift
和 expiry。工作流由本提交触发，本地只做格式、静态编译和 diff 检查，不等待 CI。

## Limitations

- provider account 目前是 provider+connection 派生 digest，不是外部账户认证或租户身份；真实
  ProviderReceipt、账号权限和远端 effect 仍未证明。
- route/authority admission 复用现有进程内 `JournalModelBudget`/EventStore；跨进程 durable
  permit recovery、动态配置 reload、rotation/revoke CAS、OAuth 与 host/DNS pinning 留后续
  CI-09/11、PD/ER/SC。
- HTTP client 仍是无 redirect、无 ambient proxy 的本地配置；fake loopback 成功不提升 live/
  physical proof，也不证明外部业务结果或计费正确。
