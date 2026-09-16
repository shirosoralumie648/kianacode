# UI-01 versioned protocol DTO baseline

> 快照日期：2026-09-16。本文记录 UI-01 的 snapshot/feed/action/capability/error/artifact DTO；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`UI-01`](ui-entrypoints.md#step-ui-01) |
| source snapshot | `ef10992`（UI-00 baseline 与 extension catalog 后的干净基线） |
| feature_status | `implemented`（kiana-protocol versioned UI DTO source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | UI client → versioned DTO parse/validate → DaemonHost/ControlPlane re-check → EventLog/Receipt projection |
| this step does | 在 protocol 增加 `UiSnapshotV1`、`UiFeedEnvelope`、`UiActionV1`/`UiActionResult`、`UiCapability`、`UiError`、Session/Run/Item、HumanActionCard、ReceiptRef、ArtifactSummary、UiNotice、EvidenceLimitation 和 cursor/retry/disposition enums；校验 schema、instance/authority epoch、cursor/sequence/revision、payload/receipt/artifact digest、bounded payload/字段和 duplicate IDs；旧 `UiSnapshot`/`UiAction`/`RunStreamEnvelope` wire 保持可解码 |
| this step does not | 不改变旧 daemon route、不直接执行动作、不建立 UI bus/第二 runner loop、不把 feed/ACK/cache 当事实或授权；UI-02 负责 capability/error handshake，UI-03+ 负责 transport/projector/reconnect |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| UI DTO | `kiana-protocol/src/ui_contracts.rs` | `60f24325993e1f91a8b5486688fd6a6ca50f23e23ca6fb75b28607353b01c0b3` |
| Protocol module/export | `kiana-protocol/src/lib.rs` | `4a72bd07d1a741b933e8d3ba03a3be48fef5bc92ef2565519263a865bb199516` |
| Domain schema registry | `kiana-domain/src/contracts.rs` | `0df78da02fcb334fd7404ea22203aade33529b6d68037b4b4e238c592a8f981f` |
| Fixtures | `kiana-protocol/tests/ui01_dto.rs`, `.github/workflows/ui01-protocol.yml` | `235c6e378b9cb5a5f0b269bc30a3b43281fc248c63032ebbc00a5c429c9ea7b1`, `007ad696b0e45eb96010c0e287173057920c99b56561f58e341511cf260bd9ef` |

hash 只用于 UI-01 源码漂移复核，不构成真实 transport、snapshot projector、跨进程恢复、审批应用或业务结果证明。

## 2. DTO boundary

`UiSnapshotV1` 是可渲染的原子读模型，携带 instance/authority epoch、server cursor、principal/workspace ID、capabilities、session/run、pending actions、artifact/notice 和 limitations。Session/Run/action/artifact/notice IDs 在一个 snapshot 内不得重复；active session 必须存在于 session summary。`UiFeedEnvelope` 使用 instance、authority epoch、feed sequence、snapshot cursor、event/aggregate identity 和 object revision，event 只作为 bounded projection payload。

`UiActionV1` 绑定 command ID、idempotency key、target、expected epoch/cursor/revision、payload digest、提交主体和 deadline；payload 在服务端仍需重做 owner/scope/policy/gate/approval/CAS 检查。`UiActionResult` 明确 Accepted/Applied/Rejected/Unknown，Applied 必须有 cursor 或 receipt，Rejected 必须有 structured error，Unknown 只允许 `QueryOriginal`。UiCapability/UiError/UiRetryDisposition 只表达可见能力、文案和安全重试建议，不授予 capability。

## 3. Compatibility and failure-first rules

新 DTO 采用 `deny_unknown_fields`；旧 protocol envelope 和旧 `UiSnapshot`/`UiAction` 类型不改 required 形状。未知/未来增量由旧 `RunStreamEvent::Unknown` 兼容忽略，但新 versioned DTO 遇到 schema/identity/digest/size/sequence 错误直接拒绝。客户端 timestamp、UI ACK、按钮预选和本地 cache 不能替代 server epoch/cursor/revision/receipt。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `ui_snapshot_feed_and_action_round_trip_with_unknown_field_guard` | Snapshot/feed/action 可 round-trip，unknown field、payload digest 漂移 fail-closed |
| `ui_action_result_keeps_unknown_and_rejected_distinct` | Rejected 必须带 error；Unknown 只能 QueryOriginal，不能伪装成功/安全重试 |

`.github/workflows/ui01-protocol.yml` 在 GitHub runner 执行 protocol DTO fixtures 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- 当前 DTO 还没有 DaemonHost 端的原子 snapshot projector、source cursor hydration、feed gap/replay、instance discovery 或 durable action journal；UI-02/03/04+ 负责。
- `UiSnapshotV1.principal` 使用 authenticated principal 合同，workspace 只传 ProjectId；客户端不能依赖路径、transcript、run stream 或本地 cache 生成权限。
- action payload/receipt/artifact 只做协议 digest/边界校验，最终 ControlPlane 仍须重新授权和提交 EventLog；Unknown/网络断开不能自动 retry。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
