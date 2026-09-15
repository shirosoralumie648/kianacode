# Kiana 当前状态账本

> 本文件是“当前状态”的唯一汇总入口。  
> 更新规则：只有绑定源码快照、精确命令和证据产物后，才能提升状态或证明等级。  
> 各结论只绑定各自证据块的源码快照；2026-09-12 核对时共享工作树另有持续变化的 WIP，不能把历史证据套用到整个当前工作树。

## 1. 状态与证明等级

状态与证明等级是两个维度：

- 状态：`implemented`、`partial`、`target`、`deferred`、`not_supported`；
- 证明等级：`source`、`local_behavior`、`durable`、`live`、`physical`。

代码存在不等于已实现，局部测试通过不等于 `local_behavior`，`local_behavior` 不等于 durable/live/physical。

## 2. 当前产品主路径

当前产品主路径是：

```text
kiana-entrypoints
  → kiana-client / kiana-protocol
  → kiana-daemon::DaemonHost
  → kiana-core::ControlPlane
  → policy + gates + approval
  → kiana-runner::KianaHarness
  → capability broker / handlers
```

### S0 concurrency and fail-closed correction evidence (2026-09-01)

```text
source_snapshot: dirty checkout with uncommitted CompanyOS WIP; apply_patch transaction and broker failure handling corrected
worktree_status: WIP; no commit/reset; unrelated existing edits preserved
command_argv:
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --lib apply_patch --locked --offline
  cargo check --workspace --locked --offline
  cargo clippy --workspace --all-targets --locked --offline
  cargo fmt --all --check
  bash scripts/v10-workbench-smoke.sh
cwd/environment: repository root; Linux; stable toolchain; offline dependencies
fixture or cassette: daemon_host packet concurrency, hook denial, harness shell, apply_patch lock tests
exit_code: 0 for all listed commands; core 43/43, daemon_host 57/57, apply_patch 17/17
status change: S0 CompanyOS primary-path failure inventory reduced; Gate 0 remains red on legacy entrypoints tests
proof-level change: local_behavior evidence for serialized patch preflight/commit and fail-closed executor errors
limitations: legacy kiana-entrypoints CLI/SDK failures remain; frozen cli.rs was not modified; no S1-S4 gate advancement
reviewer: focused adversarial regression review
```

| 能力 | 状态 | 证明等级 | 当前边界 |
|---|---|---|---|
| 本地受信项目上的受控 coding 行为 | partial | local_behavior | 受固定本机、cassette/fake-script 和现有 smoke 限制 |
| `shell` / `apply_patch` broker 主路径 | partial | local_behavior | 仍需加强 TOCTOU、原子 patch、进程树取消和输出 redaction |
| CLI / Workbench / loopback Web 共用 DaemonHost | partial | local_behavior | Web session ownership、异步状态和多标签页隔离仍有缺口 |
| 基础 trust / sandbox / role / path / memory policy | partial | local_behavior | DaemonHost 已从 stored ProjectTrust 派生项目可信度；固定本地主体、role/department assignment 和持久身份仍未完成 |
| WorkPacket / Symposium / Review 领域对象 | partial | source/local_behavior | 现有 schema 较窄，独立 Reviewer 和完整生命周期尚未完成 |
| JSONL EventLog / 基础 Receipt | partial | local_behavior | 当前按 request sequence；不是 aggregate/CAS durable authority |
| Desktop Web 壳 | partial | local_behavior | 安装、升级、worker 崩溃恢复和供应链证据未达生产级 |
| live provider | partial | live | 已用 DeepSeek 的 Anthropic 兼容端点实跑（见 2026-09-09 证据块）；单 provider / 单模型，未验证原生 OpenAI/Anthropic 凭据 |
| token streaming | partial | live | CLI / 文件夹工作台 / 网页 SSE 均已接入并逐块交付；实跑逐字节时间戳见 2026-09-09 证据块；断线重连、配额、跨进程传输未覆盖 |
| 跨进程完整 resume | deferred | source | session/run/lock/approval 仍有进程内状态 |
| 支付、外卖、打车、旅行预订 | not_supported | source | 尚无满足身份、审批、幂等、对账和证据要求的 adapter |
| 智能家居和物理设备控制 | not_supported | source | 尚无独立安全控制器、watchdog、急停和物理证据 |
| 企业租户、RBAC、远程执行 | deferred | source | 必须先完成个人本地核心并重新设计身份和租户边界 |

### Roadmap source reconciliation evidence (2026-09-12)

```text
source_snapshot: db77c2485bcafecbb1da17ec57ee509ad2ee32b4; inspected immutable git blobs, including kiana-runner/src/harness.rs, kiana-daemon/src/lib.rs, kiana-daemon/tests/daemon_host.rs, kiana-core/src/lifecycle.rs and the existing evidence below; supplementary WIP source observation at 2026-09-12 16:12 +08:00, not a tested snapshot
worktree_status: shared checkout contains pre-existing cross-slice WIP and changed during read-only review; the documentation draft from /tmp/kiana-roadmap-20260912-vx1fopqv is now applied to docs/roadmap.md and CURRENT_STATUS.md after the user's instruction superseded the old single-writer handoff restriction; unrelated edits preserved; no product source edit, commit, push, merge, reset or worktree deletion by this documentation task
command_argv:
  git rev-parse HEAD
  git status --short
  git show --stat HEAD
  git show db77c2485bcafecbb1da17ec57ee509ad2ee32b4:kiana-runner/src/harness.rs
  git show db77c2485bcafecbb1da17ec57ee509ad2ee32b4:kiana-daemon/src/lib.rs
  git show db77c2485bcafecbb1da17ec57ee509ad2ee32b4:kiana-daemon/tests/daemon_host.rs
  git show db77c2485bcafecbb1da17ec57ee509ad2ee32b4:kiana-core/src/lifecycle.rs
  cargo check --workspace --locked --offline
  git apply --check /tmp/kiana-roadmap-20260912-vx1fopqv/roadmap.patch
  git apply /tmp/kiana-roadmap-20260912-vx1fopqv/roadmap.patch
  python3 /tmp/kiana-roadmap-20260912-vx1fopqv/validate_documents.py
  git diff --check -- docs/roadmap.md CURRENT_STATUS.md
cwd/environment: repository commands at /media/shirosora/4A183E5C183E46EB/codestorage/kianacode; Linux; Cargo locked/offline; source findings pinned to git blobs; documentation draft applied to the repository and updated for the user's current instruction
fixture or cassette: none; source inspection only. The listed Cargo check observed a moving WIP snapshot and is not a product behavior fixture
exit_code: git inspection commands=0; patch applicability check=0; patch application=0; document validation=0 (74 table rows/cards, matching statuses/dependencies, acyclic graph, unchanged historical evidence); scoped diff check=0; shared-WIP cargo check=101 (runner Value/prompt_sources errors and core non-exhaustive ExecutionStatus matches); files subsequently changed by another writer, so this is neither a stable HEAD build result nor validation of later WIP
artifact paths and SHA-256: /tmp/kiana-roadmap-20260912-vx1fopqv/snapshot.json records SHA-256 of the inspected immutable source/document copies; proposed roadmap/status documents and roadmap.patch are local review artifacts, not CI evidence
status change: roadmap P0-J1-05a reopened from completed to in-progress because the HEAD model-config constructor loses wall-time configuration/invalid-value rejection; P0-G-04 reopened because existing evidence does not establish the full Invocation/projection-cache scope; P0-J1-05b moves from queued to in-progress because db77c24 adds a command limit without consuming it or supplying a role snapshot. Later WIP now selects role limits in core and consumes them per run in the harness, and adds Invocation projection/recovery source; these changes still require product-path validation. Product feature_status declarations are not promoted
proof-level change: none; these are source findings and evidence-scope corrections. Historical wall-time and Run projection local_behavior evidence remains bound to its original snapshots; existing provider/streaming live evidence is unaffected
limitations: no focused RED/GREEN run or complete regression on a stable implementation snapshot; no remote CI query; no claim that db77c24 passes or fails compilation. Role tests at that snapshot exercise a config helper or constructor limit, not the required whole product chain. Pending tests in the roadmap are plans, not executed evidence. Subsequent WIP requires separate implementation validation; applying these documents is not a capability promotion
reviewer: Codex root source review plus independent read-only roadmap_review agent; no independent runtime validation or human acceptance
```

### P0-J1-05a model-config wall-time regression evidence (2026-09-14)

```text
source_snapshot: dd6a8d5cde8da4e10ede0479109a0ba7eaaf086f; kiana-daemon/src/lib.rs and kiana-daemon/tests/daemon_host.rs
worktree_status: committed on master and pushed to origin/master; documentation backfill is the only follow-up change in this step
command_argv:
  cargo check -p kiana-daemon --tests --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; rustc/cargo stable; offline dependency cache; no test binaries executed
fixture or cassette: daemon model-config construction with fake provider; wall-time environment values `not-a-number` and `1`
exit_code: 0 for compile, format, and diff checks; local tests deliberately not run per user instruction; GitHub CI triggered by push and not awaited
status change: P0-J1-05a implementation completed. `DaemonHost::local_with_model_config` now parses the full `RuntimeConfig`, so `KIANA_HARNESS_WALL_TIME_MS` is preserved and invalid values fail before model setup. Added daemon acceptance coverage for invalid configuration and wall-time exhaustion with no completion event.
proof-level change: source plus compile/static-check evidence only; no local_behavior promotion until the remote CI job supplies its test receipt
limitations: CI result was intentionally not awaited; no local test or smoke command was run; the next step P0-J1-05b still needs product-path role-limit behavior coverage
reviewer: Codex root implementation review; read-only role/wall-time audits; no runtime test reviewer
```

### P0-J1-05b role max-steps product-path evidence (2026-09-14)

```text
source_snapshot: bd9dea0; kiana-core/src/lifecycle.rs, kiana-core/src/receipts.rs, kiana-runner/src/harness.rs, kiana-daemon/tests/daemon_host.rs
worktree_status: committed on master and pushed to origin/master; roadmap and CURRENT_STATUS backfill is the only follow-up change in this step
command_argv:
  cargo check -p kiana-daemon --tests --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; rustc/cargo stable; offline dependency cache; no test binaries executed
fixture or cassette: RoleCountingModel through one DaemonHost for architect/builder role snapshots; environment max-step override; RoleIsolationModel covering wall-time Continue reset and independent second run; runner Start command cap differs from constructor cap
exit_code: 0 for compile, format, and diff checks; local tests deliberately not run per user instruction; GitHub CI triggered by push and not awaited
status change: P0-J1-05b implementation and planned product-path coverage completed. ControlPlane resolves the role max-step snapshot (environment override first), records it in run.authorized, passes it through Start, and the harness enforces the per-run minimum of command and constructor caps. Receipt projection exposes the authorized max-step value. Continue resets only the selected run's step/clock state and preserves its cap; independent runs receive independent counters.
proof-level change: source plus compile/static-check evidence only; no local_behavior promotion until GitHub CI supplies the test receipt
limitations: CI result was intentionally not awaited; no local test or smoke command was run; model-counting behavior, Receipt assertions, and Continue isolation remain remote-CI evidence; wall-time policy still has the separate assignment bound documented by the existing runtime budget code
reviewer: Codex root implementation review; read-only role-limit audit; no runtime test reviewer
```

### P1-J3-01 memory candidate admission evidence (2026-09-14)

```text
source_snapshot: 758ffbe; kiana-daemon/src/harness_memory.rs, kiana-daemon/tests/daemon_host.rs
worktree_status: source commit is on master and pushed to origin/master; roadmap and CURRENT_STATUS backfill is included in the follow-up documentation commit
command_argv:
  cargo check -p kiana-daemon --tests --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; rustc/cargo stable; locked offline dependency cache; no test binaries executed
fixture or cassette: scripted PM model writes department:planning while attempting to spoof origin/admission/state; fresh DaemonHost search before and after the existing memory.review approval proof; legacy kiana.memory-record.v1 project fixture
exit_code: 0 for compile, format, and diff checks; local tests deliberately not run per user instruction; GitHub CI triggered by push and not awaited
status_change: P1-J3-01 implementation and product-path acceptance coverage completed. The memory broker derives model writes as origin=model and persistent candidate/draft, ignores model-supplied lifecycle/provenance fields, excludes candidates from default search, and promotes only through the operator-bound memory.review approval flow with an exact record revision. v1 records missing the new lifecycle fields retain legacy searchable admission/state while origin remains Unknown and hits remain unverifiable.
proof-level_change: source plus compile/static-check evidence only; no local_behavior promotion until GitHub CI supplies the runtime receipt
limitations: CI result was intentionally not awaited; no local test or smoke command was run; the new acceptance, approval continuation, and v1 compatibility behavior remain remote-CI evidence; full memory lifecycle (proposal extraction, expiry/revocation propagation, durable index rebuild) remains outside this step
reviewer: Codex root implementation review; no runtime test reviewer
```

### P0-G-04 invocation projection and restart recovery evidence (2026-09-15)

```text
source_snapshot: 38f23bc; kiana-core invocation projection, cache invalidation, receipt filtering, dispatch facts, and restart recovery
worktree_status: source commit is on master; documentation backfill is the only follow-up change in this step; push is pending until this evidence commit is created
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked offline dependency cache; no test binaries executed
fixture or cassette: `new_process_rebuilds_invocation_state_from_events_alone`; conflicting terminal fixture; restart pending-approval fixture with gate re-check; pre-prepare rejection and MCP result-envelope cases covered by the projection/recovery paths
exit_code: 0 for format, workspace compile, and diff checks; local tests deliberately not run per user instruction; GitHub CI is expected to execute the focused control-plane tests after push and is not awaited
status_change: P0-G-04 invocation projection is implemented at source level. Runtime facts now carry stable request identity, strict run binding, transition validation, duplicate/conflicting terminal detection, lazy cache rebuild and invalidation, result-unknown fail-closed handling, redacted pre-prepare rejection facts, and approval recovery that requires an event-backed request/approval binding. `run_state` and resume filtering propagate malformed event conflicts instead of silently returning an empty projection.
proof-level_change: source plus static compile evidence only; no local_behavior or durable promotion until remote CI supplies its receipt
limitations: CI result was intentionally not awaited; no local test, smoke, or clippy command was run; the roadmap remains 🔄 because remote acceptance and any integration regressions are still pending; legacy receipt helpers outside this slice retain their compatibility fallback behavior
reviewer: Codex root implementation review plus independent invocation/recovery audit; no runtime test reviewer
```

### CI-01 configuration, credentials, and identity baseline evidence (2026-09-15)

```text
source_snapshot: 310bcec + CI-01 working-tree slice (provider redaction, fixtures, baseline document, and GitHub Actions job)
worktree_status: source changes are scoped to the CI-01 baseline; no unrelated WIP was reverted; commit and push are pending until static verification completes
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked offline dependency cache; no test binaries executed
fixture or cassette: scripts/fixtures/config-credentials-identity/{env-precedence,profile-precedence,legacy-local-user-event,config-migration-v0,secret-channel-sentinel}.json; kiana-provider/tests/ci01_baseline.rs; scripts/ci-01-baseline.py
exit_code: 0 for format, workspace compile, and diff checks; local tests deliberately not run per user instruction; GitHub Actions CI-01 job is queued by the push and is not awaited
status_change: CI-01 source baseline is complete: production ProviderGateway precedence and test-only legacy parser boundary are recorded; ProviderConfig Debug no longer exposes raw api_key; profile unknown fields and all seven sentinel channels have deterministic CI guards; local-user migration and config migration remain explicitly deferred to CI-04/CI-06
proof-level_change: source plus static compile evidence only; no local_behavior, durable, or live promotion until remote CI supplies its receipt
limitations: bootstrap Config, daemon LocalModelConfig, and legacy services ApiKey/client still retain raw-secret compatibility gaps; Connection still stores raw credential; no ConfigResolver, SecretStore, CredentialLease, authenticated ingress, or identity migration event was implemented; remote CI result is intentionally not awaited
reviewer: Codex root implementation review plus CI-01 mapper/reviewer read-only audits; no runtime test reviewer
```

### SW-00 bounded swarm reconciliation evidence (2026-09-15)

```text
source_snapshot: 1be732612bf727d21a4d7dea54ad4aff5454f15b; docs/roadmap/swarm-baseline.md; kiana-core/tests/swarm_baseline.rs; .github/workflows/sw00-baseline.yml; 10 hashed product-path files listed in the baseline
worktree_status: SW-00 source baseline, CI guard, roadmap and status entries are scoped to this step; no unrelated WIP was reverted; commit and push are pending until static verification completes
command_argv:
  sha256sum <10 Swarm source files listed in docs/roadmap/swarm-baseline.md §1>
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked offline dependency cache; no test binaries executed
fixture or cassette: source-only `swarm_reconciliation_does_not_claim_durable_from_in_memory_cas`; MemoryCellRegistry/process-local port qualifiers; event-before-Company-StartRun ordering; no runtime fixture executed locally
exit_code: 0 for source hash, format, workspace compile, and diff checks; local tests deliberately not run per user instruction; GitHub Actions SW-00 job is queued by the push and is not awaited
status_change: SW-00 reconciliation completed at source level. Product ingress, bounded domain transitions, event-backed swarm replay, Company StartRun route, and packet child admission are marked source-wired; MemoryCellRegistry durable recovery, atomic dispatch queue/attempt/fence, fresh child session, independent integrator receipt, and Swarm-specific runtime tests remain explicitly partial/target/missing.
proof-level_change: source plus static compile evidence only; no local_behavior or durable promotion until remote CI supplies its test receipt
limitations: CI result was intentionally not awaited; no local test or smoke command was run; the event-before-dispatch window, process-local Cell registry, copied child session, and all SW-01..SW-18 runtime acceptance gaps remain; legacy kiana-commands Swarm tests are not product-path evidence
reviewer: Codex root implementation review plus independent SW-00 mapper, security, and test-boundary read-only audits; no runtime test reviewer
```

### OA-00 observability and audit inventory evidence (2026-09-15)

```text
source_snapshot: 0fb757588a232333ecb0e8304215e8c247ad0d; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; kiana-core/tests/observability_baseline.rs; .github/workflows/oa00-baseline.yml; 19 hashed product-path files listed in the baseline
worktree_status: OA-00 signal matrix, owner/proof ceiling/迁移清单、module-map link and CI guard are scoped to this step; no unrelated WIP was reverted; commit and push are pending until static verification completes
command_argv:
  sha256sum <19 observability/EventLog source files listed in docs/roadmap/observability-audit-baseline.md §1>
  rg -n 'ObservabilityPort|TraceSink|MetricSink|AuditQueryPort|HealthProbePort|OperationalLog|MetricPoint|MetricSnapshot|AuditRecord|HealthSnapshot|CorrelationContext|span_id|source_cursor|source_event_ids' kiana-domain/src kiana-ports/src kiana-core/src kiana-eventlog/src kiana-daemon/src kiana-protocol/src
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked offline dependency cache; no test binaries executed
fixture or cassette: source-only `observability_inventory_preserves_fact_projection_and_open_contracts`; RuntimeEvent/EventStore/Receipt/RunStream/GoldenTrace/usage/Incident signal matrix; no runtime fixture executed locally
exit_code: 0 for source hash, format, workspace compile, and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-00 job is queued by the push and is not awaited
status_change: OA-00 inventory completed at source level. EventLog/Transition/CommandReceipt remains the fact boundary; Receipt/Run projection and RunStream are marked derived; golden trace, UI/transcript/cache, usage naming, receipt-as-Outcome, open event kind/payload, absent formal Audit/Metric/Trace/Health contracts and migration owners are recorded explicitly.
proof-level_change: source plus static compile evidence only; no local_behavior or durable promotion; no trace, metric, audit or health runtime claim
limitations: CI result was intentionally not awaited; no local test or smoke command was run; EventStore adapter redaction/classification, receipt source cursor/projection version, telemetry queue/backpressure, audit query/export, health projector and OA-01+ schema/ports remain open; historical ER-30/P1-J8-01 evidence is not rewritten
reviewer: Codex root implementation review plus independent OA-00 mapper, security and test-boundary read-only audits; no runtime test reviewer
```

### OA-01 observability domain contract evidence (2026-09-15)

```text
source_snapshot: 435d27a8a649d4df790c657b2a38aae8e39e2030; kiana-domain/src/contracts.rs; kiana-domain/src/lib.rs; new kiana-domain/src/observability.rs; kiana-domain/tests/oa01_contracts.rs; .github/workflows/oa01-contracts.yml
worktree_status: OA-01 domain contracts, schema registry extension, focused remote-only test and roadmap/status backfill are scoped to this step; no unrelated WIP was reverted; commit and push are pending until static verification completes
command_argv:
  sha256sum kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-domain/src/observability.rs
  rg -n 'OBSERVABILITY_SCHEMA|AUDIT_RECORD_SCHEMA|METRIC_CATALOG_SCHEMA|TRACE_SUMMARY_SCHEMA|deny_unknown_fields|source_cursor|source_event_ids|validate_with_catalog' kiana-domain/src kiana-domain/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked offline dependency cache; no test binaries executed
fixture or cassette: `kiana-domain/tests/oa01_contracts.rs`; unknown major/unknown field/invalid status/empty cursor/bad digest/unregistered metric/oversized attribute rejection cases plus serde round-trip, canonical bytes, and minor additive compatibility; GitHub Actions only
exit_code: 0 for source hash, format, workspace compile, and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-01 job is queued by the push and is not awaited
status_change: OA-01 domain/schema source slice is implemented. `kiana-domain` now owns four versioned projection contracts (`ObservabilityRecord`, `AuditRecord`, `MetricCatalog`/`MetricPoint`, `TraceSummary`) with closed serde fields, same-major compatibility, bounded source lineage/attributes, canonical SHA-256 digests, audit epochs and metric catalog membership validation. No EventStore writer, projector, sink, exporter, authorization decision or health runtime was added.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test, smoke or clippy command was run; OA-02 typed correlation links, OA-03 shared redaction/classification, HealthSnapshot, runtime sinks/projectors, and all downstream observability behavior remain open; schema registration does not prove runtime or durable telemetry
reviewer: Codex root implementation review plus OA-01 domain-contract/static-boundary review; no runtime test reviewer
```

### OA-02 correlation and trace reference evidence (2026-09-15)

```text
source_snapshot: 34e606b; kiana-domain/src/correlation.rs; kiana-domain/src/contracts.rs; kiana-domain/src/lib.rs; kiana-ports/src/lib.rs; kiana-domain/tests/oa02_correlation.rs; kiana-ports/tests/oa02_correlation_port.rs; .github/workflows/oa02-correlation.yml
worktree_status: OA-02 domain correlation contracts, port adapter, focused remote-only tests and roadmap/status overlay are scoped to this step; no unrelated WIP was reverted; commit and push are pending until static verification completes
command_argv:
  sha256sum kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-domain/src/correlation.rs kiana-ports/src/lib.rs
  rg -n 'CORRELATION_CONTEXT_SCHEMA|TraceParent|TraceRef|SpanRef|SpanLink|CausationRef|AttemptRef|validate_for_request|CorrelationContextPort' kiana-domain/src kiana-ports/src kiana-domain/tests kiana-ports/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked offline dependency cache; no test binaries executed
fixture or cassette: `kiana-domain/tests/oa02_correlation.rs` and `kiana-ports/tests/oa02_correlation_port.rs`; strict W3C traceparent rejection, fresh root/foreign-parent linking, forged actor/epoch/scope rejection, run→turn→invocation→attempt command binding, child/recovery span link behavior and port fail-closed errors; GitHub Actions only
exit_code: 0 for format and workspace compile checks; local tests deliberately not run per user instruction; GitHub Actions OA-02 job is queued by the push and is not awaited
status_change: OA-02 correlation source slice is implemented. Server-derived `CorrelationContext` binds authenticated request identity, scope and authority/data epochs; W3C input is link-only; typed TraceRef/SpanRef, causation and attempt links preserve request/run/turn/invocation lineage; command/attempt, cross-scope, stale epoch, invalid parent and self/parent reuse paths fail closed. `kiana-ports` exposes only a side-effect-free construction boundary.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test, smoke or clippy command was run; OA-03 redaction/classification, runtime ingress/span bridge, EventStore commit observer, TraceSink/exporter and all downstream signal projections remain open; correlation metadata never authorizes, authenticates or proves an effect
reviewer: Codex root implementation review plus OA-02 domain/port static-boundary review; no runtime test reviewer
```

### OA-03 redaction profile and bounded encoder evidence (2026-09-15)

```text
source_snapshot: 231a77d; kiana-domain/src/redaction.rs; kiana-domain/src/contracts.rs; kiana-core/tests/observability_baseline.rs; kiana-domain/tests/oa03_redaction.rs; .github/workflows/oa03-redaction.yml
worktree_status: OA-03 profile/encoder source, schema registration, focused remote-only tests and observability inventory overlay are scoped to this step; no unrelated WIP was reverted; commit and push are pending until static verification completes
command_argv:
  sha256sum kiana-domain/src/redaction.rs kiana-domain/src/contracts.rs
  rg -n 'RedactionProfile|RedactionSignal|encode_bounded_value|encode_bounded_text|redaction_secret_sentinel_detected|redaction_.*too_large' kiana-domain/src kiana-domain/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked offline dependency cache; no test binaries executed
fixture or cassette: `kiana-domain/tests/oa03_redaction.rs`; profile digest/unknown-field rejection, nested token/password/api-key/header sentinel masking across log/metric/trace/audit/export profiles, secret_ref preservation, oversized/deep/NUL failure and no-original-fallback assertions; GitHub Actions only
exit_code: 0 for format, workspace compile, and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-03 job is queued by the push and is not awaited
status_change: OA-03 source slice is implemented. Versioned `RedactionProfile` carries signal/data classification, bounded bytes/depth and a profile digest; bounded value/text encoders reuse existing recursive redaction then fail closed on residual secret markers, NUL, depth, encoding or size violations. Errors never return the unredacted input. Runtime sinks and all producer wiring remain separate steps.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test, smoke or clippy command was run; EventStore/Receipt/Provider/Broker/TraceSink/export runtime paths still use their existing compatibility wrappers until OA-05/OA-06; arbitrary unknown secret formats outside the marker/key policy remain a later hardening concern
reviewer: Codex root implementation review plus OA-03 redaction/static-boundary review; no runtime test reviewer
```

### OA-04 committed audit taxonomy and reducer evidence (2026-09-15)

```text
source_snapshot: 6bcf7b3; kiana-domain/src/{audit,observability,redaction}.rs; kiana-domain/src/lib.rs; kiana-core/src/{audit,lib}.rs; kiana-domain/tests/oa04_audit_taxonomy.rs; kiana-core/tests/oa04_audit_reducer.rs; .github/workflows/oa04-audit.yml; docs/roadmap/observability-audit-baseline.md
worktree_status: OA-04 domain taxonomy/reducer, core facade, focused remote-only fixtures, baseline hash overlay, roadmap/module documentation and status backfill are scoped to this step; no unrelated WIP was reverted; commit and push are pending until static verification completes
command_argv:
  sha256sum kiana-domain/src/lib.rs kiana-domain/src/audit.rs kiana-core/src/audit.rs
  rg -n 'AuditEventSpec|classify_audit_event|reduce_audit_records|SERVER_AUDIT_ACTOR|audit_event_kind_untrusted|audit_decision_conflict|audit_target_binding_required' kiana-domain/src kiana-core/src kiana-domain/tests kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-domain --tests --locked --offline
  cargo check -p kiana-core --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; rustc/cargo 1.97.1; locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: `kiana-domain/tests/oa04_audit_taxonomy.rs` and `kiana-core/tests/oa04_audit_reducer.rs`; command/deny/approval/capability/credential/recovery/query/export taxonomy, source cursor/ID and digest binding, redacted reason, server actor, unbound/missing epoch/forged actor/record, duplicate/contradictory decision, unknown/self-submitted audit event rejection; GitHub Actions only
exit_code: 0 for format, domain/core test-target compilation, and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-04 job is queued by the push and is not awaited
status_change: OA-04 source slice is implemented. `kiana-domain` now classifies eight audit action families and reduces only committed RuntimeEvent facts with a nonzero EventCursor, source binding, epochs and unique source IDs. Records use a fixed server actor, bounded Audit redaction, action/input/reason/correlation/causation references and canonical record digests. Unknown non-audit events remain opaque; self-submitted `audit.*`, forged actor/record payloads, missing bindings/epochs, cursor overflow, duplicate source/logical records and decision conflicts fail closed. `kiana-core` exposes only the same pure committed-event facade; no writer, Broker, Provider, UI or exporter path was added.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; EventLog commit observer, AuditProjection checkpoint/rebuild, durable sink/query/export, append-only correction event and runtime instrumentation remain OA-05/OA-06/OA-15/OA-16 work; fixed server actor is a reducer boundary and not a replacement for the future authenticated principal/assignment contract
reviewer: Codex root implementation review plus OA-04 domain/core static-boundary review; no runtime test reviewer
```

### OA-05 observability ports and fake adapters evidence (2026-09-15)

```text
source_snapshot: bea86dc; kiana-domain/src/{observability,contracts}.rs; kiana-ports/src/lib.rs; kiana-ports/tests/oa05_observability_ports.rs; .github/workflows/oa05-ports.yml; docs/roadmap/observability-audit-baseline.md
worktree_status: OA-05 HealthSnapshot contract, signal union, sink/query/health ports, Memory/JSONL fakes, remote-only fixtures, baseline hash overlay, roadmap/module documentation and status backfill were scoped to this step; no unrelated WIP was reverted; committed and pushed as 8c3ab48
command_argv:
  sha256sum kiana-domain/src/contracts.rs kiana-domain/src/observability.rs kiana-ports/src/lib.rs
  rg -n 'ObservabilityPort|TraceSink|MetricSink|AuditQueryPort|HealthProbePort|ObservabilityCapabilities|require_observability_capabilities|MemoryObservabilitySink|JsonlObservabilitySink' kiana-domain/src kiana-ports/src kiana-ports/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-domain --tests --locked --offline
  cargo check -p kiana-ports --tests --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; rustc/cargo 1.97.1; locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: `kiana-ports/tests/oa05_observability_ports.rs`; sink signal recording, trace/metric delegation, capacity/failure/cancellation, capability negotiation, flush ack, bounded audit query page and explicit health probe; GitHub Actions only
exit_code: 0 for format, domain/ports/workspace test-target compilation, and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-05 job is queued by the push and is not awaited
status_change: OA-05 source slice is implemented. `kiana-ports` now owns a closed `ObservabilitySignalRecord` union plus `ObservabilityPort`, `TraceSink`, `MetricSink`, `AuditQueryPort` and `HealthProbePort`; wrapper variants reject mismatched inner signal kinds, capability negotiation rejects missing durable/flush/cancellation/capacity guarantees, query is bounded to redacted AuditRecord projections, and flush/cancel/append acknowledgements remain non-authorizing. `kiana-domain` adds versioned/digest-bound `HealthSnapshot`. Memory and JSONL fakes support validation, injected failure, bounded capacity, cancellation, flush ack, query and explicit health state without Broker or EventLog writes.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; fake adapters are deliberately non-durable, no EventLog commit observer/backpressure/retention/export integration exists, query authentication and DataBoundary remain future OA-16/OA-17 work, and a health snapshot does not prove provider or business health
reviewer: Codex root implementation review plus OA-05 ports/fake static-boundary review; no runtime test reviewer
```

