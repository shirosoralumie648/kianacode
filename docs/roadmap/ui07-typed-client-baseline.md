# UI-07 typed client query/feed/action API baseline

> 快照日期：2026-10-17（完成切片）。UI-07 的 typed facade、请求生命周期夹具、fixture plan、source
> guard 与 manual lane 由 GitHub Actions 执行；本地不运行测试、构建或检查，CI 回执未等待。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-07`](ui-entrypoints.md#step-ui-07) |
| source snapshot | 分支 `step/ui-07-20261017-1`（基线 master `a849af82a99a`，UI-06 合并点）；完成切片提交绑定本页 |
| feature_status | `implemented`（typed query/feed/action/artifact facade + request lifecycle fences + mock fixtures + manual lane） |
| proof_level | `source`；不提升为 local_behavior/durable/live/physical |
| canonical path | surface → `kiana-client` typed facade → versioned `RequestEnvelope` → `DaemonHost`/`ControlPlane`；client 不执行副作用 |
| manual lane | `.github/workflows/ui07-typed-client.yml`（`workflow_dispatch`-only、`permissions: contents: read`） |
| fixture plan | `kiana-client/tests/fixtures/ui07-typed-clients.json`（operation/facade/budget/required/denied 矩阵） |

UI-07 将旧的通用 client 入口旁边补出 `QueryClient`、`FeedClient`、`ActionClient` 和
`ArtifactClient`。四个 facade 共享一个协商后的 `ClientSession`，每次请求绑定 request ID、
可选 deadline 和 cancellation fence。workspace、session、协议 schema 和响应 request ID
在 client boundary 校验；未初始化、foreign workspace、过期/取消请求和未知 schema
fail closed。

完成切片把「typed facade + 少量夹具」补成完整生命周期收口：listener generation 现在同时绑定
submission cancellation token（释放或取消共享 controller token 都立即退掉该 generation），
11 个 operation 常量从 facade 公开并可被夹具/守卫逐项绑定，17 条 deny-first 夹具覆盖
timeout、late response、reconnect、gap、Unknown 对账和 listener cleanup，并重建了
UI-07 专属手动 lane（08552ada 合并 704 个 per-step workflow 时被删除，历史证据块仍引用它）。

## 2. 完成切片补齐的内容

1. **listener generation 绑定取消语义**：`kiana-client/src/typed.rs` 的 `ListenerRecord`
   持有创建它的 `CancellationToken`；`dispatch` 与 `resume` 在 generation 不匹配或 token
   已取消时统一返回 `ClientError::ListenerInactive`，且 callback 不会被调用。取消共享
   controller token（`ClientRequestOptions::with_cancellation`）无需等待 RAII handle 释放即
   隔离迟到回调。
2. **operation 常量公开**：`UI_INITIALIZE_OPERATION`、`UI_QUERY_SNAPSHOT_OPERATION`、
   `UI_QUERY_HISTORY_OPERATION`、`UI_QUERY_COMMAND_STATUS_OPERATION`、
   `UI_FEED_SUBSCRIBE_OPERATION`、`UI_FEED_RESUME_OPERATION`、`UI_ARTIFACT_PAGE_OPERATION`、
   `UI_ACTION_SUBMIT_OPERATION`、`UI_ACTION_CANCEL_OPERATION`、
   `UI_ACTION_CONTINUE_OPERATION`、`UI_ACTION_STATUS_OPERATION` 与
   `UI_HISTORY_SCHEMA`/`UI_COMMAND_STATUS_SCHEMA`/`UI_ARTIFACT_PAGE_SCHEMA` 一起从
   `kiana-client` 导出，夹具可以断言「客户端真的发了哪个 operation」。
3. **脚本化 mock transport**：`kiana-client/tests/ui07_typed_clients.rs` 记录每个 envelope、
   每个 request ID 和 cancel hook 调用；拒绝路径的请求必须**不出现**在记录里，迟到响应必须
   触发 `ClientTransport::cancel`。
4. **声明式 deny matrix**：`ui07-typed-clients.json` 固定 11 个 operation、四个 facade、
   生命周期阶段、分页/历史/limit 预算、17 条夹具名与 required/denied 清单；夹具自检 + core
   guard 双向绑定。
5. **手动 lane 重建**：`.github/workflows/ui07-typed-client.yml` 串行执行 fmt 门、三个
   target（client fixture、core guard、daemon guard）与 `--no-run` 编译；自动覆盖仍是统一
   `ci.yml` 的 `kiana-client`/`kiana-daemon` 整 crate 分片与 `kiana-core-s6/6` 的
   `ui07_typed_client_guard`。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `typed_query_fails_closed_before_initialize_and_deadline` | 未完成 initialize 与已过 deadline 的 query 不发往 transport |
| `accepted_action_is_not_promoted_to_applied_and_wrong_retry_is_denied` | `Accepted` 保持 Accepted 且无 receipt；相同 idempotency key 绑定错误 command ID 时本地拒绝 |
| `unknown_action_is_queryable_by_original_idempotency_key` | `ResultUnknown` 返回 `Unknown/QueryOriginal`，只能按原 idempotency key 对账；空 key 本地拒绝 |
| `history_rejects_foreign_instance_or_authority_epoch_frames` | `UiHistoryV1` 拒绝跨 instance/跨 epoch 的历史拼接 |
| `feed_listener_is_installed_before_subscribe_and_removed_on_drop` | listener 先注册后 subscribe；重复 listener 本地拒绝；释放后旧 generation 收不到迟到 frame |
| `every_typed_facade_requires_the_same_handshake_and_workspace` | 四个 facade 都要求同一 handshake；跨 workspace 的 query/artifact/action/subscribe 全部 `WorkspaceMismatch` 且不发请求 |
| `unknown_schemas_are_rejected_before_any_typed_projection` | 未知 handshake/action/feed schema 一律 `UnknownSchema`，被拒的 handshake 不安装 session |
| `cancelled_or_expired_requests_never_reach_the_transport` | 取消、过期 deadline 与越界 page/limit/session 参数都在本地拒绝，transport 只看到 initialize |
| `a_released_listener_generation_never_receives_a_late_frame` | 取消共享 controller token 立即退掉 generation；resume 被拒；重连拿到新 generation，旧 token 永久 inactive |
| `late_and_mismatched_responses_are_fenced_and_the_cancel_hook_is_used` | deadline 在飞行中过期 → `LateResponse` + cancel hook；request ID 不匹配 → `client_request_id_mismatch` |
| `feed_reconnect_resumes_from_the_server_cursor_and_surfaces_gaps` | boundary→delta→gap 都到达 presenter；`resume` 携带服务端 cursor（不是本地推导位置）；退役 token 不能 resume |
| `a_transport_failure_or_unknown_status_is_never_reported_as_success` | transport 错误原样传播；`Accepted`/`Failed` 信封都不是 read 结果 |
| `command_status_history_and_artifact_pages_share_the_same_fences` | command status 可读；history 拒绝 foreign instance 与未知 schema；artifact 拒绝坏 digest；成功路径给出合法页 |
| `cancel_and_continue_use_the_typed_action_operations` | cancel/continue 只发 command identity；换 command ID 复用 key 被拒；payload 不重发 |
| `typed_clients_share_one_negotiated_session_across_facades` | 一次 handshake 覆盖四个 facade，发送的 operation 序列可核验 |
| `a_denied_handshake_never_installs_a_session` | `Denied` handshake 不是 session，后续 query 仍是 `NotInitialized` |
| `ui07_deny_matrix_fixture_stays_wired_to_the_typed_client_source` | fixture plan 的 schema/operation/facade/budget/夹具名与源码常量一致 |

## 4. 请求生命周期图

```text
uninitialized
  -- initialize(handshake) --> ready(workspace, instance, authority_epoch)
  -- denied / schema mismatch --> 仍为 uninitialized，不安装 session
