# P4-K8-01 Connector 基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Connector authority boundary

`ConnectorDefinition` 固定 connector/provider/version、local adapter、读写 operation、scope、
rate limit、idempotency 和 reconciliation 要求；`AccountBinding` 固定 account、读写 scope、
fixture 路径/hash。`ConnectorBindingSnapshot` 校验 definition/binding identity、项目 scope、
active/revoked 状态和仅 `local_fixture`/`local_only` transport；读写 operation 风险由服务端
派生，wire payload 不能降级风险或切换 account/scope。

`connector.manage`/`connector.invoke` 先由 core 解析 owner/trust、binding snapshot、operation、
payload、idempotency 和 registry revision，再回到统一 `authorize_and_execute`。daemon handler
只消费 prepared permit：bind/revoke/invoke/reconcile 都写受保护 Connector EventLog，local fixture
按 payload hash 返回 `ProviderReceipt` + `EffectObservation`；重复 key 原样 replay，payload 或
binding drift、rate limit、错误 scope、缺 final approval/receipt 和未知 provider 结果 fail-closed。
Unknown 只能通过显式 receipt/query reconciliation 解决，不能自动重发或声称外部 delivered。

`connector_cannot_bypass_the_control_plane` fixture/guard 覆盖 scope/risk/revocation/transport、
ControlPlane/Broker 入口、幂等、receipt、Unknown/reconcile 与 no-direct-network/no-secret 边界。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-domain --test p4_k8_01_connector --locked -- --test-threads=1
cargo test -p kiana-core --test p4_k8_01_connector --locked -- --test-threads=1
cargo test -p kiana-core --test integrations_baseline --locked -- --test-threads=1
cargo test -p kiana-domain --test er14_effect_observation --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- 当前 connector 仅提供受控 `local_fixture`/`local_only` adapter；没有 HTTP、外部账户、真实
  provider receipt、webhook/A2A、生产 OAuth/SecretStore 或 live/physical 业务效果。
- ProviderReceipt/EffectObservation 证明的是 adapter observation 与 binding/hash 一致，不是
  外部系统已接收；Unknown 保持开放并要求人工/后续 connector reconcile。
- 连接器状态与限流仍依赖当前 EventLog/daemon 组合；跨进程 durable registry、通知投影、
  取消/补偿 worker 和外部传输留 INT/ER/PD/DEP/SC 后续步骤。