### OA-06 EventStore commit observer evidence (2026-09-15)

```
source_snapshot: 8c3ab48; kiana-ports/src/lib.rs; kiana-eventlog/src/{lib,stream}.rs; kiana-eventlog/tests/oa06_commit_observer.rs; .github/workflows/oa06-commit-observer.yml; docs/roadmap/observability-audit-baseline.md
worktree_status: OA-06 CommittedTransition/observer contract, StreamEventStore decorator, bounded observer-failure diagnostics, remote-only fixtures, baseline hash overlay, roadmap/module documentation and status backfill are scoped to this step; no unrelated WIP was reverted; static verification is complete and this slice is included in the accompanying step commit
command_argv:
  sha256sum kiana-ports/src/lib.rs kiana-eventlog/src/lib.rs kiana-eventlog/src/stream.rs
  rg -n 'CommittedTransition|EventStoreCommitObserver|StreamEventStore|CommitObserverFailure' kiana-ports/src kiana-eventlog/src kiana-eventlog/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-ports --tests --locked --offline
  cargo check -p kiana-eventlog --tests --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; rustc/cargo 1.97.1; locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-eventlog/tests/oa06_commit_observer.rs; fresh Committed notification after inner receipt visibility, replay without duplicate observer call, CAS conflict/Unknown suppression, observer failure diagnostics, forged cursor/event identity rejection; GitHub Actions only
exit_code: 0 for format, eventlog/ports/workspace test-target compilation, and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-06 job is queued by the push and is not awaited
status_change: OA-06 source slice is implemented. kiana-ports now validates receipt identity, contiguous source cursor and exact source event IDs in CommittedTransition; kiana-eventlog::StreamEventStore delegates all EventStore guarantees, advances a process-local committed cursor, and invokes observers only after fresh atomic commits. Replays, conflicts, unknown outcomes and underlying errors do not publish; observer failures remain bounded diagnostics and cannot rewrite a committed outcome.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; callback delivery is a best-effort wake hint and can be lost, so durable projections must rescan read_from; legacy append* paths intentionally do not synthesize transition receipts; no Audit/Metric/Trace/Receipt projection or backpressure/retention/export integration exists yet
reviewer: Codex root implementation review plus OA-06 eventlog/observer static-boundary review; no runtime test reviewer
```

### OA-07 Run/Turn/Invocation span lifecycle evidence (2026-09-15)

```
source_snapshot: 7923d75; kiana-domain/src/{observability,contracts}.rs; kiana-core/src/{lib,span_projection}.rs; kiana-core/tests/oa07_span_lifecycle.rs; .github/workflows/oa07-span-lifecycle.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-07 span lifecycle contract, deterministic EventLog reducer, read-only ControlPlane bridge, remote-only fixtures, schema baseline overlay, roadmap/module documentation and status backfill are scoped to this step; no unrelated WIP was reverted; static verification is complete and this slice is included in the accompanying step commit
command_argv:
  sha256sum kiana-domain/src/contracts.rs kiana-domain/src/observability.rs kiana-core/src/span_projection.rs kiana-core/src/lib.rs
  rg -n 'SpanLifecycleRecord|SpanEntityKind|SpanLifecyclePhase|project_span_lifecycle|span_lifecycle|span_state' kiana-domain/src kiana-core/src kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa07_span_lifecycle --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; rustc/cargo 1.97.1; locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa07_span_lifecycle.rs; deterministic Run/Turn/Invocation lifecycle rows, approval pause/resume, compact checkpoint, successful terminal, cancellation/unknown mapping, duplicate terminal, late delta and stale attempt; GitHub Actions only
exit_code: 0 for format, OA-07 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-07 job is queued by the push and is not awaited
status_change: OA-07 source slice is implemented. `kiana.span-lifecycle.v1` enforces entity ID relationships, single source event, bounded error/attributes and digest; `kiana-core::span_projection` derives stable trace/span IDs and lifecycle records solely from committed `RuntimeEvent` order, ignores duplicate/late/old-attempt facts, fails closed on conflicting terminal status, and exposes a read-only `ControlPlane::span_lifecycle`/`span_state` bridge. New attempts require a higher attempt number; span end never appends or mutates terminal EventLog facts.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; source-order cursor is a deterministic projection cursor rather than a durable checkpoint, observer delivery/exporter/backpressure/retention remain future OA-08/OA-10/OA-13/OA-14 work, and existing provider/model/broker/effect spans are not yet instrumented
reviewer: Codex root implementation review plus OA-07 projection/static-boundary review; no runtime test reviewer
```

### OA-08 Provider/model/stream/usage instrumentation evidence (2026-09-15)

```text
source_snapshot: fa3d713; kiana-domain/src/{observability,contracts,model}.rs; kiana-core/src/{lib,model_attempt_projection}.rs; kiana-provider/src/{lib,telemetry}.rs; kiana-daemon/src/model_client.rs; kiana-runner/src/harness.rs; kiana-core/tests/oa08_model_instrumentation.rs; kiana-provider/tests/oa08_provider_telemetry.rs; .github/workflows/oa08-model-instrumentation.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-08 model-attempt contract, provider/daemon allow-list, committed-event reducer, remote-only fixtures, schema baseline overlay, roadmap/module documentation and status backfill are scoped to this step; no unrelated WIP was reverted; static verification is complete and this slice is included in the accompanying step commit
command_argv:
  sha256sum kiana-domain/src/observability.rs kiana-domain/src/contracts.rs kiana-core/src/model_attempt_projection.rs kiana-core/src/lib.rs kiana-provider/src/telemetry.rs kiana-daemon/src/model_client.rs kiana-runner/src/harness.rs
  rg -n 'ModelAttemptRecord|ModelCacheUsage|project_model_attempts|safe_prepared_metadata|InstrumentedModelClient|usage_complete|retry_class|provider_stream_incomplete' kiana-domain/src kiana-core/src kiana-provider/src kiana-daemon/src kiana-runner/src kiana-core/tests kiana-provider/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa08_model_instrumentation --locked --offline
  cargo check -p kiana-provider --test oa08_provider_telemetry --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa08_model_instrumentation.rs and kiana-provider/tests/oa08_provider_telemetry.rs; normal complete attempt, secret sentinel, malformed/truncated/timeout/retry/missing usage, low-cardinality cache/stop/retry classification, duplicate suppression and contract fail-closed cases; GitHub Actions only
exit_code: 0 for format, core/provider/workspace test-target compilation, and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-08 job is queued by the push and is not awaited
status_change: OA-08 source slice is implemented. `kiana.model-attempt.v1` and `ModelAttemptRecord` expose bounded provider/model/route/prompt-hash, stream/latency/stop/usage/retry/cache fields with source cursor/event and stable span identity. `kiana-core::model_attempt_projection` rebuilds only from committed `run.model_turn`, deduplicates event/attempt facts, and maps malformed, truncated, timeout, retry, incomplete-usage or unattempted observations to non-Ok status. Provider and daemon share an allow-listed prepared summary; prompt text, wire body, tool arguments, endpoint, authentication header, cache key and raw response are not representable.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; model attempt projection remains a read-only source-order view without durable checkpoint, Metric/Audit/Trace sink, Receipt/cost reconciliation, provider cache instrumentation, exporter, retention or live backend; `run.model_turn` compatibility payload still carries legacy redacted fields for existing receipts, while OA-08 projection deliberately excludes them
reviewer: Codex root implementation review plus OA-08 domain/core/provider/daemon static-boundary review; no runtime test reviewer
```

### OA-09 Broker/approval/effect/stop instrumentation evidence (2026-09-15)

```text
source_snapshot: 4d5ecaf; kiana-domain/src/{observability,contracts}.rs; kiana-core/src/{capability_attempt_projection,capabilities,approvals,dispatch,events,lib}.rs; kiana-daemon/src/harness_capabilities.rs; kiana-core/tests/oa09_capability_instrumentation.rs; .github/workflows/oa09-capability-instrumentation.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-09 CapabilityAttemptRecord contract, committed-event reducer, handler-boundary execution CAS, bounded broker/approval/effect/stop metadata, remote-only fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/observability.rs kiana-domain/src/contracts.rs kiana-core/src/capability_attempt_projection.rs kiana-core/src/lib.rs kiana-core/src/capabilities.rs kiana-core/src/approvals.rs kiana-core/src/dispatch.rs kiana-core/src/events.rs kiana-daemon/src/harness_capabilities.rs
  rg -n 'CapabilityAttemptRecord|project_capability_attempts|commit_invocation_executing|effect_known|zero_effect|stop_confirmed|fenced' kiana-domain/src kiana-core/src kiana-daemon/src kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa09_capability_instrumentation --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa09_capability_instrumentation.rs; successful admission→permit→dispatch→execution→result, policy/hook deny, expired approval, TOCTOU/lease mismatch Unknown, cancel with unconfirmed stop, secret sentinel and contract fail-closed fixtures; GitHub Actions only
exit_code: 0 for source hashes, format, core OA-09 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-09 job is queued by the push and is not awaited
status_change: OA-09 source slice is implemented. `kiana.capability-attempt.v1` and `CapabilityAttemptRecord` expose bounded admission, approval, permit, dispatch, execution, effect, stop, fencing and zero-effect evidence. `kiana-core::capability_attempt_projection` rebuilds attempts only from committed facts and exposes read-only `ControlPlane::capability_attempts`/`effect_attempts`; execution starts with a handler-preceding CAS `invocation.executing` fact, so a failed boundary commit does not call the handler. Deny, expired approval, lease/TOCTOU mismatch, cancellation and unknown effect preserve zero-effect/unknown/fencing semantics.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; attempt projection has no durable checkpoint/reconcile queue, external effect receipt, provider-side verification, stop/process-group confirmation, telemetry exporter or live backend; execution-start CAS and result persistence can still leave an effect unknown and require future reconciliation
reviewer: Codex root implementation review plus OA-09 domain/core/daemon static-boundary review; no runtime test reviewer
```

### OA-10 EventLog/projector/Receipt/Artifact/Recovery metrics evidence (2026-09-15)

```text
source_snapshot: caec077; kiana-domain/src/{observability,contracts}.rs; kiana-core/src/{metrics,lib}.rs; kiana-core/tests/oa10_operational_metrics.rs; .github/workflows/oa10-operational-metrics.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-10 MetricSnapshot/metric catalog contracts, committed-event operational metrics reducer, read-all ControlPlane bridge, remote-only fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/observability.rs kiana-domain/src/contracts.rs kiana-core/src/metrics.rs kiana-core/src/lib.rs kiana-core/tests/oa10_operational_metrics.rs .github/workflows/oa10-operational-metrics.yml
  rg -n 'MetricSnapshot|project_operational_metrics|project_metrics|operational_metrics|eventlog_cursor_gap|orphan_total|unknown_total|artifact_bytes' kiana-domain/src kiana-core/src kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa10_operational_metrics --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa10_operational_metrics.rs; empty source rejection, cursor gap/projector lag, orphan dispatch, unknown effect, artifact read failure, append/flush/query latency, complete committed snapshot, digest/serde and secret sentinel fixtures; GitHub Actions only
exit_code: 0 for source hashes, format, core OA-10 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-10 job is queued by the push and is not awaited
status_change: OA-10 source slice is implemented. `kiana.metric-snapshot.v1` and `MetricSnapshot` bind status, source/projector cursors, bounded metric points, limitations and digest. `kiana-core::metrics` derives EventLog/projector/Receipt/Artifact/Recovery metrics from deduplicated committed facts, refuses empty sources and cursor-ahead input, preserves lag/gap/orphan/unknown/artifact-failure evidence, and exposes read-only `ControlPlane::operational_metrics`/`metrics` without using a run stream as system-wide health.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; metric snapshot has no durable projector checkpoint, runtime gauge feed, backpressure/queue integration, Receipt/Artifact writer reconciliation, external effect confirmation, or live exporter; latency is absent rather than fabricated when no committed sample exists, and inferred projector cursor is explicitly marked degraded
reviewer: Codex root implementation review plus OA-10 domain/core metric static-boundary review; no runtime test reviewer
```

### OA-11 Health snapshot/readiness/liveness evidence (2026-09-15)

```text
source_snapshot: 9921ec3; kiana-domain/src/observability.rs; kiana-core/src/{health,metrics,lib}.rs; kiana-daemon/src/lib.rs; kiana-core/tests/oa11_health_probes.rs; .github/workflows/oa11-health-probes.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-11 probe-kind/component-health contracts, core health aggregator, DaemonHost read-only bridge, remote-only fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/observability.rs kiana-domain/src/contracts.rs kiana-core/src/health.rs kiana-core/src/metrics.rs kiana-core/src/lib.rs kiana-daemon/src/lib.rs kiana-core/tests/oa11_health_probes.rs .github/workflows/oa11-health-probes.yml
  rg -n 'HealthProbeKind|ComponentHealth|project_health_snapshot|health_snapshot|readiness|liveness|startup_health' kiana-domain/src kiana-core/src kiana-daemon/src kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa11_health_probes --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa11_health_probes.rs; empty source fail-closed, readiness against Unknown/lag/inferred checkpoint, liveness read-only semantics, EventStore capability degradation, component state/version/last-success/limitation and serde/digest fixtures; GitHub Actions only
exit_code: 0 for source hashes, format, core OA-11 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-11 job is queued by the push and is not awaited
status_change: OA-11 source slice is implemented. `HealthSnapshot` now carries explicit startup/readiness/liveness/drain/maintenance probe semantics and bounded `ComponentHealth` rows. `kiana-core::health` combines committed operational metrics with EventStore capabilities, preserving empty-source, cursor-gap, projector lag/inferred checkpoint, Unknown/orphan and unsupported-storage evidence as degraded/unavailable; provider/Broker/telemetry without an independent probe remain unknown. `ControlPlane` and `DaemonHost` expose only read-only health/readiness/liveness bridges.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; health has no durable heartbeat/checkpoint, startup coordinator, cross-process lease/fence, provider/Broker/exporter live probe or ready admission gate; liveness success only proves the source could be projected, not external service or business health
reviewer: Codex root implementation review plus OA-11 domain/core/daemon static-boundary review; no runtime test reviewer
```

### OA-12 metric catalog/reducer/cardinality evidence (2026-09-15)

```text
source_snapshot: 5db96f2; kiana-domain/src/observability.rs; kiana-core/src/{metrics,lib}.rs; kiana-core/tests/oa12_metric_governance.rs; .github/workflows/oa12-metric-governance.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-12 typed metric catalog/quality, cardinality guard, overflow/counter/cursor reducer, replay/live parity fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/observability.rs kiana-core/src/metrics.rs kiana-core/src/lib.rs kiana-core/tests/oa12_metric_governance.rs .github/workflows/oa12-metric-governance.yml
  rg -n 'MetricQuality|MetricCardinalityGuard|MetricReducer|metric_counter_reset|metric_cardinality|catalog_digest' kiana-domain/src kiana-core/src kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa12_metric_governance --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa12_metric_governance.rs; catalog kind/unit/digest, sensitive label/value, distinct-value overflow, counter reset/cursor regression, measured/estimated quality and replay/live parity fixtures; GitHub Actions only
exit_code: 0 for source hashes, format, core OA-12 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-12 job is queued by the push and is not awaited
status_change: OA-12 source slice is implemented. MetricCatalog/MetricPoint now carry typed kind/unit, explicit measured/estimated quality and catalog digest binding; MetricCardinalityGuard rejects unregistered/sensitive/high-cardinality labels and bounded overflow; MetricReducer applies transactional catalog/cardinality/counter monotonicity/cursor checks to both incremental and replay snapshots, with duplicate digest idempotence and no EventLog/Broker side effects.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; metric reducer remains process-local with no durable sink/queue/checkpoint or runtime gauge feed, overflow reporting is not persisted, and estimated metrics never prove measured cost or external effect
reviewer: Codex root implementation review plus OA-12 domain/core metric-governance static-boundary review; no runtime test reviewer
```

### OA-14 trace exporter/W3C context evidence (2026-09-15)

```text
source_snapshot: fd264bb; kiana-domain/src/{contracts,observability}.rs; kiana-core/src/{trace_export,lib}.rs; kiana-core/tests/oa14_trace_export.rs; .github/workflows/oa14-trace-export.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-14 TraceExportSpan contract, foreign-parent parser/link adapter, bounded local/no-op exporter, sampling/capacity/closed guards, JSONL/flush/shutdown/reopen fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/contracts.rs kiana-domain/src/observability.rs kiana-core/src/lib.rs kiana-core/src/trace_export.rs kiana-core/tests/oa14_trace_export.rs .github/workflows/oa14-trace-export.yml
  rg -n 'TraceExportSpan|TraceParent|foreign_parent_link|LocalTraceExporter|SampledOut|trace_export_capacity|jsonl' kiana-domain/src kiana-core/src kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa14_trace_export --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa14_trace_export.rs; invalid W3C parent/trace ID, foreign link, sampled-out/no-op, low-cardinality secret redaction, capacity/closed, JSONL, flush/shutdown/reopen fixtures; GitHub Actions only
exit_code: 0 for source hashes, format, core OA-14 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-14 job is queued by the push and is not awaited
status_change: OA-14 source slice is implemented. `kiana.trace-export-span.v1` and `TraceExportSpan` constrain exported trace/span IDs, parent, names, status, sampling, source evidence, duration, low-cardinality attributes and digest. `foreign_parent_link` treats W3C input as a ForeignParent link only; `LocalTraceExporter`/`NoopTraceExporter` honor enabled/sampled/capacity/closed boundaries and serialize only validated local JSONL records, with no EventLog/Receipt/authority mutation.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; exporter is process-local with no OTLP/external backend, durable queue/spool, persisted sampling policy, cross-process recovery or live trace completeness, and flush only acknowledges local record handling
reviewer: Codex root implementation review plus OA-14 domain/core trace-export static-boundary review; no runtime test reviewer
```

### OA-15 AuditProjection checkpoint/rebuild evidence (2026-09-15)

```text
source_snapshot: 02d68d3; kiana-domain/src/{contracts,observability}.rs; kiana-domain/src/audit.rs; kiana-core/src/{audit,audit_projection,lib}.rs; kiana-core/tests/oa15_audit_projection.rs; .github/workflows/oa15-audit-projection.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-15 AuditProjectionSnapshot/Checkpoint contracts, deterministic rebuild/append/restore reducer, source cursor/schema/decision/checksum guards, remote-only fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/contracts.rs kiana-domain/src/observability.rs kiana-domain/src/audit.rs kiana-core/src/audit.rs kiana-core/src/audit_projection.rs kiana-core/src/lib.rs kiana-core/tests/oa15_audit_projection.rs .github/workflows/oa15-audit-projection.yml
  rg -n 'AuditProjectionSnapshot|AuditProjectionCheckpoint|rebuild_audit_projection|apply_page|restore|source_cursor_gap|checkpoint_digest' kiana-domain/src kiana-core/src kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa15_audit_projection --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa15_audit_projection.rs; deterministic rebuild/restore, contiguous incremental page, cursor gap/regression, unknown audit schema, decision conflict, duplicate source, forged checkpoint and serde/original-fact preservation fixtures; GitHub Actions only
exit_code: 0 for source hashes, format, core OA-15 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-15 job is queued by the push and is not awaited
status_change: OA-15 source slice is implemented. `kiana.audit-projection.v1` and `kiana.audit-projection-checkpoint.v1` bind projection version, source cursor/event IDs, ordered AuditRecord IDs, records/checkpoint/projection digests and bounded limitations. `rebuild_audit_projection` validates explicit cursor continuity before the OA-04 reducer; `AuditProjection::apply_page` requires the next cursor, while `from_snapshot`/`restore` validate complete bindings without mutating EventLog facts. Unknown/self-submitted audit kinds, decision conflicts, duplicate source IDs and corrupted checkpoints fail closed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; checkpoint storage is process-local, no automatic cross-process reload or EventLog commit observer consumer is wired, Artifact refs/query/export/correction/incident paths remain open, and source-only checkpoint proof is not durable audit retention
reviewer: Codex root implementation review plus OA-15 domain/core audit-projection static-boundary review; no runtime test reviewer
```

### OA-16 Audit query command/wire DTO evidence (2026-09-15)

```text
source_snapshot: da73518; kiana-domain/src/contracts.rs; kiana-core/src/{audit_projection,lib}.rs; kiana-protocol/src/lib.rs; kiana-client/src/lib.rs; kiana-daemon/src/lib.rs; kiana-protocol/tests/oa16_audit_query.rs; .github/workflows/oa16-audit-query.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-16 server-scoped AuditQuery request/response DTO, ControlPlane projection/filter route, client facade, DaemonHost authentication/permission route, remote-only wire fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/contracts.rs kiana-core/src/audit_projection.rs kiana-core/src/lib.rs kiana-protocol/src/lib.rs kiana-client/src/lib.rs kiana-daemon/src/lib.rs kiana-protocol/tests/oa16_audit_query.rs .github/workflows/oa16-audit-query.yml
  rg -n 'AuditQueryRequest|AuditQueryResponse|query_audit|audit_query|audit_query_unauthenticated|audit_query_limit_invalid|raw_events' kiana-domain/src kiana-core/src kiana-protocol/src kiana-client/src kiana-daemon/src kiana-protocol/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-protocol --test oa16_audit_query --locked --offline
  cargo check -p kiana-daemon --tests --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-protocol/tests/oa16_audit_query.rs; bounded wire round-trip, limit/cursor/filter rejection, owner/raw-event field rejection; server-scoped core/daemon route is source-wired for GitHub CI; no external cassette
exit_code: 0 for source hashes, format, protocol/daemon OA-16 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-16 job is queued by the push and is not awaited
status_change: OA-16 source slice is implemented. `kiana-protocol` carries closed `AuditQueryRequest`/`AuditQueryResponse` and `RequestBody::AuditQuery`; `KianaClient::audit_query` only transports it. `DaemonHost` rejects missing/forged actor and invalid bounds before calling `ControlPlane::query_audit`; core rebuilds OA-15 facts and filters records using server-derived actor/session/canonical project plus request/approval lineage, returning bounded redacted pages with source cursor/projection version/limitations and no raw EventLog endpoint.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; query index/filter snapshot and cursor epochs are not durable, cross-entry parity and external authenticated principal provider remain open, query currently requires EventStore read-all and can return unavailable, and export/delivery/reconcile actions are not wired
reviewer: Codex root implementation review plus OA-16 core/protocol/client/daemon static-boundary review; no runtime test reviewer
```

### OA-17 query cursor/snapshot/paging evidence (2026-09-15)

```text
source_snapshot: a8a9f32; kiana-domain/src/{contracts,observability}.rs; kiana-core/src/{audit_projection,lib}.rs; kiana-protocol/src/lib.rs; kiana-daemon/src/lib.rs; kiana-protocol/tests/oa16_audit_query.rs; kiana-core/tests/oa17_query_cursor.rs; .github/workflows/oa17-query-cursor.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-17 AuditQueryCursor contract, protocol cursor field/next-cursor shape, core epoch/filter/source binding and stale/ahead guards, remote-only cursor fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/contracts.rs kiana-domain/src/observability.rs kiana-core/src/audit_projection.rs kiana-core/src/lib.rs kiana-protocol/src/lib.rs kiana-daemon/src/lib.rs kiana-core/tests/oa17_query_cursor.rs .github/workflows/oa17-query-cursor.yml
  rg -n 'AuditQueryCursor|audit_query_cursor_stale|query_filter_digest|next_cursor|projection_version|filter_digest' kiana-domain/src kiana-core/src kiana-protocol/src kiana-daemon/src kiana-core/tests kiana-protocol/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa17_query_cursor --locked --offline
  cargo check -p kiana-protocol --test oa16_audit_query --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa17_query_cursor.rs and protocol OA-16 cursor round-trip; cursor digest/epoch/version/filter binding, after/source bounds, stale version and bounded paging contracts; GitHub Actions only
exit_code: 0 for source hashes, format, core/protocol OA-17 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-17 job is queued by the push and is not awaited
status_change: OA-17 source slice is implemented. `kiana.audit-query-cursor.v1` and `AuditQueryCursor` bind epoch, projection version, source/after cursor, filter digest and cursor digest. `AuditQueryRequest` validates cursor compatibility; `ControlPlane::query_audit` rejects stale/ahead/source/filter mismatches and emits a bound next cursor, preserving empty-vs-unavailable semantics without raw EventLog or authorization side effects.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; cursor epoch/filter snapshot is derived in process from the current projection and is not durable, retention revoke/reconnect/multi-tab parity and slow-query instrumentation remain open, query still requires read-all and export/delivery are not wired
reviewer: Codex root implementation review plus OA-17 domain/core/protocol cursor static-boundary review; no runtime test reviewer
```

### OA-18 audit export/manifest/delivery evidence (2026-09-15)

```text
source_snapshot: ea635e6; kiana-domain/src/{contracts,observability}.rs; kiana-core/src/{audit_export,audit_projection,lib}.rs; kiana-protocol/src/lib.rs; kiana-client/src/lib.rs; kiana-daemon/src/lib.rs; kiana-core/tests/oa18_audit_export.rs; kiana-protocol/tests/oa18_audit_export.rs; .github/workflows/oa18-audit-export.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-18 export/manifest/delivery contracts, server-scoped redacted materializer, protocol/client/DaemonHost route, remote-only domain/wire fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/contracts.rs kiana-domain/src/observability.rs kiana-core/src/audit_export.rs kiana-core/src/audit_projection.rs kiana-core/src/lib.rs kiana-protocol/src/lib.rs kiana-client/src/lib.rs kiana-daemon/src/lib.rs kiana-core/tests/oa18_audit_export.rs kiana-protocol/tests/oa18_audit_export.rs .github/workflows/oa18-audit-export.yml
  rg -n 'AuditExportManifest|AuditDeliveryReceipt|AuditExportRequest|export_audit|delivery_not_confirmed|audit_export_requires_explicit_permission' kiana-domain/src kiana-core/src kiana-protocol/src kiana-client/src kiana-daemon/src kiana-core/tests kiana-protocol/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa18_audit_export --locked --offline
  cargo check -p kiana-protocol --test oa18_audit_export --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa18_audit_export.rs and kiana-protocol/tests/oa18_audit_export.rs; manifest/artifact/query hash, missing purpose/recipient/retention, safe/unauthenticated deny, redacted JSONL/JSON/CSV contract, oversized/secret, Unknown-vs-Delivered confirmation and closed wire fixtures; GitHub Actions only
exit_code: 0 for source hashes, format, core/protocol OA-18 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-18 job is queued by the push and is not awaited
status_change: OA-18 source slice is implemented. `kiana.audit-export.v1` and `kiana.audit-delivery-receipt.v1` bind export/query/source/projection/artifact hashes, format, purpose, recipient, retention and confirmation state. `ControlPlane::export_audit` reuses server-scoped query, emits bounded redacted JSONL/JSON/CSV content and manifest, denies Safe/unauthenticated/invalid/oversized/secret material, and emits delivery `Unknown` without an independent confirmation; protocol/client/DaemonHost route no raw EventLog or direct delivery side effect.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; export content/manifest is process-local and not written to durable ArtifactStore, no external delivery connector or server confirmation exists, export audit event/retention/query index/cross-entry parity remain open, and `deliver=true` intentionally cannot claim delivered
reviewer: Codex root implementation review plus OA-18 domain/core/protocol/client/daemon export static-boundary review; no runtime test reviewer
```

### OA-19 observability alert/incident/recovery evidence (2026-09-15)

```text
source_snapshot: 12fd3f2; kiana-domain/src/{contracts,observability}.rs; kiana-core/src/{incident_projection,metrics,lib}.rs; kiana-core/tests/oa19_incident_projection.rs; .github/workflows/oa19-observability-incidents.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-19 observability alert/incident/snapshot contracts, committed-metric/failure rule projector, stable fingerprint dedupe, reconciliation-safe recovery association, remote-only fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/contracts.rs kiana-domain/src/observability.rs kiana-core/src/incident_projection.rs kiana-core/src/metrics.rs kiana-core/src/lib.rs kiana-core/tests/oa19_incident_projection.rs .github/workflows/oa19-observability-incidents.yml
  rg -n 'ObservabilityAlert|ObservabilityIncident|project_observability_incidents|project_incidents|requires_reconciliation|unknown_cannot_close|fingerprint' kiana-domain/src kiana-core/src kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa19_incident_projection --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa19_incident_projection.rs; projector lag, effect unknown, orphan dispatch, artifact/audit loss, redaction failure, queue overflow, dedupe, fixed recovery plan, unknown cannot close and model/UI self-report rejection fixtures; GitHub Actions only
exit_code: 0 for source hashes, format, core OA-19 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-19 job is queued by the push and is not awaited
status_change: OA-19 source slice is implemented. `kiana.observability-alert.v1`, `kiana.observability-incident.v1` and snapshot contracts bind stable rule fingerprints, severity/state, source cursor/events, alert/incident refs and bounded recovery plans. `kiana-core::project_observability_incidents` derives deduplicated projector-lag, audit/artifact loss, redaction, queue, journal, effect-unknown and orphan incidents from committed metrics/facts; reconciliation-required incidents cannot be Verified/Closed, and model/UI self-report does not trigger a rule. The bridge is read-only and does not approve, retry, close, or mutate EventLog facts.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; incident state/checkpoint and rule evaluation are process-local, no EventLog incident fact or operator triage/reconcile action exists, queue/exporter/provider live health is not fed, and source diagnostics do not prove business Incident closure
reviewer: Codex root implementation review plus OA-19 domain/core incident static-boundary review; no runtime test reviewer
```

