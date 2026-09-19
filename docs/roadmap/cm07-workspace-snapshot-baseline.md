# CM-07 Workspace/artifact snapshot 与安全读取基线

> 快照日期：2026-09-20。本页记录 bounded snapshot/read source slice；运行时夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-07`](context-memory.md#step-cm-07) |
| feature_status | `implemented`（domain snapshot contract + query secure-read adapter + CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| canonical path | canonical workspace root → symlink/hardlink metadata → trust/limits → bounded read → before/after identity fence → SourceSnapshot-compatible evidence |
| authority | snapshot/read output is context evidence only; EventLog/ControlPlane remain authority and untrusted text cannot become instruction material |

## Contract and behavior

`WorkspaceSnapshotLimits` bounds file count, total bytes, single-file bytes, depth and elapsed
time. `WorkspaceFileIdentity` retains platform identity where available (device/inode, size,
mtime and hard-link count) and the read adapter compares it before and after a bounded read.
Symlinks and hardlinks are metadata-only; untrusted or unknown roots are metadata-only; a changed
identity becomes `Fenced` and never returns text as instruction-safe content.

The query adapter canonicalizes a real directory root, skips generated/store directories, uses
`symlink_metadata`, sorts entries, caps traversal and reads at most the configured file budget.
Successful trusted UTF-8 content is returned with a digest and an indexed snapshot row; all other
rows retain a reason and remain non-instruction-safe.

## CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `trusted_read_requires_stable_before_after_identity` | trusted indexed material requires stable identity and post-read content digest |
| `untrusted_material_is_metadata_only_and_fenced_change_is_not_safe` | untrusted text cannot become instructions and TOCTOU change is fenced |
| `snapshot_rejects_duplicate_paths_and_invalid_limits` | limits, path uniqueness and strict snapshot contracts fail closed |
| `trusted_snapshot_reads_only_stable_bounded_text` | query adapter returns only bounded trusted UTF-8 content |
| `unknown_trust_never_returns_project_text_as_instructions` | unknown trust produces metadata-only rows and no content |
| `symlink_root_is_rejected_without_following_it` | symlink root is rejected before traversal (Unix CI) |
| `workspace_snapshot_keeps_trust_identity_and_read_limits_explicit` | query/domain source boundaries retain trust, identity, limit and no-second-loop markers |

## Proof ceiling and handoff

CM-07 proof ceiling is `source` plus remote CI wiring. Durable index generations, chunk offsets,
rename/delete invalidation, full ProjectTrust adapter integration and cross-process recovery remain
CM-08–CM-11/PD work; no local test, build, check or smoke command was run.
