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

## 2026-10-06 focused local receipt

`contains_sensitive` now accepts a complete `[REDACTED]` marker value while still rejecting an
unredacted marker and a non-delimited suffix such as `[REDACTED]suffix`. This lets the Redact path
validate its own output without weakening raw credential rejection.

```text
source_snapshot: current dirty worktree; kiana-domain text_normalization/redaction and CM-09 guard
worktree_status: roadmap row 320 remains 🔄; normalization fix plus focused regression, no index/provider integration
command_argv: bounded serial cargo test -p kiana-domain --test cm09_text_normalization --locked --offline -- --test-threads=1; cargo test -p kiana-core --test cm09_text_normalization_guard --locked --offline -- --test-threads=1; cargo fmt --all --check; git diff --check
cwd·environment: repository root Linux/bash; one Cargo/test worker, 32 MiB stack, stripped/non-incremental profile; domain cached run max RSS 82700 KiB, Core guard 85656 KiB, swaps 0
fixture·cassette: domain 3/3 plus explicit raw/redacted-marker suffix denial; Core guard 1/1
exit_code: all listed commands 0; logs/resources `/tmp/kiana-roadmap-20261005-evidence/cm09-{domain,core}.{log,resources}`
status_change: selected CM-09 normalization behavior is local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: selected normalization/guard source → local_behavior; no durable/live/physical promotion
limitations: full Unicode/PII classification, malformed marker variants beyond the focused suffix, durable index generation/switch/invalidation and canonical dedicated CI workflow evidence remain open
reviewer: integration agent with CM-09 read-only audit; no human release reviewer
```

## 2026-10-06 colon-form secret marker receipt

The normalization boundary now recognizes the colon forms already covered by the shared
redaction marker contract: `token:`, `password:`, `api_key:`, `access_key:`, `private_key:` and
`secret:`. Reject mode denies these values before normalized text, tokens or metadata are emitted.

```text
source_snapshot: current dirty worktree; kiana-domain/src/text_normalization.rs and kiana-domain/tests/cm09_text_normalization.rs
worktree_status: roadmap row 320 remains 🔄 / feature_status=partial / proof_level=source; no index/provider/manifest/lockfile or automatic commit/push/merge/rebase
command_argv: RED then GREEN `cargo test -p kiana-domain --test cm09_text_normalization --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test cm09_text_normalization_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, 32 MiB stack, stripped/non-incremental profile; domain GREEN 3/3 max RSS 4086720 KiB (~3.90 GiB), Core guard 1/1 max RSS 4092300 KiB (~3.90 GiB), swaps 0; `/tmp/kiana-roadmap-20261006-cm09/{red2,green,core}.{log,resources}`
fixture·cassette: RED `api_key:super-secret` exposed normalized text; GREEN rejects five colon-form credential markers under SensitiveHandling::Reject while existing Redact/ReferenceOnly behavior remains covered; no network/index/embed/model effect
exit_code: RED 101 expected; GREEN/Core/fmt/diff 0
status_change: selected colon-form secret rejection moved from source → local_behavior; whole CM-09 remains partial/source
proof-level_change: focused sensitive-input boundary source → local_behavior; no durable/live/physical promotion
limitations: marker detection remains heuristic; full Unicode/PII classification, durable index generation/switch/invalidation and dedicated CI evidence remain open
reviewer: root integration agent; CM-09 read-only audit; focused Rust test reviewer
```

## 2026-10-08 credential marker expansion receipt

The normalization sensitive-input detector now covers access, refresh and ID tokens, hyphenated
API keys, client secrets and proxy authorization in both `=` and `:` forms where applicable.
Reject mode refuses these values before normalized text, tokens or metadata are emitted.

```text
source_snapshot: current dirty worktree; kiana-domain/src/text_normalization.rs; kiana-domain/tests/cm09_text_normalization.rs; kiana-core/tests/cm09_text_normalization_guard.rs
worktree_status: CM-09 row 320 remains 🔄 / feature_status=partial / proof_level=source; selected credential marker coverage now local_behavior; no index/provider/manifest/lockfile or second redaction system
command_argv: RED `cargo test -p kiana-domain --test cm09_text_normalization secret_never_enters_index_or_embedding --locked --offline -- --exact --test-threads=1`; GREEN domain `cargo test -p kiana-domain --test cm09_text_normalization --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test cm09_text_normalization_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental tests; RED max RSS 4024888 KiB / 88% CPU; domain GREEN max RSS 4047916 KiB / 84% CPU; Core guard max RSS 4077596 KiB / 64% CPU; all measured runs reported 0 swaps; logs/resources `/tmp/kiana-roadmap-20261008-cm09-{red,domain,core}.{log,resources}`
fixture·cassette: RED accepted `api-key:CM09_RAW`; GREEN domain 3/3 and Core guard 1/1 reject access_token/refresh_token/id_token/api-key/client_secret/proxy-authorization markers with `normalization_sensitive_input_rejected`
exit_code: RED 101 expected; GREEN domain/Core 0
status_change: selected CM-09 credential marker coverage moves source → local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: focused marker denial source → local_behavior; no durable/live/physical promotion
limitations: malformed JSON-like quoted secret keys and formatted phone PII remain outside this slice; full classification, durable index generation/switch/invalidation and dedicated CI evidence remain open
reviewer: root integration agent with CM-09 read-only audit; focused text-normalization reviewer
```