ready -- query(snapshot/history/command_status) --> 校验 schema + request_id + 本地参数界限
ready -- artifact.page --> 校验 schema + digest(sha256:64hex) + mime
ready -- action(submit/cancel/continue) --> 校验 payload digest；Accepted 永不升级为 Applied
ready -- feed.subscribe(listener installed first, generation N, cancellation token T) --> listening
listening -- dispatch(token) --> callback(frame)；generation 不匹配或 T 已取消 → ListenerInactive
listening -- resume(after = 服务端 cursor) --> reconnect（服务端决定 sequence）
release / cancel / T.cancel() --> generation N 退休；迟到 frame 不投递；同 id 可重新注册新 generation
late action --> query_original(原 idempotency key)，绝不换 ID 重投
Unknown/late response --> LateResponse + best-effort transport.cancel(request_id)
```

`FeedClient` 的 callback 只接收已校验的 `UiFeedFrameV1`，不会获得 capability 或执行句柄。
`UiHistoryV1` 还要求每个 frame 绑定同一 instance 与 authority epoch，拒绝跨实例/跨 epoch 的
历史拼接。`ActionClient` 只提交 `UiActionV1`，服务端的 receipt/state 决定 `Applied`；
transport 成功、HTTP 200 或 `Accepted` 本身永远不会被提升为 `Applied`。`ClientTransport::cancel`
是 best-effort fence，无法撤回已送出的请求时，typed client 仍会丢弃迟到响应。

## 5. Migration inventory（调用迁移清单）

| 旧入口 | UI-07 facade | 迁移说明 |
|---|---|---|
| `KianaClient::initialize` / `health` | 保留兼容；`TypedClients::initialize` 把协商结果同步到四个 facade | 同一 handshake，不新增第二套 session |
| `KianaClient::command` 自由字符串 | `QueryClient::snapshot/history/command_status`、`ArtifactClient::page` | operation 常量公开，夹具按名字断言 |
| UI snapshot/feed projection | `FeedClient::subscribe/resume` + `FeedListenerToken`/`FeedSubscription` | listener 先安装后请求；释放即退役 generation |
| UI action journal | `ActionClient::submit/cancel/continue_action/query_original` | 命令 identity、幂等 key 与 Unknown 对账走同一条路径 |
| Workbench/Web/CLI presenter（UI-08/10/11/14 起） | 直接依赖 typed facade，不再自建请求状态机 | presenter 只做展示投影，仍不执行副作用 |

## 6. lane 与分片接线

- 手动 lane：`.github/workflows/ui07-typed-client.yml`（`workflow_dispatch`-only、
  `permissions: contents: read`、`cargo fetch --locked`、`cargo fmt --all --check`，随后串行
  `cargo test -p kiana-client --test ui07_typed_clients`、
  `cargo test -p kiana-core --test ui07_typed_client_guard`、
  `cargo test -p kiana-daemon --test ui07_typed_client_guard`，以
  `cargo test -p kiana-client -p kiana-core -p kiana-daemon --no-run --locked` 收尾）。
- 自动覆盖：统一 `ci.yml` 的分片册 `scripts/ci/test-shards.json` 里 `kiana-client` 与
  `kiana-daemon` 是整 crate 分片（自动发现 tests target），`kiana-core-s6/6` 收录
  `ui07_typed_client_guard`。新增 target 不需要改分片册。

## 7. Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations（必须写）/ reviewer
```

GitHub Actions 运行 `cargo fetch --locked`、`cargo fmt --all --check`、typed client mock
fixtures、core/daemon source guards 与 `cargo test --no-run`。本地不运行测试、构建、检查、
clippy 或 smoke；CI 回执未等待。

## 8. Limitations

- 客户端只做本地 fence 与 schema 校验：授权、幂等、审批和事实仍由 `DaemonHost`/`ControlPlane` 决定，
  `Accepted` 不等于 `Applied`，`Unknown` 不等于 `Rejected`。
- listener registry 是进程内状态，不是 durable subscription inbox，也不是跨进程 SSE/socket 订阅；
  迟到 frame 的隔离是 generation + cancellation fence，不是网络层取消。
- server 端 `ui.initialize`/query/feed/action/artifact route 仍属后续入口步骤（UI-10/UI-16/UI-31 等），
  本卡只提供 typed client 契约与 mock fixtures；没有真实 socket/SSE、浏览器或 provider timing 证据。
- 本卡不声称 durable/live/physical：没有本地运行测试，也没有等待 CI 回执；UI-08 reducer、
  UI-18 SSE reconnect 与 UI-33 crash recovery 继续依赖本卡的 contract。
