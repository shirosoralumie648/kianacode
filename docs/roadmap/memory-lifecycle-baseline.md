# CM-02 MemoryRecord lifecycle and legacy import baseline

> 快照日期：2026-10-02。本文记录 CM-02 的 MemoryRecord 生命周期/provenance 字段和 v1 legacy import；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CM-02`](context-memory.md#step-cm-02) |
| source snapshot | `817277a`（CM-01 来源/scope 提交后的干净基线） |
| feature_status | `partial`（domain lifecycle + daemon reader/import source；visibility also validates lifecycle; remote verification and broader provenance limits remain open） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | JSONL row → bounded decode → explicit `MemoryRecord::legacy_import`/lifecycle validation → searchable/review projection |
| this step does | 为 MemoryRecord 增加 purpose/sensitivity/validity/retention/dependencies/import mode，校验 admission/review/state/provenance 组合；v1 行显式导入为 origin Unknown/Candidate/Draft/unverifiable，不默认 approved/verified |
| this step does not | 不建立 mutation/transaction/index generation/processing-grant service，不把 Memory JSONL 文件存在当 durable 事实，不改变 EventLog authority |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain lifecycle contract | `kiana-domain/src/memory.rs` | `42ec42ab1671ce5eec354ef230899867502a92510876c1249a20ee1314f7bcda` |
| Daemon decode/review writer | `kiana-daemon/src/harness_memory.rs` | `d6e22474b4b7b0549cdf90624d9cefa3a721e06efea539c29417f059be02c036` |
| Fixtures/workflow | `kiana-domain/tests/cm02_memory.rs`, `.github/workflows/cm02-memory-lifecycle.yml` | `e2dd6cb59b6a50afb050206194f116655a5295157cfb1d804fcfba9a37286be1`, `b8999f54ec57b1a4e91ee46911f639b0916968af31c703909d81fd7a876b2770` |

hash 只用于 CM-02 源码漂移复核，不构成 Memory durable、审批或业务 Outcome 证明。

## 2. Lifecycle dimensions

`MemoryRecord` 现在区分：

- `kind`、`purpose`、`sensitivity`、`validity`、`retention`、`dependencies`；
- `origin`（Unknown/Model/Hook/Git/User）；
- `admission_state`（Candidate/Qualified/Ephemeral/Rejected）；
- `state`（Draft/Active/Rejected）；
- `import_mode`（Native/LegacyImport）。

合法组合固定为 Candidate→Draft、Qualified→Active、Ephemeral→Active、Rejected→Rejected。Qualified/Active 必须有非 Unknown origin、purpose、sensitivity、reviewer/time 和 evidence；LegacyImport 强制 Unknown origin、Candidate/Draft、无 review，且永不 `searchable` 或 `verified`。Validity/retention/dependency 各自有界校验，坏 revision/collection/layer 不进入投影。

## 3. Legacy import

Daemon JSONL reader 对 `kiana.memory-record.v1` 调用 `MemoryRecord::legacy_import`，不再把缺失 admission/state/classification 的旧行升级为 Qualified/Active。Legacy row 转成 v2-compatible in-memory projection，保留原 text/source 诊断，但 provenance 为 unverifiable、搜索不可见。CM-02 的 `memory.review promote` 不能把它原位晋升；需要带新 evidence 的 Native successor。当前 EventStore-backed daemon adapter 仍拒绝 `accept_proposal`，因此这里不声称生产 successor 路径已打通。v2 native candidate/scratch/review writer 显式填充 purpose/sensitivity/import mode；旧文件仍只读兼容。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `legacy_memory_is_unverifiable_until_reviewed` | v1 legacy import 保持 Unknown/Candidate/Draft/unsearchable/unverified |
| `invalid_admission_state_combination_is_denied` | Candidate/Active、错误 validity 等组合 fail-closed |
| `qualified_memory_requires_review_evidence_and_purpose` | Qualified/Active 缺 provenance/review/evidence 时 lifecycle validation、visibility 和 searchable 均拒绝；完整对照仍 Searchable |
| `memory_record_rejects_unknown_nested_lifecycle_fields` | `Purpose`、`Retention`、`MemoryEvidence` 的嵌套未知字段以及 v1 legacy evidence 未知字段均 fail-closed |
| `qualified_memory_requires_review_evidence_and_purpose` malformed cases | nil event/request/run IDs, blank/oversize quotes, invalid Purpose, blank reviewer and zero review time fail closed |
| `legacy_memory_is_unverifiable_until_reviewed`（daemon） | 真实 JSONL reader 使用 explicit import，不把旧行直接暴露给检索 |
| `schema_v1_native_qualified_record_requires_explicit_legacy_import` | v1 Native Qualified/Active 不能绕过 importer；命名 upcaster 仍生成 v2 LegacyImport |
| `memory_event_projection_rejects_schema_v1_native_qualified_record` | committed memory fact 中嵌入的 v1 Native Qualified/Active 记录不能通过 lifecycle/projection |
| `memory_review_without_evidence_keeps_candidate_unmodified_and_unjournaled` | 普通 model Candidate 缺 evidence 时拒绝晋升；LegacyImport 原位 review 也拒绝；两者 JSONL 字节不变且 EventStore 未追加 memory fact |
| `model_written_memory_without_evidence_is_rejected_and_stays_unsearchable` | 缺 source evidence 的批准不能把模型候选变成 Qualified/Active 或 searchable |

以前的独立 `.github/workflows/cm02-memory-lifecycle.yml` 已删除并合并进 `.github/workflows/ci.yml`：`cm02_memory` 由 `kiana-domain-s1/4` shard 执行，daemon legacy reader fixture 随 `kiana-daemon` 包测试执行，fmt 由 Rust gates 执行。本地不运行测试或格式检查。

## 5. 限制与交接

- 当前 lifecycle validation 是值对象/reader gate，不是 Memory mutation 的原子提交点；CM-04/05 负责 CAS、幂等、EventStore/index visibility。
- purpose/retention/sensitivity 尚未由 processing grant/data policy 全量派生，用户私有跨项目隔离与删除传播仍是 CM-03+/PD/SC。
- Legacy import 在内存中转成 v2-compatible record 便于统一 reader，但不改写原 v1 文件或伪造历史 review；没有 successor 之前不可检索。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。

## 6. Qualified evidence and review-value validation (2026-10-02)

Source review found that Qualified/Active previously treated any present `Purpose`, nonempty
evidence vector, and present review fields as complete. Empty Purpose fields, evidence with nil
typed IDs or blank/oversize quotes, blank reviewer strings, and a zero review timestamp could
therefore pass lifecycle admission. The domain validator now validates Purpose values and
requires evidence IDs/quotes and review identity/time to be meaningful; CI-only negative cases
cover these inputs without changing the qualified valid control or the previous fixture-error fix.

```text
source_snapshot: `ae412092` plus fix `5c8a0831`, local merge `eae22402`, and the integrated fixture ownership correction
worktree_status: CM-02 lifecycle correction is merged into local master; only memory lifecycle code, CM-02 fixture, and this baseline changed; Qualified records now validate present Purpose values, evidence IDs/quotes, and nonblank reviewer/positive review time
command_argv: gh run view 36677090825 --job 109764373568 --log-failed; gh run view 36899317942 --json jobs; git diff --check
cwd/environment: /tmp/kiana-cm02-audit; Linux; local test/build/check/clippy/fmt/smoke commands not run
fixture or cassette: kiana-domain/tests/cm02_memory.rs::qualified_memory_requires_review_evidence_and_purpose; common CI maps it into kiana-domain-s1/4 and also runs the daemon package tests
exit_code: 0 for source review and branch/integration `git diff --check`; historical CI run 36677090825 showed the pre-correction broad-error assertion; current master run 36899317942 domain-s1 job was queued when inspected; fixtures added by this correction are not yet observed on GitHub
status_change: none; CM-02 remains 🔄 pending current GitHub CI evidence
proof-level_change: none; source only
limitations: new negative fixtures are unexecuted locally and current remote domain-s1 evidence is pending; exact event-to-quote/source binding remains CM-14/CM-25+; dependency invalidation and data epochs remain CM-06; normalization/sensitive-text handling remains CM-09; no semantic recall or durable-memory claim is made
reviewer: CM-02 implementation agent source review; no runtime test reviewer
```

## 7. Nested lifecycle payload strictness audit (2026-10-02)

Source review found that `MemoryRecord` rejected unknown top-level fields while its nested
`Purpose`, `Retention`, and `MemoryEvidence` values silently ignored unknown members. Those
three value types now reject unknown fields during deserialization. Serialization is unchanged,
and the daemon reader/upcaster continue to route v1 rows through explicit legacy import. The CI
fixture accepts known nested fields in both v2 records and v1 legacy evidence, and rejects unknown
nested keys in both versions.

```text
source_snapshot: `03b6e443` plus this isolated source/test/doc change
worktree_status: CM-02 nested payload decode is strict in `Purpose`, `Retention`, and `MemoryEvidence`; native constructors and serialization shape are unchanged; daemon JSONL reads and named upcasts route v1 through explicit legacy import; new negative fixture awaits GitHub CI after integration
command_argv:
  `rg -n "deny_unknown_fields|memory_record_rejects_unknown_nested_lifecycle_fields" kiana-domain/src/governance.rs kiana-domain/src/memory_proposals.rs kiana-domain/tests/cm02_memory.rs`
  `git diff --check`
  `gh run view 36926015057 --job 110583767786 --log`