### OA-20 data governance/retention/deletion evidence (2026-09-15)

```text
source_snapshot: 54938d4; kiana-domain/src/{governance,contracts,lib}.rs; kiana-core/src/{data_governance,lib}.rs; kiana-daemon/src/data_governance.rs; kiana-domain/tests/oa20_data_governance.rs; kiana-core/tests/oa20_data_governance_projection.rs; .github/workflows/oa20-data-governance.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-20 DataPolicy schema/epoch/digest and grant validation, DataGovernanceSnapshot payload/audit metadata separation, core committed invalidation projection, daemon policy integrity/propagation output, remote-only fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/governance.rs kiana-domain/src/contracts.rs kiana-core/src/data_governance.rs kiana-core/src/lib.rs kiana-daemon/src/data_governance.rs kiana-domain/tests/oa20_data_governance.rs kiana-core/tests/oa20_data_governance_projection.rs .github/workflows/oa20-data-governance.yml
  rg -n 'DataPolicy|data_epoch|DataGovernanceSnapshot|DataRetentionObservation|project_data_governance|revocation_requested|propagation|governance_policy_integrity_failed' kiana-domain/src kiana-core/src kiana-daemon/src kiana-domain/tests kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-domain --test oa20_data_governance --locked --offline
  cargo check -p kiana-core --test oa20_data_governance_projection --locked --offline
  cargo check -p kiana-daemon --tests --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: domain/core OA-20 fixtures; policy epoch/digest and parent cascade, expiry vs retained audit metadata, pending/committed revocation propagation to derived stores, cursor gap/duplicate, tampered policy/snapshot and raw payload absence; GitHub Actions only
exit_code: 0 for source hashes, format, domain/core/daemon OA-20 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-20 job is queued by the push and is not awaited
status_change: OA-20 source slice is implemented. Versioned/digest-bound `DataPolicy` validates purpose/retention/grant scope, increments `data_epoch` on cascading revoke, and daemon policy reads reject integrity failures. `DataGovernanceSnapshot` and `DataRetentionObservation` separate payload Available/Expired/Revoked/Unknown from retained audit metadata and map propagation across receipt/audit/artifact/memory/index/cache/export; core pending invalidation fences all derived stores as Unknown while committed revoke/expire remains queryable by source event ID.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; policy/snapshot and derived-store propagation remain process-local/source projections, no durable policy/retention checkpoint or scheduler exists, EventLog/audit facts are never physically deleted, and Artifact/Memory/Index/Telemetry purge, legal hold, cross-process restart and live deletion evidence remain open
reviewer: Codex root implementation review plus OA-20 domain/core/daemon governance static-boundary review; no runtime test reviewer
```

### CO-01 CompanyOS handoff baseline evidence (2026-09-14)

```text
source_snapshot: edf82307ccaf2cb7089de890434878b1dba68c00 (UI-00 closure); docs/roadmap/companyos-baseline.md; 13 hashed files spanning kiana-domain company/business/closeout/roles/work_packets/symposiums/packet_graph/swarm and kiana-core company/company_business/collaboration/automation/swarm
worktree_status: source snapshot was clean and pushed; baseline doc plus roadmap/status backfill is this step's commit
command_argv:
  git rev-parse HEAD
  sha256sum <13 CompanyOS files listed in baseline doc §1>
  rg -n 'company_unapproved_project_cannot_dispatch|company_approved_packet_reaches_existing_harness|milestone_acceptance_cannot_use_other_milestone' --type rust
  rg -c 'cfg(test)|#\[test\]' over company source files
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; source inspection only; no product code modified
fixture or cassette: none executed; wired/types-only classification from source reading
exit_code: source inspection=0; format check=0; local tests deliberately not run per user instruction; GitHub CI triggered by push and not awaited
status_change: CO-01 baseline completed. Wired facts: CompanyState is a fully event-sourced aggregate (17 state maps + revision CAS + idempotent replay) rebuilt from the 'company' stream; all 46 CompanyCommand variants implemented through handle_company_command with byte caps, proof checks, StartRun->spawn_from_packet and cancel sweeps; five departments and six roles verified in the catalog (the only key claim that holds as tested). RED facts: zero of 46 command variants have any test through the command path (daemon/entrypoints test files contain zero company references); both CO-01 acceptance tests are docs-only targets that do not exist as code; the CO-27 waiting loop trigger is located (RequestAcceptance requires all project runs/packets complete at company.rs:1846/1859 plus milestone start requires dependencies Accepted at :1705-1713 — these checks ARE the loop, nothing detects or breaks it); dependency-DAG cycle checks exist but an acyclic graph still deadlocks the acceptance semantics; company core/daemon files (company.rs 1242 lines, company_business.rs 718, swarm, collaboration, automation, platform, durable.rs 1016) have no cfg(test) modules. Old doc claims of "business objects do not exist" are superseded by the current implementation; recorded as such.
proof-level_change: source-only evidence; no local_behavior promotion
limitations: the two named acceptance tests are "reproduce" targets — implementations exist, tests are yet to be written and should assert zero broker/model counts (deny) and the full spawn/receipt chain (happy); CO-27 requires a design change, not just tests; the 46-command test debt is the largest single gap handed to CO-02+
reviewer: multi-agent source survey with adversarial key-claim verification (9 agents); no runtime test reviewer
```



```text
source_snapshot: 05c3b288198d53e0363d84a30acb8cca9e56edad (EXT-00 closure); docs/roadmap/ui-entrypoints-baseline.md; kiana-entrypoints/src/{cli,web,workbench,workbench_chat,harness_run}.rs; kiana-protocol/src/lib.rs; kiana-client/src/lib.rs; kiana-daemon/src/run_stream.rs; contrib/desktop/tests/
worktree_status: source snapshot was clean and pushed; baseline matrix plus roadmap/status backfill is this step's commit
command_argv:
  git rev-parse HEAD
  sha256sum kiana-entrypoints/src/cli.rs kiana-entrypoints/src/web.rs kiana-entrypoints/src/workbench.rs kiana-entrypoints/src/workbench_chat.rs kiana-entrypoints/src/harness_run.rs kiana-protocol/src/lib.rs kiana-client/src/lib.rs kiana-daemon/src/run_stream.rs
  rg -n 'ui_action_stale|claim_ui_action|advance_cursor|stream_cursor_invalid' --type rust
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; source inspection only; no product code modified
fixture or cassette: none executed; existing test bindings were established by reading test files, not running them
exit_code: source inspection=0; format check=0; local tests deliberately not run per user instruction; GitHub CI triggered by push and not awaited
status_change: UI-00 baseline completed. Four entry surfaces mapped to DaemonHost with per-lifecycle-action coverage (start/run/approval/cancel/resume/receipt/export); bare 'kiana resume' falls through to unknown-command (only product_command resume with --session-id, web /api/resume, workbench /resume reach recovery); all four surfaces route through LocalDaemonTransport -> DaemonHost::handle -> ControlPlane with no second execution loop; tui confirmed parked on the legacy SDK stream. Denial matrix verified adversarially: covered = untrusted workspace write (cli_run.rs:439 spawns the real binary), web Host/Origin exact-listener (cli_web.rs:647), unknown command command_unregistered (control_plane.rs:82); RED with implementation anchors but zero tests = stale SSE cursor (subscribe_after/advance_cursor logic exists, no test sends last-event-id), foreign-session cancel (resolve_mutable_session exists, continue has an owner test but cancel does not), lost responses surfacing result_unknown at the UI layer (branches exist: stream_closed_before_terminal / stream_terminal_missing:closed, untested), old-epoch 409 after restart (claim_ui_action -> ui_action_stale wiring exists, zero tests). No implementation status promoted for UI-01 or later.
proof-level_change: source-only evidence; no local_behavior promotion
limitations: the 4 RED items are coverage gaps, not defects — implementations exist and the named tests should pass once written; desktop coverage classified from test-file reading, its node --test execution belongs to CI; line references drift as UI-01+ lands
reviewer: multi-agent source survey with adversarial denial-coverage verification (12 agents); no runtime test reviewer
```



```text
source_snapshot: c8e9578ab9e0d8a9c299a27b7ace0e9530f8a8c4 (CM-00 closure); docs/roadmap/skills-plugins-hooks-baseline.md; docs/skills-plugins-hooks-design-research.md (hashed); kiana-skills/src/{lib,types,loader,bundled,dynamic,plugins,mcp}.rs; kiana-daemon/src/{harness_skills,extensions,pre_tool_hooks}.rs; kiana-domain/src/extensions.rs
worktree_status: source snapshot was clean and pushed; baseline gap-matrix doc plus roadmap/status backfill is this step's commit
command_argv:
  git rev-parse HEAD
  sha256sum docs/skills-plugins-hooks-design-research.md kiana-skills/src/lib.rs kiana-skills/src/types.rs kiana-skills/src/loader.rs kiana-skills/src/bundled.rs kiana-skills/src/dynamic.rs kiana-skills/src/plugins.rs kiana-skills/src/mcp.rs kiana-daemon/src/harness_skills.rs kiana-daemon/src/extensions.rs kiana-daemon/src/pre_tool_hooks.rs kiana-domain/src/extensions.rs
  rg -n 'activate_conditional_skills_for_paths|register_extension_static' --type rust
  rg -c '#\[test\]|#\[tokio::test\]' over surveyed files
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; source inspection only; no product code modified
fixture or cassette: none; documentation-only baseline with adversarially verified gap matrix
exit_code: source inspection=0; format check=0; local tests deliberately not run per user instruction; GitHub CI not required for a docs-only step but the push still triggers it without being awaited
status_change: EXT-00 baseline completed. Gap matrix records: no SkillDescriptor type (Command has no version/namespace/content-hash; Frontmatter.version parsed but never propagated); static registry has no generation/mtime invalidation (clear_caches only); dynamic.rs ACTIVATED_SKILL_NAMES write-only and activate_conditional_skills_for_paths has ZERO callers (path-gated skills never activate); plugins.rs loads plain-JSON manifests with no signature verification (parallel to ExtensionRegistry's signed path); broker register_extension_static and ExtensionHandler are dead code (non-Skill components stay staged); kiana-query stop_hooks merges without dedup (same command can run multiple times); daemon blocks UpdateInput while the query-side runner honors it (semantic split); hook timeout/nonzero branches and the replay digest guard have no daemon tests; kiana-domain extensions.rs (339 lines) has zero tests. Architecture blockers checked and clear: no second runner loop, no entry-point authorization, Prompt fields are not a capability source (allowed-tools not in policy is current fact, PromptBundle.extensions unconsumed in authorization paths).
proof-level_change: source-only evidence; no local_behavior promotion
limitations: dead-code findings are coverage/wiring facts, not defect claims; the two parallel plugin systems (plugins.rs vs ExtensionRegistry) need a convergence decision in EXT-01 before migration; line references drift as EXT-01+ lands
reviewer: multi-agent source survey with adversarial gap verification (25 agents); no runtime test reviewer
```



```text
source_snapshot: e098cc8fcb84bcb2b062f07f054bcbbe02fc1f6d (P4-J7-04 closure); docs/roadmap/context-memory-baseline.md; kiana-query/tests/context_memory_baseline.rs; the 12 hashed entry files listed in the baseline doc §1
worktree_status: source snapshot was clean and pushed; baseline tests plus baseline doc and roadmap/status backfill are this step's commit
command_argv:
  git rev-parse HEAD
  sha256sum kiana-domain/src/prompts.rs kiana-domain/src/memory.rs kiana-query/src/index.rs kiana-query/src/repo_map.rs kiana-core/src/context_query.rs kiana-daemon/src/context_query.rs kiana-daemon/src/harness_memory.rs kiana-daemon/src/memory_retrieval.rs kiana-core/src/memory_proposals.rs kiana-core/src/memory_distillation.rs kiana-runner/src/compact.rs kiana-daemon/src/data_governance.rs
  ls reference/ | sort | wc -l
  grep -c '#\[test\]|cfg(test)' over the surveyed files
  cargo check -p kiana-query --tests --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked/offline dependency resolution; source inspection and static compilation only; no test binaries executed
fixture or cassette: none executed; the two acceptance tests are source-snapshot assertions (file hashes and directory inventory), not product behavior
exit_code: source inspection=0; kiana-query tests static compile=0; format check=0; local tests deliberately not run per user instruction; GitHub CI triggered by push and not awaited
status_change: CM-00 baseline completed. Acceptance tests context_memory_baseline_is_reproducible and reference_inventory_covers_all_directories landed in kiana-query/tests/context_memory_baseline.rs pinning 12 entry-file hashes and the 72-directory reference inventory. Survey with adversarial verification (29 agents, 24 verified gap claims, all still-true) fixed: no selection-list/source-version/lifecycle in prompts.rs; no chunk provenance, total-scan caps, authorized snapshots, or index generations in kiana-query; 64-dim hardcoded hash embedding; MemoryRecord lacks typed subject/scope, sensitivity labels, purpose/expiry; revoked-source filtering is substring contains; JSONL append and capability event are two commit points; promote/reject not idempotent; content_hash is text-only sha256; compact emits (no summary available) placeholder; memory distillation has ZERO tests workspace-wide despite being product-reachable; ONNX explicitly not implemented. No implementation status promoted for CM-01 or later.
proof-level_change: source plus compile/static-check evidence only; runtime receipt of the two snapshot tests delegated to GitHub CI; no local_behavior promotion
limitations: hash-pinning tests make any intentional edit to the 12 files red until the baseline doc and table are updated together — this is by design; the 72-directory count excludes hidden .claude-flow and 4 top-level data files; survey line references are point-in-time and will drift
reviewer: multi-agent source survey with adversarial gap verification (29 agents); no runtime test reviewer
```



```text
source_snapshot: 55cac10251feebbdca7233f8e64da44b5500e394 (H01 closure); docs/roadmap/provider-baseline.md; kiana-daemon/src/model_client.rs; kiana-provider/src/{lib,config,request,transport,response}.rs; kiana-services/src/api/{provider,client,retry,streaming}.rs; kiana-runner/src/model.rs; kiana-domain/src/{model,usage}.rs; kiana-services/tests/provider_standard.rs
worktree_status: source snapshot was clean and pushed; provider baseline doc plus roadmap/status backfill is this step's documentation commit
command_argv:
  git rev-parse HEAD
  sha256sum kiana-daemon/src/model_client.rs kiana-provider/src/lib.rs kiana-provider/src/config.rs kiana-provider/src/request.rs kiana-provider/src/transport.rs kiana-provider/src/response.rs kiana-services/src/api/provider.rs kiana-services/src/api/client.rs kiana-services/src/api/retry.rs kiana-services/src/api/streaming.rs kiana-runner/src/model.rs kiana-domain/src/model.rs kiana-domain/src/usage.rs kiana-services/tests/provider_standard.rs
  rg -n 'native_streaming_without_a_terminal_event_fails_closed|provider_does_not_retry_auth_errors|provider_wrapper_maps_tool_calls_and_final_text|anthropic_native_streaming_aggregates_output_without_network' kiana-daemon/src/model_client.rs
  rg -c '#\[test\]|#\[tokio::test\]' kiana-provider/src
  cargo check --workspace --tests --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked/offline dependency resolution; source inspection and static compilation only; no test binaries executed
fixture or cassette: none executed; the five named acceptance tests were classified by source reading as legacy_fixtures-only or mixed (see baseline doc §4)
exit_code: source inspection=0; workspace static compile=0; format check=0; local tests deliberately not run per user instruction; GitHub CI triggered by push and not awaited
status_change: P4-J7-04 baseline completed. Key facts fixed: kiana-daemon model_client.rs product code is lines 1-39 only (ProviderGateway + ScriptedModel); lines 42-2102 are cfg(test) legacy_fixtures never linked into product builds; kiana-provider is the sole networked product path (5 protocols, native streaming) with ZERO tests — recorded as RED for J7-05; research-doc gap table re-verified: 7 of 10 gaps already fixed by kiana-provider (profile routing, strict terminal validation, tool JSON rejection, transport limits, typed retry classification with TLS never-retry, first-class ModelFinish, final wire budget), 3 partially hold (Text-only delta, two-field usage without cache/reasoning tokens and price version, missing failure-attempt usage); kiana-services legacy stack remains reachable only via model smoke --live diagnostics and bridge/remote compat surfaces. No implementation status promoted for P4-J7-05 or later.
proof-level_change: source plus compile/static-check evidence only; no local_behavior promotion
limitations: zero-test finding for kiana-provider is a coverage gap statement, not a defect claim; legacy fixture tests still exercise the kiana-services stack that --live diagnostics use; the 2026-09-09 live DeepSeek evidence does not extend to the kiana-provider crate; hashes will drift as J7-05+ lands
reviewer: multi-agent source survey with adversarial gap verification (15 agents); no runtime test reviewer
```



```text
source_snapshot: 8709b71a54549fd7fa894904b55a4d501e68148a (fixture commit); docs/roadmap/harness-baseline.md; kiana-runner/tests/harness_contract.rs; kiana-daemon/tests/harness_runtime.rs; call-chain files hashed in the baseline doc
worktree_status: fixture commit 8709b71 was clean and pushed; CI-blocking repairs landed first (6b18a49 hash assertions + provider metadata; 8c8f3d7 fmt/unused-import); assertion fix plus baseline docs are this step's commit
command_argv:
  git rev-parse HEAD
  git status --short
  sha256sum kiana-entrypoints/src/cli.rs kiana-entrypoints/src/harness_run.rs kiana-daemon/src/lib.rs kiana-core/src/lib.rs kiana-core/src/lifecycle.rs kiana-core/src/capabilities.rs kiana-core/src/dispatch.rs kiana-core/src/history.rs kiana-runner/src/harness.rs kiana-runner/src/tools.rs kiana-runner-protocol/src/lib.rs kiana-ports/src/model.rs kiana-capability-broker/src/lib.rs kiana-daemon/src/model_client.rs
  grep -rn '"run.tool_result"' --include='*.rs' kiana-core/src kiana-daemon/src kiana-runner/src
  grep -rn 'capability.completed' kiana-core/src/capabilities.rs kiana-core/src/history.rs
  cargo check --workspace --tests --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked/offline dependency resolution; source inspection and static compilation only; no test binaries executed
fixture or cassette: CountingModel/BlockingModel/EventWriteFault/InjectedClock runner fixtures and CountingModel/CountingBroker daemon fixtures committed in 8709b71; roundtrip assertion corrected from run.tool_result to capability.completed this round; no fixture executed locally
exit_code: source inspection=0; workspace tests static compile=0; format check=0; local tests deliberately not run per user instruction; GitHub CI triggered by push and not awaited
status_change: H01 completed. Six-segment call-chain reconciliation recorded in docs/roadmap/harness-baseline.md with source hashes; acceptance skeleton tests registered; key fact boundaries fixed: successful tool outcomes are journaled as capability.completed with capability_request_id linkage while run.tool_result is cancel/not-executed-only; run terminal vocabulary is completed/failed/cancelled/result_unknown; initial trust decision is daemon-side; harness.rs:703 emitted_delta dead code flagged for H05/H06; ModelClient admission contract (prepare_call/complete_prepared/complete_admitted) located in kiana-ports. No implementation status promoted for H02 or later.
proof-level_change: source plus compile/static-check evidence only; runtime behavior of the new tests is delegated to GitHub CI; no local_behavior promotion claimed in this commit
limitations: runtime receipt for untrusted-deny and roundtrip tests arrives via Release Smoke CI, not locally; the six CI-failing pushes before 6b18a49 were caused by pre-existing baseline issues (stale 16-char hash assertions, missing provider repository metadata), repaired outside H01 scope to unblock the gate; call-chain hashes will drift as H02+ lands and must be re-verified per step
reviewer: multi-agent source survey with adversarial gap verification (37 agents); no runtime test reviewer
```



```text
source_snapshot: f366436a0a232c2a9a31b3ab2968da4a6824aa90; docs/roadmap/control-plane-entry-matrix.md; kiana-daemon/src/lib.rs; kiana-core/src/{lifecycle,approvals,capabilities,commands,company,sessions}.rs; kiana-core/tests/control_plane.rs; kiana-daemon/tests/daemon_host.rs
worktree_status: baseline source snapshot was clean and pushed; matrix plus roadmap/status backfill is the follow-up documentation commit for this step
command_argv:
  git rev-parse HEAD
  git status --short
  rg -c '^async fn ' kiana-core/tests/control_plane.rs kiana-daemon/tests/daemon_host.rs kiana-client/tests/client_methods.rs
  rg -n 'entry route and failure fixture symbols' kiana-entrypoints/src kiana-client/src kiana-core/src kiana-daemon/src
  cargo check -p kiana-core --tests --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; rustc/cargo stable; locked offline dependency cache; source inspection and static compilation only; no test binaries executed
fixture or cassette: source-indexed direct capability, Harness, approval-resume, Company, context query, broker registration, hook and cancellation paths; shared three-entry fault-injection fixture is explicitly handed to CP-04/05 and was not run in this baseline step
exit_code: source inspection=0; static compile/format/diff checks=0; local tests deliberately not run per user instruction; GitHub CI triggered by push and not awaited
status_change: CP-00 baseline completed. The matrix records all public DaemonHost request routes, the three capability entry paths, broker registrations, failure classes, precise source-index test counts (core 108, daemon 74, client 2 async tests), and the remaining direct/Harness/approval convergence gaps. No implementation status was promoted for CP-04/05.
proof-level_change: source plus compile/static-check evidence only; no local_behavior promotion
limitations: runtime denial/zero-handler-count proof for one shared three-entry fixture remains pending in CP-04/05; direct and Run-bound approval paths retain distinct continuation/event semantics; full durable authority/transaction/fencing work remains open
reviewer: Codex root source review; no runtime test reviewer
```

### ER-00 Event/Receipt/Recovery fact-boundary baseline evidence (2026-09-14)

```text
source_snapshot: 0a29de510c240584a972dad6ef14c4ca6a0dfced; docs/roadmap/event-receipt-recovery-baseline.md; docs/module-map.md; kiana-domain/src/{journal,states}.rs; kiana-ports/src/lib.rs; kiana-eventlog/src/{journal_core,jsonl,memory}.rs; kiana-core/src/{events,history,projection,receipts,recovery}.rs; kiana-daemon/src/{approval_store,journal_approvals,run_stream}.rs; kiana-query/src/index.rs
worktree_status: source snapshot was clean and pushed before this documentation follow-up; baseline matrix, module-map, roadmap and status backfill are the step documentation commit
command_argv:
  git rev-parse HEAD
  git status --short
  sha256sum kiana-eventlog/src/{lib,event_store_core,journal_core,jsonl,memory}.rs kiana-domain/src/{journal,states}.rs kiana-core/src/{events,receipts,projection,recovery,history}.rs kiana-daemon/src/{approval_store,journal_approvals,run_stream}.rs kiana-query/src/index.rs
  rg -o --no-filename '"(?:request|run|approval|capability|execution|invocation|model|resource|workspace|memory|hook|mcp|swarm|workflow|company|connector|extension|session|data|result|cell|packet|command|process|provider|usage|failure|review|closing|feedback|version|trace|golden|human|authority|budget|lease|artifact|reconciliation|recovery)\.[A-Za-z0-9_.-]+"' kiana-core/src kiana-daemon/src kiana-eventlog/src kiana-domain/src | tr -d '"' | sort -u | wc -l
  rg -c '^async fn ' kiana-core/tests/control_plane.rs kiana-daemon/tests/daemon_host.rs kiana-client/tests/client_methods.rs
  cargo check -p kiana-eventlog --tests --locked --offline
  cargo check -p kiana-core --tests --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked/offline dependency resolution; source inspection and static compilation only; no test binaries executed
fixture or cassette: source-indexed run/tool/approval/unknown ID chain; EventStore Memory/JSONL capability matrix; cache-vs-fact matrix; source-indexed denial, read-failure, missing-terminal and restart receipt assertions; no runtime fixture executed locally
exit_code: source inspection=0; eventlog/core static checks=0; format/diff checks=0; local tests deliberately not run per user instruction; GitHub CI triggered by push and not awaited
status_change: ER-00 fact-boundary baseline completed. Snapshot, hashes, EventStore capabilities, fact ownership, command/event/effect/receipt boundaries, minimum correlation chain, cache distinction and failure classes are recorded. No implementation status promoted for ER-01 or later.
proof-level_change: source plus compile/static-check evidence only; no local_behavior, durable, live or physical promotion
limitations: event-literal count includes action/schema compatibility strings and is not a registry; some legacy events lack complete causation/typed-turn metadata; Memory is non-durable; JSONL capability flags are source declarations rather than runtime durability proof; external effect exactly-once and reconciliation remain later ER steps; CI runtime evidence was intentionally not awaited
reviewer: Codex root source review; no runtime test reviewer
```

### CAP-00 Capability execution baseline evidence (2026-09-14)

```text
source_snapshot: b49cd62772943aa117c8ea4adec383580f739237; docs/roadmap/capability-baseline.md
worktree_status: source snapshot was clean and pushed before this documentation follow-up; baseline, module-map, roadmap and status changes are this step's documentation commit
command_argv:
  git rev-parse HEAD
  git status --short
  sha256sum kiana-domain/src/{tool_catalog,actions,capabilities}.rs kiana-runner/src/tools.rs kiana-core/src/{capabilities,events,sessions}.rs kiana-capability-broker/src/lib.rs kiana-daemon/src/{harness_sandbox,harness_capabilities,apply_patch,harness_mcp,mcp_stdio,pre_tool_hooks,harness_memory}.rs
  rg -n 'five model tools|CapabilityBrokerPort|ExecutionPermitVerifierPort|catalog_sealed|timeout|result_unknown' kiana-domain/src kiana-runner/src kiana-core/src kiana-capability-broker/src kiana-daemon/src
  cargo check -p kiana-capability-broker --tests --locked --offline
  cargo check -p kiana-runner --tests --locked --offline
  cargo check -p kiana-daemon --tests --locked --offline
  cargo check -p kiana-core --tests --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux/bash; stable Rust toolchain; locked offline dependency cache; no local tests or test binaries executed
fixture or cassette: source-indexed registry/scope/cancel/patch/MCP/memory paths and named CI assertions; no runtime fixture executed locally
exit_code: all listed static checks and source inspection expected 0; local tests deliberately not run per user instruction; GitHub CI triggered by push and not awaited
status_change: CAP-00 capability baseline artifact completed (`feature_status=implemented`, `proof_level=source`); product capabilities remain independently `partial`, `target`, `deferred` or `not_supported`
proof-level_change: source plus static-check evidence only; no local_behavior, durable, live or physical promotion
limitations: no unified prepare/authorize/dispatch/finalize contract, durable dispatch CAS, common execution scope, stop confirmation, patch recovery, MCP drift/HTTP transport, or memory blocking-write cancellation is claimed; five model tools remain the current compatibility boundary
reviewer: Codex root source review; no runtime test reviewer
```

### Run state event projection evidence (2026-09-10)

```text
source_snapshot: 3a319be (kiana-core/src/projection.rs, kiana-core/src/lib.rs, kiana-core/tests/control_plane.rs)
worktree_status: committed on master and pushed to origin
command_argv:
  bash scripts/release-smoke.sh        # GitHub Actions release-smoke job, run 34500579350 completed/success
  cargo test -p kiana-core --test control_plane run_state --locked --offline -- --test-threads=1
cwd/environment: GitHub Actions release-smoke job (ubuntu-latest, stable, bubblewrap); local reproduction with rustc/cargo 1.97.1 and --locked --offline
fixture or cassette: in-memory scripted runner over MemoryEventLog; hand-written event sequences with duplicate and conflicting terminal kinds
exit_code: 0 (run 34500579350 completed/success); the acceptance test `new_process_rebuilds_run_state_from_events_alone` passes in that run
status change: no capability status promotion. `kiana-core/src/projection.rs` folds `run.*` / `approval.*` events into a read-only `RunState` ordered by run-level stream version (falling back to sequence with request-scoped deduplication, the same convention as the history fold); two distinct terminal kinds for one run fail closed with `run_terminal_conflict:<kinds>`, repeated identical terminals are idempotent, and events after the terminal are ignored.
proof-level change: local_behavior evidence for rebuilding run state from the ledger alone.
limitations: the projection is a new read path only — the in-memory maps are unchanged and not yet degraded to write-through caches; invocation-level projection is deferred; conflicting terminals are reported as an error because the ledger holds no reconciliation authority, so callers receive `run_terminal_conflict` rather than a guessed outcome.
reviewer: Claude reviewed the Codex diff (no assertions deleted), ran the focused tests locally before pushing, and verified the acceptance test in the green run
```

### Model-visible history rebuild and pre-dispatch pairing evidence (2026-09-10)

```text
source_snapshot: 41bb971 (kiana-core/src/history.rs, kiana-core/src/lib.rs) + 32692da/9d255d1 (kiana-core/src/capabilities.rs, kiana-core/tests/control_plane.rs) + d3f7601 (kiana-domain/src/contracts.rs)
worktree_status: committed on master and pushed to origin
command_argv:
  bash scripts/release-smoke.sh        # GitHub Actions release-smoke job, run 34386563396 completed/success
  cargo test -p kiana-core --test control_plane pre_dispatch_failure_still_pairs_with_its_tool_call_in_the_ledger --locked --offline
  cargo test -p kiana-domain --locked --offline
cwd/environment: GitHub Actions release-smoke job (ubuntu-latest, stable, bubblewrap); local reproduction with rustc/cargo 1.97.1 and --locked --offline
fixture or cassette: in-memory scripted runner over MemoryEventLog; FailingBroker hitting the dispatch-error branch; kiana-domain ID contract table with per-type round-trip tests
exit_code: 0 (run 34386563396 completed/success); the three acceptance tests `resume_rebuilds_model_visible_history_from_ledger`, `pre_dispatch_failure_still_pairs_with_its_tool_call_in_the_ledger`, and `contracts::tests::every_public_type_has_one_owner_and_a_conversion_test` all pass in that run
status change: no capability status promotion. (1) `ControlPlane::model_visible_history` folds `run.prompt` / `run.delta` / `capability.completed|failed` into `Vec<ConversationMessage>` with request-scoped deduplication, pairing tool results to their `call_id` via `capability_request_id`; no new event kind was added because the capability result already records the fact. (2) The three pre-dispatch `capability.failed` sites (hook error, cell admission, dispatch error) now carry `capability_request_id` so failed results still pair. (3) `kiana-domain` registers every canonical ID (18 uuid types + SessionId + WorkFingerprint) in one `ID_CONTRACTS` table with uniqueness assertions against the real workspace manifest.
proof-level change: local_behavior evidence for history rebuild, failure pairing, and the ID contract registry.
limitations: history rebuild is read-only and no resume entry exists yet; unknown event kinds are skipped, not interpreted; the registry asserts uniqueness and shape, it does not yet cover non-ID schemas or unknown-field/migration rules; run-level deduplication falls back to sequence-wide first-wins only for legacy events without stream metadata.
reviewer: Claude reviewed the Codex diffs (no assertions deleted); the first hand-written pairing test reached the wrong failure site (cell admission instead of dispatch error) and was corrected after CI caught it; by the product owner's instruction the full suite ran only on GitHub CI
```

