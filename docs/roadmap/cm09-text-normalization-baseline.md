# CM-09 文本规范化、语言和敏感数据边界基线

> 快照日期：2026-09-20。本页记录 deterministic normalization source slice；夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-09`](context-memory.md#step-cm-09) |
| feature_status | `implemented`（domain normalization/redaction boundary + CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| canonical path | source text → BOM/line/fullwidth canonicalization → identifier/CJK tokens → sensitive policy → separate embedding/LLM metadata |
| authority | normalized text/tokens are derived context material; no index, embedding, prompt or capability authority is created here |

## Contract and behavior

`TextNormalizationProfile` pins a bounded `nfkc-lite-fullwidth-v1`, LF/BOM policy and
camel/snake/CJK token policy. `SensitiveHandling` is explicit: Reject returns a stable error,
Redact stores only redacted text, and ReferenceOnly stores digest/metadata without text or tokens.
Secret markers and simple email/phone PII are covered by the boundary. `EmbeddingMetadata` and
`LlmTextMetadata` are separate strict digest-bound records; the local embedding representation is
token metadata only and makes no network/model-quality claim.

## CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `normalization_is_deterministic_for_unicode_and_cjk` | BOM, CRLF, full-width ASCII, identifier and CJK normalization is byte-stable |
| `secret_never_enters_index_or_embedding` | reject/redact/reference-only behavior prevents secret/PII text from output or tokens |
| `embedding_and_llm_metadata_are_separate_versioned_contracts` | embedding and LLM metadata have distinct schemas/digests |
| `normalization_keeps_unicode_sensitive_and_metadata_boundaries_explicit` | source guard retains bounded/no-network/no-second-loop boundaries |

## Proof ceiling and handoff

CM-09 proof ceiling is `source` plus remote CI wiring. This is not full Unicode normalization,
provider tokenizer/billing evidence, production PII classification or durable index enforcement;
index generation/atomic switching and invalidation remain CM-10–CM-11/PD work.