cwd/environment: `/tmp/kiana-cm02-audit-20261002`; read-only GitHub CLI for receipts; local Cargo test/build/check/fmt/clippy/smoke commands not run
fixture or cassette: `kiana-domain/tests/cm02_memory.rs::memory_record_rejects_unknown_nested_lifecycle_fields`; historical run `36926015057` logged all three existing CM-02 fixtures passing, while the domain shard failed in unrelated `aut07_workflow_queue_claim::queue_claim_rejects_cycle_duplicate_and_parallel_overflow`; run `36958156294` predates this fixture and its domain shard failed, so it is not evidence for this change
exit_code: historical CM-02 fixture cases 3/3 passed in GitHub CI; `git diff --check` exit 0; new negative fixture has no CI result yet
status_change: none; CM-02 remains 🔄 and this change does not claim the roadmap step complete
proof-level_change: none; source only
limitations: at this historical evidence snapshot, `validate_lifecycle` still permitted a fully populated schema-v1 Native object constructed directly in memory; the later schema-v1 correction is recorded in §8. Nested strictness does not bind evidence quotes to immutable EventLog facts or establish durable approval; no local tests or builds were run
reviewer: CM-02 isolated source review; no runtime test reviewer for the new fixture
```

## 8. Schema-v1 lifecycle bypass correction (2026-10-02)

Source review found that a fully populated schema-v1 Native Qualified/Active `MemoryRecord` could
pass lifecycle validation and the EventLog memory projection, bypassing the explicit legacy import
contract. Lifecycle validation now accepts only v2 records; v1 records return
`memory_record_legacy_import_required`, and visibility denies every non-v2 record even when called
without validation. `legacy_import` now accepts only v1 payloads. The named storage upcaster leaves
the source schema intact while validating its field boundary, then delegates the conversion to that
explicit importer.

```text
source_snapshot: source commit `84c1c95ea2e791d927a4244185a88114db537574` based on `f28f1b5b`, integrated as `9ac1918f` with evidence push `feef5422`
worktree_status: schema-v1 Native records fail lifecycle validation and visibility; EventLog projection inherits the same denial; explicit JSONL import and named storage upcast still produce v2 LegacyImport records; roadmap remains partial/source pending GitHub CI
command_argv: source review of MemoryRecord lifecycle/import/visibility, storage upcast, and CM-02/CM-05 fixtures; `git diff` manual review; cherry-pick `84c1c95e`; push `origin/master` through `feef5422` (exit 0); no Cargo tests/build/check/fmt/clippy/smoke
cwd/environment: source review in `/tmp/kiana-cm02-v1-lifecycle-20261002`; integration in repository root; GitHub Actions is the only test executor
fixture or cassette: `schema_v1_native_qualified_record_requires_explicit_legacy_import`; `memory_event_projection_rejects_schema_v1_native_qualified_record`; `legacy_memory_is_unverifiable_until_reviewed` includes explicit upcaster control
exit_code: source commit, cherry-pick, evidence update, and push succeeded; no local tests, build, check, format, clippy, smoke, or diff-check command run; CI run `36981359579` remained pending/in progress at this evidence update
status_change: CM-02 remains 🔄; v1 Native Qualified/Active records can no longer become searchable through direct lifecycle or EventLog projection paths, while explicit v1 import/upcast remains available
proof-level_change: `feature_status=partial`; `proof_level=source`; no runtime/durable/live/physical promotion
limitations: fixtures await GitHub CI; event-to-quote/source binding remains CM-14/CM-25+; dependency invalidation remains CM-06; purpose/sensitivity derivation and retention policy remain later scope; no semantic recall or durable-memory claim is made
reviewer: isolated source review of schema gates, import compatibility, lifecycle visibility, and EventLog projection; no runtime test reviewer
```

## 9. Lifecycle-validated visibility (2026-10-02)

`MemoryRecord::visibility()` now requires a valid lifecycle before exposing a record as
Searchable, SessionOnly or ReviewOnly. The existing Qualified fixture retains a complete valid
control and additionally asserts that missing origin/evidence, purpose, sensitivity, reviewer or
review time returns Denied and `searchable() == false`.

```text
source_snapshot: master `db8a4606` plus isolated source commit `075b4cf1`; integrated source `8979ed29`
worktree_status: only the lifecycle-to-visibility gate and its existing domain fixture changed; no reader, projection, store, manifest or lockfile behavior was otherwise altered
command_argv: source/diff review; `git cherry-pick 075b4cf17f44bde7733cc37ab39ef543fea2c007`; no local cargo test/build/check/fmt/clippy/smoke
cwd·environment: isolated source worktree `/tmp/kiana-cm02-visibility-validation-20261002`; integration in repository root on Linux; GitHub Actions is the only test executor
fixture·cassette: `qualified_memory_requires_review_evidence_and_purpose`; run `36926015057` on older source logged three pre-existing CM-02 fixtures passing, but it predates this direct visibility assertion. Run `36981359579` was cancelled and run `36992023615` was in progress when inspected; no result is inferred for the new assertions.
exit_code: source review and cherry-pick succeeded; no local test/runtime exit code exists
status_change: CM-02 remains 🔄 / `feature_status=partial`; lifecycle-invalid v2 records cannot be surfaced through `visibility()` or `searchable()` even if callers bypass `validate_lifecycle()`
proof-level change: source only; no local_behavior, durable, live or physical promotion
limitations: unified run `36992023615` had a Rust gates failure at `cargo fmt --all --check`, but its full result/log was unavailable and this failure is not attributed to CM-02; run `36981359579` was cancelled; event-to-quote binding, durable mutation/index visibility, retention/revocation/deletion and semantic recall remain outside this source slice
reviewer: root source review confirmed lifecycle validation has no visibility recursion, valid Qualified control remains searchable, and invalid metadata is denied; no local runtime test reviewer
```

## 10. Evidence-required review promotion (2026-10-02)

The existing model-memory integration test exposed that the daemon review command could change a
Candidate with empty evidence to Qualified/Active. LegacyImport rows had the same in-place path,
which contradicts the import contract and then fails lifecycle validation. Promotion now fails
before mutation construction when evidence is absent, all review mutation of LegacyImport is
rejected, and the final record is lifecycle-validated before EventStore append or JSONL projection.
The positive `MemoryRecord` contract control remains in
`qualified_memory_requires_review_evidence_and_purpose`; there is no currently reachable
EventStore-backed daemon proposal-acceptance path to claim as a successful runtime promotion flow.

```text
source_snapshot: isolated source commit `4633379eee34dbfcf6cc77f6f9e05b0e831dd976` based on `491e6bd78b9527b0cc840f8f3aa615d73eb7c6a0`, cherry-picked onto current master; `kiana-daemon/src/harness_memory.rs`; `kiana-daemon/tests/daemon_host.rs`; this baseline; `CURRENT_STATUS.md`; `docs/roadmap.md`
worktree_status: CM-02 fail-closed source and fixture slice is integrated on master; ordinary model Candidate promotion requires evidence; LegacyImport review mutation is denied pending a Native successor; final lifecycle validation runs before journal/file append
command_argv: source/diff review; `cargo fmt --all --check`; `git diff --check`; `git cherry-pick 4633379eee34dbfcf6cc77f6f9e05b0e831dd976`; `git show --check HEAD`; no local test/build/Cargo check/clippy/smoke command
cwd·environment: source review in `/home/shirosora/kiana-wt/cm02-review-evidence-20261002`; integration in repository root; Linux; GitHub Actions remains the test executor
fixture·cassette: prior run `36994107681` / daemon job `110797094435` logged `harness_memory::tests::legacy_memory_is_unverifiable_until_reviewed ... ok` and the earlier `model_written_memory_stays_unsearchable_until_approved ... FAILED`; domain job `110797094451` ran `cm02_memory.rs` 5/5 and `cm05_memory_eventstore.rs` 3/3; new `memory_review_without_evidence_keeps_candidate_unmodified_and_unjournaled` checks model/legacy denial, byte-identical JSONL and an empty EventStore; renamed daemon-host test checks post-review state
exit_code: formatter, diff checks, cherry-pick and `git show --check` exited 0; new source fixtures have no post-integration GitHub result; no local test result exists
status_change: CM-02 remains 🔄 / `feature_status=partial`; unsupported promotion now fails closed at the adapter boundary
proof-level change: source only; no local_behavior, durable, live or physical promotion
limitations: the success-side `accept_proposal` helper is not reachable with the production EventStore-backed MemoryReviewHandler, which returns `memory_proposal_event_journal_required`; event-to-quote verification and a journaled Native successor path remain open; retention, revocation, durable recovery and semantic recall remain later scope; post-integration CI is pending
reviewer: isolated source review confirmed denial precedes MemoryMutation/EventStore/JSONL side effects and accepted records are lifecycle-validated; no runtime test reviewer
```

## 11. Review fixture revision precondition correction (2026-10-02)

Run 37005322134 / daemon job 110832244670 marked the new negative review fixture failed.
The model Candidate was built with MemoryRecord::default(), whose revision is zero; JSON
serialization kept that value, so review returned memory_revision_conflict before it reached
the intended evidence check. The fixture now sets revision 1 to match its review command. This
changes no production behavior.

The same cancelled daemon job marked model_written_memory_without_evidence_is_rejected_and_stays_unsearchable failed, but its test binary did not emit assertion details before cancellation. Its cause is unconfirmed and remains open for the next run.

```text
source_snapshot: CM-02 fail-closed source commit 43e1cbe8 plus the fixture precondition correction in kiana-daemon/src/harness_memory.rs; run 37005322134 / daemon job 110832244670
worktree_status: only the model Candidate fixture now sets revision 1, allowing the stale-revision gate to pass before the test asserts missing-evidence denial; production code is unchanged
command_argv: read-only GitHub log inspection; source review; cargo fmt --all --check; git diff --check; no local test/build/Cargo check/clippy/smoke command
cwd·environment: repository root; GitHub Actions Ubuntu runner for the observed failure; all tests remain remote-only
fixture·cassette: memory_review_without_evidence_keeps_candidate_unmodified_and_unjournaled failed because its serialized Candidate had revision 0 while expected_revision was 1; the separate daemon-host integration test was marked failed without an emitted assertion detail before cancellation
exit_code: run 37005322134 / job 110832244670 cancelled before target summaries; the corrected fixture has no new CI result; local formatting/whitespace checks only
status_change: CM-02 remains 🔄 / feature_status=partial; fixture correction awaits GitHub CI
proof-level change: none; proof_level=source
limitations: no cause is confirmed for the daemon-host integration failure; EventStore-backed proposal acceptance remains unavailable and quote/evidence binding, durable recovery, retention/revocation/deletion and semantic recall remain open
reviewer: root source and log review; no local runtime test reviewer
```

### Capability result source field collision (2026-10-03)

GitHub run `37041941851`, CM-02 job `110954471639`, ran
`model_written_memory_without_evidence_is_rejected_and_stays_unsearchable` and failed at the
initial `Completed` assertion with `ResultUnknown / result_unknown:result_event_persistence_failed`.
Source tracing confirmed the memory handler first commits its `memory.fact` and JSONL record, then
returns the business `source` field as part of its result. The capability-event builder flattened
that result into the event payload, where `ControlPlane::append_event` interpreted every top-level
`source` as a trusted notification source. The fixture's model-declared source is not a registered
notification source, so appending `capability.completed` failed and the finalizer conservatively
reported Unknown after the memory write had already committed.

The narrow correction keeps notification validation unchanged and moves a capability result's
business `source` field to `result_source` for both Run and direct capability events. A Core unit
fixture checks the field boundary for both payload builders; the existing daemon target remains the
end-to-end check that a candidate is committed, review without evidence is denied, and the candidate
remains unsearchable. CM-02 stays partial/source pending those GitHub receipts.

```text
source_snapshot: master `5ec79dca`; `kiana-core/src/events.rs`; `kiana-core/src/redaction.rs`; `kiana-core/tests/nm03_event_registry_guard.rs`; `kiana-daemon/src/harness_memory.rs`; `kiana-daemon/tests/daemon_host.rs`; `CURRENT_STATUS.md`
worktree_status: capability event result payloads preserve business `source` as `result_source`; server notification metadata validation remains unchanged. A unit fixture covers both run and direct event builders; existing CM-02 daemon target covers committed candidate, missing-evidence denial and unsearchability.
command_argv: GitHub logs `gh run view 37041941851 --job 110954471639 --log-failed`; source tracing with `rg`/`sed` of memory write ordering, capability result construction, and EventStore append validation; `cargo fmt --all --check`; `git diff --check`; no local tests/build/check/clippy/smoke
cwd·environment: repository root; Linux/bash; tests run only in GitHub Actions
fixture·cassette: prior run `37041941851` / job `110954471639` exact target returned `result_unknown:result_event_persistence_failed` before later search assertions; source trace identifies fixed rejection `notification_event_source_unknown`. New Core helper fixture and daemon target are queued for CI after push.
exit_code: source tracing complete; local formatting/diff checks only; post-change Core and daemon CI pending
status_change: CM-02 remains 🔄 / `feature_status=partial` / `proof_level=source`; result source metadata is now isolated from notification source metadata, while the intended end-to-end evidence-denial path awaits remote verification
proof-level change: none
limitations: the prior CI run did not reach its post-write visibility assertions; the new run must confirm the record committed and stays unsearchable. EventStore-backed proposal acceptance, event-to-quote binding, durable recovery, retention/revocation/deletion and semantic recall remain unproven.
reviewer: source review traced memory journal/JSONL commit before result emission and capability payload flattening into notification source validation; no local runtime test reviewer
```

### Daemon-host storage-redaction diagnostic (2026-10-02)

The later targeted run `37016271351` / daemon job `110868200314` resolved the earlier
daemon-host failure attribution: `model_written_memory_without_evidence_is_rejected_and_stays_unsearchable`
failed at `daemon_host.rs:3781` because EventLog returned
`model_admission_denied:port_failed:eventlog_storage_secret_sentinel_detected` for the initial
model event. Its serialized optional `credential_revision` was null. The value scanner already
skipped null, but structured redaction changed any sensitive-key value to `[REDACTED]`, so
`validate_secret_free` rejected the absent metadata before a candidate write or the intended
evidence-denial branch.

```text
source_snapshot: failing pushed head bcd7bb42cb7d7b91d89c12c92c6e179d01ec7881 based on 75226ee625aaf56fc6afa8e88f8c9066bc83f8cd; isolated correction preserves null sensitive fields; run 37016271351 / daemon job 110868200314
worktree_status: source correction keeps null unchanged in recursive redaction and excludes it from residual unredacted-secret detection; non-null values under sensitive keys are still redacted and fail storage validation; PD-28 regression fixture reuses existing domain target
command_argv: read-only GitHub log inspection; static source trace and diff review; `rustfmt --edition 2021` on changed Rust files; `git diff --check`; no local tests/build/check/clippy/smoke
cwd·environment: isolated source worktree; failing result from GitHub Actions Ubuntu; tests remain remote-only
fixture·cassette: `model_written_memory_without_evidence_is_rejected_and_stays_unsearchable` failed before candidate persistence at the first Completed-status assertion with `model_admission_denied:port_failed:eventlog_storage_secret_sentinel_detected`; it does not establish the later CM-02 evidence-denial assertion. The new domain null/non-null fixture has no CI receipt yet.
exit_code: observed remote case failed; source formatting and whitespace checks passed; no local runtime result
status_change: CM-02 remains 🔄 / feature_status=partial; attribution is now known, but the intended daemon evidence-denial path still awaits a fresh CI receipt
proof-level change: none; proof_level=source
limitations: candidate JSONL and evidence-denial checks were not reached in the observed run; EventStore-backed proposal acceptance and event/quote binding, durable recovery, retention/revocation/deletion, and semantic recall remain unproven
reviewer: isolated source review traced `credential_revision: null` through model-event serialization, EventStore validation and redaction; no runtime test reviewer
```

### Shared EventStore across recreated daemon hosts (2026-10-03)

Unified run `37048582415` / CM-02 job `110976529590` reached the pre-review search after the
memory write had returned Completed and the Candidate record was present in JSONL. That search
failed with `result_unknown:port_failed:memory_projection_unjournaled`: the test had created a
second `scripted_host`, which owns a new empty in-memory EventStore, while reusing the first host's
JSONL projection. `ensure_memory_projection` correctly refused the inconsistent journal/file pair.

The fixture now recreates writer, search, review, and post-review hosts against one persistent
`JsonlEventLog` path. This keeps the cross-host workflow and production unjournaled-projection guard
intact. A later CI run with value-free phase markers completed the writer and pre-review search,
then overflowed the test thread after `memory.review` began and before the command returned. Static
source tracing shows the handler's evidence check has not run; the remaining interval is capability
preparation, policy/gate, and approval staging/commit, but no recursive call is identified. The
Candidate's post-review search assertion remains unobserved. CM-02 remains partial/source.

```text
source_snapshot: persistent EventLog fixture `e855a414`; marker fixture `937e04ed`; run `37054968622`, head `8d42319c`, CM-02 job `110997880560`; `kiana-daemon/tests/daemon_host.rs`; `kiana-core/src/{commands,approvals,memory_proposals}.rs`; `kiana-daemon/src/harness_memory.rs`
worktree_status: diagnostic target logs fixed phases without values, payloads, paths, or identifiers. Writer and pre-review search completed; `memory.review` command overflowed before returning AwaitingApproval. No production behavior changed.
command_argv: GitHub job log via `gh api repos/shirosoralumie648/kianacode/actions/jobs/110997880560/logs`; read-only source trace; no local tests/build/check/clippy/smoke
cwd·environment: repository root; GitHub Actions Ubuntu runner; runtime tests remain remote-only
fixture·cassette: exact target `model_written_memory_without_evidence_is_rejected_and_stays_unsearchable`; logs show `review command begins`, then stack overflow/SIGABRT before `review command completes`
exit_code: Cargo exit 101 / SIGABRT; no assertion receipt after review command
status_change: CM-02 remains `partial/source`; failure boundary narrowed to review command preparation/authorization/approval staging, with root cause still unknown
proof-level change: none
limitations: no stack trace or source-level recursive call identified; MemoryReviewHandler, evidence rejection, unchanged memory JSONL and post-review unsearchability were not reached
reviewer: source trace through command dispatch, authorize/gate and approval staging; no local runtime reviewer
```

### Zero revision is refused at persistence and authority boundaries (2026-10-03)

```text
source_snapshot: `0753da22`; kiana-daemon/src/harness_memory.rs; kiana-domain/src/memory_journal.rs; kiana-domain/tests/cm02_memory.rs; kiana-daemon/tests/cm05_memory_eventstore.rs; .github/workflows/ci.yml
worktree_status: daemon JSONL read_records_file rejects v2 records with revision=0 before projection; MemoryJournalFact::validate rejects the same invalid authority fact; pure in-memory lifecycle/visibility and CM-04 CAS paths are unchanged
command_argv: isolated cargo fmt --all --check; isolated git diff --check; root cherry-pick 0753da22; git push origin master; no local tests/build/check/clippy/smoke
cwd·environment: isolated worktree `/tmp/kiana-cm02-zero-revision-20261003` integrated at repository root Linux/bash; GitHub Actions is the runtime authority
fixture·cassette: memory_read_rejects_zero_revision_before_projection; memory_fact_rejects_zero_revision_at_authority_boundary; memory_reader_has_zero_revision_persistence_fence; current-head CI run 37103038176 / head 4d70ea82 pending
exit_code: isolated format/diff 0; integration/push 0; remote runtime pending
status_change: CM-02 remains 🔄 / feature_status=partial / proof_level=source; zero-revision persistence/authority gap is closed in source, without proof promotion
proof-level change: none
limitations: no current-head CI receipt yet; existing memory.review stack-overflow/event-contract failure is separate; durable recovery, retention/revocation/deletion, semantic recall and live/physical effects remain open
reviewer: isolated CM-02 source audit and deny-first fixture review; no local runtime reviewer
```
