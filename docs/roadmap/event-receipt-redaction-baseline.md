# ER-03 event redaction and protected artifact reference baseline

> 快照日期：2026-09-16。本文记录 ER-03 的唯一 EventStore boundary redaction、payload bounds、secret
> scan、Artifact reference、data epoch 与 resumability 标记；本地不运行测试，运行时夹具只在 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`ER-03`](event-receipt-recovery.md#step-er-03) |
| source snapshot | `066bdfa`（ER-02 identity links 后的干净基线） |
| feature_status | `implemented`（domain redaction profile + core EventStore boundary metadata/source） |
| proof_level | `source`；静态编译和远程夹具不提升为 durable/live/physical |
| canonical path | raw event/result → `prepare_event_payload` → recursive redaction/bounds → identity/artifact metadata → EventLog → Receipt/projection |
| this step does | 递归脱敏、跨 chunk marker、NUL/深度/字节限制、profile digest、payload recoverability、data epoch 和 protected artifact refs；脱敏不稳定/超限时拒绝且不返回原文 |
| this step does not | 不把 `[REDACTED]` 当作 secret store，不把 artifact ref 当 artifact bytes，不声称所有 legacy writer、provider process memory、durable retention/delete 或外部效果已覆盖 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain redaction/runtime event | `kiana-domain/src/redaction.rs`, `kiana-domain/src/states.rs`, `kiana-domain/src/contracts.rs` | `90938d7b6aab71634661bb53c15a674008d4820e60c0e2853b788afd17b3d481`, `3028a73e30c749aa17ac493eb03f911709ba942fd1fc7b403ae2d23a66baed52`, `cfd802764d31a331eddfeecdd97270af146c39f868513c75a36cdd7e59212954` |
| Core redaction/artifact/receipt | `kiana-core/src/events.rs`, `kiana-core/src/redaction.rs`, `kiana-core/src/artifacts.rs`, `kiana-core/src/receipts.rs` | `bac9e871d5b55850025a135cc0a56c584e271027b844c5aae87a81ef5d2eaf20`, `1d14343fd49a9c05766b824a71eb3a057a6cd15e836797540ca861525913e237`, `dbc0776cdfebff5d10ff65238cf09ca3c41fa4ed350dc769b3f6bc4eaceaf1d0`, `dda33c346b1ffd94389093e2856f99433ea083a3c61b35cf562484f9f4bc7e1e` |
| Fixtures/guard/workflow | `kiana-domain/tests/er03_redaction.rs`, `kiana-core/tests/er03_redaction_guard.rs`, `.github/workflows/er03-event-redaction.yml` | `a3bea4dd62bcf837295d91c3d7e6962947ced5c6d9751b10a7e97e096edc2263`, `cea2d78ee78538d2d98467247723147bc93608a49538fc561235d2d956f7fcaa`, `7f620c664ca7d86882317b80d84e53fafb472435e18ca3f3b30c90d4ecf420da` |

hash 只用于 redaction/metadata 源码漂移复核，不是 secret、artifact、retention 或执行效果证明。

## 2. Redaction contract

`RedactionProfile` 按 signal/data class、max bytes/depth 和 profile digest 版本化。`encode_bounded_value`/`encode_bounded_text` 先验证形状，再递归 redaction、二次稳定性/secret marker 检查和 bounded serialization；失败返回结构化 reason，不回退未脱敏值。`StreamingRedactor` 保留有限 overlap，跨 chunk 的 token/password/Bearer 等 marker 仍在结束时屏蔽。

EventStore boundary 的 `prepare_event_payload` 使用同一 recursive redactor，额外拒绝深度、NUL、序列化大小、malformed data epoch 和非法 artifact refs；重复 redaction 不得改变结果。新 core 事件写入 `redaction_profile`、`payload_recoverable=false`、可选 `data_epoch` 与 `artifact_refs`；旧 RuntimeEvent 缺这些字段仍可读取。

## 3. Artifact and resumability boundary

事件只保留 `artifact:<id>`/受控引用字符串和摘要，不嵌入 artifact bytes、secret 或完整受限 payload。`artifact_refs` 有数量/长度上限，ArtifactStore/Review/Receipt 仍在各自 owner scope 中校验引用、digest、producer/run 和路径；引用存在不证明 artifact 内容或业务结果正确。

脱敏是不可逆展示/事实边界：`payload_recoverable=false` 表示原文不能从普通事件恢复。RunSnapshot/Resume 若需要原始受限材料，必须通过后续受控 artifact/SecretRef handle 重新取得并重新授权；redacted snapshot 不能作为执行 payload 回放。`data_epoch` 让后续 retention/revocation projection 识别数据治理代次，但本步不实现 purge/delete scheduler。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `secret_never_enters_event_or_receipt` | nested key/text secret 不出 redacted value；secret_ref/artifact ref 保留 |
| `redaction_changes_snapshot_marks_non_resumable` | profile、non-resumable、data epoch、artifact refs metadata 校验；伪造 recoverable 拒绝 |
| `oversize_payload_is_rejected` | size/depth 超限在 projection 前 fail-closed |
| `event_redaction_boundary_and_artifact_reference_policy_are_source_owned` | EventStore/core/domain/artifact/receipt source guard |

现有 core redaction tests、JSONL frame/event bounds、Receipt redaction 和 artifact identity tests 继续由其各自 workflow 执行；ER-03 workflow 只补本步边界和远程聚焦命令。

## 5. 限制与交接

- EventStore core 新 append/terminal path 已使用 bounded redaction；仍有直接 `RuntimeEvent::new`/transition adapter/legacy writer 需要 ER-04+ 逐步统一，不能把本步覆盖范围推广到所有事件。
- 文本 marker/key redaction 不是任意编码 secret 检测；provider/handler 进程内存、OS argv/env、外部日志和网络 proxy 仍需 CAP/SC/provider steps。
- Profile/data epoch/artifact refs 是 metadata，不是 SecretStore、RetentionPolicy、Delete/Tombstone、ArtifactStore 或执行授权；脱敏不证明业务 Outcome。
- 本地只做格式、workspace test-target 静态编译和 diff 检查；GitHub CI 结果不等待，不提升 durable/live/physical。