### Read-only persisted web session history evidence (2026-09-10)

```text
source_snapshot: 1c504a0 (kiana-entrypoints/src/web.rs, kiana-entrypoints/src/web_page.html; read path added by 0a9a56b in kiana-core/src/receipts.rs and kiana-daemon/src/lib.rs)
worktree_status: committed on master and pushed to origin
command_argv:
  bash scripts/release-smoke.sh        # GitHub Actions release-smoke job
  cargo check -p kiana-entrypoints --all-targets --locked --offline
cwd/environment: GitHub Actions release-smoke job, ubuntu-latest, stable toolchain, bubblewrap installed
fixture or cassette: a real DaemonHost::local() run writes the ledger, the app and host are dropped, a fresh host and WebApp read it back
exit_code: 0 (run 34380076942 completed/success)
status change: no capability status promotion. A WebApp constructed over an existing ledger lists previously recorded sessions read-only; opening one performs no execution, appends no events and cannot mutate. The projection reads through `DaemonHost::persisted_events()` and never parses the ledger file itself.
proof-level change: local_behavior evidence for the read-only history projection and its fail-closed paths.
limitations: the listing is read-only and does not resume or re-run a session; `Ok(None)` (a store without full reads) returns `web_session_history_unsupported` rather than an empty list; the first version of this slice parsed the JSONL file inside the entrypoint and was corrected before commit.
reviewer: Claude reviewed each Codex diff (no assertions deleted) and required the ledger read to move behind the daemon port; the full suite ran only on GitHub CI
```

### Tool argument validation at capability mapping evidence (2026-09-10)

```text
source_snapshot: 9095ea7 (kiana-runner/src/tools.rs, kiana-runner/src/harness.rs)
worktree_status: committed on master and pushed to origin
command_argv:
  bash scripts/release-smoke.sh
  cargo test -p kiana-runner --locked --offline
cwd/environment: GitHub Actions release-smoke job; local reproduction with rustc/cargo 1.97.1 and --locked --offline
fixture or cassette: scripted model emitting malformed tool arguments; existing cassette shapes with extra keys
exit_code: 0 (run 34380323510 completed/success); kiana-runner 50 unit + 2 integration
status change: no capability status promotion. `capability_for_tool` now validates the model's arguments against the existing schema table before building a capability request, and a malformed call fails with `invalid_arguments:<tool>:<field>` before any `CapabilityRequested` is emitted.
proof-level change: local_behavior evidence for the mapping-time rejection path.
limitations: the validator implements only the `type` / `required` / `minimum` / `enum` subset and is driven by the existing `tool_schemas()` table; `additionalProperties` is deliberately not denied, so undeclared keys still pass; the alias table added by this slice (`schema_name_for_tool`) duplicates the one in `capability_for_tool`, which the registry unit must collapse.
reviewer: Claude reviewed the Codex diff (no assertions deleted) and compiled before pushing; the full suite ran only on GitHub CI
```

### Run-level wall-time budget evidence (2026-09-10)

```text
source_snapshot: 409cfc7 (kiana-runner/src/harness.rs) with the product wiring in 747ff8b (kiana-daemon/src/lib.rs)
worktree_status: committed on master and pushed to origin
command_argv:
  bash scripts/release-smoke.sh
  cargo test -p kiana-runner --locked --offline
cwd/environment: GitHub Actions release-smoke job; local reproduction with rustc/cargo 1.97.1 and --locked --offline
fixture or cassette: scripted model with a step between budget checks; env-driven config lookup in unit tests
exit_code: 0 (run 34380542728 completed/success); kiana-runner 54 unit + 2 integration
status change: no capability status promotion. `RuntimeConfig` gained `wall_time_budget`; exceeding it between model steps fails closed with `run_budget_exceeded:wall_time`; `continue_run` resets the clock.
proof-level change: local_behavior evidence for the run-level time bound.
limitations: the budget defaults to `None`, so the product path is unbounded unless `KIANA_HARNESS_WALL_TIME_MS` is set; `max_steps_per_turn` is still 32 on every product path and `RoleSpec.max_steps` remains unused, so the bounded-loop unit is only partially landed.
reviewer: Claude reviewed the Codex diff (no assertions deleted) and compiled before pushing; the full suite ran only on GitHub CI
```

### Ledger prompt and tool-call identity evidence (2026-09-10)

```text
source_snapshot: 40420bd (kiana-core/src/lifecycle.rs, kiana-core/src/capabilities.rs, kiana-core/tests/control_plane.rs)
worktree_status: committed on master and pushed to origin
command_argv:
  bash scripts/release-smoke.sh        # GitHub Actions release-smoke job
  cargo test -p kiana-core --locked --offline -- --test-threads=1
  cargo check --workspace --locked --offline
cwd/environment: GitHub Actions release-smoke job, ubuntu-latest, stable toolchain, bubblewrap installed; local reproduction with rustc/cargo 1.97.1 and --locked --offline
fixture or cassette: in-memory scripted runner over MemoryEventLog; sentinel secrets embedded in the prompt text
exit_code: 0 (run 34378214555 completed/success); kiana-core 11 + 92 + 2
status change: no capability status promotion. `run.prompt` is now recorded for start and continue with the prompt redacted at the event boundary, and `run.tool_call` records the model's tool-call identity (`call_id`, capability class, broker operation) next to the capability decision, so the ledger carries the fields needed to rebuild model-visible history.
proof-level change: local_behavior evidence for the two new event kinds and their redaction.
limitations: the ledger still cannot reconstruct history or resume a run; `call_id` is read from the capability request arguments and is `null` when the runner supplied none; every new event adds an append, so the per-turn `run.delta` aggregation recorded in the earlier evidence block must keep holding (the granularity test still passes).
reviewer: Claude reviewed the Codex diff (no assertions deleted) and compiled the slice before pushing; by the product owner's instruction the full suite ran only on GitHub CI
```

### Streaming default, SSE reconnect, tool-call repetition and ledger session rebuild evidence (2026-09-10)

```text
source_snapshot: af38d47 (kiana-entrypoints/src/{cli,web}.rs, kiana-entrypoints/src/web_page.html, kiana-entrypoints/tests/{cli_run,cli_help,cli_web}.rs, kiana-runner/src/harness.rs, kiana-core/src/{lifecycle,receipts,sessions}.rs, kiana-core/tests/control_plane.rs)
worktree_status: clean; all four slices committed on master and pushed
command_argv:
  bash scripts/release-smoke.sh        # GitHub Actions release-smoke job, ubuntu-latest, stable toolchain, bubblewrap installed
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1
  cargo test -p kiana-core --locked --offline
  cargo test -p kiana-runner --locked --offline
cwd/environment: GitHub Actions release-smoke job; slices also reproduced locally with rustc/cargo 1.97.1 and --locked --offline
fixture or cassette: cassette scripts with chunked text plus shell tool calls; in-process ChunkedModel; disk JSONL EventLog; mock Anthropic endpoints in cli.rs and cli_session.rs that answer SSE when the request carries stream=true
exit_code: 0 (run 34376675138 completed/success); kiana-core 11 + 91 + 2, kiana-runner 43, kiana-entrypoints lib 444
status change: no capability status promotion. Four slices landed: (1) `kiana run` streams by default with `--no-stream` / `--json` as the kill switch, and `--stream` with an exclusive mode still returns `stream_requires_run`; (2) an SSE subscription attached to a run already in progress emits `stream_gap` and the page never marks an incomplete turn complete; (3) consecutive identical tool calls fail closed with `repeated_tool_call:<name>`; (4) session→run bindings are rebuilt from `run.authorized` in the ledger when the in-memory cache misses, and still pass `same_session_principal`.
proof-level change: local_behavior evidence for these four paths, verified by the repository's own release-smoke gate on GitHub.
limitations: the CLI default flip first failed `run_without_stream_keeps_final_only_human_output` on CI (run 34373627747) and was corrected in d704add, where the test now states its path explicitly and a second test covers the default; `make test-fast` does not cover the entrypoints test targets, so this class of regression is only caught by CI. The ledger rebuild reads the whole event stream (`read_all`) on a cache miss and does not yet reconstruct model-visible history or pending work; no cross-process resume, no run-level cost/wall-time budget, and no `sequence`/`epoch` on the wire.
reviewer: Claude reviewed each Codex diff for deleted assertions (zero deletions in the test files) and compiled every slice before pushing; by the product owner's instruction the full test suite runs only on GitHub CI
```

### Incomplete provider stream fail-closed and CI hang correction evidence (2026-09-09)

```text
source_snapshot: 3b65af2 (kiana-daemon/src/model_client.rs, kiana-entrypoints/src/cli.rs, kiana-entrypoints/tests/cli_session.rs)
worktree_status: committed on master and pushed to origin
command_argv:
  cargo test -p kiana-daemon --lib native_streaming --locked --offline
  KIANA_STREAMING=off target/debug/deps/kiana_entrypoints-* --test-threads=1 --exact cli::tests::bridge_stream_json_loop_round_trips_permission_prompt_and_result_event
  cargo test -p kiana-entrypoints --lib bridge_stream_json_loop_round_trips_permission_prompt_and_result_event --locked --offline
  cargo test -p kiana-entrypoints --lib --locked --offline
  cargo check -p kiana-entrypoints --all-targets --locked --offline
  strace -f -e trace=network,futex,read,write target/debug/deps/kiana_entrypoints-* --exact cli::tests::bridge_stream_json_loop_round_trips_permission_prompt_and_result_event
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; --locked --offline; local sandbox
fixture or cassette: EventProvider truncating the stream after message_start; the four cli.rs mock model servers and the cli_session mock now answer Anthropic SSE when the request carries stream=true
exit_code: 0 for every listed command; daemon lib 84 passed; the bridge test passes in 0.04s with KIANA_STREAMING=off and with the SSE mock; entrypoints lib 444 passed in ~165s; check clean
status change: no capability status promotion. A provider stream that ends without a terminal event now returns the machine-readable `provider_stream_incomplete` instead of Ok(empty output), which is the fail-closed behavior docs/streaming-unfreeze-plan.md §6 requires.
proof-level change: local_behavior negative evidence for the incomplete-stream path. Root cause of the nine-hour CI stall is recorded: since 3b6f31a the harness calls complete_streaming, the SDK/bridge mocks answered non-SSE JSON, the SSE parser produced zero events, and the bridge test then waited forever for a permission request that could never arrive.
limitations: the full entrypoints integration package was not re-run locally after the cli_session mock fix (the local run before that fix showed 512 passed / 1 failed); verification of that package is delegated to the GitHub release-smoke run. The SDK/bridge surface now streams by default because it shares the DaemonHost spine; no disconnect/reconnect, quota, or cross-process transport evidence was added. `make test-fast` does not cover the entrypoints test targets, which is why this regression reached master.
reviewer: Claude-run root-cause analysis (strace futex wait plus CI log comparison) and TDD RED/GREEN observation; no independent reviewer
```

### Streamed delta ledger granularity correction evidence (2026-09-09)

```text
source_snapshot: e9df8b4 (kiana-core/src/lifecycle.rs, kiana-core/tests/control_plane.rs, kiana-daemon/tests/daemon_host.rs)
worktree_status: committed on master and pushed to origin
command_argv:
  cargo test -p kiana-core --locked --offline
  cargo test -p kiana-core --test control_plane stream_deltas --locked --offline
  cargo test -p kiana-daemon --test daemon_host streamed_run_writes_one_run_delta_per_turn_to_the_ledger --locked --offline
  cargo test -p kiana-runner --locked --offline
  KIANA_HOME=<repo>/target/test-home cargo test -p kiana-daemon --locked --offline --no-fail-fast
  cargo test -p kiana-entrypoints --test cli_run --locked --offline
  KIANA_HOME=<repo>/target/test-home make test-fast
  cargo fmt --all --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; --locked --offline; daemon tests serialized where the sandbox requires it
fixture or cassette: ChunkedDeltaRunner emitting contiguous chunks, TwoTurnDeltaRunner splitting delta runs with a capability request, daemon ChunkedModel over a disk JSONL EventLog; no cassette or protocol schema change
exit_code: 0 for every listed command; kiana-core 101 passed; kiana-daemon 84 lib + 13 control_plane + 67 daemon_host; kiana-runner 39; cli_run 33; make test-fast 31 targets / 411 passed; fmt clean
status change: no capability status promotion. ControlPlane::drive_run now records one `run.delta` per model turn instead of one per provider chunk, restoring the coarse-grained ledger docs/streaming-unfreeze-plan.md §5 requires.
proof-level change: local_behavior evidence for durable-ledger granularity. Before the fix a two-run live web smoke wrote 129 `run.delta` events (target/live-smoke/web plus the session events.jsonl of /tmp/kiana-live-web), and every append paid a full EventLog read_stream plus CAS.
limitations: the aggregate is one event per contiguous delta run, not a fixed flush interval; both new tests were observed failing against 5ecc4a3 before the fix (3 events vs 1; ["first"," turn","second"," turn"] vs ["first turn","second turn"]); no timing measurement of the append-cost reduction was taken
reviewer: Claude-run TDD cycle with RED/GREEN observation; no independent reviewer
```

### Protocol additive run-stream subscription evidence (2026-09-09)

```text
source_snapshot: 3b6f31ae3584692fbcf656e310f0a41398db4fee + uncommitted run-stream WIP
worktree_status: dirty checkout; modified kiana-protocol/src/lib.rs, kiana-protocol/tests/wire_contracts.rs, kiana-daemon/src/lib.rs, kiana-daemon/tests/daemon_host.rs; new untracked kiana-daemon/src/run_stream.rs; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo test -p kiana-protocol --locked --offline
  cargo test -p kiana-daemon --lib run_stream --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host run_subscription_receives_ordered_deltas_and_terminal_response --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host run_without_subscription_keeps_the_complete_response_path --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-protocol -p kiana-daemon --locked --offline
  cargo test -p kiana-protocol -p kiana-daemon --locked --offline -- --test-threads=1
  cargo check --workspace --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo dependency resolution; daemon tests serialized; sandbox forbids bwrap bind/netlink and writes to HOME-level KIANA_HOME
fixture or cassette: protocol RunStreamEnvelope serde fixture with unknown future event; chunked in-process model emitting alpha/beta/gamma; no-subscriber counting RunnerPort fixture; existing daemon control-plane fixtures; no cassette schema change
exit_code: fmt=0; fmt check=0; kiana-protocol=0 (10 unit + 3 wire-contract tests); daemon run_stream lib=0 (2/2); focused daemon_host subscription=0 (1/1); focused daemon_host no-subscription=0 (1/1); daemon control_plane=0 (13/13); requested package test=101 in both parallel and serial runs (24 daemon lib failures: 14 apply_patch_lock_unavailable/HOME-lock fixtures, 10 bwrap/loopback-bind permission failures); workspace check=0; git diff check=0
status change: additive RunStreamEnvelope/RunStreamEvent and DaemonHost::subscribe_run(run_id) are implemented; subscribers receive ordered Delta events and a Terminal ResponseEnvelope; no subscriber keeps the original RunnerPort::send path. Token streaming remains not_supported overall because CLI/TUI/Web, live provider, and cross-process transport are not wired.
proof-level change: local_behavior evidence for the in-process protocol projection and subscription fan-out; no durable/live/physical promotion and no claim that the existing sandbox-dependent daemon tests pass in this sandbox.
limitations: subscription is process-local and must be established before the run starts to observe deltas; no late-subscriber replay; no cross-process transport or reconnect; terminal is emitted only when the final ResponseEnvelope exposes a parseable run_id and terminal status; real-time delta redaction/backpressure/backpressure accounting remain for the later negative-path slices; the requested package test command is environment-blocked by bwrap/network/HOME permissions, not by a failing run-stream assertion; daemon_host full serial run was interrupted after the sandbox-dependent hang
reviewer: focused protocol additive compatibility, subscription ordering, terminal receipt projection, and no-subscriber path review; no second execution loop, ledger granularity change, cassette schema change, UI change, or frozen-path change
```

### P1-03 in-flight stream cancellation evidence (2026-09-09)

```text
source_snapshot: dirty checkout + uncommitted run-stream WIP; this slice changes kiana-runner/src/harness.rs, kiana-core/src/{events,lifecycle,capabilities,approvals,commands,lib}.rs, and kiana-daemon/tests/daemon_host.rs
worktree_status: dirty checkout; existing user/WIP changes preserved; no commit/reset/push/merge/worktree deletion performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-runner --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host cancelling_mid_stream_never_completes_or_emits_a_late_delta --locked --offline -- --test-threads=1
  cargo test -p kiana-runner -p kiana-daemon --locked --offline
  cargo clippy -p kiana-runner -p kiana-core -p kiana-daemon --all-targets --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; CARGO_BUILD_JOBS=2 and -j 2; locked/offline dependency resolution; daemon/core tests serialized; default sandbox forbids bwrap bind/loopback and HOME-level KIANA_HOME writes
fixture or cassette: in-process HoldMidStreamModel emits `before`, waits, then attempts `after`; run-stream subscriber; disk JSONL EventLog; runner HoldingModel unit fixture; no protocol, cassette-schema, or UI change
exit_code: fmt check=0; runner=0 (37 unit + 2 malicious-model tests); core control_plane=0 (86/86); focused daemon_host cancellation=0 (1/1); clippy=0 with existing non-fatal workspace warnings; diff check=0; requested `cargo test -p kiana-runner -p kiana-daemon --locked --offline`=101 in the default sandbox (kiana-daemon lib 59 passed/24 failed: 15 apply_patch_lock_unavailable due HOME-level lock writes, 9 bwrap loopback/NETLINK or local-socket EPERM). With KIANA_HOME set to a workspace path and --no-fail-fast --test-threads=1: runner=0, daemon control_plane=13/13, daemon_host=62/66, daemon lib=74/83; remaining 9 lib + 4 integration failures are the same bwrap/loopback/EPERM or HOME/trust environment fixtures
status change: P1-03 remains partial; KianaHarness now registers an in-flight cancellation token before the model await, Cancel addresses that token, stream callbacks reject deltas after cancellation, returned model output/tool calls are discarded, and the active run-driver terminal scope permits only the first terminal event. Event append now retries bounded CAS/payload-key contention without weakening idempotent replay.
proof-level change: local_behavior negative evidence for mid-stream cancellation: no `run.completed`, no late `after` delta in stream/response/event log, one `run.cancelled` terminal, and no late model delta; runner unit regression covers the same owner-crate boundary. No durable/live/physical promotion and no claim that token streaming is generally supported.
limitations: cancellation state and terminal scopes are process-local; there is no durable cancel epoch, cross-process runner recovery, or restart reconciliation; the exact requested package command remains environment-red in this sandbox because bwrap cannot create NETLINK_ROUTE/loopback and default HOME/KIANA_HOME lock paths are unwritable. The daemon_host trust-fixture failure under workspace KIANA_HOME is likewise an environment/HOME artifact, not a cancellation assertion failure.
reviewer: focused implementation plus core/runner/daemon regression review; no assertion was weakened, skipped, or deleted, and no second execution loop, protocol, cassette-schema, or UI change was made
```

## 3. Gate 0 当前结果

Gate 0 的历史证据块保留其当时结论；当前快照在 MCP 参数边界、completion-marker 与 JSONL 尾记录修复后重新验证为绿。S0 证据门已通过，但这不提升 S1-S4 的状态，也不把本地证据写成 durable/live/physical。

### Live provider and token streaming evidence via DeepSeek (2026-09-09)

```text
source_snapshot: 4a479a4 + uncommitted live-smoke tool-name mapping fix in kiana-daemon/src/model_client.rs
worktree_status: dirty at capture time; the wire tool-name mapping fix and this evidence block land in the same commit
command_argv:
  kiana run --stream --sandbox read-only -- "用 shell 工具跑 pwd，然后告诉我当前目录"
  kiana run --receipt <session_id> --json
  kiana run --stream --sandbox read-only -- "从 1 数到 20，每个数字之间用逗号隔开"   # 每读到一个字节打时间戳
cwd/environment: temp project /tmp/kiana-live3 + KIANA_HOME /tmp/kiana-home3; Linux x86_64; rustc 1.97.1; KIANA_PROVIDER=anthropic; ANTHROPIC_BASE_URL=https://api.deepseek.com/anthropic; ANTHROPIC_MODEL=deepseek-v4-flash
fixture or cassette: none (real provider); artifacts under target/live-smoke/
exit_code: 0 for both runs; receipt status=completed
artifact paths and SHA-256: target/live-smoke/{environment.txt,run-tool.txt,receipt.json,stream-timing.txt}; no SHA-256 generated
status change: live provider not_supported -> partial; token streaming not_supported -> partial. Evidence: a real DeepSeek call returned "1+1等于2。"; a tool-using run called shell.exec through the broker and answered with the real cwd; the receipt records model_id=deepseek-v4-flash, steps=2, stop_reason=end_turn, usage{input_tokens=30, output_tokens=13}; per-byte arrival timestamps in stream-timing.txt show incremental delivery (~9ms apart), not a single buffered flush
proof-level change: live for this exercised path (real external provider + brokered tool call + receipt projection). No durable claim
limitations: one provider (DeepSeek via its Anthropic-compatible endpoint) and one model; no native OpenAI/Anthropic credentials exercised; no disconnect/reconnect, quota/backpressure, or cross-process transport evidence; workbench and web were not run against a real provider; tool names containing dots require the new wire mapping (memory.search -> memory_search) and that mapping is currently a two-entry table, not a general scheme
reviewer: Claude-run live smoke with captured artifacts; no independent reviewer
```

### CompanyOS spec alignment and design-gap fill (2026-09-08)

```text
source_snapshot: dirty checkout at 842e5a4 + uncommitted CompanyOS WIP; this slice edits documentation only (docs/, CURRENT_STATUS.md)
worktree_status: WIP; documentation-only change; no source, test, manifest, or frozen-path change
command_argv:
  git diff --check -- docs/
  git diff --stat -- docs/
  (markdown link-integrity sweep over docs/*.md and docs/features/*.md)
cwd/environment: repository root; Linux x86_64; no cargo command executed (documentation-only slice)
fixture or cassette: none; the 14 changed documents and their cross-references are the artifact
exit_code: 0 for diff check and link sweep; no test/build command was run because no code changed
artifact paths and SHA-256: docs/company-os-*.md (12), docs/coding-pack-matrix.md, docs/schemas/README.md, docs/README.md, docs/features/ (10 walkthroughs + index); no SHA-256 generated
status change: no capability status promotion. Documentation status statements were aligned to existing ledger evidence blocks: security-constitution SEC-03 not_supported -> partial (P3-01 2026-08-31), SEC-06 Web auth partial with in-process token and exact Host/Origin (P1-01 2026-09-07), SEC-08/09/11 intent_only -> partial (P1-03/P1-06/P1-12); implementation-outline Gate 0, external-risk, filesystem, and event-privacy rows now cite their closing evidence blocks
proof-level change: source-level only (documentation). No new local_behavior/durable/live/physical evidence; no test was run
limitations: documents describe the dirty working tree; no code fix was made for any reported spec-vs-code conflict (RiskLevel four-level vs R0-R5 mapping, entrypoint auto-approve, WorkPacket ACK state, Run cancel_requested intermediate state remain open decisions recorded in the documents); new normative text (state machines, object contracts, determinism contract) is drafted and awaits owner review
reviewer: 10 parallel read-only audits plus a coordinator cross-document consistency sweep (proof-level vocabulary, status vocabulary, P-sequence canonicalization, cancel naming, link integrity)
```

### Current Gate 0 revalidation after MCP, Swarm, and JSONL recovery slices (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user/WIP changes and the pre-existing kiana-entrypoints/created.txt fixture preserved; no commit/reset/push/merge or worktree deletion performed
command_argv:
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo clippy --workspace --all-targets --locked --offline
  cargo test --workspace --locked --offline --no-fail-fast
  bash scripts/release-smoke.sh
  bash scripts/v10-workbench-smoke.sh
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain (rustc/cargo 1.97.1); locked/offline Cargo dependency resolution; release smoke uses its temporary install/project fixtures
fixture or cassette: full workspace unit/integration/doc tests; advertised stdio MCP schema fixtures; Swarm temporary worker/artifact fixtures; JSONL crash-tail fixtures; release product-shell install fixture; v1.0 workbench cassette
exit_code: 0 for every listed command; workspace tests passed including kiana-entrypoints library 433/433, kiana-commands swarm_command 46/46, and kiana-eventlog 25/25; release smoke reported product shell smoke passed and completed release build/install; workbench smoke reported ok
artifact paths and SHA-256: no new persistent release/workbench artifact retained; command exit records and the dirty source snapshot are the evidence artifacts; no artifact hash generated
status change: S0 Gate 0 remains green for this current snapshot; P1 security and P2 recovery remain partial, and S1-S4 gates remain open
proof-level change: current source/build/test/release/workbench evidence establishes local_behavior for the exercised local paths; the JSONL slice adds disk close/reopen evidence but does not establish full durable runtime recovery; no live/physical claim
limitations: checkout remains dirty and existing compiler/clippy warnings are non-fatal; release/workbench checks use local temporary fixtures/cassettes; authenticated principal, durable PendingInvocation/cancellation reconciliation, full effect-time TOCTOU protection, complete redaction coverage, aggregate projector rebuild, and cross-process lifecycle recovery remain incomplete
reviewer: focused MCP, Swarm concurrency, and JSONL recovery review plus the exact full Gate 0 regression; no assertions weakened, tests skipped, frozen paths changed, or second execution path added
```

### P1-05 shell secret sentinel boundary evidence (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (72 status entries); existing user/WIP changes preserved; no commit, reset, push, merge, or worktree deletion performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-daemon --test daemon_host shell_secret_sentinels_do_not_reach_events_receipt_or_model_context --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-core --lib event_redaction --locked --offline -- --test-threads=1
  cargo check -p kiana-daemon --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo dependency resolution; serialized daemon integration test; local bubblewrap sandbox available
fixture or cassette: scripted KianaHarness sends an array-form shell argv ["/bin/echo", "api_key=argv-sentinel"], then a shell command emitting Authorization Bearer stdout and X-Api-Key stderr markers; host KIANA_SHELL_SECRET=env-sentinel probes sandbox environment filtering; disk-backed JSONL events, the public Receipt read, complete protocol responses, and the captured next ModelRequest are scanned
exit_code: fmt=0; focused daemon sentinel regression=0 (1/1); daemon_host=0 (59/59); core event_redaction=0 (7/7); daemon check=0; diff check=0; existing kiana-core dead-code warning remains non-fatal
artifact paths and SHA-256: temporary disk JSONL fixture is created below the OS temp directory by the test; no repository artifact or SHA-256 was generated; test assertions and command exits are the evidence
status change: P1-05 remains partial; the exercised shell route now has an end-to-end regression proving that the four sentinels do not reach durable events, the public Receipt, complete run/receipt response envelopes, or the following model request, while redacted output remains observable
proof-level change: adds local_behavior evidence for the exercised local shell/Broker boundary only; this does not promote SEC-05 from intent_only or claim durable, live, physical, or complete SecretRef enforcement
limitations: redaction still depends on the current sensitive-key and text-marker set; arbitrary unmarked secret output, provider echoes, external caches, child-Cell inputs, process memory, and live OS argv inspection are not covered; the environment probe covers the sandbox whitelist/name filter rather than a general secret-classification system
reviewer: focused negative-path shell/Broker privacy regression and serialized daemon/core verification; no production execution path, frozen surface, or dependency manifest changed
```

### P1-01 Web exact-listener Host/Origin denial evidence (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (73 status entries); existing user/WIP changes preserved; no commit, reset, push, merge, or worktree deletion performed
command_argv:
  cargo test -p kiana-entrypoints --test cli_web web_rejects_wrong_origin_and_host_without_mutating_trust --locked --offline -- --test-threads=1
  cargo test -p kiana-entrypoints --test cli_web web_rejects_foreign_bearers_and_sessions_without_mutation --locked --offline -- --test-threads=1
  cargo test -p kiana-entrypoints --test cli_web --locked --offline -- --test-threads=1
  cargo test -p kiana-entrypoints --lib web::tests --locked --offline -- --test-threads=1
  cargo check -p kiana-entrypoints --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo dependency resolution; serialized child-process Web integration tests
