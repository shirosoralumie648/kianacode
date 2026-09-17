# P4-L3-01 版本治理与 drift 基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Version dimensions

`RouteDecision` 是 ControlPlane 从已提交 `run.model_turn` 审计元数据派生的只读版本
身份，绑定 provider/model、`ModelProfile`、`PromptBundle` 的 prompt hash、route digest、
configuration revision、budget schema 和 runtime version。缺失的兼容字段归入有界的
`unknown` 桶，不会把该轮从指标中静默丢弃；路由 digest 缺失时由受控 route 元数据重新
计算，绝不包含 provider 请求正文、凭据或 prompt 原文。

`DriftReport` 使用确定性的 `BTreeMap<version_key, DriftBucket>`，每个桶累计 turns、
errors、elapsed_ms 和去重的 committed event IDs。桶 key 是完整版本维度的 canonical
digest；报告固定 `measurement=observed_turns`、`cost=unknown`，并将
`automatic_model_switch=false` 作为验证不变量。报告是 projection/receipt 输入，不是
模型选择器、授权凭证或自动升级机制。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-domain --test p4_l3_01_versioning --locked -- --test-threads=1
cargo test -p kiana-core --test p4_l3_01_versioning --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- `ModelProfile` 的 provider catalog 仍由 `kiana-services` 提供；本切片只绑定其审计标签，
  不宣称 provider 能力探测、质量因果或 billing 成本已完成。
- Drift 读取当前 EventLog 的 committed model-turn projection，仍是进程内查询；没有
  durable EvalStore、在线告警、Promote/Rollback、自动模型切换或 live/physical 证据。
- 版本桶只保证归因维度和确定性，不证明每个 provider response 的业务正确性；后续 EQ/ER/
  DEP 质量与供应链步骤继续负责更高证明等级。
