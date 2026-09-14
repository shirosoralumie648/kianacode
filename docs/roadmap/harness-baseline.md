# H01 Harness 接线可复核基线

> 快照日期：2026-09-14。本文是 `H01` 的 source-only 基线 + 验收骨架，不是运行时验收，也不改变产品行为（测试夹具除外）。
> 本轮不在本地运行测试；行为测试由 GitHub CI 执行。类型、源码测试名称和静态检查不能提升为 `local_behavior`、`durable`、`live` 或 `physical` 证明。

## 1. 快照与范围

| 项目 | 记录 |
|---|---|
| source snapshot | `8709b71a54549fd7fa894904b55a4d501e68148a`（测试夹具提交）+ 本轮断言修正 |
| worktree 基线 | `master`，`8c8f3d7` 已推送；CI 阻塞修复（`6b18a49`）先行落地 |
| 目标 | 按 [H01 卡](harness.md#step-h01) 交付：① 从 CLI 到 Provider/Broker/Receipt 的实际调用链对账；② runner/daemon 验收测试骨架；③ 步骤→原单元→实现符号→测试→证据映射 |
| 本轮范围 | 调用链六段对账（CLI、Host、Core、Runner、Broker/Handler、Provider/Receipt）；`tests/harness_contract.rs` 与 `tests/harness_runtime.rs` 夹具；成功路径事件断言修正 |
| 本轮不做 | 不实现 H02+ 的生命周期/状态机改造，不改产品行为，不声称后续功能完成 |

相关源码的快照 hash（用于后续漂移复核）：

| 边界 | 文件 | SHA-256 |
|---|---|---|
| CLI 路由 | `kiana-entrypoints/src/cli.rs` | `bb7ee58554f9f1f7d45c2312f949d05ec25389afef08561a3ebd51874f54997f` |
| envelope 构造 | `kiana-entrypoints/src/harness_run.rs` | `a04d341aa4eee20ed42b4c7de424c8f9696b287ccc3308ee032c9c0d90be3e1e` |
| 组合根 | `kiana-daemon/src/lib.rs` | `f262e808ab239eed0e01c1e28aef75cb5d78b819d809c1d07592ab34d367d281` |
| ControlPlane 声明 | `kiana-core/src/lib.rs` | `2052e47b7aabb892601b0a5b605082151c062641a22f23b2d99265901cf195e7` |
| 生命周期/驱动 | `kiana-core/src/lifecycle.rs` | `346ecb8ea2879b24a53a2ee238bb46c3f7793ffa6211c126ec86ad49c47bc237` |
| capability 准入 | `kiana-core/src/capabilities.rs` | `3409701ee141c1c9dd8e6df90e7eeb47efa9f02f3202c25cdee541c26cb5b39d` |
| dispatch/deliver | `kiana-core/src/dispatch.rs` | `39791a7c23088b6f915c34b81251f5a533b7fe0ccc760a48de323cbee8dc372a` |
| history 重建 | `kiana-core/src/history.rs` | `91e1998251ac9ffd1444c06555421c2ec957c81d5dccda7bc7bfbe0cf5955645` |
| runner 主循环 | `kiana-runner/src/harness.rs` | `a167b907484895a5eda71309f39cebf6ffbd318ac9efe51bcd3e20d50fd83e05` |
| 工具映射 | `kiana-runner/src/tools.rs` | `2a757b5ba06622622d949cccacbf50f4caf6d38b04c8806611853e6f4498d213` |
| runner 协议 | `kiana-runner-protocol/src/lib.rs` | `a5e738dabc96e1fb71fb73141ff9f2347ac904959fe2b247d457aa91bc4282e9` |
| 模型端口 | `kiana-ports/src/model.rs` | `3677190f103b43962273061a0dc8f5a2099c9538baa6d14b4e8111f30d1582f6` |
| broker | `kiana-capability-broker/src/lib.rs` | `9f68ef808f0370403a5f8e9b91fc643c339bbf7405326c6fc4407bdb23612b1c` |
| 模型客户端装配 | `kiana-daemon/src/model_client.rs` | `e04c10bf4291a2bc0ac426bee9475acdd40f6c0cdd6d49b7d61a7e3dd2bfa847` |

## 2. 实际调用链（六段对账）

以下为当前源码的真实接线，每段给出锚点。**这是基线事实，不是目标设计**；与 harness.md §15 目标设计的差异见 §4。

```text
[CLI] cli.rs:346 run_main
  → harness_run.rs:86 run_envelope_on_host（stream 模式经 stream_render.rs:17）
  → harness_run.rs:626 sandbox_policy_from_options → 621 SandboxPolicy{sandbox, permission_profile}
  → harness_run.rs:233 client_on_host：RequestMetadata::local + actor_id + project_trusted + profile + role
  → kiana-client KianaClient::run → RequestEnvelope::run（kiana-protocol lib.rs:187，schema kiana.protocol.v1）
[WIRE] LocalDaemonTransport（harness_run.rs:46）→ DaemonHost::handle（kiana-daemon lib.rs:425）
[HOST] DaemonHost 组合根（lib.rs:50；构造器 100–369，产品路径 local_with_model_config:282）
  → request_may_execute（lib.rs:777）+ effective_permission_profile（lib.rs:805）先拒
  → ControlPlane::start_run_with_id（kiana-core lifecycle.rs:27）
[CORE] 准入链：prompt/role/department/step-limit/sandbox/trust 校验（lifecycle.rs:27–282）
  → run.authorized / run.prompt 落账（lifecycle.rs:169–208）
  → drive_run（lifecycle.rs:790）：消费 RunnerEvent 流
  → broker_harness_capability（capabilities.rs:568）
     → prepare（82）→ run.tool_call + run.capability_requested 落账 → authorize（166）
     → capability.decision → GateDecision{Denied | AwaitingApproval | Allowed}
     → dispatch_capability_action（418）→ dispatch_authorized（dispatch.rs:334）
     → finalize（capabilities.rs:481）→ capability.completed / .failed / .result_unknown 落账
     → deliver_capability_result（dispatch.rs:127）→ runner.send(CapabilityResult)
[RUNNER] KianaHarness（harness.rs:211）：send（331）/ model_step（635）/ invoke_model（825）
  → tools.rs:27 capability_for_tool：五工具 shell/apply_patch/mcp/memory.search/memory.write
  → emit_tool_request（1066）→ pending_tools 串行配对 → on_capability_result（507）
[PROVIDER] model_client.rs:9 from_config/from_env → kiana-provider ProviderGateway（lib.rs:12）
  → request.rs:32 compile（tool 名 mangle、64 工具上限）→ transport.rs:9 send（SSE Framer:245）
  → response.rs:57 decode + :311 Accumulator（不完整流 fail-closed）
[RECEIPT] receipts.rs:11 read_receipt → :256 receipt_from_events（纯事件投影）
  → lib.rs:268 persisted_events 端口 → ResponseEnvelope（kiana-protocol lib.rs:505）
```

### 关键事实边界（对账中新固定的）

1. **成功工具结果的事件 kind 是 `capability.completed`**（`capabilities.rs:542` 由 `finalize_capability_action` 写，payload 带 `capability_request_id` 回链 `run.capability_requested`）；`run.tool_result` **只在取消/未执行路径**写（`capabilities.rs:386/403`、`lifecycle.rs:596/654/822`、`data_governance.rs:179/218`，payload 均带 `not_executed:true`）。history 重建（`history.rs:131/148`）两个 kind 都消费。
2. **run 级终态词表**：`run.completed` / `run.failed` / `run.cancelled` / `run.result_unknown`（`lifecycle.rs:963–988` 按错误前缀分类）；`ResultUnknown` 是一等状态而非失败（`deliver_capability_result` 的 `result_unknown:result_delivery_already_claimed` 单次消费保护，`dispatch.rs:160`）。
3. **信任判定在 daemon 侧**：`kiana-core` 不做初始 trust/identity 判定，只消费 `RequestContext.project_trusted`；`StoredProjectTrustAuthority`（`kiana-daemon lib.rs:61`）委托 `kiana_types::read_project_trust`。
4. **runner 侧有死代码**：`harness.rs:703` `let emitted_delta = true;` 硬编码，`718–723` 的兜底 Delta 发射不可达；live 路径在 `invoke_model` 的流式回调（`904–917`）。H05/H06 收口时应删除而非依赖。
5. **模型准入合同已在 ports**：`ModelClient` 的真实面是 `prepare_call` / `complete_prepared` / `complete_admitted`（`kiana-ports/src/model.rs:10/44/57`，`ModelCallPermit` 由 `ModelBudgetPort` 签发，`kiana-ports lib.rs:846`）；`kiana-runner/src/model.rs:16` 只是 re-export。预算链 `JournalModelBudget`（`kiana-core/src/model_budget.rs:7–361`）已存在。
6. **CLI sandbox 无类型**：sandbox 以 `Option<String>` 走 envelope body（`RunRequest.sandbox`），`PermissionProfile` 才是类型化字段（进 metadata）；`danger-full-access` 在 `harness_run.rs:709` 硬拒绝。

## 3. 验收骨架（已入库，CI 执行）

| 测试 | 文件 | 断言口径 |
|---|---|---|
| `untrusted_run_never_reaches_model_or_broker` | `kiana-daemon/tests/harness_runtime.rs:117` | 未信项目 + workspace-write → `Blocked`，模型调用计数 0，无 `run.capability_requested` 事件 |
| `host_model_tool_result_next_step_receipt_roundtrip` | 同上 `:156` | 信任项目完整链：shell 工具 → `capability.completed` + `run.completed` 落账 → receipt 二次读取一致 → 模型恰好 2 次调用 |
| `runner_event_for_other_run_is_rejected` | `kiana-runner/tests/harness_contract.rs:85` | 伪造其它 run 的 CapabilityResult → `run_not_found` 拒绝，模型仍 1 次 |
| `event_write_fault_stops_before_model_call` | 同上 `:128` | 事件写入失败 → `event_sink_failed` fail-closed，模型 0 次 |
| `blocking_model_fixture_releases_deterministically` | 同上 `:152` | 可阻塞模型夹具确定性释放（为 H07/H08 预置） |
| `counting_broker_fixture_counts_authorized_calls` | daemon `:219` | 可计数 Broker 夹具（为取消/竞态步骤预置） |

夹具修正：`8709b71` 的 roundtrip 断言错写 `run.tool_result`（成功路径不产生该 kind，见 §2.1），本轮已改为 `capability.completed` + `capability_request_id` 关联断言。

## 4. 与 harness.md §15 目标设计的差异（后续 step 的输入）

| 差异 | 目标设计 | 后续 owner |
|---|---|---|
| `Continue` 复用 `RunId`，run 非 Turn/Run 分层 | Session→Turn→Run 原生语义 | H02、CP-02 |
| 生命周期分散在 harness.rs 的 map/临时变量（`ActiveRun:112`、`pending_tools:121`） | RunFrame/TurnFrame 状态驱动器 | H03 |
| `emitted_delta` 死代码 + stop_reason 仅元数据 | 统一停止原因/终止分类 | H05、H06 |
| `run.tool_result` 仅取消路径；成功结果无 run 域 tool_result 事件 | 完整调用批次记录 | H13（与 ER-04 receipt 合同对齐） |
| inbox.rs/compact.rs 语义局部化，压缩只产 `(no summary available)` 占位 | 检查点/恢复贯通 | H18、H22–H25 |
| CLI sandbox 为字符串、profile 与 sandbox 双轨 | 单一授权输入 | CP-03/05 |

## 5. CI 验收索引

静态检查本轮已过：`cargo check --workspace --tests --locked --offline`、`cargo fmt --all --check`。行为回执由 `Release Smoke` workflow（`.github/workflows/release-smoke.yml`）执行：bwrap 安装 + userns 解除后 `cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1`。

本轮先修复了三个既有 CI 阻塞（`6b18a49`）：kiana-query 六处 16 位 hash 长度陈旧断言（实现已是 `sha256:<hex>`）、`kiana-provider/Cargo.toml` 缺 `repository.workspace`、rustfmt 漂移。这些是基线遗留，不属于 H01 范围，但不修则 H01 的 CI 回执无法转绿。
