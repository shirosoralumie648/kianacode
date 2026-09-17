# P1-L1-01 EvalSuite 与 GoldenTrace 基线

> 快照日期：2026-09-18。本文记录现有 GoldenTrace capture/replay 兼容路径和其安全边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Capture

`kiana-core::handle_version_command("trace.capture")` 只接受已认证且 project-trusted owner 的明确 `run_id`，从受控 project artifact reader 读取声明的 source files，生成 source manifest/hash；随后从 EventLog 过滤同一 run（排除 snapshot），绑定 author session、runtime version、events hash、prompt input hash 和 `receipt_from_events`，把新 `golden_trace.captured` 事实追加到 EventLog。源文件/Run 不做原地覆盖；缺失 owner、source manifest、path containment、run 或 revoked data fail-closed。

Capture 的兼容 JSON 保留 `events`/`events_hash`/`receipt` 字段，domain 同时拥有严格 `kiana.golden-trace.v1` `GoldenTrace` value contract（typed IDs、source snapshot、input/artifact/receipt hash、cursor、normalized events、target versions、expiry/provenance/digest）。后续 EQ-08/17/23 将负责把兼容 capture 迁移到 isolated FixtureStore/TraceNormalizer 和 typed object registry，不在本步创建第二 evaluator。

## Replay

`trace.replay` 只从 owner/project 匹配的 committed `golden_trace.captured` 读取，先重算完整 event hash，再校验 RunId 和 data revocation。成功路径只调用 `fold_model_visible_history` 与 invocation projection，返回 `kiana.golden-replay.v1`，明确 `side_effects=false`、`provider_calls=0`；不调用 Model/Provider/Broker、shell、MCP 或写入 workspace。hash drift、未知 trace、owner/project mismatch、revoked data 保持 blocked/error，不猜测成功。

## CI-only 验收

`golden_trace_replay_is_bound_and_side_effect_free` source guard 覆盖 capture/replay 的 owner/source/input/version/receipt/event digest、revocation 与 no-side-effect boundary；domain `eq03_eval_objects` 继续验证严格 GoldenTrace object，core `eval_baseline`/OA-23 作为 provider-independent replay/evidence 回归。

```text
cargo fmt --all --check
cargo test -p kiana-core --test p1_l1_01_golden_trace --locked -- --test-threads=1
cargo test -p kiana-domain --test eq03_eval_objects --locked -- --test-threads=1
cargo test -p kiana-core --test eval_baseline --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- Legacy `trace.capture` JSON 与 strict domain `GoldenTrace` 尚未由 durable EvalStore/FixtureStore 统一承载；normalized event diff、experiment/result/candidate/gate、isolated runner、Judge 和 promote/rollback 仍由 EQ-08+ / ER / PD / SC 负责。
- replay 只证明本地折叠无副作用，不证明被评估 run 的 provider/model 质量、真实外部结果、业务 Outcome、付款或 live/physical gate。
- source manifest 使用受控 artifact reader 和 hash，但尚无跨进程 capture lease、immutable snapshot store、retention projector 或自动版本迁移；旧 compatibility fields 保留且不扩大权限。

