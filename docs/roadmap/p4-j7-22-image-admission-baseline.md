# P4-J7-22 image input and sensitive-data admission baseline

> 快照日期：2026-09-24。本页记录图片 Artifact 准入、ProcessingGrant 数据边界和 provider
> 编码合同；本地不运行测试，所有夹具由 GitHub Actions 执行。

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

| Fixture | 断言 |
|---|---|
| `untrusted_image_path_never_reaches_provider` | grant source path 与不受控绝对路径不一致时拒绝 |
| `revoked_artifact_grant_blocks_send` | revoked ProcessingGrant 在 provider 前阻断 |
| `image_payload_limit_applies_after_encoding` | base64 扩张后的 payload 超限，即使原始 bytes 未超限也拒绝 |
| `remote_image_url_is_not_fetched_implicitly` | `http(s)://` 不是 Artifact，不执行隐式网络抓取 |
| `authorized_image_input_reaches_vision_capable_provider` | vision capability、MIME、scope、route 绑定的 admission 可进入编码边界 |
| `image_hash_matches_admitted_payload` | 实际读出的 bytes hash 必须等于 Artifact 与 grant hash |

`.github/workflows/p4-j7-22-image-admission.yml` 在 GitHub runner 执行 domain fixtures、provider library
fixtures、core source guard、fmt 和 workspace test-target compile；本地不运行测试/build/check/clippy/smoke。

## 4. Evidence

```text
source_snapshot: current master `640400f7` plus P4-J7-22 image admission slice and CM-36 fmt dependency
worktree_status: ImageInputAdmission binds Artifact/ProcessingGrant/policy/route and provider encodes only admitted bytes
command_argv: GitHub Actions runs cargo fmt --all --check; cargo test -p kiana-domain --test p4_j7_22_image_admission; cargo test -p kiana-provider --lib; cargo test -p kiana-core --test p4_j7_22_image_admission_guard; cargo check --workspace --tests --locked
cwd/environment: GitHub Actions ubuntu-latest, Rust 1.97.1; local test/build/check/clippy/smoke deliberately not run; `kiana-domain/src/memory_workbench.rs` included in workflow path filter
fixture·cassette: domain image admission fixtures, provider unit fixtures and core source guard in p4-j7-22-image-admission.yml
exit_code: not observed locally; fresh post-CM-36 GitHub CI exit code pending/unobserved
status change: P4-J7-22 image input admission and CI wiring remain 🔄; workflow now requests fresh remote evidence after CM-36, no pass is claimed
proof-level change: source plus remote CI wiring only; no local_behavior, durable, live or physical proof
limitations: no real provider request, ArtifactStore durable recovery, remote URL fetch, document upload, audio/video, image generation or external outcome is claimed
reviewer: Codex source review; no local runtime test reviewer
```