fixture or cassette: real `kiana web --no-open --bind 127.0.0.1:0` child processes over temporary untrusted Git fixtures; their process-local page tokens and session cookies are extracted, then wrong-loopback-port/host/origin, hostile origin/host, missing token, wrong token, foreign token, and foreign session combinations are attempted before an exact-Origin success request
exit_code: focused WEB-01 regression before the implementation change=101 (wrong-loopback-port Origin returned HTTP 200 instead of 401); focused regression after the change=0 (1/1); foreign bearer/session regression=0 (1/1); cli_web=0 (6/6); in-process web module=0 (9/9); entrypoints check=0; initial fmt check=1 for one formatting-only diff and final fmt check=0; diff check=0; existing kiana-core dead-code warning remains non-fatal
artifact paths and SHA-256: temporary Web home/project directories and child process are owned by the test; no repository artifact or SHA-256 was generated; HTTP status/state assertions and command exits are the evidence
status change: P1-01 remains partial; WebApp now retains the actual listener SocketAddr, the token index requires that exact Host, and authenticated state/mutation routes require the exact Host plus an exact listener match for any supplied Origin; a correct token paired with another loopback port is rejected without trusting the project
proof-level change: adds local_behavior negative evidence for the process-local Web mutation boundary; this does not promote SEC-06 beyond its current transport/authentication limits or claim durable session ownership
limitations: the token, session map, and trust UI state remain process-local; health remains intentionally unauthenticated and Origin remains optional for direct non-browser clients; there is no durable authenticated principal, session recovery, or OS-level local-user boundary, so this slice does not claim cross-process durable ownership
reviewer: Codex focused child-process and in-process Web verification; no independent reviewer, dependency change, frozen-path change, or second execution path
```

### Gate 0 full regression after Web listener and bearer/session slices (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (73 status entries); existing user/WIP changes preserved; no commit/reset/push/merge or worktree deletion performed
command_argv:
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo clippy --workspace --all-targets --locked --offline
  cargo test --workspace --locked --offline --no-fail-fast
  bash scripts/release-smoke.sh
  bash scripts/v10-workbench-smoke.sh
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo dependency resolution; noninteractive workspace/release execution; serialized daemon and Web child-process fixtures
fixture or cassette: full workspace unit/integration/doc tests; release temporary install/project and product-shell fixtures; v1.0 Workbench cassette; Web wrong-authority and cross-process bearer/session fixtures
exit_code: 0 for every listed command; workspace tests passed including kiana-entrypoints library 433/433 and cli_web 6/6; noninteractive release smoke completed product-shell smoke, release build, and temporary install; workbench smoke returned `v10-workbench-smoke: ok`; diff check=0
artifact paths and SHA-256: release/workbench temporary artifacts were removed by their scripts; no persistent artifact or SHA-256 generated; command exits, test assertions, and the dirty source snapshot are the evidence artifacts
status change: S0 Gate 0 remains green for the current snapshot; P1-01/P1-02/P1-03/P1-04/P1-05/P1-06 and P2 recovery remain partial; no S1-S4 promotion
proof-level change: current source/build/test/release-install/workbench evidence establishes local_behavior for the exercised local paths; no durable/live/physical claim
limitations: checkout remains dirty and existing compiler/clippy warnings are non-fatal; release/workbench evidence uses local temporary fixtures/cassettes; authenticated principal, durable PendingInvocation/cancellation reconciliation, full effect-time TOCTOU, complete redaction, aggregate projector rebuild, and cross-process lifecycle recovery remain incomplete
reviewer: focused S0 Gate 0 regression after three security slices; no assertions weakened, tests skipped, frozen paths changed, or second execution path added
```

### P1-04 apply_patch descriptor-anchored update commit evidence (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (73 status entries); existing user/WIP changes preserved; no commit, reset, push, merge, or worktree deletion performed
command_argv:
  cargo fmt --all
  cargo test -p kiana-daemon --lib committed_update_keeps_the_preflighted_parent_after_path_replacement --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --lib apply_patch --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo check -p kiana-daemon --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo dependency resolution; serialized daemon tests
fixture or cassette: an update preflights a nested regular file, opens every present parent directory with `O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC`, then the pathname parent is renamed and replaced before commit; descriptor-relative `openat`/`renameat` must update only the original directory and preserve the original file mode
exit_code: fmt=0; focused adversarial regression=0 (1/1); apply_patch=0 (20/20); daemon_host=0 (60/60); daemon check=0; final fmt check=0; diff check=0; existing kiana-core dead-code warning remains non-fatal
artifact paths and SHA-256: temporary project directories are created below the OS temp directory and removed by the test; no repository artifact or SHA-256 was generated; file-content, mode, path-isolation assertions and command exits are the evidence
status change: P1-04 remains partial; Linux single-file update commits now anchor temporary creation, target replacement, and target-mode lookup to a parent descriptor opened before final precondition verification, so a later pathname-parent replacement cannot redirect that update
proof-level change: strengthens local_behavior negative evidence for one Linux apply_patch parent-directory TOCTOU boundary; no durable/live/physical promotion
limitations: Linux update operations only receive this descriptor-relative treatment; add/delete/move, rollback, non-Linux fallback, target replacement after verification, bind-mount substitution, and cross-process durable recovery remain open
reviewer: focused apply_patch adversarial review plus serialized daemon regression; no independent reviewer, dependency change, frozen-path change, or second execution path
```

### P1-04 apply_patch temporary-file collision fencing evidence (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (73 status entries); existing user/WIP changes preserved; no commit, reset, push, merge, or worktree deletion performed
command_argv:
  cargo fmt --all
  cargo test -p kiana-daemon --lib apply_patch --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo check -p kiana-daemon --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo dependency resolution; serialized daemon tests
fixture or cassette: apply_patch unit fixtures for full preflight, rollback, symlink/hardlink rejection, planted temporary-sibling symlink, and exhaustion of all temporary names; daemon_host exercises the brokered trusted and read-only apply_patch paths
exit_code: fmt=0; apply_patch=0 (19/19); daemon_host=0 (59/59); daemon check=0; fmt check=0; diff check=0; existing kiana-core dead-code warning remains non-fatal
artifact paths and SHA-256: temporary project and outside-target fixtures are created below the OS temp directory and removed by tests; no repository artifact or SHA-256 was generated; assertions and command exits are the evidence
status change: P1-04 remains partial; temporary update-file creation now uses exclusive create with bounded candidate rotation, so an occupied candidate (including a symlink) is never followed or reused and exhaustion fails closed without truncating existing candidates
proof-level change: strengthens local_behavior evidence for the apply_patch commit boundary and preserves daemon integration behavior; no durable/live/physical promotion
limitations: this closes pre-existing temporary-name collisions but does not provide fd-relative/no-follow guarantees against every effect-time parent-directory rename or external filesystem race; project lock, snapshot checks, rollback, symlink/hardlink checks, and atomic rename remain bounded by the current process/filesystem model
reviewer: focused apply_patch adversarial review plus serialized daemon regression; no independent review performed for this narrow implementation slice
```

### P1-04 EventLog descriptor-anchored append evidence (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (73 status entries); existing user/WIP changes preserved; no commit, reset, push, merge, or worktree deletion performed
command_argv:
  cargo fmt --all
  cargo test -p kiana-eventlog --locked --offline
  cargo check -p kiana-eventlog --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo dependency resolution; serialized core/daemon tests
fixture or cassette: EventLog opens a nested sessions directory, pre-opens it with `O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC`, replaces the pathname parent, and appends one event through `openat`; the original directory must receive the record while the replacement remains untouched; existing reopen, CAS, idempotency, torn-tail, and symlink regressions remain green
exit_code: fmt=0; kiana-eventlog=0 (28/28 plus 0 doc-tests); eventlog check=0; core control-plane=0 (82/82); daemon control-plane=0 (13/13); daemon_host=0 (60/60); final fmt check=0; diff check=0; existing kiana-core dead-code warning remains non-fatal
artifact paths and SHA-256: temporary EventLog parent fixtures are created below the OS temp directory and removed by the test; no repository artifact or SHA-256 was generated; file-content/path-isolation assertions and command exits are the evidence
status change: P1-04/P2-01 remain partial; Linux JSONL append now anchors creation/open/append to a pre-opened no-follow parent directory, preventing a pathname-parent replacement after preflight from redirecting the event write
proof-level change: strengthens local_behavior negative evidence for the EventLog append filesystem boundary while preserving existing local disk reopen/CAS/recovery behavior; no durable/live/physical promotion
limitations: load, torn-tail truncation, final-newline repair, lock-file creation, parent creation, non-Linux fallback, and concurrent parent replacement outside the opened descriptor remain bounded by the current process/filesystem model; aggregate projector rebuild, durable PendingInvocation/cancellation reconciliation, provider-effect verification, and cross-process lifecycle recovery remain open
reviewer: focused EventLog filesystem-fencing review plus serialized core/daemon regression; no assertions weakened, tests skipped, frozen paths changed, dependency change, or second execution path
```

### Gate 0 full regression after descriptor-anchored apply_patch and EventLog slices (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (73 status entries); existing user/WIP changes preserved; no commit, reset, push, merge, or worktree deletion performed
command_argv:
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo clippy --workspace --all-targets --locked --offline
  cargo test --workspace --locked --offline --no-fail-fast
  bash scripts/release-smoke.sh
  bash scripts/v10-workbench-smoke.sh
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo dependency resolution; noninteractive workspace/release execution; serialized daemon and Web child-process fixtures
fixture or cassette: full workspace unit/integration/doc tests including apply_patch and EventLog regressions; release temporary install/project and product-shell fixtures; v1.0 Workbench cassette
exit_code: 0 for every listed command; workspace tests passed including kiana-entrypoints library 433/433 and cli_web 6/6; release smoke completed product-shell smoke, release build, and temporary install; workbench smoke returned `v10-workbench-smoke: ok`; diff check=0
artifact paths and SHA-256: release/workbench temporary artifacts were removed by their scripts; no persistent artifact or SHA-256 generated; command exits, test assertions, and the dirty source snapshot are the evidence artifacts
status change: S0 Gate 0 remains green for the current snapshot; P1-01/P1-02/P1-03/P1-04/P1-05/P1-06 and P2 recovery remain partial; no S1-S4 promotion
proof-level change: current source/build/test/release-install/workbench evidence establishes local_behavior for the exercised local paths; no durable/live/physical promotion
limitations: checkout remains dirty and existing compiler/clippy warnings are non-fatal; release/workbench evidence uses local temporary fixtures/cassettes; authenticated principal, durable PendingInvocation/cancellation reconciliation, remaining effect-time TOCTOU paths outside the covered Linux update/append operations, complete redaction, aggregate projector rebuild, and cross-process lifecycle recovery remain incomplete
reviewer: focused S0 Gate 0 regression after Linux apply_patch and EventLog filesystem-fencing slices; no assertions weakened, tests skipped, frozen paths changed, or second execution path added
```

### P1-04 approval-store temporary-file collision fencing evidence (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (73 status entries); existing user/WIP changes, including approval challenge risk/formatting edits, preserved; no commit/reset/push/merge or worktree deletion performed
command_argv:
  cargo fmt --all
  cargo test -p kiana-daemon --lib approval_store --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo check -p kiana-daemon --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo dependency resolution; serialized daemon tests
fixture or cassette: approval-store unit fixtures for a planted first-candidate symlink to an independent outside directory and exhaustion of all 16 candidates; persisted approval reopen/merge/CAS fixtures; DaemonHost and daemon control-plane integration fixtures
exit_code: fmt=0; approval_store=0 (11/11); daemon_host=0 (59/59); daemon control-plane=0 (13/13); daemon check=0; fmt check=0; diff check=0; existing kiana-core dead-code warning remains non-fatal
artifact paths and SHA-256: temporary approval roots and outside targets are created below the OS temp directory and removed by tests; no repository artifact or SHA-256 was generated; file-content assertions and command exits are the evidence
status change: P1-04 remains partial; approval records now allocate a same-directory temporary sibling with exclusive create and bounded candidate rotation, so a pre-existing file or symlink is never followed or truncated and exhaustion fails closed with `approval_store_temp_unavailable`
proof-level change: local_behavior evidence for the approval persistence write boundary, while existing reopen, independent-approval merge, and stale-disk CAS behavior remains green; no durable/live/physical promotion
limitations: the bounded helper does not provide fd-relative/no-follow protection against every effect-time parent-directory rename or an external filesystem race; process locking, aggregate projector rebuild, durable PendingInvocation/cancellation reconciliation, provider-effect verification, and cross-process lifecycle recovery remain open
reviewer: focused approval-store adversarial review plus serialized daemon regression; no assertions weakened, tests skipped, frozen paths changed, or second execution path added
```

### P1-04 JSONL EventLog final-path symlink fencing evidence (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user/WIP changes preserved; no commit/reset/push/merge or worktree deletion performed
command_argv:
  cargo fmt --all
  cargo test -p kiana-eventlog --locked --offline
  cargo check -p kiana-eventlog --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo dependency resolution; serialized core/daemon tests
fixture or cassette: existing JSONL reopen, torn-tail repair, idempotency, CAS, and concurrent-instance fixtures; a valid outside event file reached through a planted EventLog symlink; an opened EventLog path replaced by a symlink before append
exit_code: fmt=0; kiana-eventlog=0 (27/27 plus 0 doc-tests); eventlog check=0; core control-plane=0 (80/80); daemon control-plane=0 (13/13); daemon_host=0 (59/59); fmt check=0; diff check=0; existing kiana-core dead-code warning remains non-fatal
artifact paths and SHA-256: temporary EventLog/outside fixtures are created below the OS temp directory and removed by tests; no repository artifact or SHA-256 was generated; outside-file content assertions and command exits are the evidence
status change: P1-04/P2-01 remain partial; Unix JSONL lock, read, append, torn-tail truncation, and final-newline repair opens now use `O_NOFOLLOW`, and presence checks use `symlink_metadata`, so a final path symlink or replacement is rejected before reading or mutating its target
proof-level change: local_behavior negative evidence for the EventLog final-path symlink and effect-time replacement boundary, while existing disk reopen/CAS/recovery behavior remains green; no durable/live/physical promotion
limitations: Unix final-component no-follow does not cover every parent-directory rename or non-Unix equivalent, and it does not establish aggregate projector rebuild, durable PendingInvocation/cancellation reconciliation, provider-effect verification, or cross-process lifecycle recovery
reviewer: focused EventLog filesystem-fencing review plus serialized core/daemon regression; no assertions weakened, tests skipped, frozen paths changed, or second execution path added
```

### P1-04 memory JSONL final-path symlink fencing evidence (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (73 status entries); existing user/WIP changes preserved; no commit/reset/push/merge or worktree deletion performed
command_argv:
  cargo fmt --all
  cargo test -p kiana-daemon --lib harness_memory --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo check -p kiana-daemon --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo dependency resolution; serialized daemon tests
fixture or cassette: harness-memory unit fixtures for an existing symlink on the read and append paths, with an independent outside JSONL target; daemon_host memory/search, authorization, shell, approval, and lifecycle fixtures
exit_code: fmt=0; harness_memory=0 (4/4); daemon_host=0 (59/59); daemon check=0; fmt check=0; diff check=0; existing kiana-core dead-code warning remains non-fatal
artifact paths and SHA-256: temporary memory and outside-target fixtures are created below the OS temp directory and removed by tests; no repository artifact or SHA-256 was generated; outside-file content assertions and command exits are the evidence
status change: P1-04/P2 memory persistence remain partial; memory reads now distinguish dangling/symlink presence with `symlink_metadata`, and Unix final-file opens use `O_NOFOLLOW`, so an existing final-path symlink is rejected without reading or appending to its target
proof-level change: local_behavior negative evidence for the memory broker's final-path read/write boundary, while existing role and collection authorization behavior remains green; no durable/live/physical promotion
limitations: Unix final-component no-follow does not cover every parent-directory rename or non-Unix equivalent; memory records still lack full provenance/promotion/expiry lifecycle, cross-process locking, aggregate rebuild, durable PendingInvocation/cancellation reconciliation, and complete secret classification
reviewer: focused memory filesystem-fencing review plus serialized daemon regression; no assertions weakened, tests skipped, frozen paths changed, or second execution path added
```

### P1-04 governance artifact symlink and atomic-write evidence (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (73 status entries); existing user/WIP changes preserved; no commit/reset/push/merge or worktree deletion performed
command_argv:
  cargo test -p kiana-core --test control_plane review_artifact_symlink_is_rejected_without_mutating_target --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane review_artifact_symlink --locked --offline -- --test-threads=1
  cargo test -p kiana-core --lib artifact_path_tests --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host closing_rejects_symlinked_review_and_merge_artifacts --locked --offline -- --test-threads=1
  cargo clippy -p kiana-core --all-targets --locked --offline
  cargo fmt --all --check
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-core --locked --offline -- --test-threads=1
  cargo check -p kiana-core -p kiana-daemon --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo dependency resolution; serialized core/daemon integration tests
fixture or cassette: the pre-fix core regression planted `gate/REVIEW.json` as a symlink to an outside sentinel; the fixed public regressions cover final-file and `gate/` parent symlinks plus rejection-event projection; private prepared-write fixtures deterministically replace the parent directory, insert a final symlink, change the target version, and plant a hardlink between prepare and commit; the daemon fixture runs a brokered Builder apply_patch cassette, produces valid independent Review and Merge artifacts, replaces each artifact with an outside symlink, and attempts Closing
exit_code: pre-fix focused regression=101 (review incorrectly returned Completed); first descriptor-test compile attempt=101 (invalid `Result` equality in new test support, corrected before behavioral result); fixed core symlink regressions=0 (2/2); descriptor race regressions=0 (4/4); focused daemon Closing regression=0 (1/1); core clippy=0 with pre-existing warnings only; fmt check=0; core control-plane=0 (82/82); daemon_host=0 (60/60); full kiana-core=0 (11 unit, 82 control-plane, 2 dependency-boundary, 0 doc-tests); affected-crate check=0; diff check=0; existing kiana-core dead-code warning remains non-fatal
artifact paths and SHA-256: temporary project and outside-target fixtures are created below the OS temp directory; no repository artifact or SHA-256 was generated; outside-content, blocked-response, event-kind, and absent-Closing assertions plus command exits are the evidence
status change: P1-04 and the S3 governance-artifact boundary remain partial; on Linux, Symposium decisions/packets, Review packets, Merge receipts, Closing receipts, and lessons now traverse/create parents with no-follow directory descriptors, create bounded exclusive descriptor-relative temporary files, snapshot single-link regular targets, revalidate root/parent identity and target version, and commit with `renameat2` no-replace/exchange; Review/Merge reads revalidate file identity/version and path identity before returning; rejected Review writes emit `run.rejected` and never `review.closed`, while symlinked gate inputs cannot produce Closing artifacts
proof-level change: local_behavior negative evidence for governance artifact final-file/parent symlink denial, hardlink denial, directory-rename fencing, target-version fencing, and per-file atomic replacement, with the existing end-to-end Builder -> Review -> Closing path remaining green; no durable/live/physical promotion
limitations: descriptor anchoring prevents a raced path from redirecting the write outside the opened project directory, but bind-mount substitution is not rejected and a parent rename after the final identity revalidation can still commit into the already-open original directory; non-Linux builds retain the pathname fallback; multi-file Symposium and Closing writes are not transactional; directory fsync is best-effort and its failure is not represented as `result_unknown`; exchange rollback failure also has no durable reconciliation record; crash recovery/reconciliation from governance artifacts remains incomplete
reviewer: focused adversarial filesystem review plus core and serialized DaemonHost regression; no independent reviewer, assertion weakening, skipped test, frozen-path change, dependency change, or second execution path
```

### Gate 0 full regression after P1-04 (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (73 status entries before this evidence block); existing user/WIP changes preserved; no commit/reset/push/merge or worktree deletion performed
command_argv:
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo clippy --workspace --all-targets --locked --offline
  cargo test --workspace --locked --offline --no-fail-fast
  bash scripts/release-smoke.sh
  bash scripts/release-smoke.sh; rc=$?; printf 'RELEASE_SMOKE_EXIT=%s\n' "$rc"; exit "$rc"
  KIANA_RELEASE_SMOKE_SKIP_BUILD_GATES=1 bash scripts/release-smoke.sh
  bash scripts/v10-workbench-smoke.sh
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked/offline Cargo resolution; workspace tests and daemon tests serialized where required; release/workbench use temporary local fixtures
fixture or cassette: full workspace unit/integration/doc tests; release temporary install/project and product-shell fixtures; v1.0 Workbench cassette
exit_code: fmt=0; workspace check=0; clippy=0; workspace tests=0; full release-smoke=0 on the explicit status-capturing rerun (including embedded serialized workspace tests, product-shell smoke, release build, and temporary install); explicit skip-build-gates release-smoke=0; workbench=0; diff check=0
artifact paths and SHA-256: release/workbench temporary artifacts were removed by their scripts; no persistent artifact or SHA-256 generated; command exits, test assertions, and the dirty source snapshot are the evidence artifacts
status change: S0 Gate 0 remains green for the current snapshot; P1-01/P1-02/P1-03/P1-04/P1-05/P1-06 and P2 recovery remain partial; no S1-S4 promotion
proof-level change: source/build/test/release-install/workbench evidence establishes local_behavior for the exercised local paths; JSONL and approval persistence remain bounded local disk evidence, with no durable/live/physical claim
limitations: checkout remains dirty and existing warnings are non-fatal; release/workbench evidence uses temporary local fixtures; authenticated principal, durable PendingInvocation/cancellation reconciliation, full effect-time TOCTOU, complete redaction, aggregate projector rebuild, and cross-process lifecycle recovery remain incomplete
reviewer: focused S0 Gate 0 regression after three security slices; no assertions weakened, tests skipped, frozen paths changed, or second execution path added
```

### S0 Gate 0 exit evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo clippy --workspace --all-targets --locked --offline
  cargo test --workspace --locked --offline --no-fail-fast
  bash scripts/release-smoke.sh
  bash scripts/v10-workbench-smoke.sh
cwd/environment: repository root; Linux 6.8.0-138-generic x86_64; rustc 1.97.1; cargo 1.97.1; locked/offline dependency resolution; workspace test run non-interactively
fixture or cassette: release-smoke temporary install/project; trusted context artifact cache deny-before-approval and approved-write paths; v10 workbench cassette
exit_code: 0 for each of the six exact commands; workspace entrypoints 433/433 and release-smoke embedded regression suite passed
artifact paths and SHA-256: no persistent release/workbench artifact retained; command exit records and the checked-in source snapshot are the evidence artifacts; no artifact hash generated
status change: S0 Gate 0 red → green; this block makes no S1 completion claim
proof-level change: Gate 0 source/build/test/smoke evidence establishes local_behavior for the exercised local product path; no durable/live/physical claim
limitations: worktree remains dirty; existing compiler/clippy warnings remain non-fatal; release and workbench evidence uses local temporary fixtures/cassettes; S1-S4 security, persistence, business, and platform gates remain open
reviewer: Codex implementation pass plus independent focused approval-risk review; daemon wire challenge risk assertion passed
```

### P1-06 MCP advertised-schema argument fence evidence (2026-09-06)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user/WIP changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo check -p kiana-daemon --locked --offline
  cargo clippy -p kiana-daemon --all-targets --locked --offline
  cargo test -p kiana-daemon --lib harness_mcp --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host trusted_builder_stdio_mcp_echoes_through_daemon --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host unknown_stdio_mcp_tool_is_rejected_before_tools_call --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux x86_64; stable rust toolchain; locked/offline dependencies; serialized daemon tests
fixture or cassette: advertised stdio MCP tool schemas, required/type/additionalProperties argument cases, unsupported combinator case, trusted echo server, and unknown-tool server marker
exit_code: 0 for every listed command; harness_mcp 10/10 and both daemon integration tests passed
status change: P1-06 remains partial; the daemon now validates an advertised tool's bounded schema and arguments before issuing tools/call
proof-level change: local_behavior negative evidence for missing, wrongly typed, unknown, deeply nested, and unsupported-schema inputs, plus preserved authorized stdio execution
limitations: only the daemon-enforced JSON-Schema subset is supported; arbitrary combinators, full provenance/attestation, durable schema snapshots, HTTP MCP, live providers, and external adapters remain open
reviewer: focused MCP boundary review and serialized daemon regression suite
```

### S0/P1-03 Swarm completion-marker race correction evidence (2026-09-06)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user/WIP changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all
  cargo test -p kiana-commands --test swarm_command --locked --offline -- --test-threads=1
  cargo test -p kiana-commands --all-targets --jobs 1 --locked --offline --no-fail-fast -- --test-threads=1 --nocapture
  cargo test --workspace --locked --offline --no-fail-fast
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked/offline dependencies; workspace test targets run concurrently while each target uses its configured test threads
fixture or cassette: bounded Swarm worker shell fixtures, temporary workflow artifacts, authenticated result packets, and full workspace test fixtures
exit_code: 0 for every listed command; swarm target 46/46, kiana-commands all targets passed, workspace tests passed with no failed targets
status change: Gate 0 workspace test requirement is green for this snapshot; P1-03 remains partial because broader durable cancellation/recovery proof is open
proof-level change: local_behavior evidence that a worker completion marker is observed only after its completion timestamp, preventing a partial result packet from racing an immutable artifact commit; full workspace regression now passes under concurrent Cargo target execution
limitations: release-smoke and static quality gates still need the post-fix rerun; worker completion metadata remains a local file protocol and does not by itself establish cross-process durable recovery
reviewer: focused Swarm race investigation and full workspace regression; no assertions weakened and no tests skipped
```

### Current workspace regression after Swarm marker fix (2026-09-06)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user/WIP changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo test --workspace --locked --offline --no-fail-fast
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked/offline dependencies; default Cargo target scheduling
fixture or cassette: workspace package and integration test fixtures, including kiana-commands Swarm and kiana-entrypoints CLI/SDK paths
exit_code: 0; all workspace targets passed, including kiana-commands swarm_command 46/46 and kiana-entrypoints library 433/433
status change: Gate 0 workspace test leg changed from red to green for this snapshot; release smoke remains to be rerun
proof-level change: confirms the completion-marker ordering fix holds under the previously failing cross-target scheduling
limitations: this command does not cover release packaging/install assertions or workbench smoke; warnings remain non-fatal
reviewer: full workspace regression after the Swarm marker fix
```

### Current workspace/release smoke recheck evidence (2026-09-06)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user/WIP changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo test --workspace --locked --offline --no-fail-fast
  bash scripts/release-smoke.sh
  cargo test -p kiana-commands --test swarm_command --locked --offline -- --test-threads=1
  cargo test -p kiana-commands --test swarm_command --locked --offline
  bash scripts/v10-workbench-smoke.sh
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked/offline dependencies; workspace/release commands allow Cargo target parallelism; focused swarm reruns are isolated
fixture or cassette: workspace package test targets; swarm workflow/worker temporary fixtures; v1.0 workbench cassette
exit_code: workspace 101 (only target failure `-p kiana-commands --test swarm_command`); release smoke 101 at its workspace test gate with the same target; focused swarm serial 0 (46/46); focused swarm default 0 (46/46); workbench smoke 0
status change: none; Gate 0 remains partial/red for this snapshot
proof-level change: confirms the failure is reproducible only in the multi-target workspace/release execution while the swarm target and workbench smoke pass in isolation; no product status upgrade
limitations: first swarm failure reports an artifact-content conflict during repeated worker result observation and poisons the target's process-local environment mutex, so later failures cascade as `PoisonError`; root cause of the cross-target timing sensitivity still needs a production/test-isolation fix; warnings remain non-fatal
reviewer: focused concurrency classification; no assertions weakened and no failed target skipped
```

### Current Gate 0 evidence after Swarm marker fix (2026-09-06)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user/WIP changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo clippy --workspace --all-targets --locked --offline
  cargo test --workspace --locked --offline --no-fail-fast
  bash scripts/release-smoke.sh
  bash scripts/v10-workbench-smoke.sh
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain (rustc/cargo 1.97.1); locked/offline dependencies; release smoke uses temporary install fixtures and serial workspace tests; workbench uses the local debug binary
fixture or cassette: full workspace unit/integration/doc tests; Swarm temporary worker/artifact fixtures; release product-shell install fixture; v1.0 workbench cassette
exit_code: 0 for every listed command; workspace tests passed including kiana-commands swarm_command 46/46 and kiana-entrypoints library 433/433; release smoke completed build/install/product-shell checks; workbench smoke reported ok
status change: S0 Gate 0 changed from partial/red to green for this snapshot; P1-03 and P1-06 remain partial and S1-S4 gates remain open
proof-level change: current source/build/test/release/workbench evidence establishes local_behavior for the exercised local product path; no durable/live/physical claim
limitations: checkout remains dirty and existing compiler/clippy warnings are non-fatal; release/workbench checks use local temporary fixtures/cassettes; Swarm completion metadata is still a local file protocol; persistence/recovery, full security-constitution coverage, business closure, and platform gates remain incomplete
reviewer: focused Swarm concurrency review, MCP boundary review, and full Gate 0 regression; no assertions weakened, tests skipped, or frozen paths changed
```

`kiana-core` 已移除 `kiana-query` 与 `kiana-types`；`kiana-daemon` 作为组合根保留并显式允许其 `kiana-query`、`kiana-skills` 与 `kiana-types` 适配依赖。边界测试不再报告 forbidden internal dependencies。

### G0-02 slice evidence (2026-08-28)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; no reset or unrelated formatting changes
command_argv:
  cargo test -p kiana-core --test dependency_boundaries --offline
  cargo check -p kiana-core -p kiana-daemon --locked --offline
  cargo test -p kiana-core --test control_plane --offline
  cargo test -p kiana-daemon --test daemon_host --offline -- --test-threads=1
  cargo clippy -p kiana-core -p kiana-daemon --all-targets --locked --offline
fixture or cassette: existing daemon_host/core control_plane fixtures
exit_code: 0 for boundary, check, focused tests, and non-denying clippy
status change: G0-02 control-plane dependency boundary implemented
proof-level change: source + contract; preserved local_behavior regression tests
limitations: full workspace Gate 0 remains unverified; --all workspace fmt and -D warnings clippy remain blocked by pre-existing dirty/baseline findings
reviewer: independent adversarial review completed; UpdateInput fail-open finding fixed and regression-tested
```

The slice adds `PreToolHookPort` to `kiana-ports`, injects the existing query hook implementation from `kiana-daemon`, and removes `kiana-query`/`kiana-types` from `kiana-core`. The later P1-02 slice adds same-host approval continuation for harness Ask requests without introducing a second runtime loop; durable approval and cancellation guarantees remain open.

### P1-02 same-host continuation slice evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; no reset or unrelated formatting changes
command_argv:
  cargo check -p kiana-domain -p kiana-core --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-runner --locked --offline
cwd/environment: repository root; stable toolchain; offline dependency resolution
fixture or cassette: existing control_plane/daemon_host fixtures and scripted harness tests
exit_code: 0 for final check and all focused test commands
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P1-02 changed from target to partial; PendingInvocation same-host continuation implemented
proof-level change: source + local_behavior for tested in-process path; not durable
limitations: proof fields are now available on the approval wire request and MemoryApprovalStore validates supplied hash/nonce; CLI/TUI/REPL and app-server approval entrypoints now forward challenge proof; legacy in-process API remains intentionally compatible and does not require proof; pending invocations and harness runs are process-local; dedicated cancellation-after-awaiting regression coverage remains incomplete
reviewer: focused adversarial inspection completed; no claim of durable or cross-process recovery
```

### P1-02 cancel-after-awaiting regression evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; selective regression tests merged; no reset or unrelated formatting changes
command_argv:
  cargo test -p kiana-core --test control_plane cancel_after_awaiting_approval --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host cancel_after_awaiting_approval --locked --offline -- --test-threads=1
cwd/environment: repository root; stable toolchain; offline dependency resolution
fixture or cassette: scripted apply_patch cassette; CountingBroker and real MemoryApprovalStore
exit_code: 0 for both commands; 1 core test and 1 daemon test passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P1-02 cancellation fencing regression coverage added; P1-02 remains partial
proof-level change: local_behavior regression evidence for same-host cancel-before-approval continuation
limitations: pending invocations, approval store, and harness runs remain process-local; invalidate errors are still ignored by cancel_run; no claim of durable or cross-process recovery
reviewer: focused adversarial inspection completed; broker call count remained zero and later approval was blocked
```

