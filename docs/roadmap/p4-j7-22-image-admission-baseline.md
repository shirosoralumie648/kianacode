# P4-J7-22 image input and sensitive-data admission baseline

> 快照日期：2026-09-24。本页记录图片 Artifact 准入、ProcessingGrant 数据边界和 provider
> 编码合同；本地不运行测试，所有夹具由 GitHub Actions 执行。
> 2026-10-17 增补见文末「completion slice (CI-only)」段：图片按引用消息落点注入、dispatch
> 时刻使用真实墙钟校验保留期，并新增 provider 端到端准入夹具。

## 1. Scope and proof ceiling

| 项目 | 记录 |
|---|---|
| roadmap card | [`P4-J7-22`](provider.md#step-p4-j7-22) |
| feature_status | `implemented`（domain admission + provider wire encoding source） |
| proof_level | `source`；本地只做 diff 检查，CI 结果不等待 |
| canonical path | controlled Artifact bytes → `ImageInputAdmission` → `ProviderGateway::prepare_call_with_images` → protocol inline/base64 block |
| this step does | 绑定 ArtifactRef、ProcessingGrant、policy digest/revision/data epoch、purpose/data-class scope、route、vision capability、MIME、源 hash、保留期；在 provider 编码后重新检查 base64/请求体预算 |
| this step does not | provider 不读文件或抓 URL；不实现文档上传、远端 URL 抓取、音视频、生成图片、跨进程 ArtifactStore 恢复或真实 live provider 证明 |

## 2. Contract

`ImageInputAdmission` 是不可序列化的短生命周期值。它只接收已在受控 Artifact 边界读出的 bytes，并将
`ArtifactRef.content_hash`、`ProcessingGrant.content_hash/source_path/revoked/retention`、策略 digest、
`data_epoch`、purpose 和允许的数据分类绑定到最终 `ModelRoute`。`AttachmentRef` 没有对应 admission 时，
普通 provider 编译在网络请求前返回 `image_admission_required`；`http(s)://` 和任意路径不会被解析或抓取。

Provider 不持有 filesystem API。`prepare_call_with_images` 只接受已授权 admission，按 Anthropic、OpenAI
Chat、OpenAI Responses、Ollama、Gemini 的各自图片块形状写入同一请求 body，并在 base64 编码后重新执行
`max_encoded_bytes` 与最终 body 限制。请求 hash/budget 冻结编码后的 wire body；原始 bytes 不进入 receipt/audit。
`application/pdf`/文档、`audio/*`/`video/*` 和其它未列入 allowlist 的媒体返回明确 `unsupported_*`，不被
降级为图片或隐式上传。

## 3. CI-only fixture matrix

| Fixture（位置） | 断言 |
|---|---|
| `untrusted_image_path_never_reaches_provider`（domain） | grant source path 与不受控绝对路径不一致时拒绝 |
| `revoked_artifact_grant_blocks_send`（domain + provider e2e） | revoked ProcessingGrant 在 provider 前阻断 |
| `image_payload_limit_applies_after_encoding`（provider lib） | base64 扩张后的 payload 超限，即使原始 bytes 未超限也拒绝 |
| `remote_image_url_is_not_fetched_implicitly`（domain） | `http(s)://` 不是 Artifact，不执行隐式网络抓取 |
| `authorized_image_input_reaches_vision_capable_provider`（domain + provider e2e） | vision capability、MIME、scope、route 绑定的 admission 可进入编码边界 |
| `image_hash_matches_admitted_payload`（domain）/ `image_hash_matches_admitted_payload_after_encoding`（provider e2e） | 实际读出的 bytes hash 必须等于 Artifact 与 grant hash，且编码后可解码回同 hash |
| `untrusted_attachment_without_admission_never_reaches_provider`（provider e2e） | 普通 `AttachmentRef` 无 admission 时在编码前 `image_admission_required` |
| `untrusted_image_path_and_remote_url_never_reach_provider`（provider e2e） | 路径/URL 形式附件与无 admission 附件一样在网络前拒绝 |
| `retention_expiry_is_checked_at_dispatch_time`（provider e2e） | 保留期按 dispatch 时刻真实墙钟判断，而不是建档时刻 |
| `admission_bound_to_another_route_cannot_be_replayed`（provider e2e） | 绑定到另一 route 的 admission 不能被复用 |
| `non_vision_route_rejects_attachment_before_send`（provider e2e） | 非 vision route（`image_capability_unsupported`）在发送前拒绝 |
| `unreferenced_and_oversized_admission_lists_stay_before_send`（provider e2e） | 未引用的 admission（`image_admission_unreferenced`）与超过 32 份的 admission 列表（`image_admission_count_limit`）在发送前拒绝 |
| `document_audio_and_video_inputs_are_explicitly_unsupported`（domain + provider e2e） | `application/pdf`/docx/text → `unsupported_document_upload`，`audio/*`/`video/*` → `unsupported_audio_video_input`，`image/svg+xml` → `image_media_type_unsupported` |
| `tool_result_image_reaches_anthropic_tool_result`（provider e2e） | 工具结果图片进入对应 Anthropic `tool_result.content`，不改写其它消息 |
| `tool_result_image_is_rejected_without_a_protocol_tool_image_shape`（provider e2e） | 没有工具图片 wire 形状的协议返回 `image_tool_result_protocol_unsupported` |

`.github/workflows/p4-j7-22-image-admission.yml` 在 GitHub runner 执行 `cargo fmt --all --check`、
`cargo check --workspace --tests --locked`、domain fixtures、provider library fixtures、provider
end-to-end fixtures 和 Core source guard；本地不运行测试/build/check/clippy/smoke。

## 4. Evidence

```text
source_snapshot: current master `640400f7` plus P4-J7-22 image admission slice and CM-36 fmt dependency
worktree_status: ImageInputAdmission binds Artifact/ProcessingGrant/policy/route and provider encodes only admitted bytes
command_argv: GitHub Actions runs cargo fmt --all --check; cargo test -p kiana-domain --test p4_j7_22_image_admission; cargo test -p kiana-provider --lib; cargo test -p kiana-core --test p4_j7_22_image_admission_guard; cargo check --workspace --tests --locked
cwd/environment: GitHub Actions ubuntu-latest, Rust 1.97.1; local test/build/check/clippy/smoke deliberately not run; the 2026-09-24 snapshot referenced a workflow path filter for `kiana-domain/src/memory_workbench.rs`，当前 lane 已无 paths 过滤（`workflow_dispatch` 手动整跑）
fixture·cassette: domain image admission fixtures, provider unit fixtures and core source guard in p4-j7-22-image-admission.yml
exit_code: not observed locally; fresh post-CM-36 GitHub CI exit code pending/unobserved
status change: P4-J7-22 image input admission and CI wiring remain 🔄; workflow now requests fresh remote evidence after CM-36, no pass is claimed
proof-level change: source plus remote CI wiring only; no local_behavior, durable, live or physical proof
limitations: no real provider request, ArtifactStore durable recovery, remote URL fetch, document upload, audio/video, image generation or external outcome is claimed
reviewer: Codex source review; no local runtime test reviewer
```

## 2026-10-17 completion slice (CI-only)

The card's remaining source-level gaps are closed, and the wire placement is now pinned by fixtures:

- **按引用消息落点注入**：图片不再固定写进「第一条 user 消息」，而是按 `ImagePlacement` 注入
  引用它的 user 轮（user ordinal 与 wire 中非 `tool_result` 的 user 项对齐）；tool 轮只在
  Anthropic `tool_result.content` 内容块数组里存在可表达形状，其它协议 fail closed 返回
  `image_tool_result_protocol_unsupported`；非 user/tool 角色携带附件返回
  `image_attachment_role_unsupported`。落点计算拆成不依赖 `Connection` 的 `image_placements`，
  因此 provider lib 单测可直接断言。
- **dispatch 时刻墙钟**：`validate_image_admissions` 改用 `crate::transport::unix_ms()` 作为
  「现在」，`admitted_at` 只作为证据时间戳；`retention_expiry_is_checked_at_dispatch_time`
  覆盖「准入时未过期、派发时已过期」。
- **重复检查收敛**：`normalize_structured_request` 中重复的 AttachmentRef capability/admission
  检查被移除（`validate_image_admissions` 已覆盖），签名改为 `(request, route)`；准入入口保持
  单一，行为不变。
- **新增 provider 端到端夹具**（离线）：`kiana-provider/tests/p4_j7_22_image_admission.rs` 用
  `KIANA_MODEL_PROFILES_JSON` 注册三个 profile（`executing`=vision Anthropic、
  `quality`=vision Ollama、`analysis`=images:false），通过 `ProviderGateway::prepare_call_with_images`
  覆盖拒绝与成功路径；全部止步于编码/合成请求体，不发真实网络请求。
- **新错误码**：`image_attachment_role_unsupported`、`image_tool_result_protocol_unsupported`、
  `image_wire_tool_result_missing`、`image_wire_tool_result_content_invalid`；Core guard 连同新
  夹具名、`transport::unix_ms` 与 `cargo fmt --all --check` 一并固定。
- 旧 daemon provider 边界（`kiana-daemon/src/model_client.rs`）仍对 `AttachmentRef` 返回
  `unsupported_content_block_fails_before_request`，由 guard 固定：daemon 没有绕过准入的图片通道。

Status: roadmap §1.1 row 399 `🔄` → `✅` (CI pending) with proof_level `source`. The unified
`ci.yml` shards remain the automatic lane (`kiana-provider` 整 crate 分片自动发现新 e2e target；
`kiana-core-s5/6` 已列出 `p4_j7_22_image_admission_guard`；`kiana-domain-s3/4` 已列出
`p4_j7_22_image_admission`); the dedicated workflow is dispatch-only by the 2026-09-27 fan-out
consolidation rule. No GitHub receipt had been observed when this slice was written, so nothing
above is `local_behavior`, `durable`, `live` or `physical`.
