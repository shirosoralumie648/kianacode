# UI-07 typed client query/feed/action API baseline

> 快照日期：2026-09-24。UI-07 的 typed facade、请求生命周期夹具和 authority source
> guard 由 GitHub Actions 执行；本地不运行测试、构建或检查。

## 1. 范围与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-07`](ui-entrypoints.md#step-ui-07) |
| source snapshot | `45e66545`（UI-07 分支基线，提交后绑定本提交） |
| feature_status | `implemented`（typed query/feed/action/artifact facade + request fences + remote fixtures） |
| proof_level | `source`；不提升为 local_behavior/durable/live/physical |
| canonical path | surface → `kiana-client` typed facade → versioned `RequestEnvelope` → `DaemonHost`/`ControlPlane`；client 不执行副作用 |

UI-07 将旧的通用 client 入口旁边补出 `QueryClient`、`FeedClient`、`ActionClient` 和
`ArtifactClient`。四个 facade 共享一个协商后的 `ClientSession`，每次请求绑定 request ID、
可选 deadline 和 cancellation fence。workspace、session、协议 schema 和响应 request ID
在 client boundary 校验；未初始化、foreign workspace、过期/取消请求和未知 schema
fail closed。

## 2. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `typed_query_fails_closed_before_initialize_and_deadline` | 未完成 initialize 与已过 deadline 的 query 不发往 transport |
| `accepted_action_is_not_promoted_to_applied_and_wrong_retry_is_denied` | `Accepted` 保持 Accepted；相同 idempotency key 绑定错误 command ID 时拒绝重投 |
| `unknown_action_is_queryable_by_original_idempotency_key` | `ResultUnknown` 返回可校验的 `Unknown/QueryOriginal`，只能按原 idempotency key 对账 |
| `feed_listener_is_installed_before_subscribe_and_removed_on_drop` | listener 先注册后 subscribe；重复 listener 拒绝；释放后旧 generation 收不到迟到 frame |
| `ui07_typed_client_guard` | typed client 不创建 Broker/模型循环，副作用仍由 daemon/core authority 处理 |

## 3. Lifecycle and listener contract

```text
uninitialized
  -- initialize(handshake) --> ready(workspace, instance, authority_epoch)
ready -- query/action/artifact --> response validated by schema + request_id
ready -- feed.subscribe --> listener installed --> subscribe/resume request
listener released/cancelled --> old generation inactive; late frame dropped
Unknown/late action --> query_original(idempotency_key), never synthesize a new command
```

`FeedClient` 的 callback 只接收已校验的 `UiFeedFrameV1`，不会获得 capability 或执行句柄。
`ActionClient` 只提交 `UiActionV1`，服务端的 receipt/state 决定 `Applied`；HTTP/transport
成功或 `Accepted` 本身永远不会被提升为 `Applied`。`ClientTransport::cancel` 是 best-effort
fence，无法撤回已送出的请求时，typed client 仍会丢弃迟到响应。

## 4. Migration inventory

| 旧入口 | UI-07 facade |
|---|---|
| `KianaClient::initialize` / `health` | 保留兼容；初始化状态同步到 typed clients |
| `KianaClient::command` | Query/Feed/Action/Artifact 按 operation 常量分流 |
| UI snapshot/feed projection | `QueryClient::snapshot/history`、`FeedClient::subscribe/resume` |
| UI action journal | `ActionClient::submit/cancel/continue_action/query_original` |
| artifact viewer | `ArtifactClient::page` |

## 5. Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the typed client mock
fixtures, daemon/core source guards and `cargo check --workspace --tests --locked`. Local tests,
builds, checks, clippy and smoke are deliberately not run; CI result is not awaited.

Limitations: the generic command routes are contracts only until daemon UI query/feed/action/artifact
routes are wired by later entrypoint cards; feed listener state is process-local and bounded, not a
durable subscription inbox; no cross-process socket/SSE, browser callback timing, artifact blob
download, provider/live timing or physical proof is claimed. UI-08 reducer, UI-18 reconnect and
UI-33 crash recovery remain subsequent cards.