### P1-02 invalidation fail-closed evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; selective core cancellation fencing change; no reset or unrelated formatting changes
command_argv:
  cargo check -p kiana-core --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host cancel_after_awaiting_approval --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; stable toolchain; offline dependency resolution
fixture or cassette: core control-plane fixtures; scripted apply_patch cassette; real MemoryApprovalStore
exit_code: 0 for all commands; core 31/31 and daemon cancellation 1/1 passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P1-02 cancellation fencing fail-open path changed to fail-closed; remains partial
proof-level change: local_behavior evidence now covers invalidate failure handling and cancel-after-awaiting behavior
limitations: pending invocations, approval store, and harness runs remain process-local; durable recovery, process-group cancellation, canonical digest, and mandatory wire proof remain open
reviewer: focused adversarial review plus serialized core/daemon regression tests
```

### P1-02 authorization-context binding evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; approval binding extended without reset or unrelated cleanup
command_argv:
  cargo test -p kiana-daemon approval_store --locked --offline
  cargo test -p kiana-core --test control_plane cancel_after_awaiting_approval --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; stable toolchain; offline dependency resolution
fixture or cassette: MemoryApprovalStore unit fixtures; core scripted continuation fixture
exit_code: 0 for all commands; approval store 4/4 and core cancellation 1/1 passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: approval context binding now includes role, department, and path allow scope; P1-02 remains partial
proof-level change: local_behavior negative coverage for altered path scope not consuming an approval
limitations: digest remains record-local SHA-256 rather than a documented canonical proof; legacy id-only in-process approval remains compatible; state is process-local
reviewer: focused adversarial review; no claim of durable, signed, or cross-process approval
```

### P1-02 structured approval digest evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; selective digest hardening; no reset or unrelated cleanup
command_argv:
  cargo test -p kiana-daemon approval_store --locked --offline
  cargo test -p kiana-core --test control_plane cancel_after_awaiting_approval --locked --offline -- --test-threads=1
  cargo check -p kiana-daemon -p kiana-entrypoints --locked --offline
  git diff --check
cwd/environment: repository root; stable toolchain; offline dependency resolution
fixture or cassette: MemoryApprovalStore fixtures; scripted core continuation fixture
exit_code: 0 for all commands; approval store 4/4 and core cancellation 1/1 passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: approval request hash now includes request and authorization context with sha256 prefix; P1-02 remains partial
proof-level change: local_behavior evidence for context-sensitive approval digest and existing proof verification
limitations: JSON canonical ordering is not yet a documented cross-implementation canonicalization; legacy id-only in-process API remains compatible; no signature, durable persistence, or cross-process recovery
reviewer: focused adversarial review; no claim of durable, signed, or live approval
```

### P1-02 wire proof contract evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; protocol proof roundtrip and store negative coverage added; no reset or unrelated cleanup
command_argv:
  cargo test -p kiana-protocol --locked --offline
  cargo test -p kiana-daemon approval_store --locked --offline
  cargo test -p kiana-daemon --test daemon_host cancel_after_awaiting_approval --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; stable toolchain; offline dependency resolution
fixture or cassette: protocol envelope fixtures; MemoryApprovalStore proof fixture; scripted daemon cancellation cassette
exit_code: 0 for all commands; protocol 9/9, approval store 4/4, daemon cancellation 1/1 passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: proof-aware approval envelope roundtrip is covered; P1-02 remains partial
proof-level change: local_behavior negative proof mismatch coverage and protocol serialization evidence
limitations: end-to-end approve retry currently exposes an existing event_sequence_not_monotonic aggregate-sequencing gap and remains unclaimed; legacy id-only compatibility remains; canonical JSON ordering, mandatory wire proof, durable persistence, and cross-process recovery remain open
reviewer: focused adversarial review; failed exploratory wire retry was not treated as passing evidence
```

### P1-02 wire approval continuation event-cursor evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; PendingInvocation event aggregate/cursor fix and daemon retry regression added; no reset or unrelated cleanup
command_argv:
  cargo check -p kiana-domain -p kiana-core -p kiana-daemon -p kiana-entrypoints --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host wire_approval_proof_retry_resumes_original_run --locked --offline -- --test-threads=1
  cargo test -p kiana-protocol --locked --offline
  cargo test -p kiana-daemon approval_store --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; stable toolchain; offline dependency resolution; serialized daemon tests
fixture or cassette: scripted apply_patch cassette; real DaemonHost and MemoryApprovalStore; proof retry through KianaClient wire envelope
exit_code: 0 for affected-crate check, core control-plane regression, protocol/approval tests, wire retry regression, and diff check; workspace format check remains subject to existing baseline drift outside this slice
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: approval continuation now preserves the original run event aggregate and next event cursor; P1-02 remains partial
proof-level change: local_behavior evidence now covers wrong-proof rejection followed by correct-proof retry without event_sequence_not_monotonic; no durable upgrade
limitations: PendingInvocation, approval store, runner, and event authority remain process-local; read-only approval continuation correctly produces no filesystem write; canonical JSON ordering, mandatory wire proof, authenticated principal, durable persistence, and cross-process recovery remain open
reviewer: focused adversarial review; prior event_sequence_not_monotonic failure was reproduced before the cursor fix and the corrected wire retry now passes
```

### P1-02 canonical digest evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; recursive canonical JSON encoding and regression added; no reset or unrelated cleanup
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-daemon approval_store --locked --offline
  git diff --check
cwd/environment: repository root; stable toolchain; offline dependency resolution
fixture or cassette: MemoryApprovalStore unit fixtures; nested serde_json ordering regression
exit_code: 0 for all commands; approval store 5/5 passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: approval digest encoding now recursively sorts object keys while preserving array order; P1-02 remains partial
proof-level change: local_behavior evidence for order-independent nested approval digest encoding
limitations: canonical JSON profile is implemented locally but not yet documented as a cross-implementation standard; mandatory wire proof, signature, durable persistence, authenticated principal, and cross-process recovery remain open
reviewer: focused adversarial inspection; no claim of durable or signed approval
```

### G0-01 installer contract evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; updated stale source-contract assertion only; install.sh behavior unchanged
command_argv:
  cargo test -p kiana-commands release::tests::install_script_is_source_checkout_installer_with_doctor_verification --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; stable toolchain; offline dependency resolution
fixture or cassette: install.sh source contract
exit_code: 0 for all commands; installer contract 1/1 passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: landed installer test now verifies installed-binary doctor invocation and USER.md guidance; G0-01 remains partial
proof-level change: local_behavior regression evidence for the current source-checkout installer contract
limitations: this does not prove release packaging, signing, live provider support, or production installation; unrelated workspace supervisor failures remain open
reviewer: read-only history audit identified the test drift; no install.sh restoration performed
```


```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; verification executed without reset or cleanup
command_argv:
  cargo test --workspace --locked --offline --no-fail-fast
cwd/environment: repository root; stable toolchain; offline dependency resolution
fixture or cassette: workspace test fixtures and Linux supervisor subprocess fixtures
exit_code: 101
artifact paths and SHA-256: task stdout captured at tasks/bzhnnpov3.output; SHA-256 not generated
status change: none; Gate 0 remains not passed
proof-level change: none; focused local_behavior evidence is unchanged
limitations: four Linux supervisor tests failed with receipt_invalid; one existing installer string-contract test failed because install.sh did not contain the expected doctor invocation; full workspace is not green and failures were not treated as approval-slice regressions
reviewer: focused verification review; baseline versus current-slice ownership remains to be audited before remediation
```


The slice adds a domain `PendingInvocation`, stages harness Ask requests instead of returning a fake tool failure, returns `AwaitingApproval` while preserving the same session/run, and resumes through the existing `RunnerCommand::CapabilityResult` path after approval. Direct capability approval remains compatible. It does not add a second runner loop or change `Continue` semantics.

此前对当前 dirty checkout 的全 workspace 测试记录为 exit 101，并涉及多个 target；在重新验证前不得称为“全绿”。该结果必须和 commit、工作树、工具链及完整 stdout 一起保存，不能脱离快照引用。

### Capability-governance supervisor baseline remediation evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; only capability-governance smoke skip guards changed; no supervisor launcher changes
command_argv:
  bash -n scripts/capability-governance-smoke.sh
  cargo test -p kiana-capability-governance-supervisor --locked --offline --test supervisor_linux
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution
fixture or cassette: supervisor Linux subprocess fixtures and capability-governance smoke slices
exit_code: 0; 11 supervisor Linux tests passed
artifact paths and SHA-256: task stdout captured at tasks/b9u5x546u.output; SHA-256 not generated
status change: baseline receipt_invalid failures closed by explicitly skipping slices whose deleted governance/schema corpus is no longer present; Gate 0 remains not passed
proof-level change: local_behavior regression evidence for supervisor launch, receipt parsing, confinement, cancellation, and public slices
limitations: the deleted governance/schema corpus is not restored; skipped ledger/schema slices are not proof of those historical artifacts; full workspace tests and strict workspace clippy remain unverified
reviewer: independent read-only audit traced failures to deleted paths from 81335cb/a257e0c; receipt launcher/protocol was not altered
```

### P1-01 local principal and session ownership evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; ControlPlane session binding and DaemonHost local principal added; no reset or unrelated cleanup
command_argv:
  cargo check -p kiana-core --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo check -p kiana-daemon --locked --offline
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized test targets
fixture or cassette: existing control_plane and daemon_host fixtures plus actor/run ownership negative fixtures
exit_code: 0 for all commands; core 31/31 and daemon_host 51/51 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-01 changed from target to partial; process-local actor stamping and session/project/run binding implemented
proof-level change: local_behavior evidence for same-host ownership fencing, server-stamped actor receipt identity, and explicit cross-session run denial; no durable upgrade
limitations: local principal is currently the DaemonHost fixed principal; project trust remains caller-compatible and is not yet recomputed from an authenticated authority; receipt/review/packet ownership and cross-process recovery remain open
reviewer: focused verification; no claim of enterprise authentication, tenancy, or durable identity
```

### P1-01 receipt ownership and restart compatibility evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; receipt lookup now enforces live binding when present while preserving explicit-run restart reads
command_argv:
  cargo fmt --all
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized test targets
fixture or cassette: existing control_plane and daemon_host fixtures, disk receipt restart fixture
exit_code: 0; core 31/31 and daemon_host 51/51 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-01 remains partial; receipt access now shares live session ownership checks without breaking disk restart compatibility
proof-level change: local_behavior evidence for live receipt owner fencing and explicit-run event-log recovery; no durable identity upgrade
limitations: an unbound explicit run_id is still authorized by eventlog visibility during restart; authenticated project trust, durable principal/session records, and field-level receipt authorization remain open
reviewer: focused regression review; prior over-broad denial was reverted after disk restart test demonstrated the required recovery contract
```

### P1-01 review author ownership evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; durable run identity fields and review lookup filtering added; no reset or unrelated cleanup
command_argv:
  cargo test -p kiana-core --test control_plane review_author --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized test targets
fixture or cassette: core review fixtures, cross-project/cross-actor/run-session mismatch negative fixtures, daemon restart fixtures
exit_code: 0; focused review 3/3, core 34/34, daemon_host 51/51 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-01 remains partial; review author lookup now fences project, actor, and explicit run/session mismatches
proof-level change: local_behavior evidence for cross-project and cross-actor review denial while preserving same-project and restart behavior
limitations: old event records without actor/project fields remain readable for upgrade compatibility; trust is still caller-compatible; eventlog has no durable authenticated principal or tenant boundary
reviewer: independent read-only audit identified the cross-project false accept; regression tests verify no foreign gate/REVIEW.json write
```

### Entrypoint serialized regression evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; one CLI help contract line updated; no reset or unrelated cleanup
command_argv:
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-entrypoints --lib --locked --offline -- --test-threads=1
  cargo test -p kiana-entrypoints --test cli_run --locked --offline -- --test-threads=1
  cargo test -p kiana-entrypoints --test cli_run tui_help_is_parked_off_the_v0_2_product_path --locked --offline
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized test targets
fixture or cassette: daemon_host, entrypoints library, and cli_run existing fixtures
exit_code: daemon_host 0 (49/49); entrypoints library 101 (419 passed, 6 failed); cli_run before help fix 101 (29 passed, 1 failed), focused help regression after fix 0 (1/1)
artifact paths and SHA-256: daemon stdout captured at tasks/bsexnnwwn.output; library stdout at tasks/btxotnl1m.output; CLI stdout at tasks/b8qttab21.output; SHA-256 not generated
status change: TUI help contract drift fixed; Gate 0 remains not passed
proof-level change: local_behavior evidence for serialized daemon host and CLI run paths; no workspace-wide green claim
limitations: entrypoints library retains six unrelated failures (permission/direct-connect/tool-loop tests); full workspace test and strict workspace clippy remain unverified or failing
reviewer: focused verification; parallel daemon failures were not treated as product regressions after serialized 49/49 pass
```

## 4. 高优先级未完成项

| ID | 项目 | 状态 | 目标证明 |
|---|---|---|---|
| G0-01 | clean/WIP 快照、完整测试回执和文档统一 | partial | local_behavior |
| G0-02 | core dependency boundary 迁移 | implemented | source + contract |
| P1-01 | authenticated principal 与 session/project ownership | partial | local_behavior → durable |
| P1-02 | approval exact digest、single-use 和 PendingInvocation continuation | partial | local_behavior → durable |
| P1-03 | cancel fencing、进程组/孙进程停止和 Unknown | partial | local_behavior → durable |
| P1-04 | apply_patch 原子性与 effect-time TOCTOU | partial | local_behavior → durable |
| P1-05 | event/Receipt 字段级 redaction 和结果关联校验 | partial | durable |
| P1-06 | generic MCP 与外部生活能力的风险隔离 | partial | local_behavior → live/physical 前置 |
| P2-01 | aggregate event stream、CAS、重放和恢复 | partial | durable |
| P3-01 | Template/Cell/SpawnPlan/Lease/Grant/DelegationPacket | partial | durable |
| P3-02 | Planner → fresh Builder → independent Reviewer → Closer | partial | local_behavior → durable |

### P1-06 server-owned MCP risk boundary evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-policy --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux 6.8.0-138-generic x86_64; rustc 1.97.1; cargo 1.97.1; locked/offline dependency resolution; serialized core tests
fixture or cassette: trusted control-plane context; counting Broker; direct forged `mcp.call` requests with default ReadOnly and wrong CapabilityKind
exit_code: 0 for all commands; policy 19/19 and core control-plane 50/50 passed
artifact paths and SHA-256: no persistent artifact retained; test stdout and checked-in source snapshot are the evidence artifacts; no artifact hash generated
status change: P1-06 target/not_supported → partial; ControlPlane and DefaultPolicyEngine now reject MCP risk downgrade and capability-kind mismatch before Broker dispatch
proof-level change: local_behavior negative evidence for forged MCP risk and no-Broker-effect paths; stdio-only MCP remains bounded local behavior
limitations: complete argument JSON-Schema conformance and durable server executable/config/schema provenance remain open; requested-tool advertisement identity/shape and provider-result envelope checks are covered in the follow-up evidence below; configured stdio discovery still starts only after ControlPlane approval but is not a durable provenance record; HTTP MCP, live providers, commerce, travel, and physical adapters remain unsupported; no durable or live/physical claim
reviewer: independent S1 gap scan plus adversarial slice review; no remaining MCP risk-invariant blocker found in this slice
```

### P1-05 centralized event/result redaction boundary evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-core --lib --locked --offline event_redaction -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo check -p kiana-policy -p kiana-core -p kiana-daemon --locked --offline
  cargo clippy -p kiana-policy -p kiana-core -p kiana-daemon --all-targets --locked --offline
  git diff --check
cwd/environment: repository root; Linux 6.8.0-138-generic x86_64; rustc 1.97.1; cargo 1.97.1; locked/offline dependency resolution; serialized control-plane and daemon tests
fixture or cassette: sentinel Bearer/Basic/token/API-key/X-Api-Key runner delta and completion; secret-bearing Broker success/error results; legacy persisted completed/failed/result_unknown events; numeric token metrics including token_overlap; scalar/array capability result correlation
exit_code: fmt=0; event redaction=0 (7/7); core control-plane=0 (50/50); daemon control-plane=0 (13/13); daemon_host=0 (58/58); affected-crate check=0; affected-crate clippy=0; diff check=0; existing non-fatal warnings remain
artifact paths and SHA-256: no persistent artifact retained; test stdout and checked-in source snapshot are the evidence artifacts; no artifact hash generated
status change: P1-05 remains partial; EventLog append, current and legacy Receipt projection/CoreResponse error, Broker-to-Runner success/error, direct response, and direct/harness result-event paths now share recursive redaction and server-owned result correlation; ordinary Broker failures continue through Runner instead of terminating the run
proof-level change: local_behavior negative evidence that exercised sentinel credentials do not enter EventLog, Receipt, Runner result, or direct response; legacy failed/result_unknown errors are redacted on read; scalar results remain attributable to exact request/capability/grant scope
limitations: redaction uses structured-JSON recursion plus key/marker matching rather than schema-generated sensitive-field contracts and cannot prove arbitrary encoded-secret detection; raw provider process memory is outside this slice; durable audit projector/CAS recovery and complete stdout/stderr/argv/environment scan remain open; no durable SEC-05/SEC-10 completion claim
reviewer: independent adversarial review found the Broker-error continuation regression, legacy receipt error leak, and numeric token-metric compatibility regressions; all were closed with serialized regression coverage, and no further slice-local blocker was reported
```

### P1-06 advertised MCP tool/result boundary evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo test -p kiana-daemon --lib harness_mcp --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host unknown_stdio_mcp_tool_is_rejected_before_tools_call --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo clippy --workspace --all-targets --locked --offline
  cargo test --workspace --locked --offline --no-fail-fast
  bash scripts/release-smoke.sh
  bash scripts/v10-workbench-smoke.sh
  git diff --check
cwd/environment: repository root; Linux 6.8.0-138-generic x86_64; rustc 1.97.1; cargo 1.97.1; locked/offline dependency resolution; serialized daemon tests; Python 3 available for stdio integration fixture
fixture or cassette: unit fixtures for unknown, duplicate, and malformed requested-tool advertisements plus malformed provider envelopes and `isError`; Python stdio server advertises `echo`, records `tools/call` to a temporary marker, and the integration cassette requests an unknown tool
exit_code: harness_mcp=0 (6/6); unknown-tool daemon_host=0 (1/1); daemon control-plane=0 (13/13); daemon_host=0 (58/58); fmt=0; workspace check=0; workspace clippy=0; workspace tests=0 (including kiana-entrypoints 433/433); release-smoke=0; v10-workbench-smoke=0; diff check=0
artifact paths and SHA-256: temporary MCP call marker and release/workbench directories were cleaned by their tests/scripts; no persistent artifact retained and no artifact hash generated
status change: P1-06 remains partial; after authorization, the daemon checks the requested tool against the server advertisement, rejects unknown/ambiguous names and malformed object-schema shapes, bounds the provider result envelope, and maps provider `isError` to a failed capability result; an unknown-tool integration run proved no `tools/call` marker was written
proof-level change: local_behavior negative evidence for no-call-on-unknown-tool and fail-closed malformed tool/result boundaries, plus full Gate 0 source/build/test/smoke regression evidence; no durable/live/physical claim
limitations: complete argument validation against arbitrary JSON Schema is not implemented; server executable/config/schema provenance is not durable or signed; only stdio is supported and discovery still launches the configured local process after approval; HTTP MCP, live providers, commerce, travel, and physical adapters remain unsupported
reviewer: focused implementation plus independent adversarial slice review; no remaining tool/result boundary blocker was reported within this slice
```

### P1-05 cancel/continue direct-response redaction evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-core --test control_plane cancel --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane continue_runner_send_error_is_redacted_in_direct_response --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-tools --lib --locked --offline -- --test-threads=1
  cargo check --workspace --locked --offline
  cargo clippy --workspace --all-targets --locked --offline
  cargo test --workspace --locked --offline --no-fail-fast
  git diff --check
cwd/environment: repository root; Linux 6.8.0-138-generic x86_64; rustc 1.97.1; cargo 1.97.1; locked/offline dependency resolution; serialized daemon/core tests
fixture or cassette: `CancelBoundaryRunner` captures a token-bearing cancellation reason and returns token-bearing runner errors; `ContinueErrorRunner` returns a token-bearing continue-send error; assertions inspect Runner input, direct CoreResponse, and persisted event text
exit_code: fmt=0; cancel focused=0 (5/5); continue-send focused=0 (1/1); core control-plane=0 (53/53); daemon control-plane=0 (13/13); daemon_host=0 (58/58); kiana-tools lib=0 (198/198); workspace check=0; workspace clippy=0; workspace tests=0; diff check=0; existing non-fatal warnings remain
artifact paths and SHA-256: no persistent artifact retained; temporary test state was cleaned by fixtures; no artifact hash generated
status change: P1-05 remains partial; cancellation reasons are sanitized at ControlPlane ingress before Runner dispatch, Runner send/failure responses, direct CoreResponse, and event append; continue Runner send errors now receive the same direct-response redaction boundary
proof-level change: local_behavior negative evidence that token-bearing cancel/continue reasons do not cross Runner, direct-response, or event boundaries; full workspace source/build/test regression remained at exit 0; no durable/live/physical claim
limitations: structured-JSON/key/marker redaction does not prove arbitrary encoded-secret detection; raw provider process memory and complete argv/environment/stdout/stderr scanning remain outside this slice; durable projector/CAS recovery and immutable session principal/role binding remain open
reviewer: implementation plus independent `/root/next_slice_audit` adversarial review; no slice-local blocker reported
```

### P1-02 approval event-store read failure evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-core --test control_plane approval_run_lookup_failure_fails_closed_before_consumption_or_execution --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo check --workspace --locked --offline
  git diff --check
cwd/environment: repository root; Linux 6.8.0-138-generic x86_64; rustc 1.97.1; cargo 1.97.1; locked/offline dependency resolution; serialized core tests
fixture or cassette: active local-write approval backed by an EventStore whose `read_all` returns `PortError::Unavailable`; TestApprovalStore and CountingBroker inspect consumption and dispatch
exit_code: fmt=0; focused=0 (1/1); core control-plane=0 (55/55); workspace check=0; diff check=0; existing non-fatal warnings remain
artifact paths and SHA-256: no persistent artifact retained; in-memory approval/event assertions and test stdout are the evidence artifacts; no artifact hash generated
status change: P1-02 remains partial; approval run-association lookup now propagates EventStore read failures instead of treating them as an empty event set, so approval consumption and Broker execution do not proceed on an unavailable event store
proof-level change: local_behavior negative evidence for fail-closed approval lookup, preserving an active unconsumed approval and zero Broker calls on a read failure
limitations: durable approval/PendingInvocation state, cross-process recovery, and an independently persisted approval/run index remain open; callers receive a structured Core/Port failure rather than a durable recovery result
reviewer: implementation plus independent `/root/next_slice_audit` port-contract review; no slice-local blocker reported
```

### P1-01 immutable session role/department evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-core --test control_plane session_role_and_department_are_immutable_for_continue_cancel_and_receipt --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane persisted_receipt_rejects_role_department_mismatch --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo check --workspace --locked --offline
  git diff --check
cwd/environment: repository root; Linux 6.8.0-138-generic x86_64; rustc 1.97.1; cargo 1.97.1; locked/offline dependency resolution; serialized core tests
fixture or cassette: completed Builder session followed by forged `pm/planning` continue, cancel, and receipt requests; persisted run.authorized identity with builder/executing role snapshot
exit_code: fmt=0; two focused tests=0 (1/1 each); core control-plane=0 (57/57); workspace check=0; diff check=0; existing non-fatal warnings remain
artifact paths and SHA-256: no persistent artifact retained; in-memory events, response assertions, and test stdout are the evidence artifacts; no artifact hash generated
status change: P1-01 remains partial; SessionBinding now fixes actor, canonical project, role, and department for live continue/cancel/receipt ownership, and new run.authorized events persist role/department for restart receipt checks; legacy identity events without those fields remain compatible
proof-level change: local_behavior negative evidence that a same actor/project cannot swap role or department to continue, cancel, or read a live/persisted run receipt
limitations: principal authentication is still a fixed local daemon identity; SessionBinding and pending state remain process-local; legacy events missing role/department cannot prove the older assignment; durable cross-process principal/session recovery and tenant boundaries remain open
reviewer: implementation plus independent `/root/next_slice_audit` identity-boundary review; no slice-local blocker reported
```

### P1-01 direct role/department assignment boundary evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-core --test control_plane start_run_rejects_role_department_mismatch_before_runner --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo check -p kiana-core --locked --offline
  cargo check --workspace --locked --offline
  git diff --check
cwd/environment: repository root; Linux 6.8.0-138-generic x86_64; rustc 1.97.1; cargo 1.97.1; locked/offline dependency resolution; serialized core tests
fixture or cassette: trusted context forged with `role_id=pm` and `department_id=executing`; `CountingRunner` records attempted Runner dispatch; in-memory EventLog inspects the rejection
exit_code: fmt=0; mismatch focused=0 (1/1); core control-plane=0 (54/54); core check=0; workspace check=0; diff check=0; existing non-fatal warnings remain
artifact paths and SHA-256: no persistent artifact retained; in-memory event and Runner-call assertions are the evidence artifacts; no artifact hash generated
status change: P1-01 remains partial; direct ControlPlane `start_run` now rejects a valid role paired with a non-matching nonempty department before sandbox authorization or Runner dispatch and records `run.rejected` with `role_department_mismatch`
proof-level change: local_behavior negative evidence for malformed role/department assignment and no-Runner-effect path; no durable identity or cross-request session-binding claim
limitations: SessionBinding currently binds actor/project but not immutable role/department; continue/cancel/receipt ownership and durable principal recovery remain open; DaemonHost and direct-core assignment semantics are covered separately
reviewer: implementation plus independent `/root/next_slice_audit` adversarial review; no slice-local blocker reported
```

### P1-02 approval read capability distinction evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-core --test control_plane unsupported_approval_run_lookup_preserves_legacy_direct_approval --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane approval_run_lookup_failure_fails_closed_before_consumption_or_execution --locked --offline -- --test-threads=1
  cargo check --workspace --locked --offline
  cargo clippy --workspace --all-targets --locked --offline
  cargo test --workspace --locked --offline --no-fail-fast
  git diff --check
cwd/environment: repository root; Linux 6.8.0-138-generic x86_64; rustc 1.97.1; cargo 1.97.1; locked/offline dependency resolution; serialized focused tests
fixture or cassette: a read_request-only EventStore using the default `event_store_read_all_unsupported` response for a direct approval, plus a separate EventStore returning `PortError::Unavailable`; TestApprovalStore and CountingBroker inspect approval state and execution
exit_code: fmt=0; unsupported compatibility focused=0 (1/1); unavailable fail-closed focused=0 (1/1); workspace check=0; workspace clippy=0; workspace tests=0; diff check=0; existing non-fatal warnings remain
artifact paths and SHA-256: no persistent artifact retained; in-memory approval/event assertions and command output are the evidence artifacts; no artifact hash generated
status change: P1-02 remains partial; approval Run-association lookup now treats only the explicit unsupported capability as an unknown association for legacy direct approvals, while propagating real EventStore read failures before approval consumption or Broker execution
proof-level change: local_behavior positive compatibility and negative fail-closed evidence for the two distinct EventStore outcomes; no durable/live/physical claim
limitations: an adapter that cannot scan events cannot prove a persisted Run association; durable PendingInvocation/Runner recovery, an independent approval/run index, and mandatory authenticated wire proof remain open
reviewer: implementation plus independent `/root/approval_context_audit2` ApprovalStore contract audit; product DaemonHost binding remains authoritative and no slice-local blocker was reported
```

### P1/P2 EventStore receipt-read and terminal replay evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-core --test control_plane receipt_ --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane review_author_lookup --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo check -p kiana-core --locked --offline
  git diff --check
cwd/environment: repository root; Linux 6.8.0-138-generic x86_64; rustc 1.97.1; cargo 1.97.1; locked/offline dependency resolution; serialized control-plane tests
fixture or cassette: MemoryEventLog terminal-event permutations; an EventStore returning an actual read failure; a read-request-only EventStore; and a stream-only EventStore with independently addressable Run aggregates
exit_code: fmt=0; receipt filter=0 (12/12); review lookup filter=0 (2/2); core control-plane=0 (67/67); core check=0; diff check=0; existing non-fatal dead-code warning remains
artifact paths and SHA-256: no persistent artifact retained; in-memory event assertions and test stdout are the evidence artifacts; no artifact hash generated
status change: P1-02 and P2-01 remain partial; actual EventStore read failures now propagate, persisted Receipt lookup uses only full history or an exact Run stream rather than a caller request fallback, and incomplete/conflicting/errorless terminal histories project to Unknown/Failed/Cancelled without inventing Completed
proof-level change: local_behavior negative and compatibility evidence for failed versus explicitly unsupported reads, target-Run receipt isolation, exact-stream recovery, review failure propagation, and terminal replay semantics; no durable/live/physical claim
limitations: current-command receipt construction may still use a filtered in-process request stream only when full history is explicitly unsupported; public persisted receipt/review requires a full scan or exact Run stream; no durable Session/Run/Invocation projector, reconciliation queue, provider verification, or cross-process PendingInvocation recovery is claimed
reviewer: implementation plus independent `/root/event_read_audit` EventStore contract audit; its receipt-relabel and unsupported-read findings are covered by the listed regressions
```

### P1-05 persisted Run result-correlation evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (71 status entries); existing user changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-core --test control_plane receipt_ --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane review_ --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane close_without_run_id_does_not_accept_a_foreign_run_completion --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo check -p kiana-core --locked --offline
  git diff --check
cwd/environment: repository root; Linux 6.8.0-138-generic x86_64; rustc 1.97.1; cargo 1.97.1; locked/offline dependency resolution; serialized core tests
fixture or cassette: MemoryEventLog with a target run.authorized plus foreign run.completed/capability.completed under the same RequestId; a fresh ControlPlane reconstructing one persisted Builder Run without an in-memory SessionBinding
exit_code: fmt=0; receipt filter=0 (13/13); review filter=0 (11/11); closer negative=0 (1/1); core control-plane=0 (71/71); core check=0; diff check=0; existing non-fatal dead-code warning remains
artifact paths and SHA-256: temporary per-test project artifacts are retained only in OS temp fixtures; no persistent artifact or SHA-256 generated; in-memory event assertions and test stdout are the evidence artifacts
status change: P1-05 remains partial; Receipt, Reviewer, and Closer now bind event projection to an exact Run payload/aggregate identity rather than expanding through a shared RequestId; restart lookup accepts exactly one complete persisted Builder authorization and rejects zero, ambiguous, or incomplete identities
proof-level change: local_behavior negative evidence that a persisted foreign Run cannot forge a Completed Receipt, file-change list, accepted Review/Merge artifact, or author completion for close; positive reconstruction evidence covers one unambiguous persisted Builder Run
limitations: legacy events lacking data.run_id are usable only with exact durable run aggregate metadata; current-command receipt construction still has its documented request-scoped compatibility path when global history is explicitly unsupported; no durable Session/Run/Invocation projector, reconciliation queue, provider-effect verification, or cross-process PendingInvocation recovery is claimed
reviewer: implementation plus independent `/root/s1_gap_audit` result-correlation audit; its explicit Receipt and no-session-map Reviewer/Closer findings are covered by the listed regressions
```

