# CM-08 稳定 chunker 与 offset provenance 基线

> 快照日期：2026-09-20。本页记录 lossless chunk source slice；运行时夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-08`](context-memory.md#step-cm-08) |
| feature_status | `implemented`（domain ChunkSet contract + query deterministic chunker + CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| canonical path | WorkspaceReadContent/source digest → parser/chunker profile → semantic segment or UTF-8 window → contiguous ChunkSet → Context provenance |
| authority | chunks are derived context evidence; they do not become compiler facts, authority sections or execution permissions |

## Contract and behavior

`ChunkRange` records byte and line bounds. `SourceChunk` records content/transformation digests,
parser/chunker versions and optional parent group reference. `ChunkSet` rejects gaps, overlaps,
duplicate IDs, invalid paths and digest drift, and exposes `reconstruct()` only over a validated
contiguous set.

The query adapter uses a deterministic syntax-lite parser: code symbol starts for common source
extensions, Markdown-like heading starts for documents, and bounded UTF-8-safe windows for unknown
or oversized material. A semantic segment may be split into windows while retaining its parent
group; every emitted chunk remains contiguous and byte/line-addressable.

## CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `chunk_ranges_reconstruct_source_and_preserve_parent_adjacency` | contiguous byte/line ranges reconstruct source and retain parent grouping |
| `overlapping_chunks_do_not_double_count_evidence` | overlapping ranges are rejected rather than counted twice |
| `code_chunks_reconstruct_source_with_symbol_parent_provenance` | code symbol segmentation is deterministic and lossless |
| `document_and_oversized_windows_keep_byte_and_line_ranges` | heading sections and oversized Unicode windows preserve ranges |
| `stable_chunker_keeps_lossless_offsets_and_provenance` | parser/chunker/transform/version and no-second-loop source markers remain explicit |

## Proof ceiling and handoff

CM-08 proof ceiling is `source` plus remote CI wiring. Unicode normalization, secret/PII scanning,
embedding metadata separation, index generations and rename/delete invalidation remain CM-09–CM-11;
no semantic parser/compiler fact or durable index claim is made.