## 2026-10-08 malformed JSON-like secret key receipt

Bounded normalization now detects parse-failing JSON-like input containing a quoted sensitive key
and marks it sensitive before policy handling. Reject mode therefore fails closed instead of
returning the original secret-bearing text; the scan is limited to the existing normalization
input bound and does not replace structured JSON parsing.

```text
source_snapshot: current dirty worktree; kiana-domain/src/text_normalization.rs; kiana-domain/tests/cm09_text_normalization.rs; kiana-core/tests/cm09_text_normalization_guard.rs
worktree_status: CM-09 row 320 remains 🔄 / feature_status=partial / proof_level=source; malformed JSON-like quoted-key denial now local_behavior; no index/provider/manifest/lockfile or second redaction system
command_argv: RED `cargo test -p kiana-domain --test cm09_text_normalization malformed_json_like_secret_key_is_rejected_without_original_fallback --locked --offline -- --exact --test-threads=1`; GREEN domain `cargo test -p kiana-domain --test cm09_text_normalization --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test cm09_text_normalization_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental tests; RED max RSS 3996548 KiB / 89% CPU; domain GREEN max RSS 4053428 KiB / 89% CPU; Core guard max RSS 4090024 KiB / 68% CPU; all measured runs reported 0 swaps; logs/resources `/tmp/kiana-roadmap-20261008-cm09-malformed-{red,domain,core}.{log,resources}`
fixture·cassette: RED returned `{"access_token":"CM09_RAW",}` unchanged; GREEN domain 4/4 and Core guard 1/1; stable `normalization_sensitive_input_rejected`
exit_code: RED 101 expected; GREEN domain/Core 0
status_change: selected CM-09 malformed JSON-like secret denial moves source → local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: bounded malformed-key scan source → local_behavior; no durable/live/physical promotion
limitations: Redact output for malformed structures, formatted phone PII, full classification and durable index generation/switch/invalidation remain open
reviewer: root integration agent with CM-09 read-only audit; focused malformed-input reviewer
```

## 2026-10-13 valid JSON secret boundary receipt

`contains_sensitive` now reuses the redaction module's bounded, recursive
`contains_json_secret_key` vocabulary for valid JSON. This closes the gap where a quoted key such
as `{"api_key":"..."}` bypassed the text markers because the input was valid JSON. The existing
`redact_text` structured path then replaces the sensitive field before token and metadata creation.

```text
source_snapshot: current dirty worktree; kiana-domain/src/text_normalization.rs; kiana-domain/src/redaction.rs; kiana-domain/tests/cm09_text_normalization.rs; kiana-core/tests/cm09_text_normalization_guard.rs
worktree_status: CM-09 row 320 remains 🔄 / feature_status=partial / proof_level=source; valid JSON secret detection is local_behavior; no index/provider/manifest/lockfile or second redaction vocabulary
command_argv: RED fixture was added for valid `{"api_key":"CM09_RAW","note":"safe"}`; GREEN `cargo test -p kiana-domain --test cm09_text_normalization --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test cm09_text_normalization_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode`; one Cargo/compiler job and one Rust test thread; 32 MiB Rust test stack; stripped/non-incremental locked/offline profile; completed runs reported 0 swap
fixture·cassette: domain 5/5; Reject returns `normalization_sensitive_input_rejected`; Redact removes `CM09_RAW` from structured text and tokens before metadata validation; ReferenceOnly emits no text or tokens; Core guard 1/1 checks reuse of `redaction::contains_json_secret_key`
exit_code: domain 0, Core guard 0; domain max RSS 4094996 KiB / 87% CPU / 0:56.56; Core guard max RSS 4107896 KiB / 91% CPU / 1:19.45; resource receipts `/tmp/kiana-roadmap-20261010-evidence/cm09-valid-json-{green,core}.{resources,log}`
status_change: selected valid JSON secret boundary moves source → local_behavior; whole CM-09 remains open
proof-level_change: local_behavior only; no durable/live/physical promotion
limitations: heuristic marker coverage remains bounded; full Unicode/locale classification, production normalization call sites, durable index generation/switch/invalidation and dedicated CI workflow evidence remain open
reviewer: root integration agent with CM-09 read-only audit; focused redaction/text-normalization reviewer
```

## 2026-10-13 formatted phone PII boundary receipt

The normalization detector now recognizes a conservative run of three or four all-digit groups
with a total of 10–15 digits, covering common formatted values such as `138 0013 8000`. Redaction
collapses the full run to one `[PII_REDACTED]` marker before tokens and metadata are generated.

```text
source_snapshot: current dirty worktree; kiana-domain/src/text_normalization.rs; kiana-domain/tests/cm09_text_normalization.rs; kiana-core/tests/cm09_text_normalization_guard.rs
worktree_status: CM-09 row 320 remains 🔄 / feature_status=partial / proof_level=source; selected formatted-phone boundary is local_behavior; no index/provider/manifest/lockfile or second redaction system
command_argv: `cargo test -p kiana-domain --test cm09_text_normalization --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test cm09_text_normalization_guard --locked --offline -- --test-threads=1`; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode`; one Cargo/compiler job and one Rust test thread; 32 MiB Rust test stack; stripped/non-incremental locked/offline profile; completed runs reported 0 swap
fixture·cassette: domain 6/6; `138 0013 8000` is rejected under Reject, collapsed to one `[PII_REDACTED]` marker under Redact, and omitted from ReferenceOnly text/tokens; Core guard 1/1 checks the bounded group detector and marker
exit_code: domain 0, Core guard 0; domain max RSS 4094316 KiB / 91% CPU / 0:53.64; Core guard max RSS 4098492 KiB / 91% CPU / 1:18.52; resource receipts `/tmp/kiana-roadmap-20261010-evidence/cm09-phone-{green,core}.{resources,log}`
status_change: selected formatted-phone PII boundary moves source → local_behavior; whole CM-09 remains open
proof-level_change: local_behavior only; no durable/live/physical promotion
limitations: this is a bounded heuristic, not locale-aware phone/PII classification; production normalization call sites, durable index generation/switch/invalidation and dedicated CI evidence remain open
reviewer: root integration agent with CM-09 read-only audit; focused PII/text-normalization reviewer
```