### P2-01 EventStore stream-read CAS fail-closed evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: pre-existing multi-crate WIP retained; this slice changes ControlPlane aggregate-version lookup and its focused EventStore contract fixtures without reset, commit, push, merge, or worktree deletion
command_argv:
  cargo test -p kiana-core --test control_plane event_stream_read_failure_prevents_append_and_runner_dispatch --locked --offline -- --test-threads=1  # pre-fix expected failure
  cargo test -p kiana-core --test control_plane event_stream_read_failure_prevents_append_and_runner_dispatch --locked --offline -- --test-threads=1  # post-fix
  cargo fmt --all --check
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo check -p kiana-core --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; offline dependency resolution; serialized control-plane integration test execution
fixture or cassette: EventStore whose `read_stream` returns `event_stream_read_unavailable` while its append path would otherwise accept writes; CountingRunner proves no dispatch; read-all-unavailable fixtures delegate exact streams only where their declared scenario requires writes
exit_code: pre-fix focused test=101 (expected regression reproduction: event append and Runner dispatch occurred); post-fix focused test=0 (1/1); full core control-plane=0 (72/72); fmt=0; core check=0; diff check=0; existing non-fatal MemoryCellRegistry dead-code warning remains
artifact paths and SHA-256: no persistent artifact retained; in-memory EventStore/Runner assertions and test stdout are the evidence artifacts; no artifact hash generated
status change: P2-01 remains partial; ControlPlane now propagates aggregate stream-read failures before deriving an expected version, so it neither appends an unverifiable CAS event nor dispatches the Runner after that failure
proof-level change: local_behavior negative evidence for the first-write/empty-stream ambiguity boundary and compatibility evidence for adapters with independently readable aggregate streams but no global scan
limitations: this does not make all legacy adapters aggregate-stream capable, prove cross-process CAS, or rebuild durable Session/Run/Invocation state; disk-full, torn-tail reconciliation, approval continuation, provider-effect verification, and cross-process PendingInvocation recovery remain open
reviewer: independent `/root/s0_review` EventStore contract audit identified the unsafe fallback; focused regression covers its read-failure/write-success counterexample
```

### P2-01 JSONL valid-unterminated-tail recovery evidence (2026-09-06)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user/WIP changes preserved; no commit/reset/push/merge performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-eventlog --locked --offline
  cargo clippy -p kiana-eventlog --all-targets --locked --offline
  cargo test -p kiana-core --locked --offline
  cargo test -p kiana-daemon --locked --offline
  cargo check --workspace --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked/offline dependencies; daemon/core tests use their existing serialized fixtures where configured
fixture or cassette: JSONL file containing one valid RuntimeEvent without a trailing newline; reopen repairs and fsyncs the delimiter before a second append, then a fresh reopen reads both events; malformed complete final lines remain fail-closed
exit_code: 0 for every listed command; kiana-eventlog 25/25; kiana-core lib 7/7, control-plane 80/80, dependency boundaries 2/2; kiana-daemon lib 53/53, control-plane 13/13, daemon_host 58/58; workspace check 0; clippy 0; diff check 0
status change: P2-01 remains partial; JsonlEventLog now repairs a legal unterminated final record before future appends, preserving line framing across close/reopen and preventing concatenated JSON records
proof-level change: local_behavior plus disk reopen/append evidence for this JSONL crash-recovery boundary; no claim that the full Session/Run/Invocation projector or cross-process lifecycle recovery is durable
limitations: repair still depends on the local JSONL adapter and process lock; malformed complete records, disk-full/permission failures, aggregate projector rebuild, pending approval/Runner state, and cross-process lifecycle recovery remain open
reviewer: focused eventlog recovery review with core/daemon regression suite; no assertions weakened and no tests skipped
```

### P1-03 cancellation confirmation and pre-signalled fence evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (71 status entries); existing user changes preserved; no commit, reset, push, merge, or worktree deletion
command_argv:
  cargo test -p kiana-core --test control_plane unconfirmed_cancel_result_is_unknown_without_cancelling_or_forgetting_the_run --locked --offline -- --test-threads=1  # pre-fix expected failure
  cargo test -p kiana-core --test control_plane concurrent_continue_cancel_runner_not_found_preserves_fencing_before_dispatch --locked --offline -- --test-threads=1  # pre-fix expected failure
  cargo test -p kiana-core --test control_plane concurrent_continue_cancel_runner_not_found_preserves_fencing_before_dispatch --locked --offline -- --test-threads=1  # post-fix
  cargo test -p kiana-core --test control_plane cancel --locked --offline -- --test-threads=1
  cargo fmt --all --check
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo check -p kiana-core --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked/offline; serialized test target
fixture or cassette: direct fake Runner responses (non-cancel failure, foreign run, target Completion conflict, run_not_found) and a deterministic Continue/Cancel race where Continue has taken runner state and later emits CapabilityRequested; CountingBroker assertions
exit_code: pre-fix unconfirmed=101 (incorrect Cancelled projection); pre-fix concurrent race=101 (Continue Completed / expected Cancelled and broker path allowed); post-fix race=0 (1/1); cancel suite=0 (11/11); fmt=0; core control-plane=0 (76/76); core check=0; diff check=0; existing nonfatal MemoryCellRegistry dead-code warning remains
artifact paths and SHA-256: kiana-core/src/lib.rs; kiana-core/tests/control_plane.rs; CURRENT_STATUS.md; no persistent fixture or cassette created and no SHA-256 generated
status change: P1-03 remains partial; direct cancellation accepts only an unambiguous target `cancelled:` response; wrong, mixed, conflicting, transport, no-confirmation, and run_not_found responses record `run.result_unknown`; an already signalled cancellation blocks policy, pre-tool, and Broker dispatch and resolves the race with a biased cancellation branch
proof-level change: local_behavior negative evidence for the covered fake-runner paths; the deterministic race proves no Broker call after the pre-signalled fence
limitations: runner, cancellation, and session state remain in memory and there is no atomic per-Run lifecycle epoch; confirmed cancellation cleanup still needs that future lifecycle proof against a newly starting continuation; no durable restart reconciliation, cgroup/process-tree proof, kill-9 recovery, cross-process pending invocation, or real-world effect proof is claimed
reviewer: independent `/root/cancel_confirmation_audit` and `/root/cancel_stale_session_audit`; their ambiguous-response and no-`!inflight` cleanup findings are covered by the focused tests and stated limitation
```

### P1-05 Runner event Run-ID provenance evidence (2026-09-02)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout (71 status entries); existing user changes preserved; no commit, reset, push, merge, or worktree deletion
command_argv:
  cargo test -p kiana-core --test control_plane foreign_runner_completion_is_unknown_without_receipt_projection --locked --offline -- --test-threads=1  # pre-fix expected failure
  cargo test -p kiana-core --test control_plane foreign_runner_ --locked --offline -- --test-threads=1  # post-fix
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo check -p kiana-core --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain; locked/offline; serialized control-plane test target
fixture or cassette: `ForeignRunEventRunner` returns a target `Started` followed by either a foreign `CapabilityRequested` or foreign `Completed`; `CountingBroker` and a public Receipt read assert that neither capability dispatch nor completion/receipt projection is accepted
exit_code: pre-fix foreign completion=101 (target Run incorrectly returned Completed with foreign output); post-fix foreign runner filter=0 (2/2); full core control-plane=0 (78/78); core check=0; fmt=0; diff check=0; existing nonfatal MemoryCellRegistry dead-code warning remains
artifact paths and SHA-256: kiana-core/src/lib.rs; kiana-core/tests/control_plane.rs; CURRENT_STATUS.md; no persistent fixture or cassette created and no SHA-256 generated
status change: P1-05 remains partial; `drive_run` now rejects every Runner event whose `run_id` differs from its expected Run before event projection, Broker dispatch, or terminal receipt construction, recording only target `run.result_unknown`; the shared guard is used by Start, Continue, and capability-result drive paths
proof-level change: local_behavior negative evidence for target Start with foreign capability and completion events, including a public Receipt replay; source-level shared-entry coverage for Continue and capability-result paths; no durable provenance claim
limitations: this validates only the event Run ID from the local RunnerPort boundary; it does not establish durable runner/provider provenance, a reconciliation queue, cross-process Runner recovery, process-tree cancellation confirmation, or real-world side-effect verification; `result_unknown` lifecycle cleanup still lacks an atomic per-Run epoch
reviewer: independent `/root/runner_event_identity_audit` identified the pre-guard foreign capability, completion, failure, and aggregate-projection risks; focused regressions cover the capability and completion counterexamples
```

## 5. 当前禁止的表述

在相应证据出现前，不得使用：

- “所有测试已通过”或“当前全绿”；
- “已完成 live provider”；
- “已支持跨进程恢复”；
- “已支持支付、预订、IoT 或物理控制”；
- “loopback 已完成认证”；
- “Receipt 已证明现实世界结果正确”；
- “Company OS 细胞分裂已完成”；
- “企业级、生产级、签名发布已完成”。

## 6. 更新协议

每次更新本文件必须包含：

```text
source_snapshot
worktree_status
command_argv
cwd/environment
fixture or cassette
exit_code
artifact paths and SHA-256
status change
proof-level change
limitations
reviewer
```

状态只有在实现、测试和证据一致时才能提升；失败、Unknown、skipped、flaky 或文档勾选不能提升证明等级。

### App-server schema 与 approval actor 修复证据 (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; added docs/schemas app-server contracts and daemon approval identity guard; no reset or cleanup
command_argv:
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --locked --offline
  cargo test -p kiana-entrypoints direct_connect_app_contract_schemas_match_packaged_schema_files --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized focused tests
fixture or cassette: direct-connect contract fixture, approval context mismatch fixture, daemon host cassette fixtures
exit_code: 0 for all listed commands; schema 1/1, control_plane 11/11, daemon_host 51/51 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: app-server schema documentation now matches all advertised kiana.app-server endpoints; approval actor mismatch is blocked before core consumption
proof-level change: local_behavior regression evidence for schema parity and fail-closed approval identity handling
limitations: full Gate 0 remains blocked by legacy entrypoints CLI/SDK tests; approval identity is a fixed local daemon principal, not durable authentication; schema files are minimal envelope contracts rather than complete endpoint payload schemas
reviewer: focused verification; no claim of full workspace green, durable identity, or production app-server compatibility
```

### P1-05 event payload redaction evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; capability event arguments/results now pass through recursive field redaction; no reset or cleanup
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-core --lib --locked --offline event_redaction -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized tests
fixture or cassette: nested secret/token event payload unit fixture and existing control-plane capability fixtures
exit_code: 0; redaction 1/1, core control_plane 34/34 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-05 changed from target to partial; event request/result payloads redact sensitive value fields while preserving secret references and file paths
proof-level change: local_behavior evidence for recursive event-field redaction at the ControlPlane event boundary
limitations: redaction is key-based rather than schema-generated; error-string redaction and complete aggregate/CAS event correlation remain open; original capability results remain available to the runner
reviewer: focused implementation review; no claim of complete event privacy, durable audit authority, or workspace-wide green

### P1-05 correlation and regression evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; capability event redaction/correlation helper formatted and verified; no reset or unrelated cleanup
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-core --lib --locked --offline event_redaction -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; stable toolchain; offline dependency resolution; serialized regression tests
fixture or cassette: existing core/daemon control-plane fixtures, harness capability fixtures, and event redaction unit fixture
exit_code: 0 for formatting, core 34/34, redaction 1/1, daemon control-plane 11/11, daemon host 51/51, and diff check
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P1-05 remains partial; capability request arguments and successful capability results on both harness and direct approval paths now receive recursive event redaction; harness completed payloads include run/session/capability/operation/request correlation fields
proof-level change: local_behavior regression evidence for event-boundary redaction and focused CompanyOS control-plane/daemon compatibility
limitations: correlation is covered for the harness capability event shape but not yet fully asserted for every capability result shape; key-based and common key-value error redaction are implemented, while arbitrary secret-bearing error formats, aggregate/CAS event authority, and durable audit recovery remain open
reviewer: focused implementation review plus serialized core/daemon regression suite; no claim of durable event privacy or workspace-wide green

### P2 aggregate metadata compatibility evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; additive optional RuntimeEvent aggregate metadata and ControlPlane population; no reset or unrelated cleanup
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo test -p kiana-domain --locked --offline
  cargo test -p kiana-eventlog --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; stable toolchain; offline dependency resolution; serialized regression tests
fixture or cassette: RuntimeEvent legacy JSON compatibility fixture plus existing eventlog/core/daemon CompanyOS fixtures
exit_code: 0 for formatting, domain 15/15, eventlog 9/9, core 34/34, daemon host 51/51, and diff check
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: RuntimeEvent now accepts optional aggregate_type, aggregate_id, stream_version, and idempotency_key metadata; ControlPlane writes request aggregate identity and stream_version alongside its existing CAS sequence
proof-level change: source + local_behavior compatibility evidence for constructing aggregate metadata and reading legacy events without those fields
limitations: metadata is optional for compatibility; stream identity remains request_id; idempotent append semantics, cross-process recovery, half-write recovery, durable state/approval persistence, and complete aggregate event authority remain open; no durable upgrade is claimed
reviewer: focused implementation review plus serialized domain/eventlog/core/daemon regression suite; P2 remains partial
```

### P2 idempotent append evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; additive EventAppendResult and Memory/JSONL idempotent append implementations; no reset or unrelated cleanup
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo test -p kiana-ports --locked --offline
  cargo test -p kiana-eventlog --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; stable toolchain; offline dependency resolution; serialized regression tests
fixture or cassette: MemoryEventLog and JsonlEventLog idempotency fixtures plus existing core and daemon CompanyOS fixtures
exit_code: 0 for formatting, ports, eventlog 14/14, core 34/34, daemon host 51/51, and diff check
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: EventStorePort now exposes append_idempotent and append_idempotent_expected; ControlPlane event writes now supply a stable request_id/sequence key, while Memory/JSONL return the original event for an identical key and reject a different payload under the same key
proof-level change: local_behavior adapter and control-plane evidence for replay without append, payload mismatch rejection, JSONL reopen/replay behavior, concurrent independent JSONL instances producing one record, fresh reads from an already-open instance, and compatibility with disk receipt recovery
limitations: the default compatibility implementation of append_idempotent_expected delegates versioned writes to append_expected for legacy adapters; JSONL reads currently take the exclusive process lock; no kill-9/half-write recovery, durable state/approval persistence, or complete aggregate event authority; no durable upgrade is claimed
reviewer: focused contract review plus serialized eventlog/core/daemon regression suite; P2 remains partial
```

### P1-03/P1-04 process and filesystem fencing evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; process-group and apply_patch transaction hardening added without reset
command_argv:
  cargo check -p kiana-daemon --locked --offline
  cargo test -p kiana-daemon --lib harness_capabilities --locked --offline
  cargo test -p kiana-daemon --lib apply_patch --locked --offline
  cargo test -p kiana-daemon --test daemon_host model_role_arguments_cannot_widen_memory_grants --locked --offline -- --test-threads=1
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependencies
fixture or cassette: shell descendant timeout fixture, apply_patch multi-file fixture, symlink/hardlink fixtures, model role-spoof memory fixture
exit_code: 0 for all listed focused commands; harness 10/10, apply_patch 15/15, daemon identity 1/1
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P1-03 and P1-04 remain partial; process-group termination, output timeout cleanup, full patch preflight, rollback-on-commit-failure, file identity checks, symlink/hardlink rejection, and server-stamped memory identity are covered locally
proof-level change: local_behavior negative and failure-path evidence for descendant cancellation, patch partial-commit rollback, path alias rejection, effect-time version change detection, and role-argument spoofing
limitations: Unix path checks are not fd-relative/no-follow atomic fencing against every external rename race; cancellation still has process-local ControlPlane state; no durable principal, grant, approval, or reconciliation upgrade
reviewer: focused adversarial inspection; no claim of complete TOCTOU or durable cancellation proof
```

### P1-01 Web bearer and ownership guard evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; loopback Web API bearer/Host/Origin guard added without reset
command_argv:
  cargo test -p kiana-entrypoints --test cli_web --locked --offline
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependencies
fixture or cassette: loopback Web subprocess with page-issued per-instance token and cassette write
exit_code: 0; cli_web 4/4, format and diff checks passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P1-01 remains partial; Web mutation and state routes now require a per-instance bearer token and loopback Host/Origin, while unknown requested sessions fail closed
proof-level change: local_behavior evidence for authenticated Web mutation path and token propagation through the same DaemonHost
limitations: token is local-process bearer material delivered to the same-origin page; no durable authenticated principal, multi-user tenancy, CSRF framework, or cross-process session ownership
reviewer: focused Web ownership review; no claim of enterprise authentication
```

### P2 aggregate CAS and torn-tail recovery evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; aggregate-aware EventLog CAS and final-line recovery added without reset
command_argv:
  cargo test -p kiana-eventlog --locked --offline
  cargo test -p kiana-domain --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo check --workspace --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized CompanyOS suites
fixture or cassette: aggregate events across request IDs, CAS stale writer, torn JSONL final line, legacy domain packet and approval fixtures
exit_code: 0; eventlog 17/17, domain 22/22, core 34/34, daemon_host 52/52, workspace check and diff check passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P2 remains partial; aggregate metadata now participates in stream version/conflict checks and a malformed final unterminated line is repaired only after prior valid events
proof-level change: local_behavior evidence for cross-request aggregate CAS, idempotent/recovery compatibility, and fail-closed malformed-first-line handling
limitations: aggregate metadata remains optional for legacy compatibility; event authority, approval persistence, state/receipt rebuild, disk-full handling, and cross-process durable recovery remain incomplete
reviewer: focused event-store and compatibility review; no durable release claim
```

### P1/P2 adversarial review follow-up evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; lock-order, binary snapshot, process-group test, and client-scope corrections applied without reset
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-daemon --lib --locked --offline
  cargo test -p kiana-eventlog --locked --offline
  cargo test -p kiana-domain --locked --offline
  cargo test -p kiana-protocol --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-entrypoints --test cli_web --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized affected-crate suites
fixture or cassette: binary delete rollback, descendant process-group timeout, aggregate EventLog append/read lock ordering, bearer-authenticated Web route, server-ignored packet metadata
exit_code: 0 for all listed commands; daemon lib 41/41, eventlog 18/18, domain 22/22, protocol 9/9, core 34/34, daemon_host 52/52, cli_web 4/4
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: no proof-level upgrade; previously identified review regressions are corrected, while P1/P2 remain partial
proof-level change: local_behavior evidence now includes binary-safe rollback, lock-order regression coverage, corrected process-group observation, authenticated Web mutation, and rejection of client-forged packet scope
limitations: approval hash/nonce remains optional for legacy in-process calls; apply_patch is not yet fd-relative/no-follow atomic against every external rename race; eventlog durable authority and authenticated identity remain incomplete; workspace-wide entrypoint baseline still has unrelated legacy failures
reviewer: adversarial review rerun after fixes; no claim of workspace-wide green or durable recovery
```

### P1/P2 final correction evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; EventStore lock order/idempotent CAS, binary-safe patch snapshots, strict bind allowlist, Web cancellation cleanup, and packet terminal validation corrected without reset
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-eventlog --locked --offline
  cargo test -p kiana-daemon --lib --locked --offline
  cargo test -p kiana-domain --locked --offline
  cargo test -p kiana-protocol --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-entrypoints --test cli_web --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized affected-crate suites
fixture or cassette: append/read lock ordering, idempotent expected replay, binary patch delete rollback, process-group timeout, strict 127.0.0.1/::1 bind, Web token and cancellation guards, terminal packet rejection
exit_code: 0 for all listed commands; eventlog 18/18, daemon lib 41/41, domain 22/22, protocol 9/9, core 34/34, daemon_host 52/52, cli_web 4/4
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P1-01/P1-03/P1-04 and P2 remain partial; no durable proof upgrade
proof-level change: local_behavior regression evidence now covers the reviewed correctness fixes and keeps client-declared packet scope out of server Context
limitations: approval proof remains optional for legacy in-process APIs; path effect fencing is not fd-relative/no-follow; Web token is process-local; EventLog and approval/session state are not fully durable; release smoke remains blocked by legacy entrypoints tests
reviewer: adversarial review findings addressed where safe; known limitations retained explicitly
```

### P1/P2 final audit correction evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; final binary-delete, torn-tail classification, aggregate lock-order, idempotent CAS, and state-machine regressions corrected without reset
command_argv:
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo test -p kiana-eventlog --locked --offline
  cargo test -p kiana-daemon --lib --locked --offline
  cargo test -p kiana-domain --locked --offline
  cargo test -p kiana-protocol --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-entrypoints --test cli_web --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized affected-crate suites
fixture or cassette: binary Delete File success and rollback, only-EOF torn-tail repair, malformed final-line fail-closed, append/read concurrency, expected idempotent retry, blocked/awaiting packet resume, strict Web auth, process-group timeout
exit_code: 0 for every listed command; eventlog 20/20, daemon lib 41/41, domain 22/22, protocol 9/9, core 34/34, daemon_host 52/52, cli_web 4/4
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: no broad proof upgrade; P1/P2 remain partial and the current product ceiling remains local_behavior
proof-level change: final local_behavior evidence excludes false-green tests and confirms the reviewed fixes are covered by executable regressions
limitations: full CompanyOS cells/scheduler, mandatory wire approval proof, fd-relative TOCTOU fencing, durable authenticated principal, durable approval/session recovery, and external adapters remain incomplete; release smoke still fails on legacy entrypoints tests
reviewer: final serialized verification and adversarial review; no commit/push/release performed
```

### P2 durable approval fact evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; approval persistence added to the local composition root without changing scripted in-memory fixtures
command_argv:
  cargo check -p kiana-daemon --locked --offline
  cargo test -p kiana-daemon --lib approval_store --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-entrypoints --test cli_run --locked --offline -- close
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized approval/CompanyOS suites
fixture or cassette: persisted Active approval reopen, single-use consume, canonical proof, expiry/cancel, existing daemon/core approval and close fixtures
exit_code: 0 for all listed commands; approval_store 7/7, core 34/34, daemon control_plane 11/11, daemon_host 54/54, CLI negative suite 18/18
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P2 approval moved from process-only storage to partial durable fact persistence in the real local DaemonHost; P2 remains partial
proof-level change: local_behavior plus durable-reopen unit evidence for challenge/request/binding/state persistence, atomic temp-file replacement, process lock, duplicate-ID rejection, and single-use replay
limitations: PendingInvocation and Runner state remain process-local; multiple independent daemon instances do not yet merge stale approval snapshots; mandatory wire proof and authenticated principal remain open; no cross-process workflow recovery claim
reviewer: focused durable-state review; no claim of durable end-to-end run recovery
```

### P2 cross-instance approval CAS evidence (2026-08-30)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: target approval CAS and independent-record merge added without reset
command_argv:
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo test -p kiana-daemon --lib approval_store --locked --offline
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized approval/CompanyOS suites
fixture or cassette: two disk stores adding independent approvals, stale Active snapshot after another store consumed the same approval, existing approval/cancel/close fixtures
exit_code: 0; approval_store 9/9, daemon_host 54/54, core 34/34, workspace check and format passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P2 durable approval remains partial but now uses target-record CAS and preserves independent approval records across store instances
proof-level change: local_behavior/durable-reopen evidence that stale instances cannot overwrite a consumed approval and later snapshots retain unrelated approvals
limitations: approval consumers opened before a newly staged unrelated record still require refresh to address that record; PendingInvocation/Runner/session state remains process-local; mandatory wire proof, durable principal and full state projector remain open
reviewer: focused adversarial approval-store review; no cross-process run recovery claim
```

### P2 run aggregate event evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; ControlPlane now derives run aggregate identity and CAS cursor from authoritative run event data
command_argv:
  cargo check -p kiana-core -p kiana-daemon -p kiana-protocol --locked --offline
  cargo test -p kiana-core --test control_plane start_run_brokers_harness_tools --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized affected-crate suites
fixture or cassette: harness run event sequence, continuation/cancel/approval cursor compatibility, legacy custom EventStore fallback
exit_code: 0 for every listed command; focused run 1/1, core 34/34, daemon_host 54/54
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P2 aggregate event stream moved from metadata-only to partial runtime enforcement; P2 remains partial
proof-level change: local_behavior evidence that harness Run events share aggregate_type=run, aggregate_id=run_id, and monotonic stream_version across the run
limitations: request events without run identity remain request streams; aggregate CAS is not yet a complete durable state/receipt projector; custom legacy stores use sequence fallback; no cross-process pending-run recovery claim
reviewer: focused event cursor review; prior empty-stream fallback conflict fixed and regression-tested
```

### P3 WorkPacket responsibility-chain evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; spawn_from_packet now records canonical WorkPacket lifecycle events and preserves separate Run aggregate events
command_argv:
  cargo test -p kiana-core --test control_plane spawn_from_packet_starts_a_fresh_builder_session --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized packet/run suites
fixture or cassette: fresh Builder packet spawn, packet/run split streams, packet success, approval/cancel and disk receipt compatibility
exit_code: 0; focused packet 1/1, core 34/34, daemon_host 54/54, format and diff checks passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P3 packet runtime changed from prompt-only projection to partial responsibility-chain enforcement; overall P3 remains partial
proof-level change: local_behavior evidence for packet draft/approved/assigned/running/succeeded lifecycle events, independent packet aggregate CAS, path-lock ownership, and fresh Builder execution
limitations: ACK is represented by server-side packet.accepted but full external handoff/acceptor persistence is not implemented; packet status is not yet rebuilt as a durable State Store projection; failure compensation, MergeReceipt runtime, and multi-Cell scheduling remain open
reviewer: serialized core/daemon lifecycle verification; no durable end-to-end workflow claim
```

### P3 MergeReceipt runtime evidence (2026-08-30)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; Review now emits a ControlPlane-generated gate/MERGE.json and Close validates it before ClosingReceipt
command_argv:
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo test -p kiana-core --test control_plane review_author_run_uses_a_fresh_reviewer_session --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized Review/Close suites
fixture or cassette: independent Builder run, passing Review, MergeReceipt validation, Close mismatch/missing-review denial
exit_code: 0; focused review 1/1, core 34/34, daemon_host 54/54, workspace check and diff check passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P3 MergeReceipt changed from domain-only type to partial runtime enforcement; P3 remains partial
proof-level change: local_behavior evidence that needs_change reviews do not produce merge permission, passing reviews produce merge receipts, and Close rejects absent or mismatched merge receipts
limitations: MergeReceipt is still a local artifact rather than a durable State Store aggregate; no automatic merge of source trees, no external acceptor, no cross-process workflow recovery
reviewer: focused Review→Merge→Close verification; no durable or production claim
```

### Slice A/B/C canonical CompanyOS contract evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; canonical domain IDs, state machines, packet fields, and contract objects added without reset
command_argv:
  cargo test -p kiana-domain --locked --offline
  cargo test -p kiana-protocol --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo fmt --all --check
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution
fixture or cassette: legacy WorkPacket JSON compatibility, domain transition matrix, child-grant subset and budget fixtures, existing core/daemon CompanyOS fixtures
exit_code: 0 for all listed commands; domain 22/22, protocol 9/9, core 34/34, daemon_host 52/52
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: Slice A/B/C moved from intent-only shape to canonical source contracts with local validation tests; runtime Cell/SpawnPlan/Grant persistence and scheduling remain target
proof-level change: source + local_behavior contract evidence for stable IDs, illegal transition rejection, legacy packet compatibility, budget bounds, template validation, and monotonic child grant scope
limitations: new CompanyOS objects are not yet durable aggregates or daemon schedulers; WorkPacket responsibility ACK, full delegation authorization, and lifecycle event projection remain open
reviewer: focused domain/protocol review; no claim of complete CompanyOS workflow
```

### P2 durable approval and run aggregate final evidence (2026-08-29)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; durable approval facts, aggregate run cursor, and closing workflow compatibility verified without reset
command_argv:
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo clippy --workspace --all-targets --locked --offline
  cargo test -p kiana-daemon --lib approval_store --locked --offline
  cargo test -p kiana-eventlog --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-entrypoints --test cli_web --locked --offline
  bash scripts/harness-golden-smoke.sh
  bash scripts/v10-workbench-smoke.sh
  bash scripts/v10-p0-closeout-smoke.sh
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized affected-crate verification
fixture or cassette: approval reopen/consume, aggregate run stream, shell/apply_patch safety, Web bearer, independent Review→Closer receipt, golden/workbench/P0 smoke
exit_code: 0 for every listed command; approval_store 7/7, eventlog 20/20, core 34/34, daemon control_plane 11/11, daemon_host 54/54, cli_web 4/4, smoke gates passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P2 approval facts and run aggregate cursor are partial durable/local enforcement; P3 coding close path is proven locally; overall CompanyOS remains partial
proof-level change: local_behavior plus durable fact reopen and aggregate stream CAS evidence; no cross-process PendingInvocation/Runner recovery claim
limitations: release smoke and unconstrained workspace test still contain legacy entrypoint/SDK failures; mandatory wire proof, authenticated principal, fd-relative TOCTOU, durable state projector, full Cell scheduler, and external adapters remain open
reviewer: final serialized verification; no commit, push, merge, release, or worktree deletion performed
```

### P1/P2 capability result correlation evidence (2026-08-30)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; direct, harness, and approval continuation capability results now require request_id correlation
command_argv:
  cargo fmt --all --check
  cargo check -p kiana-core -p kiana-daemon --locked --offline
  cargo test -p kiana-core --test control_plane mismatched_ --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane harness_effect_without_result_event_is_result_unknown --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized failure-injection suites
fixture or cassette: Broker returns a result for the wrong request ID on direct and harness paths; event persistence failure fixture
exit_code: 0; correlation tests 2/2, Unknown 1/1, core 37/37, daemon_host 54/54, format and diff checks passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P1/P2 result correlation moved from runner-only checking to ControlPlane enforcement on every Broker path; remains partial overall
proof-level change: local_behavior evidence that mismatched results enter ResultUnknown, no capability.completed/run.completed is emitted, and the wrong result is not reinjected into Runner
limitations: durable reconciliation queue and provider-side verification remain open; current ResultUnknown event persistence has no separate incident journal if that write also fails
reviewer: focused capability correlation review; no live provider or durable workflow claim
```

### P1 mandatory wire approval proof evidence (2026-08-30)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; DaemonHost rejects proofless ApprovalDecision while direct ControlPlane compatibility remains unchanged
command_argv:
  cargo fmt --all --check
  cargo check --workspace --locked --offline
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized wire/core/daemon suites
fixture or cassette: proofless wire approval, wrong proof, correct proof retry, cancellation, approval continuation and existing command materialization approvals
exit_code: 0; daemon control_plane 12/12, daemon_host 54/54, core 34/34, format and diff checks passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P1 wire approval proof changed from optional boundary to required for DaemonHost protocol requests; direct in-process compatibility remains explicit
proof-level change: local_behavior evidence that missing hash/nonce is rejected before approval-store lookup and valid challenge proof continues to work
limitations: DaemonHost principal is still a fixed local principal; proof is digest/nonce validation rather than a signed human identity; direct ControlPlane APIs remain legacy-compatible; durable PendingInvocation/Runner recovery remains open
reviewer: focused protocol boundary review; no enterprise authentication or cross-process recovery claim
```

### P1-03 cross-process Builder path-lock evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; ControlPlane Builder packet admission now holds kernel-backed hashed lock files in addition to the in-process conflict map
command_argv:
  cargo check -p kiana-core --locked --offline
  cargo test -p kiana-core --test control_plane independent_control_planes_share_builder_path_locks --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; two independent ControlPlane instances in one process
fixture or cassette: blocking first Builder runner, second independent ControlPlane with the same trusted project and overlapping path_allow
exit_code: 0; compile and cross-instance path-lock regression 1/1 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-03 cross-process Builder path admission moved from process-local-only to kernel-backed local fencing; remains partial
proof-level change: local_behavior evidence that a second independently constructed ControlPlane cannot admit the same project path while the first reservation is live; lock descriptors release with process lifetime
limitations: lock files provide admission fencing but not durable Cell/Grant/Budget state, stale-state reconciliation, or cross-process PendingInvocation recovery; non-Unix fallback is not an equivalent kernel lock; full TOCTOU and handler cancellation fences remain open
reviewer: focused concurrency test and core compile verification
```

### P1-04 serialized patch commit lock correction evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; project patch lock now blocks for the short commit window rather than turning disjoint concurrent writers into false successful model responses
command_argv:
  cargo test -p kiana-daemon --lib apply_patch --locked --offline
  cargo test -p kiana-daemon --test daemon_host disjoint_packet_builders_write_in_parallel --locked --offline -- --test-threads=1
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies
fixture or cassette: exclusive project patch lock with a waiting second holder, two disjoint Builder packet patches sharing one project
exit_code: 0; apply_patch 17/17 and disjoint daemon regression 1/1 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-04 lock contention semantics corrected from nonblocking denial to serialized commit; remains partial
proof-level change: local_behavior evidence that disjoint patches wait and commit, while snapshot validation still fences overlapping/changed targets
limitations: no fd-relative/no-follow effect-time guarantee for external component swaps; durable patch journal and crash reconciliation remain open
reviewer: focused concurrency correctness review
```

### P1-12 Web HTTP body quota evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; authenticated Web raw JSON bodies over 128 KiB are rejected by router DefaultBodyLimit before handler parsing
command_argv:
  cargo test -p kiana-entrypoints --test cli_web --locked --offline
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies
fixture or cassette: loopback Web subprocess, valid bearer token, raw JSON body larger than MAX_WEB_BODY_BYTES, followed by normal cassette run
exit_code: 0; cli_web 4/4 passed, including HTTP 413 body rejection and normal DaemonHost write
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-12 body quota moved from router declaration to observed HTTP enforcement evidence; remains partial
proof-level change: local_behavior evidence that oversized authenticated bodies do not reach Web handlers while normal requests remain functional
limitations: prompt, artifact, global memory, backpressure, durable and cross-process quota accounting remain separately bounded/incomplete
reviewer: focused HTTP resource-limit verification
```

### P1-12 Web projection output quota evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; Web session projection now bounds item count, summary text, file count, and file path bytes before storing provider output
command_argv:
  cargo test -p kiana-entrypoints --lib web::tests::web_projection_output_limits_are_bounded --locked --offline
  cargo test -p kiana-entrypoints --lib web::tests::web_turn_history --locked --offline
  cargo test -p kiana-entrypoints --test cli_web --locked --offline
  cargo check --workspace --locked --offline
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies
fixture or cassette: oversized assistant text, 257 file paths, 129 turn items, large UTF-8 bodies, authenticated Web cassette/run
exit_code: 0; projection and transcript quota regressions passed, cli_web 4/4, workspace check passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-12 Web projection memory bounds moved from turn/body-only to all accumulated summary/list/item projections; remains partial
proof-level change: local_behavior evidence that oversized provider output is truncated or bounded before entering WebSession state
limitations: global process memory accounting, artifact store quota, backpressure, durable quotas and cross-process session state remain open
reviewer: focused Web projection resource review
```

### P1-12 Web transcript turn quota evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; Web store_turn now keeps at most 256 TurnView entries per session and evicts the oldest before append
command_argv:
  cargo test -p kiana-entrypoints --lib web::tests::web_turn_history_is_bounded --locked --offline
  cargo test -p kiana-entrypoints --test cli_web --locked --offline
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies
fixture or cassette: MAX_WEB_TURNS_PER_SESSION+1 synthetic turns and authenticated Web cassette/run fixture
exit_code: 0; transcript quota regression 1/1 and cli_web 4/4 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-12 Web resource bounds now include body, prompt, session count, and per-session transcript turns; remains partial
proof-level change: local_behavior evidence that a single Web session cannot retain unbounded turn history while the latest turn remains visible
limitations: artifact byte quotas, global memory accounting, backpressure, durable session state, and cross-process quotas remain open
reviewer: focused Web transcript resource review
```

### P1-12 Web session count quota evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; Web new_session rejects insertion at the 128-session hard cap before mutating the session map
command_argv:
  cargo test -p kiana-entrypoints --lib web::tests::web_session_capacity_fails_closed --locked --offline
  cargo test -p kiana-entrypoints --test cli_web --locked --offline
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies
fixture or cassette: empty Web session map, exactly MAX_WEB_SESSIONS synthetic sessions, authenticated Web cassette/run
exit_code: 0; quota regression 1/1 and cli_web 4/4 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-12 Web session resource bound moved from prompt/body-only to include session count; remains partial
proof-level change: local_behavior evidence that the session map cannot grow beyond the configured local hard cap
limitations: turn/artifact/durable quotas, cross-process ownership and backpressure remain open
reviewer: focused Web resource quota review
```

### P1-01 Web explicit session isolation evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; Web resolve_session now validates explicit session IDs without mutating the process-global active thread; only new_session changes active
command_argv:
  cargo test -p kiana-entrypoints --lib web::tests::explicit_session_resolution_does_not_change_global_active_thread --locked --offline
  cargo test -p kiana-entrypoints --test cli_web --locked --offline
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies
fixture or cassette: two WebSession records, explicit secondary session resolution, authenticated Web cassette/run fixture
exit_code: 0; isolation regression 1/1 and cli_web 4/4 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-01 Web session selection moved from global active mutation to request-scoped resolution; remains partial
proof-level change: local_behavior evidence that one tab/request selecting a known secondary session cannot silently change another tab's global active thread
limitations: bearer token remains process-wide, sessions are not durable or multi-user authenticated, and cross-process ownership remains open
reviewer: focused Web session isolation review
```

### P1-12 Web body and prompt quota evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; loopback Web router now enforces 128 KiB body limit and run prompt validation enforces 64 KiB prompt limit
command_argv:
  cargo test -p kiana-entrypoints --lib web::tests::web_prompt_limit_fails_closed --locked --offline
  cargo test -p kiana-entrypoints --test cli_web --locked --offline
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies
fixture or cassette: empty/oversized/trimmed prompt unit fixture and authenticated Web health/run/trust/sandbox/session smoke fixture
exit_code: 0; prompt limit 1/1 and cli_web 4/4 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-12 Web input resource limits moved from unbounded body/prompt to explicit bounded validation; remains partial
proof-level change: local_behavior evidence that oversized Web input cannot reach DaemonHost or broker and normal authenticated workbench behavior is preserved
limitations: global session/artifact quotas, backpressure, timeout budgets, and durable quota accounting remain open
reviewer: focused Web resource exhaustion review
```

### G0-01 serialized workspace failure classification evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; full workspace tests rerun with --test-threads=1 after trust, Cell, Unknown, cancellation, patch-lock, and redaction slices
command_argv:
  cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized tests inside each target
fixture or cassette: workspace fixtures, legacy CLI/SDK fixtures, stored trust fixtures, daemon packet concurrency fixtures
exit_code: 101; most targets passed, four targets failed
failures: daemon_host disjoint packet BRAVO.txt missing under full workspace execution; kiana-entrypoints lib legacy model/tool tests; cli_session legacy output contract
status change: none; Gate 0 remains not passed
proof-level change: negative evidence refreshed against the current dirty snapshot; CompanyOS focused suites remain separately green
limitations: Cargo test thread serialization does not serialize independent test executables; full cross-target environment/lock-root isolation remains open; legacy surfaces are not the CompanyOS primary path
reviewer: serialized workspace gate and focused failure classification
```

### G0-01 entrypoint stored-trust fixture evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; command_dispatch and architecture binary tests now create server-recognized stored ProjectTrust facts instead of relying on caller metadata
command_argv:
  cargo test -p kiana-entrypoints --lib command_dispatch --locked --offline -- --test-threads=1
  cargo test -p kiana-entrypoints --test cli_architecture --locked --offline -- --test-threads=1
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized entrypoint tests
fixture or cassette: temporary project roots with persisted trust records and architecture binary child process receiving isolated KIANA_HOME
exit_code: 0; command_dispatch 5/5 and cli_architecture 2/2 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: G0-01 trust fixture drift corrected; no broad Gate 0 upgrade
proof-level change: local_behavior evidence that entrypoint tests exercise the same server-derived trust authority as production
limitations: legacy CLI/SDK model-loop tests and complete workspace release smoke failures remain; local stored trust is not authenticated multi-user identity
reviewer: focused fixture migration verification
```

### P1-05 capability result scope correlation evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; capability completed/failed event payloads now retain run/session/cell/grant/budget/request correlation identity after redaction
command_argv:
  cargo test -p kiana-core --lib event_redaction --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized event/privacy suites
fixture or cassette: capability_event_payload with CellId, CapabilityGrantId, BudgetLeaseId, request_id and redacted result
exit_code: 0; event_redaction 4/4, core control_plane 43/43, daemon_host 57/57
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-05 result correlation moved from request-only metadata to result payload scope identity; remains partial
proof-level change: local_behavior evidence that a redacted capability result can be traced to its exact Cell/Grant/Budget/request scope
limitations: durable audit projector, arbitrary provider provenance, complete stdout/stderr/argv/env secret scan, and cross-process recovery remain open
reviewer: focused result correlation/privacy review
```

### P1-05 recursive event string redaction evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; redact_event_value now redacts every nested string through case-insensitive marker handling while preserving secret_ref references
command_argv:
  cargo test -p kiana-core --lib event_redaction --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized event/privacy suites
fixture or cassette: nested error strings containing Authorization Bearer, token=, JSON api_key, and opaque secret_ref
exit_code: 0; event_redaction 3/3, core control_plane 43/43, daemon_host 57/57
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-05 event string privacy moved from key-only redaction to recursive text redaction; remains partial
proof-level change: local_behavior evidence that common secret-bearing strings do not enter redacted event payloads while opaque references remain usable
limitations: arbitrary provider encodings, stdout/stderr/argv/env full-chain scans, schema-generated redaction, and durable audit recovery remain open
reviewer: focused event privacy regression and serialized core/daemon verification
```

### P1-04 apply_patch project commit-lock evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; daemon apply_patch now serializes the plan commit phase with a per-project kernel-backed lock before rechecking snapshots and applying operations
command_argv:
  cargo test -p kiana-daemon --lib apply_patch --locked --offline
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo check --workspace --locked --offline
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized patch/daemon suites
fixture or cassette: project patch lock exclusivity, full preflight, rollback, symlink/hardlink rejection, daemon harness apply_patch writes and read-only denial
exit_code: 0; apply_patch 17/17, daemon_host 57/57, workspace check passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-04 gained project-level commit serialization and second-writer snapshot fencing; remains partial
proof-level change: local_behavior evidence that concurrent commit attempts cannot interleave and a failed patch lock/commit does not leave the lock held
limitations: this does not provide fd-relative/no-follow effect-time guarantees against an external component swap between verification and write; durable patch journal, disk-full recovery, and cross-process Cell state remain open
reviewer: focused apply_patch safety review and serialized daemon regression suite
```

### P1-03 shell process-group stop confirmation evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; shell timeout now confirms process-group convergence and propagates inability to stop as result_unknown
command_argv:
  cargo test -p kiana-daemon --lib harness_capabilities --locked --offline
  cargo test -p kiana-daemon --test daemon_host shell_timeout_ms_is_brokered_as_a_timed_out_tool_result --locked --offline -- --test-threads=1
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies
fixture or cassette: shell timeout with process group termination, exit code 124, stop_confirmed result field, model-facing timeout result
exit_code: 0; daemon harness unit suite and focused timeout regression 1/1 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-03 shell timeout now distinguishes confirmed stop from process-group uncertainty; remains partial
proof-level change: local_behavior evidence that ordinary timeout returns stop_confirmed and an unconfirmed process group is routed to shell_result_unknown
limitations: an adversarial provider/handler failure path still needs an injected non-converging group regression; durable cancel generation and full cgroup/process-tree recovery remain open
reviewer: focused shell cancellation/Unknown review
```

### P1-03 Builder path-lock cleanup evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; Builder spawn path locks are owned by an RAII guard so Cell reservation and pre-run event failures cannot leak locks
command_argv:
  cargo test -p kiana-core --test control_plane failed_cell_reservation_releases_builder_path_lock --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies
fixture or cassette: expired packet deadline after path admission, followed by a retry on the same path and fresh Builder session
exit_code: 0; focused regression 1/1 and core control_plane 42/42 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-03 path-lock cleanup moved from best-effort release to exception-safe reservation lifetime ownership; remains partial
proof-level change: local_behavior negative/failure evidence that failed Cell admission does not permanently block later work
limitations: process crash and kernel lock release are covered only by descriptor lifetime; durable Cell state, stale registry reconciliation, and effect-time filesystem fencing remain open
reviewer: focused resource-leak review and serialized core regression suite
```

### P2 Run approval restart fail-closed evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; approval decisions now inspect persisted approval.requested run association and reject approval when no in-memory PendingInvocation can resume the same Run
command_argv:
  cargo test -p kiana-core --test control_plane persisted_run_approval_without_pending_invocation_fails_closed --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo check --workspace --locked --offline
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized approval/recovery suites
fixture or cassette: persisted approval.requested with run_id, consumed approval without PendingInvocation, CountingBroker, direct approval compatibility tests
exit_code: 0; core control_plane 43/43, daemon_host 57/57, daemon control_plane 12/12, workspace check passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P2 approval restart behavior moved from unsafe direct-execute fallback to fail-closed continuation_unavailable for Run-bound approvals; remains partial
proof-level change: local_behavior negative evidence that an approval belonging to a Run cannot dispatch a capability after runner continuation state is lost
limitations: no durable PendingInvocation/Runner state recovery yet; approval fact is consumed before the unavailable continuation response; reconciliation and restart projector remain open
reviewer: focused approval replay/restart review and serialized core/daemon verification
```

### P2 receipt Unknown replay evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; read_receipt now recognizes persisted run.result_unknown before failed/cancelled fallback and never projects it as Completed
command_argv:
  cargo test -p kiana-core --test control_plane receipt_replays_result_unknown_without_claiming_success --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies
fixture or cassette: MemoryEventLog with run.authorized followed by run.result_unknown and no run.completed event
exit_code: 0; focused regression 1/1 and core control_plane 41/41 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P2 receipt projection now preserves result_unknown as a first-class replayed outcome; remains partial
proof-level change: local_behavior negative evidence that persisted Unknown facts cannot become successful receipt responses
limitations: durable reconciliation queue, provider verification, crash-safe state projector, and cross-process PendingInvocation recovery remain open
reviewer: focused receipt replay and Unknown-state review
```

### P1-03 packet Cell cancellation lifecycle evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; cancelled packet runs now transition Cell through CancelRequested to Cancelled before retirement and emit packet.cancelled
command_argv:
  cargo test -p kiana-core --test control_plane cancelled_packet_transitions_cell_and_packet_to_cancelled --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized cancellation suite
fixture or cassette: cancellable Runner, Builder packet with ALPHA.txt write set, concurrent cancel request, shared event log
exit_code: 0; focused regression 1/1 and core control_plane 40/40 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-03 packet cancellation projection moved from Cell Failed to CancelRequested→Cancelled→Retired and WorkPacket Cancelled; remains partial
proof-level change: local_behavior evidence for distinct packet/Cell cancellation events and resource retirement without completed status
limitations: kernel/process-group stop confirmation, durable cancellation generation, crash recovery, and effect Unknown reconciliation remain open
reviewer: focused cancellation lifecycle review
```

### P3-01 malformed Cell scope fail-closed evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; begin_cell_capability_from_request rejects Grant/Budget metadata without a Cell identity before broker dispatch
command_argv:
  cargo test -p kiana-core --test control_plane incomplete_cell_capability_scope_is_rejected_before_broker --locked --offline -- --test-threads=1
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies
fixture or cassette: trusted direct capability request with forged capability_grant_id and budget_lease_id but no cell_id; CountingBroker
exit_code: 0; focused regression 1/1 passed and broker call count remained zero
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P3-01 malformed Cell scope moved from implicit no-op to fail-closed rejection; remains partial
proof-level change: local_behavior negative evidence for incomplete Cell scope not reaching the capability broker
limitations: valid Cell facts and budget accounting remain process-local; no durable projector or cross-process invocation recovery
reviewer: focused negative-path review
```

### P3-01 Cell capability scope and budget fence evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; Cell-bound capability requests now receive server-owned cell/grant/budget identity before dispatch and release accounting on success, failure, cancellation, and Unknown
command_argv:
  cargo check --workspace --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo clippy -p kiana-core -p kiana-daemon --all-targets --locked --offline
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized affected suites
fixture or cassette: packet Cell lifecycle, approved continuation, mismatched capability result, result-event persistence failure, cancellation, and independent path-lock fixtures
exit_code: 0 for all listed commands; core 38/38, daemon_host 57/57, daemon control_plane 12/12
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P3-01 capability scope enforcement moved from unused registry contract to the active harness/direct/approval dispatch paths; remains partial
proof-level change: local_behavior evidence that forged/missing Cell scope is not dispatched, valid requests consume bounded budget leases, and all terminal outcomes release accounting leases
limitations: Cell registry and budget facts remain process-local; no durable projector/restart recovery, provider reconciliation, complete Grant resource descriptors, or effect-time TOCTOU fence
reviewer: focused capability correlation/accounting review plus serialized core/daemon verification
```

### P3-01 Cell lifecycle admission evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; spawn_from_packet now reserves and commits template, budget, grant, supervision, and Cell resources before the fresh Builder run, then transitions and retires the Cell
command_argv:
  cargo check -p kiana-core --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized CompanyOS suites
fixture or cassette: fresh Builder packet, empty and scoped path locks, budget/grant/template validation, approval/unknown paths, independent review/close fixtures
exit_code: 0; core control_plane 38/38 and daemon_host 57/57 passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P3-01 Cell resource admission moved from unused contract objects to the packet execution path; remains partial
proof-level change: local_behavior evidence for reserve→commit→Cell running→terminal transition, resource retirement, packet lifecycle events, and fresh Builder session isolation
limitations: registry state and budgets remain process-local apart from kernel path admission; no durable Cell projector, parent cascade across restart, full DelegationPacket authorization, or cross-process run recovery
reviewer: focused core/daemon lifecycle verification
```

### P1-03 kernel-backed Builder path-lock evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; ControlPlane now combines the in-process path map with hashed kernel-backed lock files held by reservation lifetime
command_argv:
  cargo check -p kiana-core --locked --offline
  cargo test -p kiana-core --test control_plane independent_control_planes_share_builder_path_locks --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo check --workspace --locked --offline
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized affected suites
fixture or cassette: two independently constructed ControlPlane instances, blocking first Builder runner, overlapping ALPHA.txt path_allow
exit_code: 0; core control_plane 38/38, daemon_host 57/57, daemon control_plane 12/12, workspace check passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-03 cross-process Builder path admission moved from process-local-only to kernel-backed local fencing; remains partial
proof-level change: local_behavior evidence that a second independent ControlPlane cannot admit the same project path while the first holds the lock; descriptor drop releases the lock on normal completion or process exit
limitations: lock files fence admission but do not persist/rebuild Cell/Grant/Budget state; non-Unix fallback is not an equivalent kernel lock; stale state, effect-time TOCTOU, handler confirmation, and full durable recovery remain open
reviewer: focused concurrency regression and serialized core/daemon verification
```

### P1-03 distinct cancelled lifecycle evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; ExecutionStatus now has a distinct cancelled terminal value and cancel responses use it
command_argv:
  cargo test -p kiana-domain --locked --offline
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo check --workspace --locked --offline
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized cancellation suites
fixture or cassette: cancelled execution state transition, cancel-after-approval, in-flight shell cancellation, packet/cell cancellation mapping
exit_code: 0 for all listed commands; domain 23/23, core 38/38, daemon_host 57/57, workspace check passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-03 cancellation responses and lifecycle projection distinguish cancelled from failed; remains partial because process-group fencing and durable cancellation/unknown reconciliation are incomplete
proof-level change: local_behavior evidence for a semantically distinct cancelled terminal state, JSON wire value, and packet/cell mapping without completed status
limitations: cancellation is still process-local; effect-time handler confirmation, kill-9 recovery, and result_unknown reconciliation remain open; full workspace/release gates retain legacy failures
reviewer: serialized domain/core/daemon cancellation verification
```

### P1-01 restart receipt ownership fencing evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; persisted receipt reads now validate run.authorized session, actor, and canonical project before projection
command_argv:
  cargo test -p kiana-daemon --test daemon_host disk_receipts_survive_restart_and_do_not_overwrite_the_first_run --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized daemon tests
fixture or cassette: two disk daemon instances, valid receipt reopen, known run_id requested by a foreign session
exit_code: 0; focused restart ownership 1/1, daemon_host 57/57, diff check passed
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-01 receipt ownership moved from live in-memory binding only to a persisted run.authorized identity check; remains partial because authenticated principal and full state recovery are incomplete
proof-level change: local_behavior evidence that a restarted host cannot project a known run receipt to a different session while preserving the original owner's receipt
limitations: old events without run.authorized identity remain compatibility-readable; actor is still a fixed local principal; no durable pending-run/session recovery or tenant boundary
reviewer: focused adversarial restart test and serialized daemon regression suite
```

### P1-01 host-derived ProjectTrust authority evidence (2026-08-31)

```text
source_snapshot: 4f3003b16ddd1f66d97f786380402994c6ac7a09 + uncommitted CompanyOS WIP
worktree_status: WIP; DaemonHost now resolves project trust through an injected authority; no reset or cleanup
command_argv:
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --lib --locked --offline
  cargo check --workspace --locked --offline
  cargo clippy --workspace --all-targets --locked --offline
  bash scripts/v10-workbench-smoke.sh
  git diff --check
cwd/environment: repository root; Linux; rustc 1.97.1; offline dependencies; serialized daemon tests
fixture or cassette: stored ProjectTrust without a record, fixed trusted/denying fixture authorities, forged project_trusted and permission_profile metadata
exit_code: 0 for all listed commands; daemon_host 57/57, daemon control_plane 12/12
artifact paths and SHA-256: test stdout is the command artifact; SHA-256 not generated
status change: P1-01 project trust authority moved from caller-compatible metadata to DaemonHost server-derived trust; remains partial because principal and role assignment are not durable authentication
proof-level change: local_behavior negative evidence that project_trusted=true and Autonomous metadata cannot authorize an untrusted project or bypass read-only approval; trusted stored records still support the existing workbench path
limitations: actor is still a fixed local principal; role/department remain request-selected; authority is not a cross-process authenticated identity or tenant boundary; full workspace and release gates still contain legacy entrypoint/supervisor failures
reviewer: focused security review plus serialized daemon/control-plane verification
```

### P2 harness result-unknown evidence (2026-08-30)

```text
source_snapshot: dirty checkout at 4f3003b + uncommitted CompanyOS WIP
worktree_status: WIP; harness capability result-event persistence failure now returns ResultUnknown instead of Failed/Completed
command_argv:
  cargo fmt --all --check
  cargo check -p kiana-core -p kiana-daemon -p kiana-protocol --locked --offline
  cargo test -p kiana-core --test control_plane harness_effect_without_result_event_is_result_unknown --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
  git diff --check
cwd/environment: repository root; Linux; stable toolchain; offline dependency resolution; serialized failure-injection suites
fixture or cassette: successful fake Broker effect with intentionally failing capability.completed EventStore append
exit_code: 0; focused Unknown 1/1, core 35/35, daemon_host 54/54, format and diff checks passed
artifact paths and SHA-256: not generated; test stdout is the command artifact
status change: P2 Unknown handling moved from command-only behavior to the harness capability path; P2 remains partial
proof-level change: local_behavior evidence that an effect with missing result-event persistence returns result_unknown, records no run.completed, and does not auto-report success
limitations: durable reconciliation queue and provider-side effect verification remain incomplete; a persistence failure while writing the Unknown event itself is still only returned as an error; no cross-process runner recovery claim
reviewer: focused failure-injection review; no live provider or durable workflow claim
```

### S0 legacy CLI boundary evidence (2026-09-01)

```text
source_snapshot: local worktree after explicit authorization to touch frozen CLI compatibility surface
worktree_status: dirty; preserves pre-existing user/WIP changes
command_argv: cargo test -p kiana-entrypoints --lib --locked --offline -- --test-threads=1; focused direct-connect tests
cwd/environment: repository root; locked offline Cargo; serialized environment tests
fixture or cassette: local mock model servers and temporary session/workspace fixtures
exit_code: entrypoints lib 101 (429 passed, 2 legacy fixture failures); direct-connect text/permission focused tests 0
status change: provider options propagate into DaemonHost construction through explicit immutable configuration
proof-level change: local_behavior for supported CompanyOS direct-connect model execution
limitations: legacy permission/tool-loop fixtures request TaskCreate or frozen Read/Write/Edit/Bash tools and conflict with the owned five-tool harness; no legacy runner or tool surface was reattached
reviewer: Codex
```

### P1-02 restart approval proof preflight evidence (2026-09-07)

```text
source_snapshot: 842e5a4fea8d61dfe083e74315957a29e7772d6a + uncommitted CompanyOS WIP
worktree_status: dirty checkout; existing user/WIP changes preserved; no commit, reset, push, merge, or worktree deletion performed
command_argv:
  cargo fmt --all --check
  cargo test -p kiana-core --test control_plane persisted_run_approval_without_pending_invocation_fails_closed --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --lib disk_store_survives_reopen_and_consumes_once --locked --offline -- --test-threads=1
  cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
  cargo test -p kiana-daemon --lib approval --locked --offline -- --test-threads=1
  cargo check --workspace --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain (rustc/cargo 1.97.1); locked/offline Cargo dependency resolution; serialized approval/core/daemon tests
fixture or cassette: persisted Run-bound approval with explicit challenge hash/nonce and no live PendingInvocation; repeated approval retry; reopened disk approval validated before consume and rejected after single-use consume
exit_code: 0 for every listed command; focused restart regression 1/1, disk reopen regression 1/1, core control_plane 82/82, daemon approval 11/11, workspace check=0, format=0, diff check=0
artifact paths and SHA-256: temporary JSONL approval fixture is created below the OS temp directory and removed by the test; no repository artifact or SHA-256 was generated; assertions and command exits are the evidence
status change: P1-02/P2 restart approval behavior remains partial but now validates authenticated proof without consuming a durable Active approval when the persisted Run has no in-memory continuation; the unavailable continuation marker is idempotent across retries
proof-level change: local_behavior plus durable-reopen evidence for non-consuming proof validation, exact hash/nonce/context/integrity checks in the production approval adapter, and single-use state after a subsequent consume
limitations: PendingInvocation and Runner state are still process-local; preflight validation and later consume are separate operations and do not establish an atomic cross-process recovery transaction; no durable continuation projector, kill-9 reconciliation, or full Session/Run/Invocation rebuild is claimed
reviewer: focused approval replay/restart review with serialized core/daemon verification; no assertions weakened, frozen paths changed, dependency manifests changed, or second execution path added
```
