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

### OA-21 replay/reconciliation diagnostics evidence (2026-09-15)

```text
source_snapshot: ef3bf0d; kiana-domain/src/{contracts,observability}.rs; kiana-core/src/{replay_diagnostics,audit_projection,health,metrics,invocation_projection,span_projection,lib}.rs; kiana-core/tests/oa21_replay_diagnostics.rs; .github/workflows/oa21-replay-diagnostics.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-21 replay diagnostic/snapshot contracts, deterministic projection comparison, bounded divergence locator, expectation/status/input checks, remote-only fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/contracts.rs kiana-domain/src/observability.rs kiana-core/src/replay_diagnostics.rs kiana-core/src/lib.rs kiana-core/tests/oa21_replay_diagnostics.rs .github/workflows/oa21-replay-diagnostics.yml
  rg -n 'ReplayDiagnostic|ReplayDiagnosticSnapshot|diagnose_replay|ReplayExpectation|UnknownEffect|StatusMismatch|replay_source_gap' kiana-domain/src kiana-core/src kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa21_replay_diagnostics --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa21_replay_diagnostics.rs; deterministic replay, source duplicate, unknown effect, forged audit schema, expectation status/input divergence, safe error code/secret absence and bounded expectation fixtures; GitHub Actions only
exit_code: 0 for source hashes, format, core OA-21 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-21 job is queued by the push and is not awaited
status_change: OA-21 source slice is implemented. `kiana.replay-diagnostic.v1`/snapshot bind source cursor/events, projection digests, invocation/attempt/input digest, expected/observed status, safe error code and divergence kind. `diagnose_replay` reuses committed Invocation/Run/Metric/Audit/Health/Span projections, reports unknown/schema/gap/duplicate/terminal/status divergence without invoking Model/Provider/Broker or Recovery actions, and leaves unknown effects unresolved/fenced.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; diagnostics and projection comparison are process-local, no provider-side receipt/reconcile/compensation or durable checkpoint exists, and replay does not prove crash/fault/capacity behavior or external health/incident consistency
reviewer: Codex root implementation review plus OA-21 domain/core replay static-boundary review; no runtime test reviewer
```

### OA-22 crash/fault injection matrix evidence (2026-09-15)

```text
source_snapshot: 95fa693; kiana-domain/src/{fault,contracts,lib}.rs; kiana-core/src/{fault_injection,lib}.rs; kiana-core/tests/oa22_fault_injection.rs; .github/workflows/oa22-fault-injection.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-22 replay-only FaultCase/FaultMatrix contracts, eight-point deterministic simulator, seed/source binding and safety invariants, remote-only fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/fault.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/fault_injection.rs kiana-core/src/lib.rs kiana-core/tests/oa22_fault_injection.rs .github/workflows/oa22-fault-injection.yml
  rg -n 'FaultInjectionPoint|FaultCase|FaultMatrix|fault_matrix|fault_matrix_from_events|unknown_not_fenced|false_success|duplicate_effect' kiana-domain/src kiana-core/src kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa22_fault_injection --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa22_fault_injection.rs; eight prepare/commit/dispatch/result/flush/projector/export/shutdown cases, seed replay, source duplicate/limit, rejected/unknown/observed safety, tampered false-success/duplicate/unfenced-unknown and serde fixtures; GitHub Actions only
exit_code: 0 for source hashes, format, core OA-22 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-22 job is queued by the push and is not awaited
status_change: OA-22 source slice is implemented. `kiana.fault-case.v1`/`kiana.fault-matrix.v1` bind injection point, seed, status, effect started/known, resource fencing, source cursor/events and digests. The replay-only simulator covers eight fault boundaries with rejected/unknown/observed classifications; rejected cases prove no started effect, unknown cases require fencing, duplicate-effect/false-success and mixed source/seed tampering fail closed. `ControlPlane::fault_matrix` is read-only and performs no crash, handler, provider, exporter, shutdown or EventLog action.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; matrix cases are safety models rather than real fault hooks, process kills or crash recovery, no durable fault/incident/receipt checkpoint exists, and cross-process resource/lease/provider/export/shutdown evidence remains open
reviewer: Codex root implementation review plus OA-22 domain/core fault-matrix static-boundary review; no runtime test reviewer
```

### OA-23 provider-independent eval evidence (2026-09-15)

```text
source_snapshot: 22a7cd4; kiana-domain/src/{eval,contracts,lib}.rs; kiana-core/src/{eval,replay_diagnostics,metrics,audit_projection,health,span_projection,lib}.rs; kiana-core/tests/oa23_provider_independent_eval.rs; .github/workflows/oa23-provider-independent-eval.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-23 EvalCaseSpec/Result/Suite contracts, provider-independent committed-fact evaluator, normalized event/Audit/Metric/Span/Run/Replay evidence checks, secret/forbidden-effect/cost/latency gates, remote-only fixtures, workflow and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/eval.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/eval.rs kiana-core/src/lib.rs kiana-core/tests/oa23_provider_independent_eval.rs .github/workflows/oa23-provider-independent-eval.yml
  rg -n 'EvalCaseSpec|EvalCaseResult|EvalSuiteReport|evaluate_provider_independent|forbidden_effect|secret_detected|replay_diverged|promote' kiana-domain/src kiana-core/src kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa23_provider_independent_eval --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux; stable Rust toolchain (rustc/cargo 1.97.1); locked offline dependency cache; static compilation/source inspection only; no test binaries executed
fixture or cassette: kiana-core/tests/oa23_provider_independent_eval.rs; committed success evidence, secret/forbidden effect Blocked, replay/terminal divergence Fail, normalized event/audit/metric/span/receipt digests and promote gate; GitHub Actions only
exit_code: 0 for source hashes, format, core OA-23 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-23 job is queued by the push and is not awaited
status_change: OA-23 source slice is implemented. `kiana.eval-case.v1`/`eval-result.v1`/`eval-suite.v1` bind explicit evidence requirements, normalized event/projection digests, status, effect/secret/replay flags, bounded failures and cost/latency classification. `evaluate_provider_independent` reuses committed Audit/Metric/Span/Run/Replay projections; missing evidence, secret, forbidden effect, status/replay divergence or measured usage mismatch yields Fail/Blocked, and suite `promote` is recomputed true only when every case passes. No Model/Provider/Broker or promotion side effect is invoked.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; evaluator/specs/results are process-local, no real provider/model/broker or Promptfoo runner, durable eval artifact, provider receipt/cost reconciliation, automatic promotion/rollback or cross-process eval state exists
reviewer: Codex root implementation review plus OA-23 domain/core eval static-boundary review; no runtime test reviewer
```

### OA-24 four-entrypoint audit/health/Receipt parity evidence (2026-09-15)

```text
source_snapshot: 4bc24f6; kiana-domain/src/{parity,contracts,lib}.rs; kiana-core/src/{parity,lib}.rs; kiana-protocol/src/lib.rs; kiana-client/src/lib.rs; kiana-daemon/src/lib.rs; kiana-entrypoints/src/{cli,harness_run,web,workbench_chat}.rs; kiana-core/tests/oa24_entrypoint_parity.rs; kiana-protocol/tests/oa24_parity_wire.rs; .github/workflows/oa24-entrypoint-parity.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-24 owner-scoped EntryPointParitySnapshot, protocol/client parity request, DaemonHost routing, CLI/Web/Workbench entrypoint adapters, Desktop Web endpoint reuse, authoritative liveness response, remote-only fixtures and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/parity.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/parity.rs kiana-core/src/lib.rs kiana-protocol/src/lib.rs kiana-client/src/lib.rs kiana-daemon/src/lib.rs kiana-entrypoints/src/cli.rs kiana-entrypoints/src/harness_run.rs kiana-entrypoints/src/web.rs kiana-entrypoints/src/workbench_chat.rs kiana-core/tests/oa24_entrypoint_parity.rs kiana-protocol/tests/oa24_parity_wire.rs .github/workflows/oa24-entrypoint-parity.yml
  rg -n 'EntryPointParitySnapshot|entrypoint_parity|RequestBody::Parity|parity_envelope|/api/parity|SignalStatus::Ok|source_cursor_gap' kiana-domain/src kiana-core/src kiana-protocol/src kiana-client/src kiana-daemon/src kiana-entrypoints/src kiana-core/tests kiana-protocol/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa24_entrypoint_parity --locked --offline
  cargo check -p kiana-protocol --test oa24_parity_wire --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/oa24_entrypoint_parity.rs; four entrypoint labels over identical committed run facts, digest/status/limitation/source reference equality, terminal conflict/result_unknown, explicit source gap, completed-without-receipt and unknown-field rejection; kiana-protocol/tests/oa24_parity_wire.rs request round-trip/unknown-field/missing-entrypoint guards; GitHub Actions only
exit_code: 0 for source hashes, format, OA-24 core/protocol test-target checks, workspace test-target compilation and diff check; local tests deliberately not run per user instruction; GitHub Actions OA-24 job is queued by the push and is not awaited
status_change: OA-24 source slice is implemented. `kiana.entrypoint-parity.v1` binds entrypoint, source cursor/event IDs, projection version, status, Receipt/Audit/Health digests, retention/unknown limitations and snapshot digest. ControlPlane resolves authenticated session/run ownership, filters committed run facts, rejects missing owner/read-all/source evidence, and projects retention revoke as result_unknown; protocol/DaemonHost/harness route all four surfaces through the same read-only path. Web health now derives ok/status from liveness or explicit unavailable limitations instead of hard-coded success.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; parity and health remain process-local EventLog projections, no durable query index/cursor or external authenticated principal/health backend exists, Desktop reuses the Web adapter rather than an independent process, Receipt still does not prove external business Outcome, and retention/reconcile remains source-level only
reviewer: Codex root implementation review plus OA-24 domain/core/protocol/daemon/entrypoint static-boundary review; no runtime test reviewer
```

### OA-25 capacity/performance/migration rehearsal evidence (2026-09-15)

```text
source_snapshot: 49c9e8e; kiana-domain/src/{performance,contracts,lib,journal}.rs; kiana-core/src/{performance,lib}.rs; kiana-eventlog/src/{jsonl,journal_core}.rs; kiana-ports/src/observability_queue.rs; kiana-core/src/{audit_projection,audit_export}.rs; kiana-core/tests/oa25_capacity_migration.rs; .github/workflows/oa25-capacity-migration.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-25 bounded PerformanceBaseline/BenchmarkSummary/CapacityEnvelope/MigrationObservation contracts, checked percentile reducer, hard-limit/migration fixture and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/performance.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/performance.rs kiana-core/src/lib.rs kiana-core/tests/oa25_capacity_migration.rs kiana-eventlog/src/jsonl.rs kiana-eventlog/src/journal_core.rs kiana-ports/src/observability_queue.rs kiana-core/src/audit_projection.rs kiana-core/src/audit_export.rs .github/workflows/oa25-capacity-migration.yml
  rg -n 'PerformanceBaseline|BenchmarkSummary|CapacityEnvelope|MigrationObservation|percentile_micros|MAX_JOURNAL|writer_version|high_cardinality|oversize|backpressure' kiana-domain/src kiana-core/src kiana-eventlog/src kiana-ports/src kiana-core/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa25_capacity_migration --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/oa25_capacity_migration.rs; six operation percentile summaries, source-bound baseline/capacity envelope, rotation/archive/upgrade/downgrade observations, unknown writer-version frame, safety guard and unknown-field rejection; GitHub Actions only
exit_code: 0 for source hashes, format, OA-25 test-target/workspace compilation and diff check; local tests deliberately not run per user instruction; GitHub Actions OA-25 job is queued by the push and is not awaited
status_change: OA-25 source slice is implemented. `PerformanceBaseline` binds bounded p50/p95/p99 summaries for append/flush/project/rebuild/query/export to a source cursor/digest, hard journal/page/export/queue/artifact limits and explicit high-cardinality/oversize/backpressure guards. `MigrationObservation` records read-only rotation/archive/upgrade/downgrade outcomes with bounded reasons; unknown writer versions and unsafe direction/guard claims fail closed. Existing JSONL writer marker, frame/event/log limits and upgrade-after-legacy checks are covered by the new source evidence contract; no production benchmark or migration write path is introduced.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; percentile samples are remote fixtures rather than production SLOs, no durable benchmark artifact or real large-artifact/slow-exporter load run exists, cross-platform rotation/archive and live capacity telemetry remain open, and migration observations do not perform writes or prove cross-process upgrade/downgrade recovery
reviewer: Codex root implementation review plus OA-25 domain/core/journal/capacity static-boundary review; no runtime test reviewer
```

### OA-26 local durable observability gate evidence (2026-09-15)

```text
source_snapshot: 959c598; kiana-domain/src/{journal,performance,parity}.rs; kiana-core/src/{receipts,audit_projection,audit_export,parity,performance,lib}.rs; kiana-eventlog/src/{jsonl,journal_core}.rs; kiana-ports/src/observability_queue.rs; kiana-core/tests/{oa24_entrypoint_parity,oa26_durable_gate}.rs; scripts/oa26-durable-observability-gate.sh; .github/workflows/oa26-durable-observability.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-26 CI-only durable JSONL reopen/parity/Unknown/queue gate, source/release/artifact hash manifest, secret scan and roadmap/status overlays are scoped to this step; local invocation guard is intentional; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/journal.rs kiana-domain/src/performance.rs kiana-domain/src/parity.rs kiana-core/src/receipts.rs kiana-core/src/audit_projection.rs kiana-core/src/audit_export.rs kiana-core/src/parity.rs kiana-eventlog/src/jsonl.rs kiana-eventlog/src/journal_core.rs kiana-core/tests/oa26_durable_gate.rs kiana-ports/src/observability_queue.rs scripts/oa26-durable-observability-gate.sh
  rg -n 'JsonlEventLog|read_all|result_unknown|ObservabilityQueue|critical_rejected|remote_ci_required|release-binary-sha256|secret_sentinel' kiana-core/tests scripts .github/workflows
  bash -n scripts/oa26-durable-observability-gate.sh
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa26_durable_gate --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; CI-only test commands are recorded for GitHub Actions; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/oa26_durable_gate.rs; JsonlEventLog append/close/reopen with committed run.authorized/invocation.executing/run.result_unknown facts, OA-24 CLI/Desktop parity projection equality, JSONL digest and queue critical preservation; GitHub Actions only; CI script emits dist/oa26-durable/{source-sha256,release-binary-sha256,manifest}.*
exit_code: 0 for source hashes, shell syntax, format, OA-26 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; script would exit 2 outside CI; GitHub Actions OA-26 job is queued by the push and is not awaited
status_change: OA-26 source slice is implemented. The CI-only gate refuses local execution, verifies the durable JSONL adapter's fsync-backed close/reopen fact reconstruction, reprojects result_unknown and cross-entrypoint digests, checks critical queue rejection, writes bounded source/release/artifact hashes and scans evidence for secret sentinels. Existing journal frame/event/log limits and unknown writer checks remain fail-closed; no telemetry or release smoke result is promoted to authority.
proof-level_change: source plus remote-test design/static evidence only; no local_behavior, durable, live or physical promotion until GitHub CI returns; even after CI, scope is local machine reopen rather than physical power-loss or live backend proof
limitations: CI result was intentionally not awaited; no local test or smoke command was run; no physical power-loss/kill-9/network-filesystem/concurrent cross-process proof, external provider/Broker/telemetry backend, durable benchmark artifact, retention/reconcile store or business Outcome confirmation exists; release binary hash is an artifact identity, not a release signature
reviewer: Codex root implementation review plus OA-26 journal/receipt/queue/evidence static-boundary review; no runtime test reviewer
```

### OA-27 cross-entry/company governance gate evidence (2026-09-15)

```text
source_snapshot: fe81e6a; kiana-domain/src/{company,governance_gate,contracts,lib}.rs; kiana-core/src/{company,company_governance,commands,lib}.rs; kiana-protocol/src/lib.rs; kiana-client/src/lib.rs; kiana-daemon/src/lib.rs; kiana-entrypoints/src/{cli,harness_run,web,workbench_chat}.rs; kiana-core/tests/oa27_company_governance.rs; kiana-protocol/tests/oa27_company_governance_wire.rs; .github/workflows/oa27-company-governance.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-27 CompanyGovernanceSnapshot, read-only company.governance command, ControlPlane source projection, protocol/client/DaemonHost route, CLI/Workbench/Web adapters (Desktop Web reuse), remote-only governance fixtures and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/governance_gate.rs kiana-domain/src/company.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/company_governance.rs kiana-core/src/company.rs kiana-core/src/commands.rs kiana-core/src/lib.rs kiana-protocol/src/lib.rs kiana-client/src/lib.rs kiana-daemon/src/lib.rs kiana-entrypoints/src/cli.rs kiana-entrypoints/src/harness_run.rs kiana-entrypoints/src/web.rs kiana-entrypoints/src/workbench_chat.rs kiana-core/tests/oa27_company_governance.rs kiana-protocol/tests/oa27_company_governance_wire.rs .github/workflows/oa27-company-governance.yml
  rg -n 'CompanyGovernanceSnapshot|project_company_governance|company.governance.v1|runtime_completed_not_business_outcome|closing_chain_incomplete|review_author_session_overlap|company-governance|/governance|company-governance' kiana-domain/src kiana-core/src kiana-protocol/src kiana-client/src kiana-daemon/src kiana-entrypoints/src kiana-core/tests kiana-protocol/tests
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-core --test oa27_company_governance --locked --offline
  cargo check -p kiana-protocol --test oa27_company_governance_wire --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/oa27_company_governance.rs; runtime Completed without Acceptance remains InProgress/not Outcome, complete independent Review→Acceptance→Confirmed Delivery→Closer→ClosingReceipt chain projects Closed, forged closed chain becomes Unknown, unknown wire/owner override guards; kiana-protocol/tests/oa27_company_governance_wire.rs command round-trip and raw owner/scope rejection; GitHub Actions only
exit_code: 0 for source hashes, format, OA-27 core/protocol test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-27 job is queued by the push and is not awaited
status_change: OA-27 source slice is implemented. `CompanyGovernanceSnapshot` binds CompanyState project/runtime statuses, Acceptance/Review/Delivery/ClosingReceipt/Outcome references, source IDs, bounded limitations and digest. The reducer refuses to call runtime Completed a business Outcome; Closed requires independent reviewer/closer, accepted/waived acceptance, confirmed delivery, all runtime Completed, project Closed and a closing receipt, otherwise Unknown/InProgress with explicit limitation. ControlPlane exposes an authenticated read-only command; protocol/client/daemon and CLI/Workbench/Web/Desktop adapters reuse the same path without Broker/Provider/Recovery side effects.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; CompanyState projection is process-local over EventLog, no durable business query index or cross-organization authenticated principal exists, external delivery confirmation and Outcome measurement remain unimplemented, Desktop is Web-shell reuse, and no live/physical governance or reconcile proof exists
reviewer: Codex root implementation review plus OA-27 CompanyOS chain/ownership/source-boundary static review; no runtime test reviewer
```

### OA-28 live/physical handoff boundary evidence (2026-09-16)

```text
source_snapshot: e4a72a4; kiana-domain/src/{live_handoff,contracts,lib}.rs; kiana-domain/tests/oa28_live_handoff.rs; scripts/oa28-live-handoff-preflight.sh; docs/roadmap/oa28-live-handoff.md; .github/workflows/oa28-live-handoff.yml; docs/roadmap/observability-audit-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: OA-28 explicit LiveHandoffManifest target/status contract, provider/connector/OTLP/OS target matrix, non-secret credential/approval/receipt/retention/cleanup validation, CI-only opt-in preflight, remote manifest fixtures and roadmap/status overlays are scoped to this step; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/live_handoff.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-domain/tests/oa28_live_handoff.rs scripts/oa28-live-handoff-preflight.sh docs/roadmap/oa28-live-handoff.md .github/workflows/oa28-live-handoff.yml
  rg -n 'LiveHandoffManifest|live-handoff.v1|live_opt_in_required|secret-ref:|provider_receipt|operator_approval|cleanup_plan|NotSupported|Unknown' kiana-domain/src kiana-domain/tests scripts docs/roadmap/oa28-live-handoff.md .github/workflows/oa28-live-handoff.yml
  bash -n scripts/oa28-live-handoff-preflight.sh
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-domain --test oa28_live_handoff --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; test target compiled only; no test, provider, connector, OTLP, network or physical command executed locally
fixture or cassette: kiana-domain/tests/oa28_live_handoff.rs; explicit not_supported, verified evidence requirements, raw-secret rejection, unknown limitation, unknown-field and digest tamper fixtures; scripts/oa28-live-handoff-preflight.sh default-deny contract; GitHub Actions only
exit_code: 0 for source hashes, shell syntax, format, OA-28 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions OA-28 job is queued by the push and is not awaited
status_change: OA-28 source slice is implemented. `kiana.live-handoff.v1`/`LiveHandoffManifest` binds one provider/connector/OTLP/OS target to an isolated environment, non-secret SecretRef, config/source digests, operator approval, independent provider receipt, retention, incident, cleanup and bounded limitations. `verified` requires approval+receipt+cleanup, `unknown`/`not_supported` require reasons, and raw credentials/secret sentinels/unknown fields/digest tampering fail closed. The opt-in preflight refuses non-CI/default/no-receipt execution and never contacts external systems; runbook defines fence/reconcile/rotation/cleanup evidence through the existing DaemonHost→ControlPlane→Broker path.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; no real provider/connector/OTLP backend, isolated account, external receipt, network/OS effect, rollback/compensation, retention deletion, incident or physical safety controller was exercised; all live/physical targets remain not_supported/source until separate approved evidence blocks exist
reviewer: Codex root implementation review plus OA-28 live-handoff/credential/receipt/opt-in static-boundary review; no runtime test reviewer
```

### AUT-01 automation baseline and migration guard evidence (2026-09-16)

```text
source_snapshot: 2d31b8d; kiana-domain/src/automation.rs; kiana-workflow/src/{lib,durable}.rs; kiana-core/src/automation.rs; kiana-daemon/src/lib.rs; kiana-protocol/src/lib.rs; kiana-entrypoints/src/sdk.rs; kiana-workflow/tests/state_matrix.rs; kiana-core/tests/automation_baseline.rs; .github/workflows/aut01-baseline.yml; docs/roadmap/automation-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: AUT-01 source-only workflow/trigger inventory, single-spine guard, legacy watch_scheduled_tasks compatibility boundary, fixture catalog and roadmap/status overlays are scoped to this step; no product scheduler or second execution loop was added; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/automation.rs kiana-workflow/src/durable.rs kiana-core/src/automation.rs kiana-daemon/src/lib.rs kiana-protocol/src/lib.rs kiana-entrypoints/src/sdk.rs kiana-workflow/tests/state_matrix.rs kiana-core/tests/automation_baseline.rs .github/workflows/aut01-baseline.yml docs/roadmap/automation-baseline.md
  rg -n 'watch_scheduled_tasks|WorkflowDefinition|TriggerDefinition|plan_command|commit_workflow|authorize_and_execute|tokio::spawn|CapabilityBroker|EventStore' kiana-domain/src kiana-workflow/src kiana-core/src kiana-daemon/src kiana-entrypoints/src/sdk.rs kiana-core/tests/automation_baseline.rs
  cargo fmt --all
  cargo fmt --all --check
  bash -n scripts/oa28-live-handoff-preflight.sh
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/automation_baseline.rs; pure planner/ControlPlane/DaemonHost single-spine source assertions, legacy watcher presence/count, no direct planner Broker/EventStore, fixture names for AUT-02..24; GitHub Actions only
exit_code: 0 for source hashes, format, shell syntax, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions AUT-01 job is queued by the push and is not awaited
status_change: AUT-01 source baseline is implemented. `kiana-workflow::plan_command` remains pure and side-effect free; `kiana-core::automation` commits workflow facts before routing AgentTask/Capability through existing Company/ControlPlane paths; DaemonHost routes workflow commands through the same spine. `kiana-entrypoints::sdk::watch_scheduled_tasks` is explicitly compatibility-only (directory creation plus legacy DTO/notification), not a scheduler, EventLog occurrence source, claim queue or authority. The source guard records the gap and blocks a second scheduler/direct capability path without claiming the target scheduler exists.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; ClockPort, durable trigger/queue/claim/lease/fence, scheduler worker, event/webhook ingress, restart recovery, retry/cancel/compensation and four-entrypoint automation UAT remain AUT-02..AUT-24; old watcher remains a compatibility API and no migration/upcaster or runtime timer is provided
reviewer: Codex root implementation review plus AUT-01 automation/source-boundary reconciliation; no runtime test reviewer
```

### NM-00 notifications/messaging baseline evidence (2026-09-16)

```text
source_snapshot: a71179b; kiana-domain/src/platform.rs; kiana-core/src/platform.rs; kiana-daemon/src/run_stream.rs; kiana-entrypoints/src/{web,workbench_chat,cli}.rs; kiana-protocol/src/lib.rs; kiana-client/src/lib.rs; kiana-domain/src/company.rs; kiana-core/src/{company_business,recovery}.rs; kiana-core/tests/notifications_baseline.rs; .github/workflows/nm00-baseline.yml; docs/roadmap/notifications-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: NM-00 source-only event→recipient→channel inventory, HumanInbox/RunStream/SSE/transcript fact-boundary guard, durable notification/read-state gap list, legacy watcher separation and NM-01..22 fixture catalog are scoped to this step; no NotificationStore/outbox/DeliveryWorker/message bus was added; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/platform.rs kiana-core/src/platform.rs kiana-daemon/src/run_stream.rs kiana-entrypoints/src/web.rs kiana-entrypoints/src/workbench_chat.rs kiana-entrypoints/src/cli.rs kiana-protocol/src/lib.rs kiana-client/src/lib.rs kiana-domain/src/company.rs kiana-core/src/company_business.rs kiana-core/src/recovery.rs kiana-core/tests/notifications_baseline.rs .github/workflows/nm00-baseline.yml docs/roadmap/notifications-baseline.md
  rg -n 'HumanInboxItem|human_items|RunStreamBus|broadcast::channel|/api/events|/api/receipt|watch_scheduled_tasks|NotificationStore|DeliveryWorker|not.*EventLog|不是 EventLog 或 Receipt' kiana-domain/src kiana-core/src kiana-daemon/src kiana-entrypoints/src kiana-core/tests docs/roadmap/notifications-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/notifications_baseline.rs; HumanInbox/Approval/Company/Failure projection, bounded RunStream epoch/sequence/gap/terminal, Web REST/SSE and Workbench display-only source assertions, absent durable NotificationStore/read state/outbox/DeliveryWorker; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions NM-00 job is queued by the push and is not awaited
status_change: NM-00 source baseline is implemented. EventLog/Approval/Company/Recovery facts remain canonical; `HumanInboxItem` is a bounded action projection, `RunStreamBus`/SSE is process-local best-effort display with gap/terminal replay, and Web/Workbench/CLI transcript/file summaries are disposable. No durable Notification/Message/Subscription/DeliveryAttempt/NotificationStore/DeliveryWorker/read-state or external channel exists; legacy `watch_scheduled_tasks` remains compatibility-only and cannot create notifications or scheduling authority. The source guard prevents a second message bus/loop or direct channel effect from being mistaken for product delivery.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; unread/read/ACK/subscription/outbox/lease/delivery/reconcile stores, external Email/Slack/Webhook/A2A/OS channels, cross-process restart and notification query parity remain NM-01..NM-22; RunStream delta/HTTP ACK/toast/model text never proves business action or delivery
reviewer: Codex root implementation review plus NM-00 notification/source-boundary reconciliation; no runtime test reviewer
```

### EQ-00 evaluation/quality baseline evidence (2026-09-16)

```text
source_snapshot: 6d0b084; kiana-commands/src/eval.rs; kiana-commands/tests/eval_command.rs; kiana-entrypoints/src/{cli,command_dispatch}.rs; kiana-entrypoints/tests/cli_eval.rs; kiana-core/src/eval.rs; kiana-core/tests/eval_baseline.rs; scripts/release-smoke.sh; .github/workflows/eq00-baseline.yml; docs/roadmap/evaluation-baseline.md; docs/module-map.md; docs/roadmap.md
worktree_status: EQ-00 source-only inventory of legacy EvalCommand vs OA-23 provider-independent core reducer, input/output/fact boundaries, quality-platform gaps, migration guard, fixture catalog and roadmap/status overlays are scoped to this step; no Quality DTO/EvalStore/second evaluator/model loop was added; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-commands/src/eval.rs kiana-commands/tests/eval_command.rs kiana-entrypoints/src/cli.rs kiana-entrypoints/src/command_dispatch.rs kiana-entrypoints/tests/cli_eval.rs kiana-core/src/eval.rs kiana-core/tests/eval_baseline.rs scripts/release-smoke.sh .github/workflows/eq00-baseline.yml docs/roadmap/evaluation-baseline.md
  rg -n 'EvalCommand|kiana.eval-suite.v1|kiana.eval-report.v1|kiana.eval-baseline.v1|run_suite|evaluate_provider_independent|rebuild_audit_projection|project_operational_metrics|diagnose_replay|QualityGate|EvalStore|TraceNormalizer|eval-curated' kiana-commands kiana-entrypoints kiana-core scripts docs/roadmap/evaluation-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/eval_baseline.rs; legacy suite/report/baseline source anchors, CLI command-dispatch boundary, OA-23 committed-fact reducer source anchors, release smoke wiring and missing Quality platform/second-loop guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EQ-00 job is queued by the push and is not awaited
status_change: EQ-00 source baseline is implemented. Legacy `kiana-commands::EvalCommand` remains a bounded read-only caller-fixture parser/report (`kiana.eval-suite.v1`/`kiana.eval-report.v1`/optional baseline) reached through historical command dispatch; it does not own identity, EventLog, Receipt, quality promotion or provider authority. OA-23 `kiana-core::evaluate_provider_independent` is a separate read-only committed-fact projection reusing Audit/Metric/Span/Run/Replay evidence and never invoking Model/Provider/Broker. The baseline freezes migration rules and fixture names for EQ-01..51 without adding QualityStore, TraceNormalizer, Judge, Promote/Rollback or a second execution loop.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; legacy eval still reads caller-selected paths, no isolated target/EvalStore/quality finding/baseline registry/feedback/drift/Promote/Rollback/real model quality or durable evidence artifact exists, and release smoke/GoldenTrace/score cannot prove business Outcome or live provider safety
reviewer: Codex root implementation review plus EQ-00 evaluation/source-boundary reconciliation; no runtime test reviewer
```

### PD-00 persistence/data-layer baseline evidence (2026-09-16)

```text
source_snapshot: 5226b97; kiana-domain/src/{journal,governance}.rs; kiana-ports/src/lib.rs; kiana-eventlog/src/{lib,jsonl,memory}.rs; kiana-daemon/src/{approval_store,harness_memory}.rs; kiana-query/src/{index,repo_map}.rs; kiana-core/src/{artifacts,receipts,recovery,projection}.rs; kiana-core/tests/persistence_baseline.rs; .github/workflows/pd00-baseline.yml; docs/roadmap/persistence-data-layer-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: PD-00 source-only persistence/data-layer inventory, fact/projection/cache owner matrix, JSONL/Memory/Approval/Artifact/Index capability and failure boundary, logical StorageRoot target, migration/backup/retention guard and PD-01..35 fixture catalog are scoped to this step; no StorageCoordinator/second EventLog/second execution loop was added; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/journal.rs kiana-domain/src/governance.rs kiana-ports/src/lib.rs kiana-eventlog/src/lib.rs kiana-eventlog/src/jsonl.rs kiana-eventlog/src/memory.rs kiana-daemon/src/approval_store.rs kiana-daemon/src/harness_memory.rs kiana-query/src/index.rs kiana-query/src/repo_map.rs kiana-core/src/artifacts.rs kiana-core/src/receipts.rs kiana-core/src/recovery.rs kiana-core/src/projection.rs kiana-core/tests/persistence_baseline.rs .github/workflows/pd00-baseline.yml docs/roadmap/persistence-data-layer-baseline.md
  rg -n 'EventStorePort|commit_transition|read_from|JsonlEventLog|MemoryEventLog|JOURNAL_HEADER_SCHEMA|MAX_JOURNAL_LOG_BYTES|sync_all|durable_commits: false|StorageRoot|ProjectionStore|ArtifactStore|backup|migration|retention|不能把 Memory 标成 durable' kiana-domain/src kiana-ports/src kiana-eventlog/src kiana-daemon/src kiana-query/src kiana-core/src kiana-core/tests/persistence_baseline.rs docs/roadmap/persistence-data-layer-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/persistence_baseline.rs; EventStore/Transition/CAS/cursor, JSONL v2/fsync/torn-tail/capacity, Memory non-durable, Approval proof/TTL, Artifact identity/hash, Memory scope, Query generation/cache and absent StorageRoot/Projection/Backup/Migration/Retention service assertions; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions PD-00 job is queued by the push and is not awaited
status_change: PD-00 source baseline is implemented. EventStore/RuntimeEvent and atomic Transition/CommandReceipt remain the sole fact write authority; JSONL v2 exposes lock/fsync/checksum/CAS/dedup/cursor/limit boundaries while Memory explicitly reports non-durable. ApprovalStore, Artifact bytes, Memory records, Query/index/cache, Receipt and projections are separate scoped/derived layers with visible read/identity/hash failures. Logical StorageRoot/StoreIdentity, Projection/Artifact/Backup/Migration/Retention ports and cross-process recovery are recorded as targets only; no second store or service was introduced.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; unified StorageRoot/owner namespace, projector checkpoint/generation, Artifact/Backup/Migration/Retention stores, Approval/Cell/PendingInvocation durable convergence, cross-process/power-loss/SQLite conformance, and data-layer UAT remain PD-01..PD-35; Memory/index/cache/CI/file existence cannot prove durable or business outcome
reviewer: Codex root implementation review plus PD-00 persistence/fact-boundary reconciliation; no runtime test reviewer
```

### INT-00 integrations/connectors baseline evidence (2026-09-16)

```text
source_snapshot: e41d69e; kiana-domain/src/connectors.rs; kiana-core/src/connectors.rs; kiana-daemon/src/connectors.rs; kiana-daemon/src/mcp_stdio.rs; kiana-protocol/src/lib.rs; kiana-client/src/lib.rs; kiana-core/tests/integrations_baseline.rs; .github/workflows/int00-baseline.yml; docs/roadmap/integrations-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: INT-00 source-only Provider/Connector/MCP/A2A/Notification terminology and boundary inventory, local_fixture/stdio owner-scope/idempotency/receipt/reconcile guard, external/live gap list, migration rules and INT-01..33 fixture catalog are scoped to this step; no external adapter/network/credential path or second execution loop was added; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/connectors.rs kiana-core/src/connectors.rs kiana-daemon/src/connectors.rs kiana-daemon/src/mcp_stdio.rs kiana-protocol/src/lib.rs kiana-client/src/lib.rs kiana-core/tests/integrations_baseline.rs .github/workflows/int00-baseline.yml docs/roadmap/integrations-baseline.md
  rg -n 'ConnectorDefinition|AccountBinding|ProviderReceipt|local_fixture|authorize_and_execute|binding_snapshot|external_effect_performed|connector_idempotency|stdio|A2A|not_supported|raw secret|no direct Broker|scope intersection' kiana-domain/src kiana-core/src kiana-daemon/src kiana-protocol/src kiana-core/tests/integrations_baseline.rs docs/roadmap/integrations-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test, provider, connector, MCP, webhook, A2A, network or external-account command executed locally
fixture or cassette: kiana-core/tests/integrations_baseline.rs; Provider/Connector/MCP/A2A/Notification boundary, local_fixture binding/invoke/reconcile, server-owned scope/idempotency/receipt and absent external/live adapter assertions; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions INT-00 job is queued by the push and is not awaited
status_change: INT-00 source baseline is implemented. Provider model endpoint/usage, Connector local_fixture definition/binding/operation/risk/receipt, MCP stdio and Notification projections remain distinct. Connector requests normalize through ControlPlane authorization and local daemon fixture output explicitly marks `external_effect_performed=false`; binding/project/scope/idempotency/payload/receipt/reconcile checks are server-owned. HTTP/remote MCP, A2A/webhook ingress, OAuth/SecretRef lease lifecycle, external accounts, delivery channels, real provider effects and physical operations remain not_supported/source; migration and fixture catalog are frozen for INT-01..33.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; only local_fixture/stdio source paths exist, no external provider/connector receipt/query/reconcile or OAuth/PKCE/account/tenant boundary is live, no webhook/A2A/HTTP MCP or physical effect is exercised, and no durable connector registry/rate ledger/retention propagation or four-entrypoint UAT exists
reviewer: Codex root implementation review plus INT-00 connector/provider/MCP/A2A/source-boundary reconciliation; no runtime test reviewer
```

### CP-01 server principal and project identity evidence (2026-09-16)

```text
source_snapshot: b923553; kiana-domain/src/{identity,contracts,lib,roles}.rs; kiana-core/src/sessions.rs; kiana-daemon/src/lib.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/cp01_identity.rs; kiana-core/tests/cp01_identity_guard.rs; .github/workflows/cp01-identity.yml; docs/roadmap/control-plane-identity-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/control-plane.md; docs/roadmap.md
worktree_status: CP-01 server-owned AuthenticatedPrincipalRef, deterministic ProjectIdentity (canonical root/device/inode/trust digest), typed SessionAssignment CAS/rebuild validation, daemon effectful project identity resolution, remote identity fixtures and roadmap/status overlays are scoped to this step; no external auth provider or second execution loop was added; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/identity.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/sessions.rs kiana-daemon/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/cp01_identity.rs kiana-core/tests/cp01_identity_guard.rs .github/workflows/cp01-identity.yml docs/roadmap/control-plane-identity-baseline.md
  rg -n 'AuthenticatedPrincipalRef|ProjectIdentity|SessionAssignment|principal_role_not_authorized|project_identity|project_authority|append_expected|session_assignment_mismatch|RequestMetadata' kiana-domain/src kiana-core/src kiana-daemon/src kiana-protocol/src kiana-domain/tests kiana-core/tests docs/roadmap/control-plane-identity-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-domain --test cp01_identity --locked --offline
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; identity/source guard test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp01_identity.rs deterministic canonical project IDs and assignment digest/unknown-field/tamper checks; kiana-core/tests/cp01_identity_guard.rs daemon server-principal/project/assignment/CAS source guard; GitHub Actions only
exit_code: 0 for source hashes, format, CP-01 test-target/workspace compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-01 job is queued by the push and is not awaited
status_change: CP-01 source slice is implemented. DaemonHost now owns an immutable local `AuthenticatedPrincipalRef`, overwrites wire actor with the server principal, enforces a server-side role allowlist and reads ProjectTrust through its injected authority. Effectful requests derive a deterministic `ProjectIdentity` from canonical root/device/inode and trust revision before authority synchronization. `SessionAssignment` is appended with CAS, typed principal/project/role/department/epoch and digest, and persisted rebuild validates the typed assignment when present. Continue/cancel/approval/receipt remain bound by existing session/run owner checks; no enterprise/OAuth principal or cross-tenant auth is claimed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; principal is still fixed local-user with env/catalog role allowlist, OS credential/OAuth/tenant provider and durable identity/assignment revocation epochs are absent, non-effectful queries do not mint assignments, and device/inode identity is platform-limited; no cross-process auth, external account or business outcome proof exists
reviewer: Codex root implementation review plus CP-01 identity/project-scope/assignment static-boundary review; no runtime test reviewer
```

### CP-02 Run/Turn/Invocation/Execution identity evidence (2026-09-16)

```text
source_snapshot: 388dfe1; kiana-domain/src/{execution_identity,contracts,lib}.rs; kiana-core/src/{lifecycle,capabilities,dispatch,recovery}.rs; kiana-core/src/invocation_projection.rs; kiana-protocol/src/lib.rs; kiana-client/src/lib.rs; kiana-domain/tests/cp02_execution_identity.rs; kiana-core/tests/cp02_execution_guard.rs; .github/workflows/cp02-execution-identity.yml; docs/roadmap/control-plane-execution-identity-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/control-plane.md; docs/roadmap.md
worktree_status: CP-02 typed TurnIdentity/InvocationIdentity, explicit run.turn.v2 new-turn boundary, legacy Continue marker, Broker execution identity binding, remote fixtures and roadmap/status overlays are scoped to this step; direct commands do not receive fabricated Harness identity; no unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/execution_identity.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/lifecycle.rs kiana-core/src/capabilities.rs kiana-core/src/dispatch.rs kiana-core/src/recovery.rs kiana-core/src/invocation_projection.rs kiana-protocol/src/lib.rs kiana-client/src/lib.rs kiana-domain/tests/cp02_execution_identity.rs kiana-core/tests/cp02_execution_guard.rs .github/workflows/cp02-execution-identity.yml docs/roadmap/control-plane-execution-identity-baseline.md
  rg -n 'TurnIdentity|InvocationIdentity|TurnSemantics|run.turn.v2|run.predecessor|LegacyContinue|run_not_terminal_use_resume|run_unknown_requires_reconciliation|typed_invocation_identity|invocation_terminal_conflict|ContinueRequest|ResumeRequest' kiana-domain/src kiana-core/src kiana-protocol/src kiana-client/src kiana-domain/tests/cp02_execution_identity.rs kiana-core/tests/cp02_execution_guard.rs docs/roadmap/control-plane-execution-identity-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp02_execution_identity.rs Turn Start/NewTurn/LegacyContinue/Resume and Invocation call_id/attempt/digest fixtures; kiana-core/tests/cp02_execution_guard.rs lifecycle/dispatch/projection/protocol source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-02 job is queued by the push and is not awaited
status_change: CP-02 source slice is implemented. `TurnIdentity` records server-derived Session/Run/Turn IDs, predecessor and explicit Start/NewTurn/LegacyContinue/Resume semantics. `run.turn.v2` refuses non-terminal or result-unknown predecessors and creates a fresh Run/Turn; v1 Continue remains a separately marked same-Run compatibility path; Resume writes an explicit `run.resume_prepared` identity claim and remains the same Run. Capability request facts carry server-derived turn/invocation/attempt references, while Broker permit/dispatch/execution/result events bind the complete typed InvocationIdentity with real ExecutionId. Reused model `call_id` is correlation only; projection terminal conflicts remain fail-closed; direct commands do not fabricate Harness identity.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; legacy history without TurnId has no durable upcaster yet, full InvocationLedger/RunSnapshot and cross-process Resume remain future CP/ER/PD work, retry/reconciliation still requires later steps, `call_id` remains untrusted correlation metadata, and no provider/connector/live/physical effect or business Outcome is proven
reviewer: Codex root implementation review plus CP-02 identity/state-machine/Continue-Resume/projection source-boundary review; no runtime test reviewer
```

### SC-00 security and compliance baseline evidence (2026-09-16)

```text
source_snapshot: 7a009ea; docs/company-os-security-constitution.md; docs/roadmap/security-compliance.md; docs/roadmap/security-compliance-baseline.md; docs/module-map.md; docs/roadmap.md; CURRENT_STATUS.md; kiana-domain/src/{identity,execution_identity,redaction,contracts}.rs; kiana-core/src/{lib,events,dispatch,recovery,projection,data_governance}.rs; kiana-policy/src/lib.rs; kiana-gates/src/lib.rs; kiana-daemon/src/lib.rs; kiana-entrypoints/src/lib.rs; kiana-runner/src/harness.rs; kiana-eventlog/src/lib.rs; kiana-core/tests/security_baseline.rs; .github/workflows/sc00-baseline.yml
worktree_status: SC-00 security asset/entry/control/evidence inventory, SEC-01..12 status matrix, T01..T12 and SC-01..43 handoff, module-map/roadmap links, remote source guard and status evidence are scoped to this step; no new security service, auth provider, secret path, external effect or second execution loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum docs/company-os-security-constitution.md docs/roadmap/security-compliance.md docs/roadmap/security-compliance-baseline.md docs/module-map.md docs/roadmap.md CURRENT_STATUS.md kiana-domain/src/{identity,execution_identity,redaction,contracts}.rs kiana-core/src/{lib,events,dispatch,recovery,projection,data_governance}.rs kiana-policy/src/lib.rs kiana-gates/src/lib.rs kiana-daemon/src/lib.rs kiana-entrypoints/src/lib.rs kiana-runner/src/harness.rs kiana-eventlog/src/lib.rs kiana-core/tests/security_baseline.rs .github/workflows/sc00-baseline.yml
  rg -n 'SEC-0[1-9]|SEC-1[0-2]|T0[1-9]|T1[0-2]|SC-00|SC-43|feature_status|proof_level|not_supported|SecretRef|EventLog|ControlPlane|Broker' docs/company-os-security-constitution.md docs/roadmap/security-compliance.md docs/roadmap/security-compliance-baseline.md docs/module-map.md CURRENT_STATUS.md kiana-core/tests/security_baseline.rs
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/security_baseline.rs constitution/threat/step/asset/spine and partial-boundary source fixtures; GitHub Actions only; no provider/connector/credential or external system contacted
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SC-00 job is queued by the push and is not awaited
status_change: SC-00 source baseline is implemented. Security assets, trusted/untrusted boundaries, six product entry categories, EventLog/ControlPlane/Broker execution spine, SEC-01..12 feature/proof matrix and T01..T12 threat references are recorded. Existing local controls remain explicitly partial/source/local_behavior according to their own snapshots; external/physical effects remain not_supported, and authentication, SecretStore, full TOCTOU/egress, durable recovery/retention/delete, SBOM/signing and security UAT remain open SC/CP/CAP/ER/PD/DEP/INT work.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; this inventory does not enforce the normative constitution, does not rewrite historical evidence, and cannot prove production secrets, tenant identity, complete prompt-injection/red-team coverage, cross-process durability, external receipt correctness, live provider/connector safety or physical safety; next security card is SC-01 threat register
reviewer: Codex root implementation review plus SC-00 asset/entry/control/proof-boundary reconciliation; no runtime test reviewer
```

### P0-G-04 projection and restart recovery closure evidence (2026-09-16)

```text
source_snapshot: cb5c0a0; kiana-core/src/{events,projection,invocation_projection,recovery}.rs; kiana-core/tests/control_plane.rs; kiana-core/tests/p0_g04_projection_guard.rs; kiana-domain/src/execution_identity.rs; docs/roadmap/event-receipt-recovery-baseline.md; docs/roadmap.md; docs/module-map.md; .github/workflows/p0-g04-projection.yml
worktree_status: P0-G-04 Run/Invocation event projection, lazy cache invalidation, restart pending approval recheck, terminal conflict guard and focused remote workflow are scoped to this closure; CP-02 typed identity fields are consumed as additive facts; no second EventLog/projection authority or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-core/src/events.rs kiana-core/src/projection.rs kiana-core/src/invocation_projection.rs kiana-core/src/recovery.rs kiana-core/tests/control_plane.rs kiana-core/tests/p0_g04_projection_guard.rs kiana-domain/src/execution_identity.rs docs/roadmap/event-receipt-recovery-baseline.md docs/roadmap.md docs/module-map.md .github/workflows/p0-g04-projection.yml
  rg -n 'new_process_rebuilds_run_state_from_events_alone|new_process_rebuilds_invocation_state_from_events_alone|invocation_projection_conflicting_terminals_fail_closed|projection_cache_miss_rebuilds_pending_invocations_with_authorization_recheck|project_run_state|project_invocations|cache_invocation_projection|invalidate_invocation_projection|run_snapshot_stale|result_unknown' kiana-core/src kiana-core/tests docs/roadmap/event-receipt-recovery-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/control_plane.rs focused new-process Run/Invocation rebuild, conflicting terminal, pending approval authorization recheck; kiana-core/tests/p0_g04_projection_guard.rs source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-G-04 job is queued by the push and is not awaited
status_change: P0-G-04 is closed at source level. EventLog remains the authority for Run/Invocation state; restarted ControlPlane can rebuild run and invocation projections without Runner memory, cache invalidation tracks broker-side appends, pending approvals are reconstructed and re-authorized, missing request/run identity fails closed, `result_unknown` remains fenced, and conflicting/reordered terminal facts cannot be selected arbitrarily. A focused CI workflow now owns the named acceptance tests.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; the current focused tests are remote-only and still use in-memory/fake adapters, JSONL power-loss and cross-process durability are not proven, legacy events without typed identity remain compatibility-readable, full RunSnapshot/InvocationLedger/lease reconciliation and external effect receipts remain future ER/CP/PD/SC work
reviewer: Codex root implementation review plus P0-G-04 EventLog/projection/recovery source-boundary review; no runtime test reviewer
```

### CP-03 action catalog and PreparedAction evidence (2026-09-16)

```text
source_snapshot: 2e3b7f6; kiana-domain/src/{actions,tool_catalog,contracts,lib}.rs; kiana-core/src/{approvals,capabilities,dispatch,events}.rs; kiana-runner/src/tools.rs; kiana-daemon/src/harness_capabilities.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/cp03_action_contract.rs; kiana-core/tests/cp03_action_guard.rs; .github/workflows/cp03-action-contract.yml; docs/roadmap/control-plane-action-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/control-plane.md; docs/roadmap.md
worktree_status: CP-03 closed ACTION_OPERATIONS descriptor catalog, explicit action-catalog schema registry, bounded argument/result schema, server-owned risk/resource/effect/cancel/reconcile/idempotency metadata, PreparedAction catalog/action digest validation, remote fixtures and roadmap/status overlays are scoped to this step; no model-visible tool was added and no second execution path was introduced; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/actions.rs kiana-domain/src/tool_catalog.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/approvals.rs kiana-core/src/capabilities.rs kiana-core/src/dispatch.rs kiana-core/src/events.rs kiana-runner/src/tools.rs kiana-daemon/src/harness_capabilities.rs kiana-protocol/src/lib.rs kiana-domain/tests/cp03_action_contract.rs kiana-core/tests/cp03_action_guard.rs .github/workflows/cp03-action-contract.yml docs/roadmap/control-plane-action-baseline.md
  rg -n 'ACTION_OPERATIONS|CapabilityActionDescriptor|validate_action_catalog|PreparedAction|capability_action_digest|action_risk_downgrade|parse_bounded_json|json_duplicate_key|stamp_request_identity|pin_action_authority|authorize_and_execute|additionalProperties' kiana-domain/src kiana-core/src kiana-runner/src kiana-daemon/src kiana-protocol/src kiana-domain/tests/cp03_action_contract.rs kiana-core/tests/cp03_action_guard.rs docs/roadmap/control-plane-action-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp03_action_contract.rs descriptor completeness, forged ReadOnly risk, PreparedAction normalization/catalog digest, duplicate JSON and numeric rejection; kiana-core/tests/cp03_action_guard.rs source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-03 job is queued by the push and is not awaited
status_change: CP-03 source slice is implemented. The closed action catalog now has per-operation capability/minimum-risk/schema/resource/effect/cancellation/reconciliation/idempotency/binding metadata and validates schema boundaries before preparation. ControlPlane removes caller authority fields, stamps server context, normalizes canonical operation/path/sandbox/defaults and hooks, creates immutable PreparedAction with catalog/action digests, and pins the exact action before policy/approval/dispatch. ReadOnly risk downgrades, unknown/incomplete operations, duplicate JSON keys, invalid numeric/path/alias inputs and catalog drift fail closed; Runner, policy, approval, Broker and handler consume the same normalized action value. Direct Company/context commands retain explicit scope and do not fabricate Harness identity.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; JSON Schema support is a bounded subset, descriptor compatibility fields remain explicit but permissive for migration, PreparedAction is not a permit, full ExecutionContext/Grant intersection/authority epoch and durable ToolSnapshot remain CP-04+ and CAP/ER/PD work, all handler TOCTOU/egress and external/provider/physical effects are not proven, and no automatic retry/reconciliation is introduced
reviewer: Codex root implementation review plus CP-03 catalog/normalization/risk/digest source-boundary review; no runtime test reviewer
```

### CP-04 scope intersection and monotonic authorization evidence (2026-09-16)

```text
source_snapshot: 4903fdb; kiana-domain/src/{scope,capabilities,actions,contracts,lib}.rs; kiana-core/src/{capabilities,approvals,cell_registry}.rs; kiana-policy/src/lib.rs; kiana-gates/src/lib.rs; kiana-domain/tests/cp04_scope.rs; kiana-core/tests/cp04_scope_guard.rs; .github/workflows/cp04-scope.yml; docs/roadmap/control-plane-scope-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/control-plane.md; docs/roadmap.md
worktree_status: CP-04 typed ScopeSet/ScopeDimension/ScopeLimit with digest, bounded values, path-aware intersection, subset checks and NotApplicable-vs-Restricted distinction, ControlPlane action/context scope guard, monotonic policy/gate/hook merge and remote fixtures/roadmap overlays are scoped to this step; no second authorization loop or permission union was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/scope.rs kiana-domain/src/capabilities.rs kiana-domain/src/actions.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/capabilities.rs kiana-core/src/approvals.rs kiana-core/src/cell_registry.rs kiana-policy/src/lib.rs kiana-gates/src/lib.rs kiana-domain/tests/cp04_scope.rs kiana-core/tests/cp04_scope_guard.rs .github/workflows/cp04-scope.yml docs/roadmap/control-plane-scope-baseline.md
  rg -n 'ScopeSet|ScopeDimension|ScopeLimit|NotApplicable|Restricted|scope_intersection_empty|effective_action_scope|scope_intersection_invalid|grant.contains|GateDecision::AwaitingApproval|hard_policy_denial' kiana-domain/src kiana-core/src kiana-policy/src kiana-gates/src kiana-domain/tests/cp04_scope.rs kiana-core/tests/cp04_scope_guard.rs docs/roadmap/control-plane-scope-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp04_scope.rs path-aware parent/child subset, budget/depth min, NA vs empty Restricted, digest/unknown-field guards; kiana-core/tests/cp04_scope_guard.rs source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-04 job is queued by the push and is not awaited
status_change: CP-04 source slice is implemented. Domain ScopeSet now separates NotApplicable from Restricted, bounds and canonicalizes values, intersects operation/path/namespace/network by narrowing (path prefixes choose the narrower path), takes min budget/depth, and rejects empty restricted intersections. ControlPlane computes action/context scope before policy/Broker; policy hard denial remains non-overridable, any Gate/Hook Deny remains denied, Ask requirements remain pending and merge deterministically, and Cell parent Grant containment remains enforced. PreparedAction/action digest, Approval and Permit remain distinct authority boundaries.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; ScopeSet is not yet a durable full ExecutionContext or authority epoch ledger, Cell/Grant/Approval dimensions are only partially represented, path string intersection does not prove descriptor-relative TOCTOU/egress/OS isolation, pure monotonicity does not prove human approval or external effect, and property/fuzz/cross-process/revocation UAT remain later CP/CAP/ER/PD/SC work
reviewer: Codex root implementation review plus CP-04 scope/monotonic-policy/permission-union source-boundary review; no runtime test reviewer
```

### CP-05 capability entry-path parity evidence (2026-09-16)

```text
source_snapshot: b42a36c; kiana-core/src/{approvals,capabilities,dispatch,lifecycle}.rs; kiana-core/tests/control_plane.rs; kiana-core/tests/cp05_entry_paths_guard.rs; kiana-domain/src/{actions,scope,capabilities}.rs; kiana-runner/src/tools.rs; kiana-daemon/src/harness_capabilities.rs; kiana-domain/tests/cp03_action_contract.rs; .github/workflows/cp05-entry-paths.yml; docs/roadmap/control-plane-entry-path-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/control-plane.md; docs/roadmap.md
worktree_status: CP-05 direct authorize/execute, Harness broker capability and approval continuation all reuse prepare/authorize/stage-or-dispatch/finalize helpers; current decision context, action digest, Cell/Grant/permit, result/Unknown and EventLog boundaries plus focused remote fixtures/source guard are scoped to this step; no second model loop or raw Broker path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-core/src/approvals.rs kiana-core/src/capabilities.rs kiana-core/src/dispatch.rs kiana-core/src/lifecycle.rs kiana-core/src/events.rs kiana-core/tests/control_plane.rs kiana-core/tests/cp05_entry_paths_guard.rs kiana-domain/src/actions.rs kiana-domain/src/scope.rs kiana-domain/src/capabilities.rs kiana-runner/src/tools.rs kiana-daemon/src/harness_capabilities.rs kiana-domain/tests/cp03_action_contract.rs .github/workflows/cp05-entry-paths.yml docs/roadmap/control-plane-entry-path-baseline.md
  rg -n 'authorize_and_execute|broker_harness_capability|resume_approved_invocation|execute_authorized_request|prepare_capability_action|authorize_capability_action|stage_capability_action|dispatch_capability_action|finalize_capability_action|decision_context|verify_and_consume|result_unknown' kiana-core/src kiana-core/tests/control_plane.rs kiana-core/tests/cp05_entry_paths_guard.rs docs/roadmap/control-plane-entry-path-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/control_plane.rs direct result mismatch, Harness result mismatch, approval continuation monotonic event, cancel-after-approval and untrusted command fixtures; kiana-core/tests/cp05_entry_paths_guard.rs shared-pipeline source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-05 job is queued by the push and is not awaited
status_change: CP-05 source slice is implemented. Direct `authorize_and_execute`, Harness `broker_harness_capability`, and approval `resume_approved_invocation` all use the same ControlPlane preparation, policy/gate/hook merge, approval stage, dispatch permit/Broker, result normalization and EventLog finalization helpers. Approval continuation rebinds current decision context and exact action before dispatch; direct commands preserve explicit scope; result mismatches, cancellation, persistence ambiguity and Unknown remain fail-closed. No UI, Provider, Workflow or Runner path can call raw Broker execution.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; event sequence and adapter responses still differ by ingress, ApprovalStore/PendingInvocation/CellRegistry/cancel state have process-local parts, atomic transition/once permit and cross-process recovery remain CP-06+ and ER/PD/SC work, and fake Broker/Runner fixtures do not prove provider/connector/OS/physical effects
reviewer: Codex root implementation review plus CP-05 direct/Harness/approval pipeline parity source-boundary review; no runtime test reviewer
```

### CP-06 atomic transition and command idempotency evidence (2026-09-16)

```text
source_snapshot: b744469; kiana-domain/src/{journal,contracts,lib}.rs; kiana-ports/src/lib.rs; kiana-eventlog/src/{event_store_core,journal_core,memory,jsonl,stream}.rs; kiana-core/src/{dispatch,events,approvals}.rs; kiana-eventlog/tests/cp06_atomic_transitions.rs; kiana-core/tests/cp06_transition_guard.rs; .github/workflows/cp06-atomic-transitions.yml; docs/roadmap/control-plane-transition-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/control-plane.md; docs/roadmap.md
worktree_status: CP-06 TransitionBatch/read-set/CommandReceipt/CommitOutcome contracts, Memory/JSONL shared transition planner, EventStore capability negotiation, all-or-none CAS/idempotency and focused remote fixtures/source guard/roadmap overlays are scoped to this step; no second EventLog or append-based authorization bypass was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/journal.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-ports/src/lib.rs kiana-eventlog/src/event_store_core.rs kiana-eventlog/src/journal_core.rs kiana-eventlog/src/memory.rs kiana-eventlog/src/jsonl.rs kiana-eventlog/src/stream.rs kiana-core/src/dispatch.rs kiana-core/src/events.rs kiana-core/src/approvals.rs kiana-eventlog/tests/cp06_atomic_transitions.rs kiana-core/tests/cp06_transition_guard.rs .github/workflows/cp06-atomic-transitions.yml docs/roadmap/control-plane-transition-baseline.md
  rg -n 'TransitionBatch|CommandReceipt|CommitOutcome|expected_versions|commit_transition|event_store_command_digest_mismatch|TransitionPlan::Conflict|CommitOutcome::Unknown|commit_confirmed|journal_write_not_in_read_set|journal_frame_size_limit' kiana-domain/src kiana-ports/src kiana-eventlog/src kiana-core/src kiana-eventlog/tests/cp06_atomic_transitions.rs kiana-core/tests/cp06_transition_guard.rs docs/roadmap/control-plane-transition-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-eventlog/tests/cp06_atomic_transitions.rs stale aggregate CAS, same command/different payload, stale allow recomputation; existing oa06 commit-observer fixtures; kiana-core/tests/cp06_transition_guard.rs source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-06 job is queued by the push and is not awaited
status_change: CP-06 source slice is implemented. TransitionBatch validates command digest, bounded events/read-set, aggregate membership, contiguous stream versions and frame limits. EventStorePort advertises atomic transition/receipt/cursor capabilities and explicitly rejects unsupported downgrade; MemoryEventLog, JsonlEventLog and StreamEventStore share CAS/idempotency planning. Same command with the same digest replays the original receipt; a different digest conflicts; stale expected versions return changed versions without partial aggregate writes; Unknown never publishes observer authority and requires command confirmation before dispatch.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; Memory is non-durable, JSONL locking/frame checks do not prove power-loss/cross-host durability, compatibility append remains available, external/provider/connector effects need separate receipts/reconcile, and approval/budget/lease all-in-one transitions plus disk recovery remain CP-07+ and ER/PD/SC work
reviewer: Codex root implementation review plus CP-06 transition/CAS/idempotency/Unknown source-boundary review; no runtime test reviewer
```

### ER-01 event schema and kind registry evidence (2026-09-16)

```text
source_snapshot: b597dfe; kiana-domain/src/{event_contracts,contracts,states,journal,lib}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/er01_event_contract.rs; kiana-core/tests/er01_event_contract_guard.rs; .github/workflows/er01-event-schema.yml; docs/roadmap/event-receipt-schema-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md
worktree_status: ER-01 EventKindSpec registry, runtime-event schema contract, required/allowed payload IDs, terminal/secret/aggregate metadata, unknown opaque vs required-family fail-closed policy, schema downgrade check, deterministic migration map, legacy RuntimeEvent decode boundary, remote fixtures/source guard and roadmap overlays are scoped to this step; no second EventLog or event rewrite path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/event_contracts.rs kiana-domain/src/contracts.rs kiana-domain/src/states.rs kiana-domain/src/journal.rs kiana-domain/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/er01_event_contract.rs kiana-core/tests/er01_event_contract_guard.rs .github/workflows/er01-event-schema.yml docs/roadmap/event-receipt-schema-baseline.md
  rg -n 'EventKindSpec|EVENT_KIND_SPECS|EVENT_MIGRATIONS|unknown_required_event_kind|event_schema_version_incompatible|event_payload_unknown_field|validate_runtime_event|secret_policy|required_ids|RUNTIME_EVENT_SCHEMA' kiana-domain/src kiana-protocol/src kiana-domain/tests/er01_event_contract.rs kiana-core/tests/er01_event_contract_guard.rs docs/roadmap/event-receipt-schema-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/er01_event_contract.rs registry metadata, opaque/required unknown, schema downgrade, migration, required IDs and payload unknown-field fixtures; kiana-core/tests/er01_event_contract_guard.rs source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-01 job is queued by the push and is not awaited
status_change: ER-01 source slice is implemented. `EventKindSpec`/`EVENT_KIND_SPECS` now register critical request/run/capability/approval/invocation/execution/action/session kinds with schema/version, aggregate owner, required IDs, terminal and secret policy, allowed fields and migration name. `validate_runtime_event` preserves non-required opaque events without granting execution while unknown required-family kinds, major downgrades, missing IDs and payload unknown fields fail closed; old RuntimeEvent/JSONL decode remains compatibility-readable and deterministic family migration is explicit.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; registry is an additive interpretation layer and does not yet embed schema/version in every legacy event, cover all historical kind literals, or force every EventStore/projector path; migration cannot infer missing identity/owner/secret provenance, and redaction, CAS, receipt correctness, durable recovery/retention/delete, external effect/reconcile and live/physical evidence remain ER-02+ / CP/PD/SC work
reviewer: Codex root implementation review plus ER-01 event kind/version/migration/unknown-field source-boundary review; no runtime test reviewer
```

### ER-02 identity, correlation and ordering evidence (2026-09-16)

```text
source_snapshot: 863b30f; kiana-domain/src/{states,correlation,contracts,journal,lib}.rs; kiana-core/src/{events,invocation_projection,span_projection}.rs; kiana-eventlog/src/{event_store_core,memory,journal_core}.rs; kiana-domain/tests/er02_identity.rs; kiana-eventlog/tests/er02_identity.rs; kiana-core/tests/{er02_identity,er02_identity_guard}.rs; .github/workflows/er02-identity.yml; docs/roadmap/event-receipt-identity-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md
worktree_status: ER-02 RuntimeEvent optional command/correlation/causation/parent links, default request correlation, self-link validation, existing CorrelationContext/AttemptRef/CausationRef scope checks, EventStore event_id/command digest guards, run-aware invocation/span projection and legacy query-only boundary are scoped to this step; no sequence-based cross-run pairing or second fact source was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/states.rs kiana-domain/src/correlation.rs kiana-domain/src/contracts.rs kiana-domain/src/journal.rs kiana-domain/src/lib.rs kiana-core/src/events.rs kiana-core/src/invocation_projection.rs kiana-core/src/span_projection.rs kiana-eventlog/src/event_store_core.rs kiana-eventlog/src/memory.rs kiana-eventlog/src/journal_core.rs kiana-domain/tests/er02_identity.rs kiana-eventlog/tests/er02_identity.rs kiana-core/tests/er02_identity.rs kiana-core/tests/er02_identity_guard.rs .github/workflows/er02-identity.yml docs/roadmap/event-receipt-identity-baseline.md
  rg -n 'command_id|correlation_id|causation_event_id|parent_event_id|validate_identity_links|with_identity_links|stamp_event_links|CausationRef|AttemptRef|event_id_duplicate|event_store_command_digest_mismatch|event_matches_run|turn_id|invocation_id' kiana-domain/src kiana-core/src kiana-eventlog/src kiana-domain/tests kiana-core/tests docs/roadmap/event-receipt-identity-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/er02_identity.rs new/legacy RuntimeEvent links; kiana-eventlog/tests/er02_identity.rs event_id/command digest conflicts; kiana-core/tests/er02_identity.rs cross-run sequence pairing; kiana-core/tests/er02_identity_guard.rs source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-02 job is queued by the push and is not awaited
status_change: ER-02 source slice is implemented. New RuntimeEvent facts carry a request-root correlation and can record explicit command, causation-event and parent-event links; legacy events without links remain readable. Links validate self-reference and correlation/command requirements. Existing CorrelationContext/AttemptRef/CausationRef enforce run/turn/invocation/execution/attempt scope, EventStore rejects event_id reuse and same-command digest drift, and Run/Invocation/Span projectors filter by stable run/aggregate IDs rather than request-local sequence. Direct/Harness/approval paths retain the same ID chain while legacy metadata remains query-only.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; links remain additive and not enforced on every legacy writer, event kind registry/upcast/redaction/receipt binding remains partial, in-memory/JSONL duplicate/CAS guards do not prove power-loss/cross-host durability, and complete InvocationLedger/attempt retry/reconcile, external effect receipt, retention/delete and live/physical evidence remain future ER/CP/PD/SC work
reviewer: Codex root implementation review plus ER-02 identity/correlation/causation/order/projection source-boundary review; no runtime test reviewer
```

### ER-03 event redaction and protected artifact reference evidence (2026-09-16)

```text
source_snapshot: 066bdfa; kiana-domain/src/{redaction,states,contracts}.rs; kiana-core/src/{events,redaction,artifacts,receipts}.rs; kiana-domain/tests/er03_redaction.rs; kiana-core/tests/er03_redaction_guard.rs; .github/workflows/er03-event-redaction.yml; docs/roadmap/event-receipt-redaction-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md
worktree_status: ER-03 bounded recursive redaction at core append/terminal boundary, profile digest/non-resumable/data epoch/artifact refs metadata, payload depth/size/NUL/stability checks, domain redaction/streaming contracts, remote fixtures/source guard and roadmap overlays are scoped to this step; no raw secret/artifact bytes or second EventStore path was introduced; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/redaction.rs kiana-domain/src/states.rs kiana-domain/src/contracts.rs kiana-core/src/events.rs kiana-core/src/redaction.rs kiana-core/src/artifacts.rs kiana-core/src/receipts.rs kiana-domain/tests/er03_redaction.rs kiana-core/tests/er03_redaction_guard.rs .github/workflows/er03-event-redaction.yml docs/roadmap/event-receipt-redaction-baseline.md
  rg -n 'prepare_event_payload|redact_event_value|payload_depth|event_payload_size_limit|event_redaction_not_stable|event_data_epoch_invalid|event_artifact_refs|with_redaction_metadata|validate_redaction_metadata|encode_bounded_value|StreamingRedactor' kiana-domain/src kiana-core/src kiana-domain/tests/er03_redaction.rs kiana-core/tests/er03_redaction_guard.rs docs/roadmap/event-receipt-redaction-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/er03_redaction.rs secret sentinel/reference, non-resumable/data epoch/artifact metadata, oversize/deep payload; kiana-core/tests/er03_redaction_guard.rs EventStore/Receipt/Artifact source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-03 job is queued by the push and is not awaited
status_change: ER-03 source slice is implemented. EventStore core append and terminal paths now redact recursively, enforce bounded depth/size/NUL and idempotent redaction, parse data epoch/artifact references and attach redaction profile digest plus non-resumable metadata; invalid payload metadata fails before projection and never returns raw input. Domain RedactionProfile/StreamingRedactor remain the shared redaction contract, while Receipt/Artifact consume references rather than bytes; legacy RuntimeEvent fields remain readable.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; direct transition/legacy writers still need later boundary adoption, key/marker redaction cannot detect arbitrary encoded secrets or process memory/argv/env, profile/data epoch are metadata not SecretStore/Retention/Delete authority, artifact refs do not prove content/business outcome, and durable retention/delete/cross-process recovery/external effects remain ER-04+ / PD/SC/CAP work
reviewer: Codex root implementation review plus ER-03 redaction/size/artifact/resumability source-boundary review; no runtime test reviewer
```

### ER-04 CommandReceipt and transition read-set evidence (2026-09-16)

```text
source_snapshot: 3942623; kiana-domain/src/{journal,contracts,states,lib}.rs; kiana-ports/src/lib.rs; kiana-eventlog/src/{event_store_core,journal_core,memory,jsonl,stream}.rs; kiana-core/src/{dispatch,events,approvals}.rs; kiana-eventlog/tests/er04_command_receipt.rs; kiana-core/tests/er04_command_receipt_guard.rs; .github/workflows/er04-command-receipt.yml; docs/roadmap/event-receipt-command-receipt-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md
worktree_status: ER-04 CommandReceipt schema/validation, TransitionBatch read-set/CAS/digest binding, EventStore committed/replayed/conflict/unknown confirmation, no-partial-write guard, remote fixtures/source guard and roadmap overlays are scoped to this step; compatibility append remains explicit and no second authority was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/journal.rs kiana-domain/src/contracts.rs kiana-domain/src/states.rs kiana-domain/src/lib.rs kiana-ports/src/lib.rs kiana-eventlog/src/event_store_core.rs kiana-eventlog/src/journal_core.rs kiana-eventlog/src/memory.rs kiana-eventlog/src/jsonl.rs kiana-eventlog/src/stream.rs kiana-core/src/dispatch.rs kiana-core/src/events.rs kiana-core/src/approvals.rs kiana-eventlog/tests/er04_command_receipt.rs kiana-core/tests/er04_command_receipt_guard.rs .github/workflows/er04-command-receipt.yml docs/roadmap/event-receipt-command-receipt-baseline.md
  rg -n 'COMMAND_RECEIPT_SCHEMA|CommandReceipt|validate_against|command_receipt_identity_mismatch|command_receipt_cursor_invalid|command_receipt_event_ids_mismatch|command_receipt_read_set_missing|commit_transition|read_command|CommitOutcome::Unknown|journal_write_not_in_read_set' kiana-domain/src kiana-ports/src kiana-eventlog/src kiana-core/src kiana-eventlog/tests/er04_command_receipt.rs kiana-core/tests/er04_command_receipt_guard.rs docs/roadmap/event-receipt-command-receipt-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-eventlog/tests/er04_command_receipt.rs missing dependency, stale read-set, Unknown commit and command receipt boundary fixtures; kiana-core/tests/er04_command_receipt_guard.rs source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-04 job is queued by the push and is not awaited
status_change: ER-04 source slice is implemented. `CommandReceipt::validate_against` binds batch command/digest, contiguous cursor, ordered unique event IDs and every expected aggregate version. EventStore/ControlPlane use the same TransitionBatch CAS/read-set and `read_command` confirmation: missing dependencies, stale versions and same-command digest drift fail without partial writes; Committed/Replayed return authoritative receipts, Conflict reports changed versions, Unknown never grants dispatch or observer authority. The command-receipt schema is registered and legacy append remains explicit compatibility only.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; receipt validation cannot prove handler/provider/connector effect correctness, Memory is non-durable, JSONL/frame/lock behavior does not prove power-loss/cross-host consistency, compatibility append and some approval/budget/lease transitions still need ER-05+/CP-07+/PD/SC atomic migration, and external reconciliation/live/physical evidence is absent
reviewer: Codex root implementation review plus ER-04 receipt/read-set/CAS/Unknown source-boundary review; no runtime test reviewer
```

### CAP-01 capability authority and handler binding evidence (2026-09-16)

```text
source_snapshot: f41c636; kiana-domain/src/{actions,tool_catalog,capabilities,contracts,lib}.rs; kiana-capability-broker/src/lib.rs; kiana-daemon/src/{lib,harness_capabilities,harness_memory,harness_mcp}.rs; kiana-runner/src/tools.rs; kiana-domain/tests/cap01_registry.rs; kiana-capability-broker/tests/cap01_registry.rs; kiana-core/tests/cap01_authority_guard.rs; .github/workflows/cap01-authority.yml; docs/roadmap/capability-authority-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/capability.md; docs/roadmap.md
worktree_status: CAP-01 domain action catalog/descriptor/schema/risk metadata, Broker exact handler registration/version checks, DaemonHost catalog seal, Runner five-tool/operator-only boundary, unknown/duplicate/fallback rejection fixtures/source guard and roadmap overlays are scoped to this step; no model-visible tool or raw Broker path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/actions.rs kiana-domain/src/tool_catalog.rs kiana-domain/src/capabilities.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-capability-broker/src/lib.rs kiana-daemon/src/lib.rs kiana-daemon/src/harness_capabilities.rs kiana-daemon/src/harness_memory.rs kiana-daemon/src/harness_mcp.rs kiana-runner/src/tools.rs kiana-domain/tests/cap01_registry.rs kiana-capability-broker/tests/cap01_registry.rs kiana-core/tests/cap01_authority_guard.rs .github/workflows/cap01-authority.yml docs/roadmap/capability-authority-baseline.md
  rg -n 'ACTION_OPERATIONS|CapabilityActionDescriptor|PreparedAction|validate_action_catalog|validate_catalog_bindings|capability_handler_already_registered|capability_binding_version_mismatch|capability_catalog_sealed|tool_schemas|operator_only_action|tool_unsupported|execution_permit_verifier_required' kiana-domain/src kiana-capability-broker/src kiana-daemon/src kiana-runner/src kiana-domain/tests kiana-capability-broker/tests kiana-core/tests/cap01_authority_guard.rs docs/roadmap/capability-authority-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cap01_registry.rs model-visible five-tool and operator-only separation; kiana-capability-broker/tests/cap01_registry.rs duplicate/alias/version/unregistered fallback guards; kiana-core/tests/cap01_authority_guard.rs source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CAP-01 job is queued by the push and is not awaited
status_change: CAP-01 source slice is implemented. Domain action catalog is closed and validated; each registered operation has canonical capability, minimum risk, argument/result schema, resource/effect/cancellation/reconciliation/idempotency and binding version metadata. DaemonHost registers handlers through the existing Broker and seals the catalog before requests; Broker revalidates catalog, normalized action and permit before dispatch, rejects duplicate/unknown/kind/version drift and never falls back to shell. Runner exposes only the five model tools; operator-only/direct Company/context operations remain explicit server commands.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; catalog/binding is process-local static source without durable ToolSnapshot/signature/provenance or dynamic revoke, PreparedAction is not Grant/Approval/Permit/ExecutionScope, bounded schema and compatibility fields remain partial, handler TOCTOU/egress/stop/reconcile and provider/connector/external/live/physical effects remain later CAP/CP/ER/PD/SC work
reviewer: Codex root implementation review plus CAP-01 catalog/registry/model-surface/binding source-boundary review; no runtime test reviewer
```

### CAP-02 capability input boundary and digest evidence (2026-09-16)

```text
source_snapshot: 7d269b7; kiana-domain/src/{tool_catalog,actions,capabilities}.rs; kiana-core/src/capabilities.rs; kiana-capability-broker/src/lib.rs; kiana-runner/src/tools.rs; kiana-daemon/src/harness_capabilities.rs; kiana-domain/tests/cap02_input.rs; kiana-core/tests/cap02_input_guard.rs; .github/workflows/cap02-input.yml; docs/roadmap/capability-input-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/capability.md; docs/roadmap.md
worktree_status: CAP-02 bounded JSON/schema/duplicate/depth/bytes/items validation, shell/argv/patch/MCP/Memory normalization, reserved authority cleanup boundary, canonical input digest, PreparedAction input/action/catalog digest validation, remote fixtures/source guard and roadmap overlays are scoped to this step; no network schema fetch or second Broker path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/tool_catalog.rs kiana-domain/src/actions.rs kiana-domain/src/capabilities.rs kiana-core/src/capabilities.rs kiana-capability-broker/src/lib.rs kiana-runner/src/tools.rs kiana-daemon/src/harness_capabilities.rs kiana-domain/tests/cap02_input.rs kiana-core/tests/cap02_input_guard.rs .github/workflows/cap02-input.yml docs/roadmap/capability-input-baseline.md
  rg -n 'TOOL_JSON_MAX_BYTES|TOOL_JSON_MAX_DEPTH|TOOL_JSON_MAX_ITEMS|parse_bounded_json|json_duplicate_key|validate_schema_contract|canonical_action_input_digest|PreparedAction|action_tool_alias_conflict|action_command_required|action_numeric_argument_invalid|stamp_request_identity|capability_action_not_prepared|command_argv' kiana-domain/src kiana-core/src kiana-capability-broker/src kiana-runner/src kiana-daemon/src kiana-domain/tests/cap02_input.rs kiana-core/tests/cap02_input_guard.rs docs/roadmap/capability-input-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cap02_input.rs equivalent/changed digest, duplicate key, schema reference/depth, alias conflict and reserved field fixtures; kiana-core/tests/cap02_input_guard.rs source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CAP-02 job is queued by the push and is not awaited
status_change: CAP-02 source slice is implemented. Bounded parser/schema helpers reject duplicate keys, unsupported schema references/keywords, excessive depth/bytes/items and invalid numeric values. Runner and daemon normalize shell string/argv, patch paths, MCP aliases/arguments and Memory fields; ControlPlane removes caller authority fields, stamps server context, applies defaults and hooks, then creates PreparedAction with canonical input/action/catalog digests. Equivalent normalized inputs share input digest, execution-affecting changes do not, and Broker rejects unprepared/drifted actions before handler dispatch.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; validator supports only the bounded local schema dialect, reference/outputSchema/secret/TOCTOU/egress/OS/process boundaries remain partial, compatibility fields may remain permissive until versioned migration, PreparedAction/input digest are not Grant/Approval/Permit/ExecutionScope, and handler/provider/connector/external/live/physical effects remain unproven
reviewer: Codex root implementation review plus CAP-02 input/schema/argv/alias/digest source-boundary review; no runtime test reviewer
```

### CAP-03 immutable ExecutionScope evidence (2026-09-16)

```text
source_snapshot: 8996d12; kiana-domain/src/{execution_scope,capabilities,contracts,lib}.rs; kiana-core/src/{capabilities,invocation_projection,events}.rs; kiana-capability-broker/src/lib.rs; kiana-domain/src/scope.rs; kiana-domain/tests/cap03_execution_scope.rs; kiana-core/tests/cap03_execution_scope_guard.rs; .github/workflows/cap03-execution-scope.yml; docs/roadmap/capability-execution-scope-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/capability.md; docs/roadmap.md
worktree_status: CAP-03 server-derived ExecutionScope typed contract, CapabilityRequest binding, ControlPlane action/context/identity/resource/epoch/digest derivation, Broker pre-handler scope verification, direct-vs-Harness Run semantics, remote fixtures/source guard and roadmap overlays are scoped to this step; caller/model scope is cleared before derivation and no second authorization path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/execution_scope.rs kiana-domain/src/capabilities.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/capabilities.rs kiana-core/src/invocation_projection.rs kiana-core/src/events.rs kiana-capability-broker/src/lib.rs kiana-domain/src/scope.rs kiana-domain/tests/cap03_execution_scope.rs kiana-core/tests/cap03_execution_scope_guard.rs .github/workflows/cap03-execution-scope.yml docs/roadmap/capability-execution-scope-baseline.md
  rg -n 'ExecutionScope|EXECUTION_SCOPE_SCHEMA|permission_scope|authority_epoch|trust_revision|data_epoch|cancellation_epoch|deadline_unix_ms|fencing_token|build_execution_scope|execution_scope_required|validate_for_request|execution_scope_digest_mismatch' kiana-domain/src kiana-core/src kiana-capability-broker/src kiana-domain/tests/cap03_execution_scope.rs kiana-core/tests/cap03_execution_scope_guard.rs docs/roadmap/capability-execution-scope-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cap03_execution_scope.rs typed scope digest/subset/empty/caller mismatch fixtures; kiana-core/tests/cap03_execution_scope_guard.rs source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CAP-03 job is queued by the push and is not awaited
status_change: CAP-03 source slice is implemented. `ExecutionScope` now binds server principal/project/session, optional Run/Turn/Cell, Grant/Budget refs, roots/denies, Memory/server/network dimensions, environment/workspace revision, authority/trust/data/cancel epochs, deadline/fencing and catalog/action/scope digests. ControlPlane removes caller-provided scope, derives and attaches it after final action/context normalization, while direct commands keep explicit scope without fabricated Run/Turn. Broker requires and validates the scope against the normalized request before permit/handler execution; empty/cross-scope/digest/resource/epoch drift fails closed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; epoch/fence/environment values are local controlled snapshots pending durable authority/Grant/Approval/lease ledgers, ScopeSet does not prove OS path/egress/TOCTOU or SecretStore, ExecutionScope is not a permit, legacy requests without scope remain query-compatible, and external/provider/connector/live/physical effects or cross-process recovery are not proven
reviewer: Codex root implementation review plus CAP-03 ExecutionScope derivation/resource/epoch/Broker-boundary review; no runtime test reviewer
```

### CAP-04 typed capability state and outcome evidence (2026-09-16)

```text
source_snapshot: 53d357a; kiana-domain/src/{states,capabilities,actions,contracts}.rs; kiana-core/src/{invocation_projection,capability_attempt_projection,events,lifecycle,metrics,platform,span_projection,company_business,data_governance}.rs; kiana-daemon/src/harness_capabilities.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/cap04_state.rs; kiana-core/tests/cap04_state.rs; kiana-core/tests/cap04_state_guard.rs; kiana-protocol/tests/cap04_mapping.rs; .github/workflows/cap04-state.yml; docs/roadmap/capability-state-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/capability.md; docs/roadmap.md
worktree_status: CAP-04 typed CapabilityExecutionState transitions, CapabilityResultDimensions process/stop/effect evidence, stable error-code classification, non-zero shell exit normalization, terminal/Unknown/foreign-attempt projection guards, remote fixtures/source guard and roadmap/status overlays are scoped to this step; no second execution loop or EventLog was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/states.rs kiana-domain/src/capabilities.rs kiana-domain/src/actions.rs kiana-domain/src/contracts.rs kiana-core/src/invocation_projection.rs kiana-core/src/capability_attempt_projection.rs kiana-core/src/events.rs kiana-core/src/lifecycle.rs kiana-core/src/metrics.rs kiana-core/src/platform.rs kiana-core/src/span_projection.rs kiana-core/src/company_business.rs kiana-core/src/data_governance.rs kiana-daemon/src/harness_capabilities.rs kiana-protocol/src/lib.rs kiana-domain/tests/cap04_state.rs kiana-core/tests/cap04_state.rs kiana-core/tests/cap04_state_guard.rs kiana-protocol/tests/cap04_mapping.rs .github/workflows/cap04-state.yml docs/roadmap/capability-state-baseline.md
  rg -n 'CapabilityExecutionState|can_transition_via|CapabilityResultDimensions|CapabilityProcessState|ForeignAttemptResult|terminal_execution_cannot_transition_to_success_again|unknown_effect_cannot_be_projected_as_cancelled|foreign_attempt_result_is_rejected|nonzero_shell_exit_remains_a_structured_tool_result' kiana-domain/src kiana-core/src kiana-daemon/src kiana-domain/tests/cap04_state.rs kiana-core/tests/cap04_state.rs kiana-core/tests/cap04_state_guard.rs docs/roadmap/capability-state-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cap04_state.rs typed state/non-zero-exit fixtures; kiana-core/tests/cap04_state.rs terminal/Unknown/foreign-attempt/recovery projection fixtures; kiana-core/tests/cap04_state_guard.rs source boundary guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CAP-04 job is queued by the push and is not awaited
status_change: CAP-04 source slice is implemented. CapabilityExecutionState now has an explicit queued state and legal queued/authorized/dispatching cancellation, startup-failure and recovery-Unknown edges, with compressed event transitions centralized in the domain. CapabilityResultDimensions separates process/stop/effect/exit/failure code; normalization turns success=true plus non-zero exit or unknown effect into a structured failure. Invocation projection rejects terminal resurrection and Unknown→Cancelled, while capability attempt projection requires an existing request/attempt and rejects foreign turn/invocation/execution identities. Event/lifecycle classification uses CapabilityErrorCode rather than result_unknown substring matching.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; typed dimensions are bounded handler/event evidence, not a permit or external receipt; permit single-consumption, durable cross-process attempt ledger, OS/TOCTOU/egress, provider/connector/network/physical effects, reconciliation and business Outcome remain CAP-05+ and ER/PD/SC/INT work; legacy result payloads without typed fields remain conservative compatibility reads
reviewer: Codex root implementation review plus CAP-04 state-transition/outcome/error/projection source-boundary review; no runtime test reviewer
```

### H02 Harness identity and lifecycle evidence (2026-09-16)

```text
source_snapshot: 8d6f3a3; kiana-domain/src/{ids,contracts,execution_identity,event_contracts,model,observability}.rs; kiana-protocol/src/lib.rs; kiana-runner-protocol/src/lib.rs; kiana-runner/src/harness.rs; kiana-daemon/src/harness_skills.rs; kiana-core/src/{lifecycle,model_attempt_projection}.rs; kiana-domain/tests/h02_identity.rs; kiana-core/tests/h02_lifecycle.rs; kiana-runner-protocol/tests/h02_wire.rs; kiana-provider/tests/oa08_provider_telemetry.rs; .github/workflows/h02-lifecycle.yml; docs/roadmap/harness-identity-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/harness.md; docs/roadmap.md
worktree_status: H02 Step/ModelAttempt typed identities, optional Start TurnId wire field, native ControlPlane propagation, Harness ActiveRun/checkpoint/model facts, ModelAttemptRecord identity projection, legacy reader compatibility and focused remote fixtures/roadmap/status overlays are scoped to this step; no second runner loop or authority path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/execution_identity.rs kiana-domain/src/model.rs kiana-domain/src/observability.rs kiana-protocol/src/lib.rs kiana-runner-protocol/src/lib.rs kiana-runner/src/harness.rs kiana-daemon/src/harness_skills.rs kiana-core/src/lifecycle.rs kiana-core/src/model_attempt_projection.rs kiana-provider/tests/oa08_provider_telemetry.rs kiana-domain/tests/h02_identity.rs kiana-core/tests/h02_lifecycle.rs kiana-runner-protocol/tests/h02_wire.rs .github/workflows/h02-lifecycle.yml docs/roadmap/harness-identity-baseline.md
  rg -n 'StepId|ModelAttemptId|StepIdentity|ModelAttemptIdentity|run.model_turn|turn_id: Option<TurnId>|start_in_with_history_and_turn|native_continue_creates_new_run_while_legacy_contract_is_preserved|duplicate_run_terminal_is_rejected|late_result_cannot_complete_a_new_turn' kiana-domain/src kiana-protocol/src kiana-runner-protocol/src kiana-runner/src kiana-daemon/src kiana-core/src kiana-domain/tests/h02_identity.rs kiana-core/tests/h02_lifecycle.rs kiana-runner-protocol/tests/h02_wire.rs docs/roadmap/harness-identity-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/h02_identity.rs Step/ModelAttempt digest and tamper fixtures; kiana-core/tests/h02_lifecycle.rs Run terminal/cross-run projection and source guard fixtures; kiana-runner-protocol/tests/h02_wire.rs Start TurnId round-trip and legacy decode; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H02 job is queued by the push and is not awaited
status_change: H02 source slice is implemented. `StepId` and `ModelAttemptId` are registered UUID contracts with typed `StepIdentity` and `ModelAttemptIdentity` digests. Native ControlPlane Start/Continue now sends a server-owned TurnId through the optional runner Start field; the Harness retains turn identity across ActiveRun/checkpoint, creates one StepId per model step and one ModelAttemptId per provider attempt, and records both in bounded `run.model_turn` metadata. `ModelAttemptRecord` can project these IDs while legacy facts remain read-compatible. Closed-run/new-turn and cross-run late-result guards are covered by remote fixtures.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; Session-level durable input queue/driver lease, durable RunSnapshot/InvocationLedger, cross-process restore, full H03 state driver, H10 stable invocation identity, provider receipt and external/live/physical effects remain later work; legacy records without Step/ModelAttempt fields stay conservative query-only compatibility
reviewer: Codex root implementation review plus H02 Session/Run/Turn/Step/ModelAttempt identity and lifecycle source-boundary review; no runtime test reviewer
```

### H03 Harness state driver evidence (2026-09-16)

```text
source_snapshot: 86673f6; kiana-runner/src/{state_driver,lib,harness}.rs; kiana-runner/src/inbox.rs; kiana-runner/tests/h03_state_driver.rs; .github/workflows/h03-state-driver.yml; docs/roadmap/harness-state-driver-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/harness.md; docs/roadmap.md
worktree_status: H03 pure RunDriver/RunFrame/TurnFrame reducer, explicit phase/intents, driver ownership/mailbox bounds, non-recursive Harness step loop, ActiveRun/checkpoint/Inbox integration, remote fixtures and roadmap/status overlays are scoped to this step; no second model loop or authority path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-runner/src/state_driver.rs kiana-runner/src/lib.rs kiana-runner/src/harness.rs kiana-runner/src/inbox.rs kiana-runner/tests/h03_state_driver.rs .github/workflows/h03-state-driver.yml docs/roadmap/harness-state-driver-baseline.md
  rg -n 'RunDriver|RunFrame|TurnFrame|DriverInput|DriverIntent|model_step_once|second_driver_for_same_turn_is_rejected|full_mailbox_does_not_drop_accepted_input|RecoveryRequired|harness_driver_mailbox_full' kiana-runner/src kiana-runner/tests/h03_state_driver.rs docs/roadmap/harness-state-driver-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source fixture/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-runner/tests/h03_state_driver.rs pure reducer fixtures for second driver, full mailbox, isolated cancellation, stream/buffered equivalence and frame validation; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H03 job is queued by the push and is not awaited
status_change: H03 source slice is implemented. The new `RunDriver` is a pure reducer over versioned RunFrame/TurnFrame state and explicit DriverInput/DriverIntent values; it enforces one driver owner, bounded input admission, phase transitions and Unknown recovery without I/O. KianaHarness now uses an explicit loop over `model_step_once`, updates the same driver for step/model/tool/result/cancel boundaries, serializes it in checkpoints, and rejects unknown tool effect before continuing the model. Existing RunnerPort, ControlPlane and Broker paths remain unchanged.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; RunDriver/ActiveRun and Inbox remain process-local, Session durable queue/ACK and cross-process driver ownership/recovery are not proven, state reducer does not authorize effects, and structured Provider/stream/stop/retry contracts remain H04+ work
reviewer: Codex root implementation review plus H03 pure-state/reducer/ownership/mailbox source-boundary review; no runtime test reviewer
```

### H04 structured model content and provider conversion evidence (2026-09-16)

```text
source_snapshot: 6d493c4; kiana-domain/src/{model,contracts}.rs; kiana-provider/src/{request,response}.rs; kiana-provider/tests/h04_model_content_guard.rs; kiana-runner/src/model.rs; kiana-daemon/src/model_client.rs; kiana-domain/tests/h04_model_content.rs; .github/workflows/h04-model-content.yml; docs/roadmap/harness-model-content-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/harness.md; docs/roadmap.md
worktree_status: H04 typed ModelContent/ProviderContinuation, legacy text/tool_calls compatibility, structured history/orphan validation, provider route/opaque/unsupported pre-wire guard, model output compatibility updates, remote fixtures/source guard and roadmap/status overlays are scoped to this step; no second model loop, provider transport or authorization path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/model.rs kiana-domain/src/contracts.rs kiana-provider/src/request.rs kiana-provider/src/response.rs kiana-provider/tests/h04_model_content_guard.rs kiana-runner/src/model.rs kiana-daemon/src/model_client.rs kiana-domain/tests/h04_model_content.rs .github/workflows/h04-model-content.yml docs/roadmap/harness-model-content-baseline.md
  rg -n 'ModelContent|ProviderContinuation|content_blocks|normalize_structured_request|opaque_item_cannot_cross_provider|unsupported_content_block_fails_before_request|orphan_tool_result_is_rejected|legacy_cassette_and_typed_items_roundtrip' kiana-domain/src kiana-provider/src kiana-runner/src kiana-daemon/src kiana-domain/tests/h04_model_content.rs kiana-provider/tests/h04_model_content_guard.rs docs/roadmap/harness-model-content-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/h04_model_content.rs legacy/typed/opaque/attachment/orphan fixtures; kiana-provider/tests/h04_model_content_guard.rs route/unsupported/raw-response source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H04 job is queued by the push and is not awaited
status_change: H04 source slice is implemented. ModelMessage/ModelOutput retain legacy text/tool_calls fields while supporting versioned Text/ToolCall/ToolResult/AttachmentRef/ProviderOpaque content and reference-only ProviderContinuation. Structured history validation rejects orphan or cross-role tool results. Provider request compilation normalizes only safe text/tool/result blocks and rejects unsupported attachment wire mappings or provider/protocol/route-digest mismatches before any network request; raw response, headers and opaque bytes remain outside ordinary messages and telemetry.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; attachments/opaque/continuation are reference-only or reject-first without live multimodal/provider adapters, provider receipt, cross-connection migration, UI content rendering or external/live/physical effect evidence; legacy cassette compatibility is serde/source-checked only
reviewer: Codex root implementation review plus H04 structured-content/provider-route/redaction source-boundary review; no runtime test reviewer
```

### H05 model stop, error and retry evidence (2026-09-16)

```text
source_snapshot: dec35a1; kiana-domain/src/{model,contracts}.rs; kiana-runner/src/harness.rs; kiana-provider/src/response.rs; kiana-domain/tests/h05_model_outcome.rs; kiana-runner/tests/h05_stop_guard.rs; .github/workflows/h05-stop-retry.yml; docs/roadmap/harness-stop-retry-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/harness.md; docs/roadmap.md
worktree_status: H05 typed ModelStopReason/ModelOutcome/ModelError side-effect classification, legacy stop compatibility, Harness pre-dispatch/completion gate, provider parser boundary, remote fixtures and roadmap/status overlays are scoped to this step; no second model loop, retry authority or transport was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/model.rs kiana-domain/src/contracts.rs kiana-runner/src/harness.rs kiana-provider/src/response.rs kiana-domain/tests/h05_model_outcome.rs kiana-runner/tests/h05_stop_guard.rs .github/workflows/h05-stop-retry.yml docs/roadmap/harness-stop-retry-baseline.md
  rg -n 'ModelStopReason|ModelOutcome|side_effect_state|normalized_stop_reason|model_output_truncated|model_refused|model_transport_incomplete|ModelRetryClass::BeforeSend|ModelRetryClass::Rejected|harness_stop_and_retry_paths_are_typed_and_fail_closed' kiana-domain/src kiana-runner/src kiana-provider/src kiana-domain/tests/h05_model_outcome.rs kiana-runner/tests/h05_stop_guard.rs docs/roadmap/harness-stop-retry-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/h05_model_outcome.rs length/refusal/unknown/text/tool stop fixtures; kiana-runner/tests/h05_stop_guard.rs typed stop/retry source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H05 job is queued by the push and is not awaited
status_change: H05 source slice is implemented. Closed ModelStopReason and ModelOutcome contracts now separate end_turn/tool_use/length/refusal/pause/incomplete/unknown from provider strings; ModelError carries phase, retry class, request-sent and side-effect evidence with redacted safe detail. Legacy replies without stop metadata are explicitly normalized, while Harness rejects truncation/refusal/pause/incomplete/unknown before completion or tool dispatch. Provider response parsing and model-turn telemetry retain typed stop/retry/outcome evidence without persisting raw provider content.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; retry budgets/deadlines and provider-specific stream completeness remain bounded existing behavior, H06 stream accumulator/H07 budget/H08 quiet-I/O cancellation are not complete, model side-effect unknown does not prove capability effect, and no live provider/billing/external/physical outcome is claimed
reviewer: Codex root implementation review plus H05 stop/error/retry/complete-gate source-boundary review; no runtime test reviewer
```

### P4-J7-05 strict non-streaming provider response evidence (2026-09-16)

```text
source_snapshot: 6287b00; kiana-services/src/api/provider.rs; kiana-provider/src/response.rs; kiana-daemon/src/model_client.rs; kiana-services/src/api/provider.rs tests; .github/workflows/provider-strict-response.yml; docs/roadmap/provider-strict-response-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/provider.md; docs/roadmap.md
worktree_status: P4-J7-05 kiana-services OpenAI-compatible/Ollama strict non-stream response/request parsing, daemon legacy adapter identity/argument guards, malformed/missing/duplicate/empty-object/tool-result fixtures and roadmap/status overlays are scoped to this step; no new provider, transport, SDK tool loop or authorization path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-services/src/api/provider.rs kiana-provider/src/response.rs kiana-daemon/src/model_client.rs .github/workflows/provider-strict-response.yml docs/roadmap/provider-strict-response-baseline.md
  rg -n 'malformed_provider_tool_arguments_never_dispatch|missing_native_tool_identity_is_rejected|duplicate_tool_id_with_different_payload_is_rejected|valid_empty_object_arguments_are_preserved|provider_tool_result_round_trip_keeps_identity|provider_tool_arguments_invalid|missing_native_tool_identity|duplicate_tool_id' kiana-services/src/api/provider.rs kiana-provider/src/response.rs kiana-daemon/src/model_client.rs docs/roadmap/provider-strict-response-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; provider test targets compiled only; no test, network, provider, MCP or external-account command executed locally
fixture or cassette: kiana-services/src/api/provider.rs strict non-stream tests for malformed arguments, missing/duplicate IDs, valid empty object and tool-result identity; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-J7-05 job is queued by the push and is not awaited
status_change: P4-J7-05 source slice is implemented. OpenAI-compatible non-stream responses now require exactly one choice, object message, response ID, finish reason, native tool ID/name and JSON-object arguments; duplicate IDs and malformed responses fail before ModelToolCall creation. Ollama requires a terminal `done=true` response and object arguments, while preserving its explicit no-native-ID ordinal correlation rule. kiana-services request mappers and daemon legacy adapter no longer synthesize shell/tool/{} identities; tool results retain call IDs or fail closed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; kiana-services remains compatibility-only, ProviderGateway streaming/HTTP/auth/usage/receipt and cross-provider/live/physical effects are not proven, Ollama ordinal IDs are not stable Invocation identity, and strict parsing does not establish external side-effect correctness
reviewer: Codex root implementation review plus P4-J7-05 response identity/argument/duplicate/legacy adapter source-boundary review; no runtime test reviewer
```

### CM-01 shared source and scope evidence (2026-09-16)

```text
source_snapshot: d4df97d; kiana-domain/src/{context_scope,lib,contracts,memory,governance,prompts,identity,roles}.rs; kiana-domain/tests/cm01_sources.rs; .github/workflows/cm01-sources.yml; docs/roadmap/context-scope-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/context-memory.md; docs/roadmap.md
worktree_status: CM-01 SourceRef/SourceSnapshot/SourceKind/Freshness/EvidenceStatus and server-owned MemoryScope typed contracts, principal/project/session/collection/purpose binding, digest/cursor/unknown-field validation, remote fixtures and roadmap/status overlays are scoped to this step; no index/memory store/second scope authority or file I/O was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/context_scope.rs kiana-domain/src/lib.rs kiana-domain/src/contracts.rs kiana-domain/src/memory.rs kiana-domain/src/governance.rs kiana-domain/src/prompts.rs kiana-domain/src/identity.rs kiana-domain/src/roles.rs kiana-domain/tests/cm01_sources.rs .github/workflows/cm01-sources.yml docs/roadmap/context-scope-baseline.md
  rg -n 'SourceRef|SourceSnapshot|SourceKind|Freshness|EvidenceStatus|MemoryScope|source_refs_roundtrip_and_reject_missing_identity|scope_resolution_never_uses_model_principal|principal|project|session_id|scope_digest' kiana-domain/src kiana-domain/tests/cm01_sources.rs docs/roadmap/context-scope-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source fixture/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cm01_sources.rs SourceRef/Snapshot round-trip, missing identity/digest/cursor/unknown-field, server principal/project MemoryScope and forged model principal fixtures; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CM-01 job is queued by the push and is not awaited
status_change: CM-01 source slice is implemented. Domain now owns versioned SourceRef/SourceSnapshot values with source kind, locator, revision, content digest, cursor, freshness and evidence status, plus a MemoryScope bound to authenticated principal, project identity, session, normalized collections and Purpose. Scope and source validation is fail-closed for missing identity, digest/cursor/unknown-field tampering and Verified-without-source evidence; existing Memory/Prompt/query layers remain consumers and do not derive authority from model arguments or path strings.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; SourceRef does not open/verify file handles or inode freshness, MemoryScope is not yet derived from processing grants/retention/revocation or persisted across processes, ContextPlan/index generation/semantic recall and MemoryRecord lifecycle remain CM-02+ and CAP/PD/SC work, and no business/external outcome is claimed
reviewer: Codex root implementation review plus CM-01 source provenance/scope/principal-boundary review; no runtime test reviewer
```

### CM-02 MemoryRecord lifecycle and legacy import evidence (2026-09-16)

```text
source_snapshot: 817277a; kiana-domain/src/memory.rs; kiana-daemon/src/harness_memory.rs; kiana-domain/tests/cm02_memory.rs; .github/workflows/cm02-memory-lifecycle.yml; docs/roadmap/memory-lifecycle-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/context-memory.md; docs/roadmap.md
worktree_status: CM-02 MemoryRecord kind/purpose/sensitivity/validity/retention/dependencies/import_mode contract, admission/review/state/provenance validation, explicit legacy v1 import and daemon JSONL reader/writer/review wiring, remote fixtures and roadmap/status overlays are scoped to this step; no memory mutation transaction, second store or EventLog bypass was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/memory.rs kiana-daemon/src/harness_memory.rs kiana-domain/tests/cm02_memory.rs .github/workflows/cm02-memory-lifecycle.yml docs/roadmap/memory-lifecycle-baseline.md
  rg -n 'MemorySensitivity|MemoryValidity|MemoryImportMode|validate_lifecycle|legacy_import|legacy_memory_is_unverifiable_until_reviewed|invalid_admission_state_combination_is_denied|memory_admission_state_invalid|memory_active_qualification_incomplete' kiana-domain/src/memory.rs kiana-daemon/src/harness_memory.rs kiana-domain/tests/cm02_memory.rs docs/roadmap/memory-lifecycle-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source fixture/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cm02_memory.rs lifecycle/qualification/validity fixtures; kiana-daemon harness_memory unit fixture reads a v1 JSONL row through explicit legacy import; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CM-02 job is queued by the push and is not awaited
status_change: CM-02 source slice is implemented. MemoryRecord now separates kind, purpose, sensitivity, validity, retention, dependencies and import mode from origin/admission/state; invalid Candidate/Active, Qualified without review/evidence/purpose, rejected-state and validity/dependency combinations fail closed. The daemon reader explicitly transforms v1 rows into v2-compatible LegacyImport records with Unknown origin, Candidate/Draft admission, unverifiable provenance and no search visibility; native candidate/scratch/review writers carry the new metadata.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; lifecycle validation is not an atomic Memory mutation/CAS/index projection, processing grants/purpose/retention/revocation and cross-project user-private isolation remain CM-03+, legacy import does not rewrite source files or provide review evidence, and no semantic recall/business/external outcome is claimed
reviewer: Codex root implementation review plus CM-02 MemoryRecord lifecycle/provenance/legacy-reader source-boundary review; no runtime test reviewer
```

### CM-03 server-derived Memory scope evidence (2026-09-16)

```text
source_snapshot: 961965d; kiana-domain/src/context_scope.rs; kiana-core/src/capabilities.rs; kiana-daemon/src/harness_memory.rs; kiana-domain/tests/cm03_memory_scope.rs; kiana-core/tests/cm03_scope_guard.rs; .github/workflows/cm03-memory-scope.yml; docs/roadmap/memory-scope-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/context-memory.md; docs/roadmap.md
worktree_status: CM-03 MemoryScope intersection and server-derived read/write enforcement are scoped to this step; Core derives role-granted collections into ExecutionScope, daemon handlers reconstruct DomainMemoryScope from request scope, explicit collection/write checks fail closed, remote fixtures and roadmap/status overlays are included; no processing-grant store, second authorization path or direct storage bypass was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/context_scope.rs kiana-core/src/capabilities.rs kiana-daemon/src/harness_memory.rs kiana-domain/tests/cm03_memory_scope.rs kiana-core/tests/cm03_scope_guard.rs .github/workflows/cm03-memory-scope.yml docs/roadmap/memory-scope-baseline.md
  rg -n 'MemoryScope::intersect|from_execution_scope|memory_scope_collection_required|memory_scope_read_denied|memory_scope_write_denied|server_memory_scope|read_scope_is_intersection_of_all_grants|write_scope_cannot_be_widened_by_context_text' kiana-domain/src kiana-core/src kiana-daemon/src kiana-domain/tests/cm03_memory_scope.rs kiana-core/tests/cm03_scope_guard.rs docs/roadmap/memory-scope-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source fixture/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cm03_memory_scope.rs parent/child collection intersection, allow_write monotonicity and cross-session rejection; kiana-core/tests/cm03_scope_guard.rs server derivation/handler-boundary source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CM-03 job is queued by the push and is not awaited
status_change: CM-03 source slice is implemented. MemoryScope now intersects only scopes with identical server principal/project/session/purpose, chooses narrower covered collections, and ANDs write permission. ControlPlane rejects unknown/unauthorized memory collections, requires explicit write collections, and places role-granted read collections in ExecutionScope; daemon memory handlers require that scope and reject collection escapes before storage access. Model/context text cannot mint or widen principal, project, collection or write authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; ProcessingGrant/retention/revocation/epoch derivation and durable Memory scope/CAS/index generation remain CM-04/05/PD/SC, storage HOME/path is not an authorization source, collection-level intersection does not prove file handles/semantic recall, and no business/external outcome is claimed
reviewer: Codex root implementation review plus CM-03 MemoryScope intersection/Core derivation/daemon enforcement source-boundary review; no runtime test reviewer
```

### CM-04 unified Memory mutation and idempotency evidence (2026-09-16)

```text
source_snapshot: 0fe99c4; kiana-domain/src/memory_mutation.rs; kiana-domain/src/{lib,contracts}.rs; kiana-daemon/src/harness_memory.rs; kiana-domain/tests/cm04_memory_mutation.rs; kiana-daemon/tests/cm04_memory_mutation_guard.rs; .github/workflows/cm04-memory-mutation.yml; docs/roadmap/memory-mutation-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/context-memory.md; docs/roadmap.md
worktree_status: CM-04 server-owned MemoryMutation/Target/Receipt contracts cover ADD/UPDATE/DELETE/APPROVE/PUBLISH/EXPIRE/REVOKE with scope, actor, evidence, epochs, payload digest and idempotency; pure ledger preflights all targets and rejects stale/payload-drift requests; daemon memory.write/review construct the contract before JSONL append; remote fixtures and roadmap/status overlays are included; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/memory_mutation.rs kiana-domain/src/lib.rs kiana-domain/src/contracts.rs kiana-daemon/src/harness_memory.rs kiana-domain/tests/cm04_memory_mutation.rs kiana-daemon/tests/cm04_memory_mutation_guard.rs .github/workflows/cm04-memory-mutation.yml docs/roadmap/memory-mutation-baseline.md
  rg -n 'MemoryMutationOperation|MemoryMutationLedger|duplicate_memory_mutation_returns_original_receipt|stale_revision_never_last_write_wins|batch_preflight_rejects_one_stale_target_without_advancing_the_other|memory_handlers_use_server_mutation_contract' kiana-domain/src kiana-daemon/src kiana-domain/tests/cm04_memory_mutation.rs kiana-daemon/tests/cm04_memory_mutation_guard.rs docs/roadmap/memory-mutation-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cm04_memory_mutation.rs duplicate/replay, stale CAS and all-target preflight fixtures; kiana-daemon/tests/cm04_memory_mutation_guard.rs server mutation/append ordering source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CM-04 job is queued by the next push and is not awaited
status_change: CM-04 source slice is implemented. MemoryMutation validates the seven explicit verbs, server actor/scope identity, evidence SourceRefs, policy/data epochs, protected payload digest and idempotency key. MemoryMutationLedger returns the original receipt on an exact replay, rejects payload drift, and preflights every target before advancing revisions. memory.write uses a stable key-derived record identity and mutation receipt; memory.review maps promote/reject to APPROVE/REVOKE and preflights the exact record revision before append.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; daemon JSONL remains a projection until CM-05 binds Memory facts/body refs/review consumption/projection cursor to EventStore TransitionBatch; accept_proposal mixed batches, cross-process receipt/recovery, processing grants/retention/revocation/delete propagation, index generation and semantic recall remain CM-05+ / PD / SC; no business/external outcome is claimed
reviewer: Codex root implementation review plus CM-04 mutation/CAS/idempotency source-boundary review; no runtime test reviewer
```

### EXT-01 stable extension contracts evidence (2026-09-16)

```text
source_snapshot: 05c3b288 (EXT-00 baseline closure); kiana-domain/src/{ids,contracts,context_scope,extension_contracts,lib}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/ext01_contracts.rs; kiana-protocol/tests/ext01_protocol.rs; .github/workflows/ext01-extension-contracts.yml; docs/roadmap/extension-contracts-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/skills-plugins-hooks.md; docs/roadmap.md
worktree_status: EXT-01 stable ExtensionId/ComponentId/SnapshotId/HookRunId IDs, versioned Skill/Hook/Plugin/Snapshot/Error contracts, protocol re-export, remote fixtures and roadmap/status overlays are scoped to this step; descriptor validation is deny-first and no loader, hook process, capability grant or second execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/context_scope.rs kiana-domain/src/extension_contracts.rs kiana-domain/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/ext01_contracts.rs kiana-protocol/tests/ext01_protocol.rs .github/workflows/ext01-extension-contracts.yml docs/roadmap/extension-contracts-baseline.md
  rg -n 'ExtensionId|ComponentId|SnapshotId|HookRunId|SkillDescriptor|HookDecision|PluginLifecycle|ExtensionSnapshot|ExtensionError|extension_contracts_round_trip_and_reject_unknown_fields|extension_snapshot_digest_and_duplicate_identity_fail_closed|protocol_reexports_versioned_extension_contracts' kiana-domain/src kiana-protocol/src kiana-domain/tests/ext01_contracts.rs kiana-protocol/tests/ext01_protocol.rs docs/roadmap/extension-contracts-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/ext01_contracts.rs round-trip/unknown/digest/duplicate/error fixtures; kiana-protocol/tests/ext01_protocol.rs re-export fixture; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EXT-01 job is queued by the next push and is not awaited
status_change: EXT-01 source slice is implemented. Domain now owns stable extension/component/snapshot/hook-run IDs and closed contracts for skill descriptors, hook phases/decisions, plugin lifecycle, extension snapshots and bounded error codes. Source/content/trust/snapshot digests, generation/revision, duplicate identities and unknown fields fail closed; protocol exposes only serializable contract types and no execution authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; SourceResolver/ProjectTrust/path containment, strict SKILL/manifest parser, catalog generation/invalidation, signature/package verification, Hook process supervision, capability re-authorization and cross-process snapshot recovery remain EXT-02+; SourceRef locator still requires redacted projection before client display; no business/external outcome is claimed
reviewer: Codex root implementation review plus EXT-01 identity/schema/snapshot/error source-boundary review; no runtime test reviewer
```

### EXT-02 SourceResolver, ProjectTrust and path-root evidence (2026-09-16)

```text
source_snapshot: c1d59b7 (EXT-01 stable extension contracts); Cargo.lock; kiana-skills/Cargo.toml; kiana-skills/src/{source_resolver,loader,lib}.rs; kiana-domain/src/{extension_contracts,contracts}.rs; kiana-protocol/src/lib.rs; kiana-skills/tests/ext02_source_resolver.rs; .github/workflows/ext02-source-resolver.yml; docs/roadmap/source-resolver-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap/skills-plugins-hooks.md; docs/roadmap.md
worktree_status: EXT-02 SourceResolver unifies standard and explicit extension source roots with fixed precedence, digest-derived summaries and ProjectTrust decisions; loader reuses root/resource containment and rejects symlink resources; remote fixtures and roadmap/status overlays are included; no parser, package signature, hook process, capability grant or second execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-skills/src/source_resolver.rs kiana-skills/src/loader.rs kiana-skills/src/lib.rs kiana-skills/Cargo.toml Cargo.lock kiana-domain/src/extension_contracts.rs kiana-domain/src/contracts.rs kiana-protocol/src/lib.rs kiana-skills/tests/ext02_source_resolver.rs .github/workflows/ext02-source-resolver.yml docs/roadmap/source-resolver-baseline.md
  rg -n 'SourceResolver|SourceRootKind|SourceTrust|resolve_resource|source_root_symlink|source_resource_symlink|DuplicateRoot|untrusted_project_source_is_reported_and_not_usable|duplicate_roots_are_rejected_instead_of_silently_deduped|resource_resolution_rejects_escape_and_symlink' kiana-skills/src kiana-skills/tests docs/roadmap/source-resolver-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-skills/tests/ext02_source_resolver.rs trust filtering, summary path redaction, duplicate root, traversal/absolute/symlink resource fixtures; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EXT-02 job is queued by the next push and is not awaited
status_change: EXT-02 source slice is implemented. `SourceResolver` canonicalizes actual project roots before applying ProjectTrust, assigns deterministic source kind/precedence, rejects canonical duplicate roots and creates digest-derived summaries without absolute paths. Untrusted project sources remain visible only as denied decisions; trusted paths exclude them. `resolve_resource` rejects absolute/parent/control/backslash paths, symlink components and root escapes; skill loader now uses the same guard and does not follow symlinks.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; SourceResolver does not verify stored ProjectTrust integrity, package signatures/content, SKILL/manifest syntax, catalog generation/invalidation, Hook effects or cross-process snapshot recovery; loader still returns legacy Command and plugin discovery needs the same resolver-backed snapshot in EXT-03+; no business/external outcome is claimed
reviewer: Codex root implementation review plus EXT-02 trust/root/containment source-boundary review; no runtime test reviewer
```

### EXT-03 strict Skill/Plugin/Hook parser evidence (2026-09-16)

```text
source_snapshot: 6298111 (EXT-02 SourceResolver); kiana-skills/src/{types,loader,manifest,lib}.rs; kiana-domain/src/{tool_catalog,extension_contracts}.rs; kiana-skills/tests/ext03_manifest.rs; .github/workflows/ext03-strict-parsers.yml; docs/roadmap/strict-extension-parsers-baseline.md; docs/roadmap/skills-plugins-hooks.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: EXT-03 strict Skill frontmatter and Plugin/Hook manifest parser, duplicate-key detection, explicit legacy adapters, remote fixtures and roadmap/status overlays are scoped to this step; parsing remains inert metadata and does not execute entries or grant capabilities; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-skills/src/types.rs kiana-skills/src/loader.rs kiana-skills/src/manifest.rs kiana-skills/src/lib.rs kiana-domain/src/tool_catalog.rs kiana-domain/src/extension_contracts.rs kiana-skills/tests/ext03_manifest.rs .github/workflows/ext03-strict-parsers.yml docs/roadmap/strict-extension-parsers-baseline.md
  rg -n 'normalize_skill_name|deny_unknown_fields|deserialize_allowed_tools|parse_plugin_manifest|parse_hook_manifest|legacy_adapter|json_duplicate_key|skill_frontmatter_is_strict_and_names_are_normalized|strict_plugin_manifest_rejects_unknown_and_duplicate_components|legacy_plugin_and_hook_manifests_require_explicit_adapter_and_entry' kiana-skills/src kiana-domain/src kiana-skills/tests docs/roadmap/strict-extension-parsers-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-skills/tests/ext03_manifest.rs Skill frontmatter normalization/unknown/type limits, strict Plugin component duplicate/unknown, legacy adapter and Hook entry fixtures; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EXT-03 job is queued by the next push and is not awaited
status_change: EXT-03 source slice is implemented. Skill frontmatter now rejects unknown fields and malformed/oversized metadata, validates context/path/tools and normalizes directory slugs while retaining display labels. Plugin/Hook JSON uses bounded duplicate-key parsing, strict versioned fields, unique IDs, package-relative entries and guard/observer phase checks; legacy formats enter only through an explicit `legacy_adapter` branch. Parser output remains inert and cannot mint capability authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; parser does not verify filesystem entry existence, package signatures/content, ProjectTrust/snapshot generation, catalog precedence/invalidation, Hook process timeout/cancellation, dynamic activation or final ControlPlane/Broker re-authorization; legacy Command remains a compatibility DTO and no external/business outcome is claimed
reviewer: Codex root implementation review plus EXT-03 parser/compatibility/unknown-field source-boundary review; no runtime test reviewer
```

### EXT-04 deterministic extension catalog evidence (2026-09-16)

```text
source_snapshot: 86e93ef (EXT-03 strict parser); kiana-domain/src/{extension_catalog,contracts,lib}.rs; kiana-skills/src/{catalog,lib}.rs; kiana-domain/tests/ext04_catalog.rs; kiana-skills/tests/ext04_catalog.rs; .github/workflows/ext04-catalog.yml; docs/roadmap/catalog-baseline.md; docs/roadmap/skills-plugins-hooks.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: EXT-04 deterministic ExtensionCatalog winner/shadowed projection, Hook matcher ordering, legacy Command adapter, remote fixtures and roadmap/status overlays are scoped to this step; catalog remains inert metadata and does not grant activation/capability or execute hooks; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/extension_catalog.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-skills/src/catalog.rs kiana-skills/src/lib.rs kiana-domain/tests/ext04_catalog.rs kiana-skills/tests/ext04_catalog.rs .github/workflows/ext04-catalog.yml docs/roadmap/catalog-baseline.md
  rg -n 'ExtensionCatalog|CatalogCandidate|duplicate_identity_shadowed|HookOrderCandidate|order_hooks|build_skill_catalog|skill_name_collision_is_deterministic_and_audited|hook_matcher_order_is_replayable|command_catalog_selection_is_stable_and_reports_shadowed_candidates' kiana-domain/src kiana-skills/src kiana-domain/tests kiana-skills/tests docs/roadmap/catalog-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/ext04_catalog.rs deterministic winner/shadowed and Hook specificity/replay fixtures; kiana-skills/tests/ext04_catalog.rs Command adapter collision fixture; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EXT-04 job is queued by the next push and is not awaited
status_change: EXT-04 source slice is implemented. Domain catalog selection now sorts by kind/namespace/name/version/precedence/source/hash, selects one winner per identity, and preserves shadowed candidates with reason and digest. Hook candidates use a fixed precedence/specificity/event/matcher/id/source order and reject duplicate IDs. `kiana-skills` maps legacy Commands into this catalog before model-visible skill assembly, so input order and HashMap/filesystem iteration cannot silently choose a different skill.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; catalog generation is currently in-process and not persisted/CAS-invalidated, SourceResolver/ProjectTrust/signature and entry validity remain separate, Hook matcher execution/timeout/cancellation/recursion and final ControlPlane/Broker re-authorization remain EXT-05+; catalog/allowed-tools do not prove capability or business outcome
reviewer: Codex root implementation review plus EXT-04 ordering/duplicate/shadow diagnostics source-boundary review; no runtime test reviewer
```

### EXT-05 extension snapshot cache and invalidation evidence (2026-09-16)

```text
source_snapshot: ef10992 (EXT-04 deterministic catalog); kiana-skills/src/{snapshot,source_resolver,lib,dynamic}.rs; kiana-domain/src/{extension_contracts,contracts}.rs; kiana-skills/tests/ext05_snapshot.rs; .github/workflows/ext05-snapshot.yml; docs/roadmap/snapshot-invalidation-baseline.md; docs/roadmap/skills-plugins-hooks.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: EXT-05 SnapshotCacheKey/ExtensionSnapshotCache, resolver content fingerprint, dynamic/clear invalidation and skill registry key integration are scoped to this step; stale entries remain diagnostic-only and cache never grants capability; remote fixtures and roadmap/status overlays are included; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-skills/src/snapshot.rs kiana-skills/src/source_resolver.rs kiana-skills/src/lib.rs kiana-skills/src/dynamic.rs kiana-domain/src/extension_contracts.rs kiana-domain/src/contracts.rs kiana-skills/tests/ext05_snapshot.rs .github/workflows/ext05-snapshot.yml docs/roadmap/snapshot-invalidation-baseline.md
  rg -n 'SnapshotCacheKey|ExtensionSnapshotCache|invalidate_key|invalidate_all|snapshot_generation|invalidate_extension_snapshots|root_content_digest|source_root_fingerprint_changes_when_resource_content_changes|snapshot_cache_reuses_only_current_entries_and_invalidates_monotonically' kiana-skills/src kiana-skills/tests docs/roadmap/snapshot-invalidation-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; source guard/test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-skills/tests/ext05_snapshot.rs cache-key binding, monotonic invalidation, source-content fingerprint and global generation fixtures; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EXT-05 job is queued by the next push and is not awaited
status_change: EXT-05 source slice is implemented. SnapshotCacheKey includes cwd/trust/source roots/package registry/config/schema inputs; ExtensionSnapshotCache does not overwrite a current key, retains invalidated entries for diagnostics, and advances generation on invalidation/new snapshot. SourceResolver fingerprints bounded root contents so same-path content changes alter the root-set digest. Dynamic/conditional skill registration and clear_caches advance global generation, and legacy skill loading keys its projection by the resolver/config/plugin snapshot inputs.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; cache is process-local and has no durable manifest/CAS/restart recovery, package registry lifecycle is represented by digest compatibility, ProjectTrust/signature/parser/Hook activation and final capability re-authorization remain later EXT steps, invalidation cannot retract provider exposure or committed EventLog facts, and no business/external outcome is claimed
reviewer: Codex root implementation review plus EXT-05 cache-key/generation/invalidation source-boundary review; no runtime test reviewer
```

### UI-01 versioned protocol DTO evidence (2026-09-16)

```text
source_snapshot: ef10992 (EXT-05 snapshot cache); kiana-protocol/src/{ui_contracts,lib}.rs; kiana-domain/src/contracts.rs; kiana-protocol/tests/ui01_dto.rs; .github/workflows/ui01-protocol.yml; docs/roadmap/ui-protocol-baseline.md; docs/roadmap/ui-entrypoints.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: UI-01 versioned UiSnapshotV1/feed/action/result/capability/error/session/run/action-card/receipt/artifact/notice/limitation DTOs, cursor/retry/disposition validation, legacy wire compatibility, remote fixtures and roadmap/status overlays are scoped to this step; DTOs are projections/intents only and do not execute or authorize; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-protocol/src/ui_contracts.rs kiana-protocol/src/lib.rs kiana-domain/src/contracts.rs kiana-protocol/tests/ui01_dto.rs .github/workflows/ui01-protocol.yml docs/roadmap/ui-protocol-baseline.md
  rg -n 'UiSnapshotV1|UiFeedEnvelope|UiActionV1|UiActionResult|UiCapability|UiError|HumanActionCard|ReceiptRef|ArtifactSummary|UiNotice|EvidenceLimitation|ui_snapshot_feed_and_action_round_trip_with_unknown_field_guard|ui_action_result_keeps_unknown_and_rejected_distinct' kiana-protocol/src kiana-protocol/tests docs/roadmap/ui-protocol-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; protocol fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-protocol/tests/ui01_dto.rs snapshot/feed/action round-trip, unknown field, digest mismatch, rejected/unknown disposition fixtures; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions UI-01 job is queued by the next push and is not awaited
status_change: UI-01 source slice is implemented. Protocol now exposes versioned UiSnapshotV1, UiFeedEnvelope, UiActionV1/UiActionResult, UiCapability, UiError, Session/Run summaries, HumanActionCard, Receipt/Artifact refs, notices and evidence limitations. New DTOs fail closed on schema/instance/epoch/cursor/sequence/revision/size/digest/duplicate violations; Applied/Rejected/Unknown remain distinct with safe retry dispositions. Existing UiSnapshot/UiAction/RunStream envelopes remain compatible.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; no DaemonHost snapshot projector/feed hydration/gap recovery/instance transport/action journal or cross-surface capability handshake is implemented yet (UI-02+), payload validation does not authorize commands, and no business/external outcome is claimed
reviewer: Codex root implementation review plus UI-01 DTO/compatibility/digest source-boundary review; no runtime test reviewer
```

### UI-02 unified error/capability/surface handshake evidence (2026-09-16)

```text
source_snapshot: 21d2b04 (UI-01 versioned DTO); kiana-protocol/src/{ui_contracts,lib}.rs; kiana-client/src/lib.rs; kiana-domain/src/contracts.rs; kiana-protocol/tests/ui02_handshake.rs; kiana-client/tests/ui02_handshake.rs; .github/workflows/ui02-handshake.yml; docs/roadmap/ui-handshake-baseline.md; docs/roadmap/ui-entrypoints.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: UI-02 typed initialize/health handshake, principal×surface capability intersection, stable error/retry mapping, fixed user-safe messages, remote fixtures and roadmap/status overlays are scoped to this step; client remains transport-only and no daemon route/second authorization path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-protocol/src/ui_contracts.rs kiana-protocol/src/lib.rs kiana-client/src/lib.rs kiana-domain/src/contracts.rs kiana-protocol/tests/ui02_handshake.rs kiana-client/tests/ui02_handshake.rs .github/workflows/ui02-handshake.yml docs/roadmap/ui-handshake-baseline.md
  rg -n 'UiHandshakeRequest|UiHandshakeResponse|UiHealth|UiSurface|intersect_ui_capabilities|stable_error_from_response|ClientError::Protocol|client_initialize_and_health_return_typed_handshake_data|client_rejects_invalid_handshake_before_transport|handshake_capabilities_are_the_principal_surface_intersection' kiana-protocol/src kiana-client/src kiana-protocol/tests kiana-client/tests docs/roadmap/ui-handshake-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; protocol/client fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-protocol/tests/ui02_handshake.rs capability intersection, schema/unknown and stable error fixtures; kiana-client/tests/ui02_handshake.rs typed initialize/health and pre-transport invalid request fixtures; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions UI-02 job is queued by the next push and is not awaited
status_change: UI-02 source slice is implemented. Protocol defines versioned handshake/health DTOs and capability requests; principal and surface capability sets are intersected by ID/scope/enabled/action without widening. Existing capability error/status codes map to fixed UI error/retry dispositions, with Unknown forced to QueryOriginal and no raw internal detail copied. KianaClient now offers typed initialize/health facades that validate before transport and after response.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; daemon has no dedicated ui.initialize/ui.health route or durable instance/epoch handshake yet, capability intersection does not authorize commands, transport/auth/health redaction/projector/reconnect/action journal remain UI-03+, and no business/external outcome is claimed
reviewer: Codex root implementation review plus UI-02 handshake/capability/error source-boundary review; no runtime test reviewer
```

### UI-03 local instance identity/discovery/transport evidence (2026-09-16)

```text
source_snapshot: 8fe8338 (UI-02 handshake); kiana-protocol/src/ui_contracts.rs; kiana-daemon/src/{instance,lib}.rs; kiana-daemon/tests/ui03_instance.rs; .github/workflows/ui03-instance.yml; docs/roadmap/ui-instance-baseline.md; docs/roadmap/ui-entrypoints.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: UI-03 UiTransportKind/UiInstanceRecord, create-new single-instance lease, 0600 ready record/lock, bounded discovery and workspace/protocol/epoch peer checks are scoped to this step; record/lock are discovery metadata only and no socket listener/second runner/authorization path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-protocol/src/ui_contracts.rs kiana-daemon/src/instance.rs kiana-daemon/src/lib.rs kiana-daemon/tests/ui03_instance.rs .github/workflows/ui03-instance.yml docs/roadmap/ui-instance-baseline.md
  rg -n 'UiTransportKind|UiInstanceRecord|InstanceLease|acquire_instance|discover|validate_peer|ui_instance_already_running|ui_instance_lock_permissions_invalid|instance_lock_record_discovery_and_peer_checks_are_fail_closed|symlink_workspace_and_record_paths_are_rejected' kiana-protocol/src kiana-daemon/src kiana-daemon/tests docs/roadmap/ui-instance-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; daemon/protocol instance fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-daemon/tests/ui03_instance.rs single lease/discovery, duplicate acquire, peer mismatch, endpoint redaction and symlink workspace fixtures; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions UI-03 job is queued by the next push and is not awaited
status_change: UI-03 source slice is implemented. Protocol now carries a bounded UiInstanceRecord with instance ID, authority epoch, protocol/workspace/endpoint digests, PID, ready flag and integrity digest. Daemon InstanceLease creates a workspace-local `.kiana/instances/instance.lock` with create-new/0600 semantics and a single ready record; discover rejects missing/invalid lock, duplicate records, foreign workspace, protocol/epoch drift and symlink aliases. DaemonHost exposes an explicit acquire_instance API while commands remain on the existing ControlPlane spine.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; no real Unix socket/named pipe listener, OS peer credential/token authentication, PID start-time/restart epoch durable counter, crash recovery or readiness atomicity is proven; stale locks are not auto-taken over, and record/health does not imply EventStore/Runner/Provider availability or business outcome
reviewer: Codex root implementation review plus UI-03 instance/lock/discovery/peer source-boundary review; no runtime test reviewer
```

### CO-02 organization/project/workspace scope evidence (2026-09-16)

```text
source_snapshot: 9b75d21 (UI-03 instance); kiana-domain/src/{company_scope,ids,contracts,lib}.rs; kiana-core/src/{company_scope,company,lib}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/co02_scope.rs; .github/workflows/co02-company-scope.yml; docs/roadmap/company-scope-baseline.md; docs/roadmap/companyos.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: CO-02 typed Organization/Workspace/Project bindings, CompanyScopeRegistry resolve/legacy import guard, stable WorkspaceId/root digest, core Company workspace spelling guard, remote fixtures and roadmap/status overlays are scoped to this step; existing CompanyEvent stream and ControlPlane authority remain canonical; no second EventStore or execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/company_scope.rs kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/company_scope.rs kiana-core/src/company.rs kiana-core/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/co02_scope.rs .github/workflows/co02-company-scope.yml docs/roadmap/company-scope-baseline.md
  rg -n 'OrganizationBinding|WorkspaceBinding|ProjectBinding|CompanyScope|CompanyScopeRegistry|WorkspaceId|import_legacy_stream|company_scope_binding_mismatch|validate_workspace_root|two_business_projects_share_a_workspace_without_sharing_authority|company_scope_rejects_foreign_project_and_ambiguous_legacy_root' kiana-domain/src kiana-core/src kiana-domain/tests docs/roadmap/company-scope-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/core/daemon/protocol fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/co02_scope.rs shared workspace/two project isolation, foreign binding, legacy stream ambiguity, noncanonical root and digest tamper fixtures; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CO-02 job is queued by the next push and is not awaited
status_change: CO-02 source slice is implemented. Stable WorkspaceId is derived from an explicitly canonical absolute root while OrganizationId/ProjectId stay separate. CompanyScopeRegistry enforces organization membership and workspace/project ownership, permits two projects to share one workspace without sharing scope digest, and makes legacy actor+root import idempotent only for the same project; remapping is ambiguous and denied. Core Company context rejects relative/`..` workspace roots before existing EventStore command handling.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; typed bindings are an in-process snapshot and are not yet persisted/upcast into CompanyState/CompanyEvent, assignments/expiry/revocation/authority epochs and durable recovery remain CO-03+; root digest does not prove filesystem existence/inode freshness or ProjectTrust, and no business/external outcome is claimed
reviewer: Codex root implementation review plus CO-02 scope/binding/legacy migration source-boundary review; no runtime test reviewer
```

### CO-03 server-owned assignment evidence (2026-09-16)

```text
source_snapshot: 9b75d21 (UI-03 parent; CO-03 source files listed below); kiana-domain/src/{assignment,ids,contracts,lib}.rs; kiana-ports/src/lib.rs; kiana-core/src/{company,lib}.rs; kiana-core/tests/co03_assignment_guard.rs; kiana-daemon/src/lib.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/co03_assignment.rs; .github/workflows/co03-assignments.yml; docs/roadmap/assignment-baseline.md; docs/roadmap/companyos.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: CO-03 typed RoleAssignment/ProjectAssignment/ResolvedAssignment, validated AssignmentDirectoryPort, daemon server-principal/root-derived project resolution and core Company revalidation are scoped to this step; existing CompanyEvent/ControlPlane spine remains canonical; current assignment adapter is an explicit in-process snapshot and no second execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/assignment.rs kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-ports/src/lib.rs kiana-core/src/company.rs kiana-core/src/lib.rs kiana-core/tests/co03_assignment_guard.rs kiana-core/tests/co03_assignment.rs kiana-daemon/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/co03_assignment.rs .github/workflows/co03-assignments.yml docs/roadmap/assignment-baseline.md
  rg -n 'RoleAssignment|ProjectAssignment|ResolvedAssignment|AssignmentDirectory|AssignmentDirectoryPort|resolve_assignment_for_project|context_from_assignment|validate_company_assignment|assignment_expired_or_missing' kiana-domain/src kiana-ports/src kiana-core/src kiana-daemon/src kiana-domain/tests kiana-core/tests docs/roadmap/assignment-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/ports/core/daemon/protocol fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/co03_assignment.rs revoke/expiry/impersonation/reopen fixtures and kiana-core/tests/co03_assignment.rs plus co03_assignment_guard.rs core boundary checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CO-03 job is queued by the next push and is not awaited
status_change: CO-03 source slice is implemented. Role templates remain separate from server-owned assignments; project assignment bindings are bounded by organization/principal/project and role windows; revoke increments authority epoch/revision and cascades project revocation. Daemon resolves against its authenticated principal and filesystem-derived ProjectIdentity, rejects client actor/role impersonation, and core can revalidate Company context before write, Continue, approval consumption or effect dispatch.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; InMemoryAssignmentDirectory is not durable and assignment/membership/epoch facts are not yet persisted or upcast into CompanyEvent; existing legacy RequestContext/entrypoints still require later CO-05+ path-by-path integration and full capability/approval/effect revalidation
reviewer: Codex root implementation review plus CO-03 identity/expiry/revocation/impersonation source-boundary review; no runtime test reviewer
```

### CO-04 versioned role catalog evidence (2026-09-16)

```text
source_snapshot: a182830 (CO-03 parent; CO-04 source files listed below); kiana-domain/src/{roles,prompts,model,lib,contracts}.rs; kiana-domain/role-packs/{analyst,qa,librarian}.md; kiana-provider/src/config.rs; kiana-provider/tests/co04_role_model_routes.rs; kiana-core/src/{lifecycle,events,sessions,receipts}.rs; kiana-core/tests/co04_role_catalog_guard.rs; kiana-daemon/src/{lib,harness_skills}.rs; kiana-protocol/src/lib.rs; kiana-domain/src/tests.rs; kiana-domain/tests/co04_role_catalog.rs; .github/workflows/co04-role-catalog.yml; docs/roadmap/role-catalog-baseline.md; docs/roadmap/companyos.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: CO-04 versioned RoleSpec/DepartmentSpec and deterministic catalogs, Analyst/QA/Librarian role packs, PromptBundle/ModelAssignment/run/session/receipt provenance, and existing Harness/ProviderGateway path guards are scoped to this step; role packs do not grant capabilities and no second model loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/roles.rs kiana-domain/src/prompts.rs kiana-domain/src/model.rs kiana-domain/src/lib.rs kiana-domain/src/contracts.rs kiana-domain/role-packs/analyst.md kiana-domain/role-packs/qa.md kiana-domain/role-packs/librarian.md kiana-provider/src/config.rs kiana-provider/tests/co04_role_model_routes.rs kiana-core/src/lifecycle.rs kiana-core/src/events.rs kiana-core/src/sessions.rs kiana-core/src/receipts.rs kiana-core/tests/co04_role_catalog_guard.rs kiana-daemon/src/lib.rs kiana-daemon/src/harness_skills.rs kiana-protocol/src/lib.rs kiana-domain/src/tests.rs kiana-domain/tests/co04_role_catalog.rs .github/workflows/co04-role-catalog.yml docs/roadmap/role-catalog-baseline.md
  rg -n 'RoleCatalog|DepartmentCatalog|ROLE_ANALYST|ROLE_QA|ROLE_LIBRARIAN|input_schema|output_schema|role_prompt_hash|role_catalog_version|PromptBundle|ModelAssignment|model_profile_unknown|if project_trusted|role_model_profile_mismatch' kiana-domain/src kiana-provider/src kiana-core/src kiana-daemon/src kiana-protocol/src kiana-domain/tests kiana-core/tests docs/roadmap/role-catalog-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/provider/core/daemon/protocol fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/co04_role_catalog.rs catalog/prompt/role-pack fixtures, kiana-provider/tests/co04_role_model_routes.rs configured profile routing, and kiana-core/tests/co04_role_catalog_guard.rs existing Harness/Provider/ProjectTrust/provenance source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CO-04 job is queued by the next push and is not awaited
status_change: CO-04 source slice is implemented. Five departments now expose a versioned catalog with nine built-in roles, including Analyst, QA and Librarian. RoleSpec validation pins department, prompt hash, tool set, input/output schema and model profile; PromptBundle/ModelAssignment and run/session/receipt evidence carry catalog/role metadata; role lookup and explicit profile configuration fail closed without a PM/Builder fallback. Existing Harness and ProviderGateway remain the only model path.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; built-in catalogs are not durable/signed/hot-updatable, explicit multi-model request differences are not live-proven, and assignment/Company command permission revalidation remains a later CO-05+ integration concern
reviewer: Codex root implementation review plus CO-04 role/catalog/prompt/model provenance source-boundary review; no runtime test reviewer
```

### CO-05 Company command policy and human decision evidence (2026-09-16)

```text
source_snapshot: a7f4bbb (CO-04 parent; CO-05 source files listed below); kiana-domain/src/{company,company_policy,company_business,lib,contracts}.rs; kiana-core/src/{company,company_business}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/co05_policy.rs; kiana-core/tests/co05_policy_guard.rs; .github/workflows/co05-company-policy.yml; docs/roadmap/company-policy-baseline.md; docs/roadmap/companyos.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: CO-05 CompanyCommandPolicy, DecisionPurpose/ActorKind, HumanTask/HumanDecision and core admission/proof integration are scoped to this step; existing CompanyEvent/EventStore/Harness spine remains canonical, historical replay tolerates missing optional decision fields, and no second execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/company.rs kiana-domain/src/company_policy.rs kiana-domain/src/company_business.rs kiana-domain/src/lib.rs kiana-domain/src/contracts.rs kiana-core/src/company.rs kiana-core/src/company_business.rs kiana-protocol/src/lib.rs kiana-domain/tests/co05_policy.rs kiana-core/tests/co05_policy_guard.rs .github/workflows/co05-company-policy.yml docs/roadmap/company-policy-baseline.md
  rg -n 'CompanyCommandPolicy|DecisionPurpose|DecisionActorKind|HumanTask|HumanDecision|command_policy|authorize_context|human_decision|actor_is_human|company_human_decision_required' kiana-domain/src kiana-core/src kiana-protocol/src kiana-domain/tests kiana-core/tests docs/roadmap/company-policy-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/core/protocol fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/co05_policy.rs human/agent decision-purpose, option/target/scope/expiry and unknown-field fixtures; kiana-core/tests/co05_policy_guard.rs single ControlPlane policy/proof source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CO-05 job is queued by the next push and is not awaited
status_change: CO-05 source slice is implemented. Every current CompanyCommand now has an explicit policy view with allowed roles, actor kinds, assignment/evidence flags and DecisionPurpose; a future unlisted variant resolves to an empty deny matrix. Core checks that policy before loading CompanyState, rejects agent/SponsorProxy/Builder self-approval for human purposes, and records server-generated HumanDecision target revision/digest, scope, option, expiry and authority epoch inside CompanyProof. Existing ApprovalStore handles tool approvals separately and no tool approval is treated as a Charter/Acceptance/Delivery decision.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; HumanTask/inbox and decision facts are not independently durable, actor kind is derived from RequestContext cell/service markers, and full object-state/assignment/artifact/evidence/receipt policy remains CO-06+
reviewer: Codex root implementation review plus CO-05 command-policy/human-decision source-boundary review; no runtime test reviewer
```

### CO-06 immutable artifact/evidence evidence (2026-09-16)

```text
source_snapshot: 9946296 (CO-05 parent; CO-06 source files listed below); kiana-domain/src/{artifact_contracts,company,ids,contracts,lib}.rs; kiana-core/src/{artifacts,company}.rs; kiana-ports/src/lib.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/co06_artifact.rs; kiana-ports/tests/co06_artifact_port.rs; kiana-core/tests/co06_artifact_guard.rs; kiana-core/tests/oa27_company_governance.rs; .github/workflows/co06-artifact-evidence.yml; docs/roadmap/artifact-evidence-baseline.md; docs/roadmap/companyos.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: CO-06 typed ArtifactVersion/ArtifactRef/EvidenceRef/Criterion contracts, hash/scope/provenance validation, read-only versioned blob port, core confined artifact metadata path, and optional CompanyProof/CriteriaSnapshot typed references are scoped to this step; legacy text/event fields remain replay-compatible and no second EventStore/blob execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/artifact_contracts.rs kiana-domain/src/company.rs kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/artifacts.rs kiana-core/src/company.rs kiana-ports/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/co06_artifact.rs kiana-ports/tests/co06_artifact_port.rs kiana-core/tests/co06_artifact_guard.rs .github/workflows/co06-artifact-evidence.yml docs/roadmap/artifact-evidence-baseline.md
  rg -n 'ArtifactVersion|ArtifactRef|EvidenceRef|Criterion|ArtifactContentPort|artifact_version_from_content|validate_artifact_reference_content|typed_version|typed_evidence_refs|criterion_refs|artifact_blob_missing|artifact_content_hash_mismatch' kiana-domain/src kiana-core/src kiana-ports/src kiana-protocol/src kiana-domain/tests kiana-ports/tests kiana-core/tests docs/roadmap/artifact-evidence-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/ports/core/daemon/protocol fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/co06_artifact.rs immutable hash/provenance/criterion identity fixtures, kiana-ports/tests/co06_artifact_port.rs missing/blob/hash-drift fixtures, kiana-core/tests/co06_artifact_guard.rs source boundary checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CO-06 job is queued by the next push and is not awaited
status_change: CO-06 source slice is implemented. Typed ArtifactVersion/ArtifactRef/EvidenceRef/Criterion contracts bind stable IDs, versions, content hash, scope and producer provenance. ArtifactContentPort reads only persisted versioned blobs and rejects missing/hash drift; core keeps confined/O_NOFOLLOW/atomic workspace reads and emits typed metadata into CompanyProof when a legacy artifact ID is UUID-shaped. Optional typed fields let CriteriaSnapshot preserve distinct Criterion IDs without breaking historical text snapshots.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; in-memory blob storage is not durable and typed refs are not yet fully wired into Review/Acceptance/Delivery projectors, EventLog upcast, retention/deletion, CAS or cross-process recovery; current-workspace freshness versus historical blob needs CO-07+
reviewer: Codex root implementation review plus CO-06 artifact/hash/scope/provenance/path-boundary source review; no runtime test reviewer
```

### CO-07 versioned Company receipt evidence (2026-09-16)

```text
source_snapshot: 4f72c36 (CO-06 parent; CO-07 source files listed below); kiana-domain/src/{company_receipts,company,contracts,lib}.rs; kiana-core/src/{company,authority}.rs; kiana-ports/src/lib.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/co07_receipt.rs; kiana-core/tests/co07_receipt_guard.rs; .github/workflows/co07-company-receipts.yml; docs/roadmap/company-receipts-baseline.md; docs/roadmap/companyos.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: CO-07 typed CompanyCommandReceipt/DispatchIntent, deterministic logical command/payload/authority identity, Company replay receipt responses, and prepared effect handoff are scoped to this step; existing protected EventStore TransitionBatch/read_command CAS remains canonical and no second dispatcher/fact source was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/company_receipts.rs kiana-domain/src/company.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/company.rs kiana-core/src/authority.rs kiana-ports/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/co07_receipt.rs kiana-core/tests/co07_receipt_guard.rs .github/workflows/co07-company-receipts.yml docs/roadmap/company-receipts-baseline.md
  rg -n 'CompanyCommandReceipt|DispatchIntent|CompanyReceiptStatus|company_dispatch_kind|protected.command|commit_transition|read_command|replayed|ResultUnknown' kiana-domain/src kiana-core/src kiana-ports/src kiana-protocol/src kiana-domain/tests kiana-core/tests docs/roadmap/company-receipts-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/ports/core/daemon/protocol fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/co07_receipt.rs stable key/payload drift/intent/unknown fixtures and kiana-core/tests/co07_receipt_guard.rs existing EventStore CAS/source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CO-07 job is queued by the next push and is not awaited
status_change: CO-07 source slice is implemented. CompanyCommandReceipt now binds deterministic logical command ID, payload digest, authority digest, expected/committed revisions, event ID and replay/unknown status. Company core preserves old event/state fields while returning typed receipts for new/replayed commands; StartRun and selected delivery/cancel paths carry prepared DispatchIntent in CompanyProof, and commit still uses the existing protected EventStore CAS/read_command boundary.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; typed receipts are not yet a standalone durable query API, DispatchIntent consumption/unknown recovery is not fully projected, and historical Company state migration/upcast remains CO-08+
reviewer: Codex root implementation review plus CO-07 receipt/idempotency/dispatch-intent source-boundary review; no runtime test reviewer
```

### CO-08 Company replay and migration evidence (2026-09-16)

```text
source_snapshot: bfd7084 (CO-07 parent; CO-08 source files listed below); kiana-domain/src/{company_replay,company,lib,contracts}.rs; kiana-core/src/company.rs; kiana-domain/tests/co08_replay.rs; kiana-core/tests/co08_replay_guard.rs; .github/workflows/co08-company-replay.yml; docs/roadmap/company-replay-baseline.md; docs/roadmap/companyos.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: CO-08 CompanyReplayReducer, explicit legacy v0→v1 schema adapter, stream/version/idempotency/root/owner/kind checks and core load_company wiring are scoped to this step; reducer only applies pure CompanyState transitions and never executes effects; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/company_replay.rs kiana-domain/src/company.rs kiana-domain/src/lib.rs kiana-domain/src/contracts.rs kiana-core/src/company.rs kiana-domain/tests/co08_replay.rs kiana-core/tests/co08_replay_guard.rs .github/workflows/co08-company-replay.yml docs/roadmap/company-replay-baseline.md
  rg -n 'CompanyReplayReducer|company_replay_gap|company_replay_duplicate_command|company_event_schema_unsupported|LEGACY_COMPANY_EVENT_SCHEMA|migrate_company_event|CompanyState::transition' kiana-domain/src kiana-core/src kiana-domain/tests kiana-core/tests docs/roadmap/company-replay-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/core/daemon fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/co08_replay.rs identical v1 rebuild, gap/duplicate/unknown major and v0 migration fixtures; kiana-core/tests/co08_replay_guard.rs reducer wiring source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CO-08 job is queued by the next push and is not awaited
status_change: CO-08 source slice is implemented. Core Company loading now delegates to a deterministic reducer that rejects aggregate/root/owner/kind/idempotency mismatch, stream gaps/regression, duplicate logical commands and unknown schema major before state changes. A single explicit v0→v1 adapter preserves parseable legacy facts; replay applies pure CompanyState transitions and does not invoke Runner, Provider or capability effects.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; reducer has no independent durable snapshot/cursor migration journal, v0 adapter only changes the known schema label, and full business/typed-ref upcast remains later CO/PD/ER work
reviewer: Codex root implementation review plus CO-08 replay/gap/migration source-boundary review; no runtime test reviewer
```

### P0-A-01b schema registry evidence (2026-09-16)

```text
source_snapshot: 4d48220 (CO-08 parent; P0-A-01b registry files listed below); kiana-domain/src/{contracts,event_contracts}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/p0_a01b_schema.rs; kiana-core/tests/p0_a01b_schema_guard.rs; .github/workflows/p0-a01b-schema.yml; docs/roadmap/schema-registry-baseline.md; docs/roadmap.md
worktree_status: P0-A-01b verifies the existing single SCHEMA_CONTRACTS/SchemaLayer/SchemaVersion and EVENT_KIND_SPECS/event_migration boundary; no second registry or execution path was introduced; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/contracts.rs kiana-domain/src/event_contracts.rs kiana-protocol/src/lib.rs kiana-domain/tests/p0_a01b_schema.rs kiana-core/tests/p0_a01b_schema_guard.rs .github/workflows/p0-a01b-schema.yml docs/roadmap/schema-registry-baseline.md
  rg -n 'SCHEMA_CONTRACTS|SchemaLayer|check_schema_compatibility|EVENT_KIND_SPECS|event_kind_is_required|event_migration|unknown_required_event_kind' kiana-domain/src kiana-protocol/src kiana-domain/tests kiana-core/tests docs/roadmap/schema-registry-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/protocol/core fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/p0_a01b_schema.rs layer/unknown-major/minor/event migration fixtures and kiana-core/tests/p0_a01b_schema_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-A-01b job is queued by the next push and is not awaited
status_change: P0-A-01b source slice is implemented. The existing registry differentiates wire/domain/runtime layers and owner/compatibility/unknown-field policy; unknown schema/major and required unknown event kinds fail closed, while only same-major compatibility and registered migrations are accepted. Dedicated CI evidence now covers the existing implementation.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; registry coverage does not prove every legacy payload has an upcaster or that durable migration/projector retention is complete; those remain ER/PD/DEP scope
reviewer: Codex root implementation review plus P0-A-01b schema/event registry source-boundary review; no runtime test reviewer
```

### P0-A-02 stable error code evidence (2026-09-16)

```text
source_snapshot: 2e61ded (P0-A-01b parent; P0-A-02 source files listed below); kiana-domain/src/{errors,capabilities}.rs; kiana-protocol/src/lib.rs; kiana-entrypoints/src/{command_dispatch,web}.rs; kiana-domain/tests/p0_a02_error_codes.rs; kiana-core/tests/p0_a02_error_codes_guard.rs; .github/workflows/p0-a02-error-codes.yml; docs/roadmap/error-codes-baseline.md; docs/roadmap.md
worktree_status: P0-A-02 verifies the single append-only CapabilityErrorCode/CapabilityErrorPolicy and failure_code/failure_policy mapping used by domain, protocol, CLI and HTTP; no error classification or retry bypass was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/errors.rs kiana-domain/src/capabilities.rs kiana-protocol/src/lib.rs kiana-entrypoints/src/command_dispatch.rs kiana-entrypoints/src/web.rs kiana-domain/tests/p0_a02_error_codes.rs kiana-core/tests/p0_a02_error_codes_guard.rs .github/workflows/p0-a02-error-codes.yml docs/roadmap/error-codes-baseline.md
  rg -n 'CapabilityErrorCode|CapabilityErrorPolicy|failure_code|failure_policy|PathEscape|ResultUnknown|requires_reconciliation|from_reason|status_name' kiana-domain/src kiana-protocol/src kiana-entrypoints/src kiana-domain/tests kiana-core/tests docs/roadmap/error-codes-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/protocol/core/entrypoint fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/p0_a02_error_codes.rs path_escape/result_unknown/unknown reason fixtures and kiana-core/tests/p0_a02_error_codes_guard.rs cross-surface source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-A-02 job is queued by the next push and is not awaited
status_change: P0-A-02 source slice is implemented. The existing append-only CapabilityErrorCode enum provides fixed CLI/HTTP/retry/new-authorization/compensation/reconciliation policy; CapabilityResult and ResponseEnvelope expose the same classification, path_escape maps to a stable deny code, ResultUnknown cannot auto-retry, and unknown diagnostics remain conservative. Dedicated CI evidence now covers the shared mapping.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; error classification does not prove real effect/reconciliation, and future code must append (not mutate) enum semantics and keep all entrypoint mappings synchronized
reviewer: Codex root implementation review plus P0-A-02 error-code/policy cross-surface source review; no runtime test reviewer
```

### P4-J7-06 provider-neutral model contract evidence (2026-09-16)

```text
source_snapshot: b58dee2 (P0-A-02 parent; P4-J7-06 source files listed below); kiana-domain/src/{model,contracts}.rs; kiana-ports/src/model.rs; kiana-runner/src/model.rs; kiana-provider/src/{lib,request,response}.rs; kiana-domain/tests/p4_j7_06_model_contract.rs; kiana-core/tests/p4_j7_06_model_contract_guard.rs; .github/workflows/p4-j7-06-model-contract.yml; docs/roadmap/provider-model-contract-baseline.md; docs/roadmap/provider.md; docs/roadmap.md
worktree_status: P4-J7-06 verifies the provider-neutral ModelContent/ModelCall/Attempt/Finish/Error/Usage and single ModelClient port already used by the existing Runner/ProviderGateway path; legacy-vs-typed content conflict now fails closed, and no second model loop/provider execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/model.rs kiana-domain/src/contracts.rs kiana-ports/src/model.rs kiana-runner/src/model.rs kiana-provider/src/lib.rs kiana-provider/src/request.rs kiana-provider/src/response.rs kiana-domain/tests/p4_j7_06_model_contract.rs kiana-core/tests/p4_j7_06_model_contract_guard.rs .github/workflows/p4-j7-06-model-contract.yml docs/roadmap/provider-model-contract-baseline.md
  rg -n 'ModelContent|ProviderContinuation|PreparedModelCall|ModelFinish|ModelError|ModelOutcome|model_content_legacy_conflict|trait ModelClient|complete_admitted|provider_requires_model_admission|opaque_item_cannot_cross_provider' kiana-domain/src kiana-ports/src kiana-runner/src kiana-provider/src kiana-domain/tests kiana-core/tests docs/roadmap/provider-model-contract-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/ports/runner/provider/core fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/p4_j7_06_model_contract.rs ordered block/legacy conflict/major/error fixtures and kiana-core/tests/p4_j7_06_model_contract_guard.rs provider-neutral source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-J7-06 job is queued by the next push and is not awaited
status_change: P4-J7-06 source slice is implemented. Typed provider-neutral model content and request/response contracts remain in domain, ModelClient is defined in ports and only re-exported by Runner, while ProviderGateway/codec consumes frozen PreparedModelCall and admitted budget permits. Legacy text/tool_calls plus typed content conflict is rejected; unknown major and unsupported/cross-provider content remain fail-closed; structured model error/outcome retains retry and side-effect classification.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; provider facade migration, full protocol request compilation, unique streaming accumulator, provider receipts and cross-provider live evidence remain P4-J7-07/12/14+
reviewer: Codex root implementation review plus P4-J7-06 model contract/port/provider boundary source review; no runtime test reviewer
```

### P4-J7-07 provider extraction evidence (2026-09-16)

```text
source_snapshot: 7753930 (P4-J7-06 parent; P4-J7-07 source files listed below); kiana-provider/Cargo.toml; kiana-provider/src/lib.rs; kiana-daemon/src/model_client.rs; kiana-runner/Cargo.toml; kiana-core/Cargo.toml; kiana-core/tests/p4_j7_07_extraction_guard.rs; .github/workflows/p4-j7-07-provider-extraction.yml; docs/roadmap/provider-extraction-baseline.md; docs/roadmap/provider.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: P4-J7-07 confirms independent kiana-provider Gateway/codec/transport and daemon production injection; kiana-services provider references remain inside cfg(test) legacy fixtures, Runner/core do not depend on provider implementation, and no second model loop was introduced; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-provider/Cargo.toml kiana-provider/src/lib.rs kiana-daemon/src/model_client.rs kiana-runner/Cargo.toml kiana-core/Cargo.toml kiana-core/tests/p4_j7_07_extraction_guard.rs .github/workflows/p4-j7-07-provider-extraction.yml docs/roadmap/provider-extraction-baseline.md
  rg -n 'ProviderGateway|impl ModelClient for ProviderGateway|kiana_provider::ProviderGateway|cfg\(test\)|legacy_fixtures|kiana_services|kiana-provider' kiana-provider kiana-daemon/src/model_client.rs kiana-runner/Cargo.toml kiana-core/Cargo.toml kiana-core/tests/p4_j7_07_extraction_guard.rs docs/roadmap/provider-extraction-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; provider/daemon/core extraction guard targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p4_j7_07_extraction_guard.rs dependency/source boundary checks; existing provider CI fixtures cover missing route/credential fail-closed; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-J7-07 job is queued by the next push and is not awaited
status_change: P4-J7-07 source slice is implemented. kiana-provider is an independent workspace crate with the only production ProviderGateway/ModelClient implementation; daemon production config uses it, Runner and core remain below the implementation boundary, and kiana-services provider code is confined to cfg(test) legacy fixtures. Extraction does not delete compatibility APIs or claim live model behavior.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; kiana-services public provider facade remains a compatibility surface pending per-protocol migration, and profile/credential/full codec/streaming/provider receipt/live work remains P4-J7-08/12/14+
reviewer: Codex root implementation review plus P4-J7-07 dependency/assembly source-boundary review; no runtime test reviewer
```

### P4-J7-08 provider configuration snapshot evidence (2026-09-16)

```text
source_snapshot: bd67223 (P4-J7-07 parent; P4-J7-08 source files listed below); kiana-domain/src/{provider_config,contracts,lib}.rs; kiana-provider/src/{config,lib}.rs; kiana-provider/tests/p4_j7_08_config.rs; kiana-daemon/src/model_client.rs; kiana-core/tests/p4_j7_08_config_guard.rs; kiana-protocol/src/lib.rs; .github/workflows/p4-j7-08-provider-config.yml; docs/roadmap/provider-config-baseline.md; docs/roadmap/provider.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: P4-J7-08 typed provider/profile configuration snapshots, secret-free credential references, route revision/catalog projection, daemon live/cassette selection guard and streaming validation are scoped to this step; active route remains frozen by PreparedModelCall and no second model path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/provider_config.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-provider/src/config.rs kiana-provider/src/lib.rs kiana-provider/tests/p4_j7_08_config.rs kiana-daemon/src/model_client.rs kiana-core/tests/p4_j7_08_config_guard.rs kiana-protocol/src/lib.rs .github/workflows/p4-j7-08-provider-config.yml docs/roadmap/provider-config-baseline.md
  rg -n 'ProviderConfigSnapshot|ProviderProfileSnapshot|credential_ref|configuration_snapshot|configuration_revision|model_profile_unknown|model_selection_conflict|cassette_required|model_streaming_policy_invalid|inherit_default' kiana-domain/src kiana-provider/src kiana-daemon/src kiana-protocol/src kiana-provider/tests kiana-core/tests docs/roadmap/provider-config-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/provider/daemon/core/protocol fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-provider/tests/p4_j7_08_config.rs snapshot/secret/route revision/unknown profile/invalid streaming fixtures and kiana-core/tests/p4_j7_08_config_guard.rs selection source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-J7-08 job is queued by the next push and is not awaited
status_change: P4-J7-08 source slice is implemented. ProviderConfigSnapshot/ProviderProfileSnapshot separate route fields and digest-only credential references; Gateway catalog exposes the snapshot while preserving legacy route fields. Provider profile maps reject unknown/inherit/key/capability/concurrency/streaming errors, and daemon explicitly rejects live+cassette conflicts or cassette mode without a script before selecting a client.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; snapshots and route revisions are process-local, there is no durable ConfigSnapshotStore/hot-update CAS/active-run restart epoch or SecretStore rotation, and live HTTP evidence remains P4-J7-09+
reviewer: Codex root implementation review plus P4-J7-08 config/profile/source/mode boundary review; no runtime test reviewer
```

### P4-J7-09 provider credentials and endpoint evidence (2026-09-16)

```text
source_snapshot: 6fb453e (P4-J7-08 parent; P4-J7-09 source files listed below); kiana-provider/src/{config,transport,lib}.rs; kiana-provider/tests/p4_j7_09_credentials.rs; kiana-daemon/src/model_client.rs; kiana-core/tests/p4_j7_09_credentials_guard.rs; .github/workflows/p4-j7-09-credentials.yml; docs/roadmap/provider-credentials-baseline.md; docs/roadmap/provider.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: P4-J7-09 credential/header/endpoint/TLS/loopback/redirect/proxy/concurrency/streaming guards and source fixtures are scoped to this step; raw secrets remain out of snapshots/diagnostics, project text cannot override provider endpoint, and no network/effect path was duplicated; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-provider/src/config.rs kiana-provider/src/transport.rs kiana-provider/src/lib.rs kiana-provider/tests/p4_j7_09_credentials.rs kiana-daemon/src/model_client.rs kiana-core/tests/p4_j7_09_credentials_guard.rs .github/workflows/p4-j7-09-credentials.yml docs/roadmap/provider-credentials-baseline.md
  rg -n 'model_credential_header_invalid|model_credential_unavailable|model_endpoint_credentials_or_query_denied|model_endpoint_requires_tls_or_loopback|HeaderValue::from_str|credential_revision|Policy::none|project_authority' kiana-provider/src kiana-daemon/src kiana-provider/tests kiana-core/tests docs/roadmap/provider-credentials-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; provider/daemon/core credential fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-provider/tests/p4_j7_09_credentials.rs invalid header/missing secret/endpoint and loopback fixtures; kiana-core/tests/p4_j7_09_credentials_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-J7-09 job is queued by the next push and is not awaited
status_change: P4-J7-09 source slice is implemented. Provider config rejects malformed/missing credentials, URL userinfo/query/fragment, non-TLS/non-loopback endpoints, invalid streaming/concurrency and cross-origin hazards; client redirects and ambient proxies are disabled, credential revisions are digest-only, and daemon/server authority remains the sole endpoint selection path.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; SecretStore/OAuth rotation/revocation/DNS rebinding and real TLS/live provider behavior remain later work, and endpoint validity does not grant model capability or role authority
reviewer: Codex root implementation review plus P4-J7-09 credential/endpoint/redirect source-boundary review; no runtime test reviewer
```

### P4-J7-10 model capability catalog evidence (2026-09-16)

```text
source_snapshot: af5f448 (P4-J7-09 parent; P4-J7-10 source files listed below); kiana-domain/src/{model_catalog,model,contracts,lib}.rs; kiana-provider/src/{lib,config,request}.rs; kiana-provider/tests/p4_j7_10_catalog.rs; kiana-core/tests/p4_j7_10_catalog_guard.rs; .github/workflows/p4-j7-10-model-catalog.yml; docs/roadmap/provider-catalog-baseline.md; docs/roadmap/provider.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: P4-J7-10 typed ModelCatalog/Entry, source/expiry/revision and Supported/Unsupported/Unknown capability projection, deterministic `(model_id,connection_id)` resolution and Gateway catalog wiring are scoped to this step; discovery/list never grants capability or mutates active route; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/model_catalog.rs kiana-domain/src/model.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-provider/src/lib.rs kiana-provider/src/config.rs kiana-provider/src/request.rs kiana-provider/tests/p4_j7_10_catalog.rs kiana-core/tests/p4_j7_10_catalog_guard.rs .github/workflows/p4-j7-10-model-catalog.yml docs/roadmap/provider-catalog-baseline.md
  rg -n 'ModelCatalog|ModelCatalogEntry|ModelCatalogSource|model_catalog_ambiguous|supports_tools|expires_at_unix_ms|capabilities.tools|configuration_revision' kiana-domain/src kiana-provider/src kiana-provider/tests kiana-core/tests docs/roadmap/provider-catalog-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/provider/core catalog fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-provider/tests/p4_j7_10_catalog.rs slash/same-name/connection ambiguity and Unknown capability fixtures; kiana-core/tests/p4_j7_10_catalog_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-J7-10 job is queued by the next push and is not awaited
status_change: P4-J7-10 source slice is implemented. ModelCatalog/Entry preserve full model IDs and connection identity, carry capability support state/source/expiry/revision, and resolve ambiguous names only with an explicit connection. Gateway exposes the configured catalog while request compilation and policy remain the capability authority; no discovery or model list can grant tools or alter a frozen route.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; Cache/LiveDiscovery refresh and expiry are not durable/signed, catalog declarations do not prove network/model support, and role/permit/codec/transport enforcement remains P4-J7-11+
reviewer: Codex root implementation review plus P4-J7-10 catalog/capability/discovery source-boundary review; no runtime test reviewer
```

### CI-05 durable authority ledger evidence (2026-09-16)

```text
source_snapshot: 5ed3f40 + CI-05 working-tree slice; kiana-domain/src/{authority,assignment,identity_contracts,ids,contracts,lib}.rs; kiana-core/src/authority.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/ci05_authority_ledger.rs; kiana-core/tests/ci05_authority_guard.rs; .github/workflows/ci05-authority-ledger.yml; docs/roadmap/authority-ledger-baseline.md; docs/roadmap.md
worktree_status: CI-05 PolicyProfile/DataBoundary/SharingGrant contracts, AuthorityLedger event reducer and core epoch integration are scoped to this step; no execution grant or second authority path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/authority.rs kiana-domain/src/assignment.rs kiana-domain/src/identity_contracts.rs kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/authority.rs kiana-protocol/src/lib.rs kiana-domain/tests/ci05_authority_ledger.rs kiana-core/tests/ci05_authority_guard.rs .github/workflows/ci05-authority-ledger.yml docs/roadmap/authority-ledger-baseline.md
  rg -n 'PolicyProfile|DataBoundary|SharingGrant|AuthorityLedger|authority_event_version_gap_or_regression|authority_event_kind_unknown|authority_epoch_rollback|sharing_operations|AuthorityLedger::rebuild|authority_ledger_invalid' kiana-domain/src kiana-core/src kiana-protocol/src kiana-domain/tests/ci05_authority_ledger.rs kiana-core/tests/ci05_authority_guard.rs docs/roadmap/authority-ledger-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; authority fixture/source guard compiled only; no test or smoke command executed locally
fixture or cassette: ci05_authority_ledger rebuilds membership/role/project/policy/boundary/share events, checks cross-project operation intersection, revoke, unknown kind and version gap; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CI-05 job is queued by the push and is not awaited
status_change: CI-05 source slice is implemented. AuthorityLedger now deterministically rebuilds committed authority facts and exposes epoch-fenced sharing scope; core authority epoch reads use the reducer, while RoleAssignment/ProjectAssignment and new PolicyProfile/DataBoundary/SharingGrant contracts reject stale, malformed or cross-boundary metadata.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; production authority stream does not yet persist every identity/policy/share fact, Membership/Grant/Approval/Cell full intersection, durable projector, cross-process revoke and external tenant/RBAC proof remain CI-06+ and CP/SC/PD work
reviewer: Codex root implementation review plus CI-05 authority ledger/epoch/scope source-boundary review; no runtime test reviewer
```

### SW-01 typed Swarm lineage evidence (2026-09-16)

```text
source_snapshot: 41175d8 + SW-01 working-tree slice; kiana-domain/src/{ids,contracts,swarm_identity,lib}.rs; kiana-protocol/src/lib.rs; kiana-ports/src/lib.rs; kiana-core/src/swarm.rs; kiana-domain/tests/sw01_lineage.rs; kiana-core/tests/sw01_lineage_guard.rs; .github/workflows/sw01-lineage.yml; docs/roadmap/swarm-lineage-baseline.md; docs/roadmap.md
worktree_status: SW-01 typed Swarm IDs, strict lineage schema/digest, cross-swarm and monotonic epoch/revision guards are scoped to this step; existing Swarm/Company EventLog path remains the only execution spine and no scheduler/child execution loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/swarm_identity.rs kiana-domain/src/lib.rs kiana-protocol/src/lib.rs kiana-ports/src/lib.rs kiana-core/src/swarm.rs kiana-domain/tests/sw01_lineage.rs kiana-core/tests/sw01_lineage_guard.rs .github/workflows/sw01-lineage.yml docs/roadmap/swarm-lineage-baseline.md docs/roadmap.md
  rg -n 'SwarmPlanId|PartitionId|ChildCellId|AttemptId|DispatchIntentId|QueueEntryId|MergeDecisionId|SwarmLineage|validate_for_swarm|validate_against|swarm_lineage_(id|swarm_mismatch|revision_regression|epoch_regression)|SWARM_LINEAGE_SCHEMA|SwarmLineagePort' kiana-domain/src kiana-protocol/src kiana-ports/src kiana-core/src kiana-domain/tests/sw01_lineage.rs kiana-core/tests/sw01_lineage_guard.rs docs/roadmap/swarm-lineage-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; SW-01 fixture/source guard targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-domain/tests/sw01_lineage.rs typed-ID round-trip, unknown-field/schema, nil-ID, cross-swarm, self-parent, zero epoch, monotonic revision/epoch and canonical digest fixtures; kiana-core/tests/sw01_lineage_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SW-01 job is queued by the next push and is not awaited
status_change: SW-01 source slice is implemented. Seven domain-owned UUID IDs and strict SwarmLineage now locate partition/child/attempt/dispatch/queue/merge facts across parent/root/workflow/correlation/causation, while nil IDs, unknown schema/fields, cross-swarm use, self-parent and non-monotonic epoch/revision are fail-closed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; lineage is not yet persisted on every Swarm Partition/Attempt/DispatchIntent/QueueEntry fact, and WorkGraph validation, scheduler capacity/fairness, child lifecycle, replay/recovery and effect-time fencing remain SW-02+
reviewer: Codex root implementation review plus SW-01 typed identity/lineage and single-execution-spine source-boundary review; no runtime test reviewer
```

### SW-02 typed Swarm WorkGraph evidence (2026-09-16)

```text
source_snapshot: f129e47 + SW-02 working-tree slice; kiana-domain/src/{packet_graph,swarm,swarm_graph,lib}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/sw02_work_graph.rs; kiana-core/tests/sw02_work_graph_guard.rs; .github/workflows/sw02-work-graph.yml; docs/roadmap/swarm-work-graph-baseline.md; docs/roadmap.md
worktree_status: SW-02 typed Partition/SwarmWorkGraph validator and deterministic ready/blocked/failed projection are scoped to this step; shared packet_graph topology is the only dependency implementation, and SwarmPlan typed graph validation is advisory to legacy migration while no queue/claim/scheduler/child execution loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/packet_graph.rs kiana-domain/src/swarm.rs kiana-domain/src/swarm_graph.rs kiana-domain/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/sw02_work_graph.rs kiana-core/tests/sw02_work_graph_guard.rs .github/workflows/sw02-work-graph.yml docs/roadmap/swarm-work-graph-baseline.md docs/roadmap.md
  rg -n 'Partition|SwarmWorkGraph|PartitionProjection|validate_dependency_graph|swarm_partition_overlap|swarm_partition_input_unbound|swarm_partition_dependency_(cycle|missing|duplicate)|swarm_duplicate_fingerprint|swarm_first_success_unsupported|swarm_(partition_count|depth|concurrency|spawn_rate|ttl|token|model_call)_|work_graph' kiana-domain/src kiana-protocol/src kiana-core/src kiana-domain/tests/sw02_work_graph.rs kiana-core/tests/sw02_work_graph_guard.rs docs/roadmap/swarm-work-graph-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; SW-02 fixture/source guard targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-domain/tests/sw02_work_graph.rs overlap/unbound input, cycle/missing/duplicate/fingerprint/first-success, count/depth/concurrency/spawn-rate/TTL/budget and stable projection fixtures; kiana-core/tests/sw02_work_graph_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SW-02 job is queued by the next push and is not awaited
status_change: SW-02 source slice is implemented. Strict Partition and SwarmWorkGraph contracts now canonicalize input/path/scope, bind digest/output/fingerprint/typed IDs, reuse shared packet graph topology, reject unsafe overlap/limits/strategies and expose deterministic readiness without minting execution authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; `SwarmPlan.work_graph` remains optional for legacy plans, and typed partitions are not yet durable queue/attempt/dispatch facts; claim fencing, scheduler fairness, child lifecycle, replay/recovery and effect-time checks remain SW-03+
reviewer: Codex root implementation review plus SW-02 WorkGraph/packet-graph reuse and single-execution-spine source-boundary review; no runtime test reviewer
```

### SW-03 typed Swarm status reducer evidence (2026-09-16)

```text
source_snapshot: d999a5e + SW-03 working-tree slice; kiana-domain/src/{swarm,swarm_graph,swarm_reducer,lib,contracts}.rs; kiana-protocol/src/lib.rs; kiana-core/src/swarm.rs; kiana-domain/tests/sw03_reducer.rs; kiana-core/tests/sw03_reducer_guard.rs; .github/workflows/sw03-reducer.yml; docs/roadmap/swarm-reducer-baseline.md; docs/roadmap.md
worktree_status: SW-03 strict SwarmTransitionEvent/entity and pure SwarmTransitionReducer live/replay contract are scoped to this step; legacy SwarmEvent accepts an optional typed transition batch and core load validates it, while existing commit_swarm/EventLog remains the only effect path; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/swarm.rs kiana-domain/src/swarm_graph.rs kiana-domain/src/swarm_reducer.rs kiana-domain/src/lib.rs kiana-domain/src/contracts.rs kiana-protocol/src/lib.rs kiana-core/src/swarm.rs kiana-domain/tests/sw03_reducer.rs kiana-core/tests/sw03_reducer_guard.rs .github/workflows/sw03-reducer.yml docs/roadmap/swarm-reducer-baseline.md docs/roadmap.md
  rg -n 'SwarmTransition(Entity|Event|Reducer)|AttemptStatus|SWARM_TRANSITION_EVENT_SCHEMA|delegation\.(swarm|partition|attempt)_state|swarm_transition_(illegal|epoch_regression|revision_gap_or_regression)|review_complete|validate_transitions' kiana-domain/src kiana-protocol/src kiana-core/src kiana-domain/tests/sw03_reducer.rs kiana-core/tests/sw03_reducer_guard.rs docs/roadmap/swarm-reducer-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; SW-03 reducer fixture/source guard targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-domain/tests/sw03_reducer.rs illegal/terminal/Unknown/merge-review, replay/live equivalence, revision gap, epoch regression and unknown-field fixtures; kiana-core/tests/sw03_reducer_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SW-03 job is queued by the next push and is not awaited
status_change: SW-03 source slice is implemented. Typed Swarm/Partition/Attempt transition facts now share one reducer for live/replay, enforce legal edges, single terminal state, Unknown quarantine, review-gated merge and monotonic revision/authority epoch; core validates optional batches on replay without adding execution authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; legacy SwarmState commands still emit empty typed batches, so full Partition/Attempt fact generation and durable transition projection remain SW-04+/ER/PD; reducer does not reserve resources or execute effects.
reviewer: Codex root implementation review plus SW-03 typed state/replay and single-execution-spine source-boundary review; no runtime test reviewer
```

### NM-01 notification/messaging contracts evidence (2026-09-17)

```text
source_snapshot: 12e15ae + NM-01 working-tree slice; kiana-domain/src/{ids,contracts,notifications,lib}.rs; kiana-protocol/src/lib.rs; kiana-ports/src/lib.rs; kiana-core/src/communication.rs; kiana-domain/tests/nm01_contracts.rs; kiana-core/tests/nm01_contracts_guard.rs; .github/workflows/nm01-contracts.yml; docs/roadmap/notifications-contracts-baseline.md; docs/roadmap.md
worktree_status: NM-01 six strict Message/Notification/Subscription/DeliveryAttempt/ActionRef/DeliveryReceipt contracts and stable IDs are scoped to this step; no NotificationStore, outbox, DeliveryWorker or second message bus was added, and action references remain non-authoritative until ControlPlane rechecks them; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/notifications.rs kiana-domain/src/lib.rs kiana-protocol/src/lib.rs kiana-ports/src/lib.rs kiana-core/src/communication.rs kiana-domain/tests/nm01_contracts.rs kiana-core/tests/nm01_contracts_guard.rs .github/workflows/nm01-contracts.yml docs/roadmap/notifications-contracts-baseline.md docs/roadmap.md
  rg -n 'Message(Id|Kind)?|Notification(Id|Channel|Status)?|Subscription(Id|Status)?|DeliveryAttempt(Id|Status)?|ActionRef(Id)?|DeliveryReceipt(Id|Status)?|upcast_message|canonical_notification_bytes|scope_exceeds_subscription|secret_detected|MESSAGE_SCHEMA|NOTIFICATION_SCHEMA' kiana-domain/src kiana-protocol/src kiana-core/src kiana-domain/tests/nm01_contracts.rs kiana-core/tests/nm01_contracts_guard.rs docs/roadmap/notifications-contracts-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; NM-01 fixture/source guard targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-domain/tests/nm01_contracts.rs strict round-trip, scope subset, status/TTL, bounded body/secret, v0 upcast/unknown major-field and canonical digest fixtures; kiana-core/tests/nm01_contracts_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions NM-01 job is queued by the next push and is not awaited
status_change: NM-01 source slice is implemented. Domain now owns versioned message/notification/subscription/delivery/action contracts with deny-unknown fields, stable IDs, bounded secret-safe values, scope intersection, monotonic status helpers, canonical digests and explicit compatibility upcast; no delivery side effect is implied.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; recipient resolution, committed-only materialization, durable subscriptions/read state, outbox/lease/fence, external channels, CAS and unmarked secret detection remain NM-02+ / ER / PD / SC work.
reviewer: Codex root implementation review plus NM-01 strict contract/scope/secret/upcast and no-second-bus source-boundary review; no runtime test reviewer
```

### NM-02 communication lifecycle evidence (2026-09-17)

```text
source_snapshot: db0d9e1 + NM-02 working-tree slice; kiana-domain/src/{communication,event_contracts,contracts,lib}.rs; kiana-core/src/{communication,commands,events}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/nm02_lifecycle.rs; kiana-core/tests/nm02_lifecycle_guard.rs; .github/workflows/nm02-lifecycle.yml; docs/roadmap/notifications-lifecycle-baseline.md; docs/roadmap.md
worktree_status: NM-02 CommunicationLifecycleEvent and server-routed communication.ack/reject/escalate facts are scoped to this step; Handoff ACK is recipient/reason/terminal fenced, Incident escalation carries evidence, message text never invokes Company/Capability execution, and EventLog communication aggregate is the only commit path; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/communication.rs kiana-domain/src/event_contracts.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/communication.rs kiana-core/src/commands.rs kiana-core/src/events.rs kiana-protocol/src/lib.rs kiana-domain/tests/nm02_lifecycle.rs kiana-core/tests/nm02_lifecycle_guard.rs .github/workflows/nm02-lifecycle.yml docs/roadmap/notifications-lifecycle-baseline.md docs/roadmap.md
  rg -n 'CommunicationLifecycle(Event|Status)|communication\.(ack|reject|escalate|handoff_acknowledged|handoff_rejected|incident_escalated)|load_communication|communication_sender_(mismatch|role_invalid)|communication_handoff_(ack_invalid|not_pending)|communication_incident_escalation|authority_granted|aggregate_for_event' kiana-domain/src kiana-core/src kiana-protocol/src kiana-domain/tests/nm02_lifecycle.rs kiana-core/tests/nm02_lifecycle_guard.rs docs/roadmap/notifications-lifecycle-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; NM-02 fixture/source guard targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-domain/tests/nm02_lifecycle.rs directed Handoff ACK/reject, terminal/authority denial, Incident escalation evidence, unknown-field fixtures; kiana-core/tests/nm02_lifecycle_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions NM-02 job is queued by the next push and is not awaited
status_change: NM-02 source slice is implemented. Seven communication kinds now share server-authored lifecycle facts; Handoff ACK/reject requires the committed Sent fact, exact recipient and reason, Incident escalation is evidence-bearing, actor/role/project trust is rechecked, and no message can grant authority or directly dispatch.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; old request-aggregate communication events are compatibility-only, ACK does not yet drive Company/Swarm dispatch, and notification materialization/subscription resolver/outbox/read-state/external delivery/cross-process recovery remain NM-03+ / ER / PD / SC.
reviewer: Codex root implementation review plus NM-02 lifecycle/recipient/authority/eventlog source-boundary review; no runtime test reviewer
```

### NM-03 notification event registry evidence (2026-09-17)

```text
source_snapshot: e0e0b02 + NM-03 working-tree slice; kiana-domain/src/{notification_events,event_contracts,contracts,lib}.rs; kiana-core/src/events.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/nm03_event_registry.rs; kiana-core/tests/nm03_event_registry_guard.rs; .github/workflows/nm03-event-registry.yml; docs/roadmap/notifications-event-registry-baseline.md; docs/roadmap.md
worktree_status: NM-03 server-owned notification event class/source registry and critical owner guard are scoped to this step; core append validates explicit source metadata before redaction/append while legacy events without source remain compatibility facts, and no projector/delivery/second execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/notification_events.rs kiana-domain/src/event_contracts.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/src/events.rs kiana-protocol/src/lib.rs kiana-domain/tests/nm03_event_registry.rs kiana-core/tests/nm03_event_registry_guard.rs .github/workflows/nm03-event-registry.yml docs/roadmap/notifications-event-registry-baseline.md docs/roadmap.md
  rg -n 'NOTIFICATION_EVENT_(SPECS|REGISTRY_SCHEMA)|NotificationEvent(Class|Source|Spec)|notification_event_(kind_unregistered|source_untrusted|owner_required|source_mismatch)|validate_notification_(event|runtime_event)|data.get\("source"\)' kiana-domain/src kiana-core/src kiana-protocol/src kiana-domain/tests/nm03_event_registry.rs kiana-core/tests/nm03_event_registry_guard.rs docs/roadmap/notifications-event-registry-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; NM-03 registry fixture/source guard targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-domain/tests/nm03_event_registry.rs mapping, unknown kind/source, model/UI self-report and ownerless critical fixtures; kiana-core/tests/nm03_event_registry_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions NM-03 job is queued by the next push and is not awaited
status_change: NM-03 source slice is implemented. Domain now classifies only registered committed notification events, rejects unknown required families, model/UI critical self-reports, source mismatch and ownerless critical facts; core explicit-source append boundary reuses this guard without changing legacy payload compatibility.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; legacy events lacking explicit source are not yet materialized, and committed-only projector/cursor/checkpoint, recipient/scope resolver, delivery/OCC/outbox/read state and cross-process recovery remain NM-04+ / ER / PD / SC.
reviewer: Codex root implementation review plus NM-03 event registry/source/owner and no-self-reported-critical-fact source-boundary review; no runtime test reviewer
```

### EQ-01 legacy evaluation contract evidence (2026-09-17)

```text
source_snapshot: 9815fdb + EQ-01 working-tree slice; kiana-commands/src/eval.rs; kiana-commands/tests/eq01_compatibility.rs; kiana-core/tests/eq01_compatibility_guard.rs; .github/workflows/eq01-compatibility.yml; docs/roadmap/evaluation-contract-baseline.md; docs/roadmap.md
worktree_status: legacy eval suite/report/baseline schema and limits are extracted into public constants used by the parser; JSON field/error/compatibility fixture inventories are explicit deletion fences, with no second evaluator or promotion authority added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-commands/src/eval.rs kiana-commands/tests/eq01_compatibility.rs kiana-core/tests/eq01_compatibility_guard.rs .github/workflows/eq01-compatibility.yml docs/roadmap/evaluation-contract-baseline.md docs/roadmap.md
  rg -n 'EVAL_(SUITE|REPORT|BASELINE)_SCHEMA|EVAL_MAX_(CASES|FIXTURE_BYTES|FIXTURE_LINES)|LEGACY_EVAL_(JSON_FIELDS|ERROR_CODES|JSON_COMPATIBILITY_TESTS)|serde\(deny_unknown_fields\)' kiana-commands/src/eval.rs kiana-commands/tests/eq01_compatibility.rs kiana-core/tests/eq01_compatibility_guard.rs docs/roadmap/evaluation-contract-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; EQ-01 command/core compatibility targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-commands/tests/eq01_compatibility.rs legacy schema/field/error/fixture inventory; kiana-core/tests/eq01_compatibility_guard.rs parser source fence; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EQ-01 job is queued by the next push and is not awaited
status_change: EQ-01 source slice is implemented. Legacy eval schema literals and limits now have one public parser-owned contract, while report/baseline JSON fields, stable finding/error codes and compatibility fixture names are explicitly recorded against untracked deletion or rename.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; legacy parser still uses caller-selected fixture paths and has no durable EvalStore, isolated runner, TraceNormalizer, quality gate, promote/rollback or real model-quality evidence; EQ-02+ remains open.
reviewer: Codex root implementation review plus EQ-01 legacy schema/field/error compatibility source-boundary review; no runtime test reviewer
```

### EQ-02 quality identity/lifecycle evidence (2026-09-17)

```text
source_snapshot: 7b4c85f + EQ-02 working-tree slice; kiana-domain/src/{ids,contracts,quality,lib}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/eq02_quality_contract.rs; kiana-core/tests/eq02_quality_guard.rs; .github/workflows/eq02-quality-contract.yml; docs/roadmap/evaluation-quality-contract-baseline.md; docs/roadmap.md
worktree_status: EQ-02 domain-owned quality/eval object IDs, strict QualityArtifact/QualityStateTransition DTOs, status matrix, digest/owner/source/time validation and canonical bytes are scoped to this step; no EvalStore/Judge/Promote/Broker or second evaluator path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/quality.rs kiana-domain/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/eq02_quality_contract.rs kiana-core/tests/eq02_quality_guard.rs .github/workflows/eq02-quality-contract.yml docs/roadmap/evaluation-quality-contract-baseline.md docs/roadmap.md
  rg -n 'QualityArtifact(Id|Status)?|QualityTransitionId|QualityStateTransition|Eval(Dataset|Suite|Case|Experiment|Result)Id|GoldenTraceId|Quality(Candidate|Gate|GateDecision)Id|FeedbackId|DriftAlertId|QUALITY_(ARTIFACT|TRANSITION)_SCHEMA|deny_unknown_fields|canonical_quality_bytes|quality_transition_invalid' kiana-domain/src kiana-protocol/src kiana-domain/tests/eq02_quality_contract.rs kiana-core/tests/eq02_quality_guard.rs docs/roadmap/evaluation-quality-contract-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; EQ-02 domain/core quality targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-domain/tests/eq02_quality_contract.rs stable ID/status/digest/strict/time/owner fixtures; kiana-core/tests/eq02_quality_guard.rs domain ownership/no-promotion source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EQ-02 job is queued by the next push and is not awaited
status_change: EQ-02 source slice is implemented. Quality identity and lifecycle primitives now have domain-owned stable IDs, strict schemas, canonical digests/bytes, owner/source integrity and deny-first monotonic status transitions; no quality result can grant or promote authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; full EvalDataset/Suite/Case/GoldenTrace/Experiment/Result/Candidate/Gate semantics, stores, normalizer/judge, isolation, scoring, promote/rollback and cross-process durable evidence remain EQ-03+ / ER / PD / SC.
reviewer: Codex root implementation review plus EQ-02 quality ID/status/digest/strict DTO source-boundary review; no runtime test reviewer
```

### EQ-03 evaluation object evidence (2026-09-17)

```text
source_snapshot: 9346097 + EQ-03 working-tree slice; kiana-domain/src/{quality,contracts,lib}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/eq03_eval_objects.rs; kiana-core/tests/eq03_eval_objects_guard.rs; .github/workflows/eq03-eval-objects.yml; docs/roadmap/evaluation-objects-baseline.md; docs/roadmap.md
worktree_status: EQ-03 strict EvalDataset/EvalSuite/EvalCase/GoldenTrace schema/version/provenance contracts and typed ref/digest/cursor checks are scoped to this step; existing core golden_trace capture remains a compatibility input and no store/normalizer/evaluator/Broker path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/quality.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/eq03_eval_objects.rs kiana-core/tests/eq03_eval_objects_guard.rs .github/workflows/eq03-eval-objects.yml docs/roadmap/evaluation-objects-baseline.md docs/roadmap.md
  rg -n 'EvalDataset|EvalSuite|EvalCase|GoldenTrace|EVAL_DATASET_SCHEMA|EVAL_SUITE_OBJECT_SCHEMA|EVAL_CASE_OBJECT_SCHEMA|GOLDEN_TRACE_SCHEMA|provenance|target_versions|normalized_events|event_cursor|deny_unknown_fields|golden_trace_(header|digest|source)' kiana-domain/src kiana-protocol/src kiana-domain/tests/eq03_eval_objects.rs kiana-core/tests/eq03_eval_objects_guard.rs docs/roadmap/evaluation-objects-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; EQ-03 domain/core eval-object targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-domain/tests/eq03_eval_objects.rs schema/version/provenance/refs/cursor/digest/strict/secret fixtures; kiana-core/tests/eq03_eval_objects_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EQ-03 job is queued by the next push and is not awaited
status_change: EQ-03 source slice is implemented. Domain now owns strict EvalDataset/Suite/Case/GoldenTrace objects with typed identities, privacy/provenance/fixture/oracle metadata, canonical lists, cursor/hash/score/expiry validation and digest integrity; unknown fields/schema or secret config cannot pass.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; dataset/suite/case/trace refs are not yet loaded from an isolated store, GoldenTrace normalization/diff/capture is absent, and experiment/result/candidate/gate/judge/promotion/durable replay remain EQ-04+ / EQ-08+ / ER / PD / SC.
reviewer: Codex root implementation review plus EQ-03 eval object/provenance/cursor/digest and no-execution-path source-boundary review; no runtime test reviewer
```

### EQ-04 evaluation admission metadata evidence (2026-09-17)

```text
source_snapshot: d52b955 + EQ-04 working-tree slice; kiana-domain/src/quality.rs; kiana-domain/tests/eq04_admission.rs; kiana-core/tests/eq04_admission_guard.rs; .github/workflows/eq04-admission.yml; docs/roadmap/evaluation-admission-baseline.md; docs/roadmap.md
worktree_status: EvalDataset/EvalCase privacy/split/owner/expiry/minimum_sample/workload_tags admission checks are scoped to this step; legacy defaults are explicit, `validate_for_admission(now)` is pure and no FixtureStore/runner/provider/Broker path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/quality.rs kiana-domain/tests/eq04_admission.rs kiana-core/tests/eq04_admission_guard.rs .github/workflows/eq04-admission.yml docs/roadmap/evaluation-admission-baseline.md docs/roadmap.md
  rg -n 'EvalSplit|privacy_class|minimum_sample|workload_tags|validate_for_admission|eval_(dataset|case)_(expired|privacy|minimum_sample|workload_tags)|canonical_tags|valid_privacy_class' kiana-domain/src/quality.rs kiana-domain/tests/eq04_admission.rs kiana-core/tests/eq04_admission_guard.rs docs/roadmap/evaluation-admission-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; EQ-04 admission targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-domain/tests/eq04_admission.rs expired/unowned dataset, case metadata, privacy/tag invalid and admission fixtures; kiana-core/tests/eq04_admission_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EQ-04 job is queued by the next push and is not awaited
status_change: EQ-04 source slice is implemented. Dataset/Case admission metadata now has bounded privacy/split/owner/expiry/sample/tag contracts with canonical ordering and explicit expiry evaluation; expired, unowned, unknown privacy, bad tags/sample/expiry/secret inputs fail closed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; owner is not yet authenticated Principal/ProjectTrust, expiry is not a durable clock/lease, refs are not loaded through isolated FixtureStore, and runner/normalizer/judge/gate/promotion/recovery remain EQ-08+ / ER / PD / SC.
reviewer: Codex root implementation review plus EQ-04 dataset/case admission metadata and no-store/no-execution source-boundary review; no runtime test reviewer
```

### EQ-05 legacy eval adapter evidence (2026-09-17)

```text
source_snapshot: 6960d64 + EQ-05 working-tree slice; kiana-domain/src/quality.rs; kiana-commands/Cargo.toml; Cargo.lock; kiana-commands/src/eval.rs; kiana-commands/tests/eq05_legacy_adapter.rs; kiana-core/tests/eq05_legacy_adapter_guard.rs; .github/workflows/eq05-legacy-adapter.yml; docs/roadmap/evaluation-legacy-adapter-baseline.md; docs/roadmap.md
worktree_status: explicit legacy kiana.eval-suite.v1 → typed LegacyEvalQualityBundle adapter is scoped to this step; EvalCommand validates the bundle then preserves legacy report/baseline/metrics output, no fixture path is opened by the adapter and no second evaluator/runner/promotion path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/quality.rs kiana-commands/Cargo.toml Cargo.lock kiana-commands/src/eval.rs kiana-commands/tests/eq05_legacy_adapter.rs kiana-core/tests/eq05_legacy_adapter_guard.rs .github/workflows/eq05-legacy-adapter.yml docs/roadmap/evaluation-legacy-adapter-baseline.md docs/roadmap.md
  rg -n 'adapt_legacy_eval_suite|LegacyEvalQualityBundle|stable_quality_uuid|legacy_eval_(suite|case|expect)_|suite_value|EVAL_REPORT_SCHEMA|kiana-domain' kiana-domain/src/quality.rs kiana-commands/src/eval.rs kiana-commands/tests/eq05_legacy_adapter.rs kiana-core/tests/eq05_legacy_adapter_guard.rs docs/roadmap/evaluation-legacy-adapter-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; EQ-05 domain/commands/core adapter targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-commands/tests/eq05_legacy_adapter.rs deterministic typed ID/bundle and unknown-field rejection fixtures; kiana-core/tests/eq05_legacy_adapter_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EQ-05 job is queued by the next push and is not awaited
status_change: EQ-05 source slice is implemented. Legacy eval suite JSON now passes an explicit strict domain adapter with deterministic typed IDs/digests before the existing evaluator runs, while the old report/baseline/metrics wire fields remain unchanged.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; legacy EvalCommand remains caller-path/read-only compatibility surface, adapter does not authenticate owner or open FixtureStore, and normalizer/Judge/EvalStore/experiment/gate/promote/rollback/durable evidence remain EQ-06+ / ER / PD / SC.
reviewer: Codex root implementation review plus EQ-05 legacy-to-domain adapter/deterministic ID/report compatibility source-boundary review; no runtime test reviewer
```

### EQ-06 quality protocol registry evidence (2026-09-17)

```text
source_snapshot: d53615d + EQ-06 working-tree slice; kiana-protocol/src/lib.rs; kiana-domain/src/{event_contracts,contracts,lib}.rs; kiana-protocol/tests/eq06_quality_protocol.rs; kiana-core/src/commands.rs; kiana-core/tests/eq06_quality_protocol_guard.rs; .github/workflows/eq06-quality-protocol.yml; docs/roadmap/evaluation-protocol-baseline.md; docs/roadmap.md
worktree_status: versioned QualityCommandKind/Request, six quality/eval command/event names and required-family event registry are scoped to this step; RequestEnvelope uses the existing generic command route and no quality handler, evaluator, provider/Broker or promotion authority was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-protocol/src/lib.rs kiana-domain/src/event_contracts.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-protocol/tests/eq06_quality_protocol.rs kiana-core/src/commands.rs kiana-core/tests/eq06_quality_protocol_guard.rs .github/workflows/eq06-quality-protocol.yml docs/roadmap/evaluation-protocol-baseline.md docs/roadmap.md
  rg -n 'QUALITY_COMMAND_SCHEMA|QualityCommandKind|QualityCommandRequest|QUALITY_(COMMAND|EVENT)_KINDS|quality_command|eval\.(run|capture|compare)|quality\.(feedback|promote|rollback)|QUALITY_FIELDS|unknown_required_event_kind' kiana-protocol/src kiana-domain/src kiana-core/src kiana-protocol/tests/eq06_quality_protocol.rs kiana-core/tests/eq06_quality_protocol_guard.rs docs/roadmap/evaluation-protocol-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; EQ-06 protocol/core/domain targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-protocol/tests/eq06_quality_protocol.rs command/event registration, strict envelope and unknown-family fixtures; kiana-core/tests/eq06_quality_protocol_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EQ-06 job is queued by the next push and is not awaited
status_change: EQ-06 source slice is implemented. Quality/eval command names and RuntimeEvent kinds now share a versioned protocol/schema registry with strict request shape and required-family unknown rejection; the helper only routes through existing Command envelope and cannot grant or execute authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; commands have no handler yet, arguments lack operation-specific authorization/secret policy, event source/owner/materializer/store/ports and quality execution/promotion remain EQ-07+ / EQ-08+ / ER / PD / SC.
reviewer: Codex root implementation review plus EQ-06 versioned command/event/unknown-family and no-bypass source-boundary review; no runtime test reviewer
```

### EQ-07 quality ports evidence (2026-09-17)

```text
source_snapshot: a5cce7c + EQ-07 working-tree slice; kiana-ports/src/lib.rs; kiana-domain/src/quality.rs; kiana-ports/tests/eq07_quality_ports.rs; kiana-core/tests/eq07_quality_ports_guard.rs; .github/workflows/eq07-quality-ports.yml; docs/roadmap/evaluation-ports-baseline.md; docs/roadmap.md
worktree_status: EvalStore/FixtureStore/TraceSource/ArtifactReader/Judge/MetricsSink/Clock traits are scoped to this step with default structured unsupported errors; ports expose only domain typed objects, opaque refs, RuntimeEvent/cursor, ArtifactRef and JSON, no adapter or execution loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-ports/src/lib.rs kiana-domain/src/quality.rs kiana-ports/tests/eq07_quality_ports.rs kiana-core/tests/eq07_quality_ports_guard.rs .github/workflows/eq07-quality-ports.yml docs/roadmap/evaluation-ports-baseline.md docs/roadmap.md
  rg -n 'trait (EvalStore|FixtureStore|TraceSource|ArtifactReader|Judge|MetricsSink|Clock)|eval_store_unsupported|fixture_store_unsupported|trace_source_unsupported|kiana_daemon|kiana_provider|PathBuf|reqwest' kiana-ports/src/lib.rs kiana-ports/tests/eq07_quality_ports.rs kiana-core/tests/eq07_quality_ports_guard.rs docs/roadmap/evaluation-ports-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; EQ-07 ports/domain/core targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-ports/tests/eq07_quality_ports.rs compile-only fake adapters/default unsupported boundaries; kiana-core/tests/eq07_quality_ports_guard.rs dependency source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EQ-07 job is queued by the next push and is not awaited
status_change: EQ-07 source slice is implemented. Quality persistence/fixture/trace/artifact/judge/metrics/clock boundaries now exist below core without daemon/provider/filesystem/network coupling, and unsupported adapters fail explicitly rather than silently degrading.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; all new ports lack production adapters, durable CAS/recovery, scope/path isolation, source authentication, normalizer/runner integration, judge quality evidence and promotion authority; EQ-08+ / ER / PD / SC remain open.
reviewer: Codex root implementation review plus EQ-07 port layering/no-provider/no-filesystem/no-execution source-boundary review; no runtime test reviewer
```

### EQ-08 deterministic fixture loader evidence (2026-09-17)

```text
source_snapshot: 70cdc3f + EQ-08 working-tree slice; kiana-commands/src/{eval_fixtures,lib}.rs; kiana-commands/tests/eq08_fixture_loader.rs; kiana-core/tests/eq08_fixture_loader_guard.rs; tests/eval/{README,manifest.json,runtime-events.jsonl}; .github/workflows/eq08-fixture-loader.yml; docs/roadmap/evaluation-fixture-loader-baseline.md; docs/roadmap.md
worktree_status: strict EvalFixtureManifest/Case and deterministic declared-file loader are scoped to this step; canonical root/relative path/regular file/schema/size/hash checks run before bytes are returned, no implicit operator-home scan or second runner was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-commands/src/eval_fixtures.rs kiana-commands/src/lib.rs kiana-commands/tests/eq08_fixture_loader.rs kiana-core/tests/eq08_fixture_loader_guard.rs tests/eval/README.md tests/eval/manifest.json tests/eval/runtime-events.jsonl .github/workflows/eq08-fixture-loader.yml docs/roadmap/evaluation-fixture-loader-baseline.md docs/roadmap.md
  rg -n 'EvalFixtureManifest|EvalFixtureCase|LoadedFixtureManifest|load_fixture_manifest|EVAL_FIXTURE_MANIFEST_SCHEMA|fixture_schema_unknown|fixture_manifest_case_id_invalid|resolve_declared_path|canonicalize|starts_with\(root\)' kiana-commands/src kiana-commands/tests/eq08_fixture_loader.rs kiana-core/tests/eq08_fixture_loader_guard.rs tests/eval docs/roadmap/evaluation-fixture-loader-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; EQ-08 commands/core loader targets compiled only; no test or smoke command executed locally
fixture or cassette: tests/eval/manifest.json + runtime-events.jsonl; kiana-commands/tests/eq08_fixture_loader.rs path/schema/duplicate/size/unknown-field fixtures; core source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions EQ-08 job is queued by the next push and is not awaited
status_change: EQ-08 source slice is implemented. A strict manifest and loader now make fixture paths, schemas, case identity/order, file type/size and SHA-256 explicit before returning bytes; undeclared/escaped/unknown/oversized fixtures fail closed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; loader remains legacy command test adapter rather than FixtureStore/DaemonHost, does not normalize event payloads or validate secret/cursor/oracle/provider content, and no isolated KIANA_HOME or no-follow/TOCTOU guarantee exists; EQ-09+ / EQ-16+ / PD / SC remain open.
reviewer: Codex root implementation review plus EQ-08 fixture manifest/path/schema/size determinism and no-second-runner source-boundary review; no runtime test reviewer
```

### PD-01 storage root/identity evidence (2026-09-17)

```text
source_snapshot: 6e1bc2f + PD-01 working-tree slice; kiana-domain/src/{storage,ids,contracts,lib}.rs; kiana-daemon/src/{storage,lib}.rs; kiana-domain/tests/pd01_storage.rs; kiana-daemon/tests/pd01_storage.rs; kiana-core/tests/pd01_storage_guard.rs; kiana-protocol/src/lib.rs; .github/workflows/pd01-storage-root.yml; docs/roadmap/persistence-storage-root-baseline.md; docs/roadmap.md
worktree_status: PD-01 StorageRoot/OwnerScope/StoreIdentity/StorageLockRecord contracts, stable namespace/root digest, daemon KIANA_HOME/HOME resolver and create-new storage lease are scoped to this step; DaemonHost exposes the same lifecycle entry and no ControlPlane/Broker execution loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/storage.rs kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-daemon/src/storage.rs kiana-daemon/src/lib.rs kiana-domain/tests/pd01_storage.rs kiana-daemon/tests/pd01_storage.rs kiana-core/tests/pd01_storage_guard.rs kiana-protocol/src/lib.rs .github/workflows/pd01-storage-root.yml docs/roadmap/persistence-storage-root-baseline.md docs/roadmap.md
  rg -n 'Storage(Root|OwnerScope|Namespace|Backend|LockRecord)|StoreIdentity|STORAGE_(ROOT|OWNER_SCOPE|LOCK)_SCHEMA|storage_root_inside_project|storage_network_filesystem_unsupported|storage_lock_(conflict|owner_mismatch)|detect_backend|create_new\(true\)|store-identity.json' kiana-domain/src kiana-daemon/src kiana-protocol/src kiana-domain/tests/pd01_storage.rs kiana-daemon/tests/pd01_storage.rs kiana-core/tests/pd01_storage_guard.rs docs/roadmap/persistence-storage-root-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; PD-01 domain/daemon/core storage targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-domain/tests/pd01_storage.rs stable namespace/identity/network/owner/strict fixtures; kiana-daemon/tests/pd01_storage.rs resolver/identity/lock/project-local fixtures; kiana-core/tests/pd01_storage_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions PD-01 job is queued by the next push and is not awaited
status_change: PD-01 source slice is implemented. Storage roots now have domain-owned stable identity, owner/instance/authority scope, fixed namespaces and digest; daemon resolves user-level root and persists strict StoreIdentity/0600 create-new StorageLease, rejecting relative/project-local/network/identity/lock conflicts.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; legacy adapters still read KIANA_HOME independently, create-new lock is not crash-stale/fd-fenced across machines, network/TOCTOU/fsync-dir detection is bounded, and projection/artifact/backup/migration/retention adapters/recovery remain PD-02+ / ER / DEP / SC.
reviewer: Codex root implementation review plus PD-01 storage root/owner/identity/namespace/lock and no-second-execution source-boundary review; no runtime test reviewer
```

### PD-02 storage schema/canonical/upcast evidence (2026-09-17)

```text
source_snapshot: c6cce35 + PD-02 working-tree slice; kiana-domain/src/{storage_schema,memory,contracts,lib}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/pd02_schema.rs; kiana-core/tests/pd02_schema_guard.rs; .github/workflows/pd02-schema.yml; docs/roadmap/persistence-schema-baseline.md; docs/roadmap.md
worktree_status: StorageSchemaRegistry uniqueness/digest, canonical storage bytes/number policy and named memory v1→v2 upcast are scoped to this step; unknown expected/major/field/non-migratable schema paths fail closed and no migration runner/Broker/execution loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/storage_schema.rs kiana-domain/src/memory.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/pd02_schema.rs kiana-core/tests/pd02_schema_guard.rs .github/workflows/pd02-schema.yml docs/roadmap/persistence-schema-baseline.md docs/roadmap.md
  rg -n 'StorageSchemaRegistry|validate_schema_registry|schema_contracts_digest|canonical_storage_(bytes|digest)|upcast_storage_value|upcast_memory_record|storage_schema_(unknown_major|expected_unknown)|storage_migration_non_migratable_field|storage_noncanonical_number|SCHEMA_MIGRATIONS' kiana-domain/src kiana-protocol/src kiana-domain/tests/pd02_schema.rs kiana-core/tests/pd02_schema_guard.rs docs/roadmap/persistence-schema-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; PD-02 domain/protocol/core schema targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-domain/tests/pd02_schema.rs registry/canonical number/unknown-field, memory legacy upcast/unknown major/field/expected schema fixtures; kiana-core/tests/pd02_schema_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions PD-02 job is queued by the next push and is not awaited
status_change: PD-02 source slice is implemented. Domain schema registry is unique/digest-bound, canonical bytes reject unsafe numeric forms, and only a named memory-record v1→v2 migration is accepted with destination lifecycle validation; unknown schema/field/migration never silently downgrades.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; registry is process-static, legacy migration coverage is intentionally narrow, SQLite/migration journal/backup/CAS/fsync/replay/quarantine and all adapter integration remain PD-03+ / ER / DEP / SC.
reviewer: Codex root implementation review plus PD-02 schema uniqueness/canonical/upcast and fail-closed migration source-boundary review; no runtime test reviewer
```

### PD-03 storage health/errors evidence (2026-09-17)

```text
source_snapshot: f7c353e + PD-03 working-tree slice; kiana-domain/src/{storage_health,ids,contracts,lib}.rs; kiana-ports/src/lib.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/pd03_health.rs; kiana-ports/tests/pd03_error_mapping.rs; kiana-core/tests/pd03_health_guard.rs; .github/workflows/pd03-storage-health.yml; docs/roadmap/persistence-health-baseline.md; docs/roadmap.md
worktree_status: typed StorageError class/retry taxonomy, StorageCapabilities/Health/IntegrityIncident and PortError mapping are scoped to this step; Unknown/Corrupt/ResultUnknown require reconciliation and no adapter/projector/execution loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/storage_health.rs kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-ports/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/pd03_health.rs kiana-ports/tests/pd03_error_mapping.rs kiana-core/tests/pd03_health_guard.rs .github/workflows/pd03-storage-health.yml docs/roadmap/persistence-health-baseline.md docs/roadmap.md
  rg -n 'StorageError(Class|Id)?|StorageRetryDisposition|StorageCapabilities|StorageHealth(Status|Id)?|StorageIntegrityIncident(Class|Id)?|storage_class|into_storage_error|storage_error_retry_mismatch|quarantine_required|ResultUnknown|Corrupt' kiana-domain/src kiana-ports/src kiana-protocol/src kiana-domain/tests/pd03_health.rs kiana-ports/tests/pd03_error_mapping.rs kiana-core/tests/pd03_health_guard.rs docs/roadmap/persistence-health-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; PD-03 domain/ports/protocol/core health targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-domain/tests/pd03_health.rs error/health/capability/incident strict fixtures; kiana-ports/tests/pd03_error_mapping.rs PortError taxonomy; kiana-core/tests/pd03_health_guard.rs source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions PD-03 job is queued by the next push and is not awaited
status_change: PD-03 source slice is implemented. Storage errors retain six distinct classes with derived retry/reconcile disposition; capability durability conflicts, strict health snapshots and quarantine-required integrity incidents are typed/digest-bound, with conservative PortError mapping.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; mapping is not yet applied at every adapter/CLI/HTTP/Receipt call, health remains projection, capabilities are declarations, and EventLog quarantine/projector/backup/recovery/ports conformance remain PD-04+ / ER / DEP / SC.
reviewer: Codex root implementation review plus PD-03 storage error/health/reconcile taxonomy and no-success-fallback source-boundary review; no runtime test reviewer
```

### PD-04 storage lifecycle ports evidence (2026-09-17)

```text
source_snapshot: 7103b8a + PD-04 working-tree slice; kiana-ports/src/lib.rs; kiana-domain/src/{storage_health,storage_schema,storage,lib}.rs; kiana-protocol/src/lib.rs; kiana-ports/tests/pd04_storage_ports.rs; kiana-core/tests/pd04_storage_ports_guard.rs; .github/workflows/pd04-storage-ports.yml; docs/roadmap/persistence-storage-ports-baseline.md; docs/roadmap.md
worktree_status: ProjectionStorePort/ArtifactStorePort/BackupStorePort/MigrationRunnerPort/RetentionStorePort typed lifecycle boundaries are scoped to this step; defaults return explicit Unsupported and no concrete adapter, filesystem/provider/network type or execution loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-ports/src/lib.rs kiana-domain/src/{storage_health,storage_schema,storage,lib}.rs kiana-protocol/src/lib.rs kiana-ports/tests/pd04_storage_ports.rs kiana-core/tests/pd04_storage_ports_guard.rs .github/workflows/pd04-storage-ports.yml docs/roadmap/persistence-storage-ports-baseline.md docs/roadmap.md
  rg -n 'trait (ProjectionStorePort|ArtifactStorePort|BackupStorePort|MigrationRunnerPort|RetentionStorePort)|projection_store_unsupported|artifact_store_unsupported|backup_store_unsupported|migration_runner_unsupported|retention_store_unsupported|expected_source_cursor|append_tombstone|expected_tombstone_revision|StorageSchemaRegistry' kiana-ports/src/lib.rs kiana-ports/tests/pd04_storage_ports.rs kiana-core/tests/pd04_storage_ports_guard.rs docs/roadmap/persistence-storage-ports-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; PD-04 ports/domain/protocol/core targets compiled only; no test or smoke command executed locally
fixture or cassette: kiana-ports/tests/pd04_storage_ports.rs compile-only fake lifecycle ports/default Unsupported; kiana-core/tests/pd04_storage_ports_guard.rs no-fallback/no-execution source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions PD-04 job is queued by the next push and is not awaited
status_change: PD-04 source slice is implemented. Projection/artifact/backup/migration/retention lifecycle traits now have typed cursor/revision/hash/schema/tombstone boundaries and explicit Unsupported defaults, keeping storage capability absence distinct from empty success and below core/provider/filesystem layers.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; no production adapters/CAS/fsync/SQLite/JSONL/projector/backup/migration/retention implementation or cross-process recovery exists, and bytes/manifests still require later redaction/privacy/scope enforcement; PD-05+ / ER / DEP / SC remain open.
reviewer: Codex root implementation review plus PD-04 storage port layering/no-fallback/no-execution source-boundary review; no runtime test reviewer
```

### SC-01 threat register evidence (2026-09-17)

```text
source_snapshot: 7d1579f + SC-01 working-tree slice; docs/roadmap/security-threat-register.md; docs/roadmap/security-compliance-baseline.md; docs/roadmap/security-compliance.md; docs/company-os-security-constitution.md; kiana-core/tests/sc01_threat_register.rs; .github/workflows/sc01-threat-register.yml; docs/roadmap.md
worktree_status: SC-01 docs-only T01-T12 threat/asset/control/evidence register and CI-only security fixture catalog are scoped to this step; no runtime authorization/security implementation or second execution path was added, and current code/status evidence remains bounded by its snapshot; static verification is complete and commit/push are pending
command_argv:
  sha256sum docs/roadmap/security-threat-register.md docs/roadmap/security-compliance-baseline.md docs/roadmap/security-compliance.md docs/company-os-security-constitution.md kiana-core/tests/sc01_threat_register.rs .github/workflows/sc01-threat-register.yml docs/roadmap.md
  rg -n 'T(0[1-9]|1[0-2])|CI-only security fixture catalog|EventLog facts are authoritative|model/UI|external/physical|source snapshot|limitations|SC-01' docs/roadmap/security-threat-register.md docs/roadmap/security-compliance-baseline.md docs/roadmap/security-compliance.md kiana-core/tests/sc01_threat_register.rs
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; SC-01 docs/source-guard test target compiled only; no test or smoke command executed locally
fixture or cassette: kiana-core/tests/sc01_threat_register.rs source-only threat/constitution/baseline/roadmap assertions; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SC-01 job is queued by the next push and is not awaited
status_change: SC-01 source slice is implemented. T01-T12 threats, assets, controls, proof ceilings, follow-up owners and deny-first security fixture names are now explicitly registered against the constitution and current-state baseline.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; a threat register is not enforcement or certification, historical source/local evidence remains scoped, and SC-02+ must implement/verify security IDs, reason codes, context, identity, policy, secret, effect, audit, supply-chain, incident and release controls.
reviewer: Codex root implementation review plus SC-01 threat/asset/evidence catalog and non-inflation source review; no runtime test reviewer
```

### SC-02 security IDs and schema registry evidence (2026-09-17)

```text
source_snapshot: b4e0947 + SC-02 working-tree slice; kiana-domain/src/{ids,contracts,security_contracts,lib}.rs; kiana-domain/tests/sc02_security_contract.rs; kiana-core/tests/sc02_security_contract_guard.rs; kiana-protocol/src/lib.rs; .github/workflows/sc02-security-contract.yml; docs/roadmap/security-contract-baseline.md; docs/roadmap/security-compliance.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: SC-02 stable security IDs, unique canonical-registry projection, strict versioned security envelope, monotonic digest/epoch/sequence links, secret-safe payload guard, explicit upcast boundary, CI fixtures/source guard and roadmap/status overlays are scoped to this step; no SecurityContext/authn/policy/SecretStore/Audit projector, second schema registry, second execution path or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/security_contracts.rs kiana-domain/src/lib.rs kiana-domain/tests/sc02_security_contract.rs kiana-core/tests/sc02_security_contract_guard.rs kiana-protocol/src/lib.rs .github/workflows/sc02-security-contract.yml docs/roadmap/security-contract-baseline.md docs/roadmap/security-compliance.md docs/roadmap/README.md docs/roadmap.md CURRENT_STATUS.md
  rg -n 'Security(Registry|Context|Policy|Decision|Event)Id|GrantId|OperationId|AuditId|SecretRefId|EvidenceRefId|SecuritySchemaRegistry|SecurityObjectEnvelope|upcast_security_(object|registry)|security_.*(rollback|unknown_major|secret_field|duplicate_id)' kiana-domain/src kiana-domain/tests kiana-core/tests kiana-protocol/src docs/roadmap/security-contract-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain/core test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/sc02_security_contract.rs genesis/successor registry, duplicate ID, unknown major, digest/epoch/sequence rollback, secret serde/payload and explicit upcast fixtures; kiana-core/tests/sc02_security_contract_guard.rs domain-only source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SC-02 job is queued by the push and is not awaited
status_change: SC-02 source slice is implemented. Ten security-specific stable UUID ID contracts are registered; SecuritySchemaRegistry is generated from the single SCHEMA_CONTRACTS source and rejects duplicate/drifted entries, unknown major, noncanonical order, stale digest and revision/authority/data/sequence or parent-chain rollback. SecurityObjectEnvelope provides strict versioned object/event serialization, bounded canonical payloads, digest chaining and recursive secret marker/value rejection; unknown major and unregistered migration fail closed. Protocol only re-exports these domain contracts.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; the registry snapshot is process-generated and not a persisted cross-process authority, IDs do not authenticate callers or grant capability, the envelope payload remains a generic domain boundary until SC-03/04 bind reason/context objects, no SecretStore/redaction-at-every-output/Audit projector/TOCTOU or external effect is proven, and future migrations must be explicitly registered rather than inferred
reviewer: Codex root implementation review plus SC-02 ID/schema/digest/epoch/sequence/secret-boundary reconciliation; no runtime test reviewer
```

### SC-03 stable security reason codes evidence (2026-09-17)

```text
source_snapshot: 85076a2 + SC-03 working-tree slice; kiana-domain/src/{security_reasons,contracts,lib}.rs; kiana-domain/tests/sc03_security_reason.rs; kiana-core/tests/sc03_security_reason_guard.rs; kiana-protocol/src/lib.rs; .github/workflows/sc03-security-reason.yml; docs/roadmap/security-reason-baseline.md; docs/roadmap/security-compliance.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: SC-03 stable AUTH/POLICY/DATA/SECRET/EXT/FS/NET/RESOURCE/FACT/UNKNOWN reason codes, class/retryability/remediation policy, bounded legacy classifier, strict digest-only SecurityReason DTO, protocol re-export, CI fixtures/source guard and roadmap/status overlays are scoped to this step; existing CapabilityErrorCode compatibility remains unchanged; no policy evaluator, SecurityContext, SecretStore, redaction pipeline, effect/retry path or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/security_reasons.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-domain/tests/sc03_security_reason.rs kiana-core/tests/sc03_security_reason_guard.rs kiana-protocol/src/lib.rs .github/workflows/sc03-security-reason.yml docs/roadmap/security-reason-baseline.md docs/roadmap/security-compliance.md docs/roadmap/README.md docs/roadmap.md CURRENT_STATUS.md
  rg -n 'SecurityReason(Code|Policy|Class)|AUTH_|POLICY_|DATA_|SECRET_|EXT_|FS_|NET_|RESOURCE_|FACT_|UNKNOWN_|classify_security_reason|retryability|remediation|detail_digest|deny_unknown_fields' kiana-domain/src kiana-domain/tests kiana-core/tests kiana-protocol/src docs/roadmap/security-reason-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain/core test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/sc03_security_reason.rs stable code/policy/legacy mapping/unknown/reason serde/digest fixtures; kiana-core/tests/sc03_security_reason_guard.rs domain/protocol/no-raw-error source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SC-03 job is queued by the push and is not awaited
status_change: SC-03 source slice is implemented. SecurityReasonCode now covers the ten stable families from AUTH through UNKNOWN, each with an explicit class, retryability and remediation; unknown legacy text never becomes permission or retry authority and maps to conservative UNKNOWN. SecurityReason is strict/versioned, carries only typed operation/evidence references and optional SHA-256 detail digest, rejects raw error/provider/prompt/secret fields, and is re-exported by protocol without execution semantics.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; reason classification is not yet wired to every ControlPlane/entrypoint response, old CapabilityErrorCode diagnostics remain compatibility paths, policy/retry/remediation values do not themselves execute actions, and SecurityContext/authority binding, recursive output redaction, audit persistence, external reconciliation and durable/live/physical proof remain SC-04+
reviewer: Codex root implementation review plus SC-03 stable-code/unknown-classification/raw-error boundary reconciliation; no runtime test reviewer
```

### SC-04 SecurityContext and entry identity boundary evidence (2026-09-17)

```text
source_snapshot: f7c7346 + SC-04 working-tree slice; kiana-core/src/{security_context,lib}.rs; kiana-daemon/src/lib.rs; kiana-core/tests/sc04_security_context.rs; kiana-core/tests/sc04_security_context_guard.rs; kiana-domain/src/contracts.rs; .github/workflows/sc04-security-context.yml; docs/roadmap/security-context-baseline.md; docs/roadmap/security-compliance.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: SC-04 strict server-owned SecurityContext, actor/session/project/role/department/trust assertion checks, authority/data epoch and policy digest snapshot, untrusted effect gate, DaemonHost preflight wiring, CI fixtures/source guards and roadmap/status overlays are scoped to this step; no external auth provider, second authorization loop, second execution path or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-core/src/security_context.rs kiana-core/src/lib.rs kiana-daemon/src/lib.rs kiana-core/tests/sc04_security_context.rs kiana-core/tests/sc04_security_context_guard.rs kiana-domain/src/contracts.rs .github/workflows/sc04-security-context.yml docs/roadmap/security-context-baseline.md docs/roadmap/security-compliance.md docs/roadmap/README.md docs/roadmap.md CURRENT_STATUS.md
  rg -n 'SecurityContext|resolve_security_context|validate_request_assertions|apply_to_request|require_trusted_for_effect|AuthCallerUntrusted|AuthProjectMismatch|AUTH_PROJECT_UNTRUSTED|authority_epoch|data_epoch|project_trusted' kiana-core/src/security_context.rs kiana-core/src/lib.rs kiana-daemon/src/lib.rs kiana-core/tests/sc04_security_context.rs kiana-core/tests/sc04_security_context_guard.rs docs/roadmap/security-context-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; core/daemon test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/sc04_security_context.rs server-owned context round-trip, actor/anonymous/project/role/forged-trust deny and untrusted-effect gate; kiana-core/tests/sc04_security_context_guard.rs daemon/core source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SC-04 job is queued by the push and is not awaited
status_change: SC-04 source slice is implemented. ControlPlane now exposes a strict SecurityContext built from daemon-owned principal, project identity/trust, role descriptor and authority revision/epoch; caller actor/session/project/role/department/trust assertions are checked before legacy RequestContext projection, and untrusted server snapshots cannot enter effect admission. DaemonHost performs this preflight before existing command/run/approval/query routing and includes the context digest in effect configuration evidence; no new execution or authorization loop was added.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; AuthenticatedPrincipalRef remains local-user compatibility identity, ProjectTrust and authority/session assignment remain partial/in-memory or JSONL scoped, role selection still comes from a server-validated catalog/assignment boundary but is not durable external authentication, data_epoch is the current compatibility value, all four entrypoint parity and complete policy/Grant/approval/SecretStore/redaction/TOCTOU/external-effect enforcement remain future SC/CP/CAP/ER/PD work
reviewer: Codex root implementation review plus SC-04 server-owned context/caller-assertion/entry-preflight reconciliation; no runtime test reviewer
```

### SC-05 policy bundle and decision trace evidence (2026-09-17)

```text
source_snapshot: 654c3fd + SC-05 working-tree slice; kiana-policy/src/{lib,security}.rs; kiana-policy/Cargo.toml; kiana-policy/tests/sc05_policy.rs; kiana-core/tests/sc05_policy_guard.rs; kiana-domain/src/{contracts,security_reasons}.rs; .github/workflows/sc05-policy.yml; docs/roadmap/security-policy-baseline.md; docs/roadmap/security-compliance.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: SC-05 strict PolicyRule/PolicyBundle/PolicyRevision/DecisionTrace contracts, default-deny/deny-first evaluation, revision/authority snapshot fencing, input/context digest trace, BundlePolicyEngine compatibility adapter, domain registry entries, CI fixtures/source guard and roadmap/status overlays are scoped to this step; existing DefaultPolicyEngine/ControlPlane path remains compatible; no Broker/handler/approval execution, second policy loop or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-policy/src/lib.rs kiana-policy/src/security.rs kiana-policy/Cargo.toml kiana-policy/tests/sc05_policy.rs kiana-core/tests/sc05_policy_guard.rs kiana-domain/src/contracts.rs kiana-domain/src/security_reasons.rs .github/workflows/sc05-policy.yml docs/roadmap/security-policy-baseline.md docs/roadmap/security-compliance.md docs/roadmap/README.md docs/roadmap.md CURRENT_STATUS.md
  rg -n 'Policy(Bundle|Revision|Rule)|DecisionTrace|BundlePolicyEngine|PolicyEffect|PolicyOutcome|evaluate_with_snapshot|default.*deny|PolicyOperationUnregistered|PolicyAuthorityEpoch(Stale|Rollback)|policy_.*digest|hard_policy_denial' kiana-policy/src kiana-policy/tests kiana-core/tests kiana-domain/src/security_reasons.rs docs/roadmap/security-policy-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; policy/core test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-policy/tests/sc05_policy.rs default-deny/strict digest, deny-first selector, Ask/Allow trace, epoch/revision drift and unknown-field fixtures; kiana-core/tests/sc05_policy_guard.rs policy purity/trait-boundary source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SC-05 job is queued by the push and is not awaited
status_change: SC-05 source slice is implemented. PolicyRule/PolicyBundle now enforce exact selectors, canonical ordering, explicit reason requirements and a non-Allow default; evaluate_with_snapshot checks authority epoch/policy digest before existing hard denials and returns a deterministic DecisionTrace rather than falling back. PolicyRevision and trace are strict/versioned/digest-bound; BundlePolicyEngine adapts the existing PolicyEngine and malformed bundles fail closed without dispatch, approval consumption or Broker access.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; ControlPlane still uses the legacy DefaultPolicyEngine unless a caller explicitly installs BundlePolicyEngine, no durable PolicyStore/revision CAS or all-entrypoint policy parity exists, RequestContext lacks typed authority/data epoch so snapshot checks require explicit inputs, and Grant/Approval/DataBoundary/Secret/redaction/TOCTOU/external-effect enforcement remains later SC/CP/CAP/ER/PD work
reviewer: Codex root implementation review plus SC-05 default-deny/deny-first/revision-trace/dependency-boundary reconciliation; no runtime test reviewer
```

### SC-06 Principal/session/authn adapter evidence (2026-09-17)

```text
source_snapshot: 6ea69ba + SC-06 working-tree slice; kiana-domain/src/{session_contracts,identity_contracts,contracts,lib}.rs; kiana-domain/tests/sc06_session.rs; kiana-daemon/src/{authn,lib}.rs; kiana-daemon/tests/sc06_authn.rs; kiana-core/tests/sc06_authn_guard.rs; kiana-protocol/src/lib.rs; .github/workflows/sc06-authn.yml; docs/roadmap/security-authn-baseline.md; docs/roadmap/security-compliance.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: SC-06 strict SessionAssertion/AuthenticationAssurance/SessionStatus lifecycle, opaque LocalAuthnAdapter, expiry/revoke/duplicate/missing protected guards, DaemonHost preflight, protocol exports, CI fixtures/source guard and roadmap/status overlays are scoped to this step; no bearer/raw secret store, external auth provider, role assignment, second execution path or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/session_contracts.rs kiana-domain/src/identity_contracts.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-domain/tests/sc06_session.rs kiana-daemon/src/authn.rs kiana-daemon/src/lib.rs kiana-daemon/tests/sc06_authn.rs kiana-core/tests/sc06_authn_guard.rs kiana-protocol/src/lib.rs .github/workflows/sc06-authn.yml docs/roadmap/security-authn-baseline.md docs/roadmap/security-compliance.md docs/roadmap/README.md docs/roadmap.md CURRENT_STATUS.md
  rg -n 'SessionAssertion|AuthenticationAssurance|SessionStatus|LocalAuthnAdapter|validate_if_present|AUTH_SESSION_(REPLAY|MISSING|EXPIRED|REVOKED)|protected_local|bearer|access_token|refresh_token' kiana-domain/src kiana-domain/tests kiana-daemon/src kiana-daemon/tests kiana-core/tests kiana-protocol/src docs/roadmap/security-authn-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain/daemon/core test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/sc06_session.rs session/principal lifecycle and strict serde fixtures; kiana-daemon/tests/sc06_authn.rs opaque session issue/validation/expiry/revoke/replay fixtures; kiana-core/tests/sc06_authn_guard.rs source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SC-06 job is queued by the push and is not awaited
status_change: SC-06 source slice is implemented. Domain now provides strict bounded SessionAssertion with assurance, expiry, credential generation, authority epoch, digest and non-resurrecting status transitions; daemon LocalAuthnAdapter validates known sessions, rejects duplicate/revoked/expired/missing protected sessions and stores only opaque principal references. DaemonHost performs the adapter check before SecurityContext/ControlPlane while unknown legacy sessions remain explicit compatibility, and protocol re-exports session DTOs.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; session state is process-local and lost on restart, LocalAuthnAdapter is local-user compatibility rather than OAuth/Unix peer/tenant authentication, protected credential_ref remains an opaque ingress assertion, authority epoch is compatibility value 1 and no durable revocation/generation projector or cross-process fence exists, role/ProjectTrust/Grant/policy/SecretStore/redaction/provider/external/physical enforcement remains later SC/CP/CAP/ER/PD work
reviewer: Codex root implementation review plus SC-06 session lifecycle/authn opacity/restart-boundary reconciliation; no runtime test reviewer
```

### SC-07 ProjectTrust and authority snapshot evidence (2026-09-17)

```text
source_snapshot: bf8fdc8 + SC-07 working-tree slice; kiana-domain/src/{trust_snapshots,assignment,roles,identity,contracts,lib}.rs; kiana-domain/tests/sc07_trust_snapshots.rs; kiana-core/src/{security_authority,lib}.rs; kiana-core/tests/sc07_authority_snapshot.rs; kiana-core/tests/sc07_authority_guard.rs; kiana-daemon/src/lib.rs; .github/workflows/sc07-authority.yml; docs/roadmap/security-authority-baseline.md; docs/roadmap/security-compliance.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: SC-07 strict ProjectTrustSnapshot/DepartmentSnapshot, SecurityAuthoritySnapshot principal/project/role/department/epoch join, assignment/context daemon wiring, untrusted effect gate, CI fixtures/source guards and roadmap/status overlays are scoped to this step; no external auth provider, durable assignment projector, second authorization/execution path or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/trust_snapshots.rs kiana-domain/src/assignment.rs kiana-domain/src/roles.rs kiana-domain/src/identity.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-domain/tests/sc07_trust_snapshots.rs kiana-core/src/security_authority.rs kiana-core/src/lib.rs kiana-core/tests/sc07_authority_snapshot.rs kiana-core/tests/sc07_authority_guard.rs kiana-daemon/src/lib.rs .github/workflows/sc07-authority.yml docs/roadmap/security-authority-baseline.md docs/roadmap/security-compliance.md docs/roadmap/README.md docs/roadmap.md CURRENT_STATUS.md
  rg -n 'ProjectTrustSnapshot|DepartmentSnapshot|SecurityAuthoritySnapshot|context_from_assignment|project_trust_snapshot|validate_request|require_trusted_for_effect|AUTH_PROJECT_MISMATCH|AUTH_ROLE_MISMATCH|authority_epoch|department_snapshot_roles_noncanonical' kiana-domain/src kiana-domain/tests kiana-core/src kiana-core/tests kiana-daemon/src docs/roadmap/security-authority-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain/core/daemon test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/sc07_trust_snapshots.rs trust/department/assignment fixtures; kiana-core/tests/sc07_authority_snapshot.rs authority join/foreign scope/untrusted effect fixtures; kiana-core/tests/sc07_authority_guard.rs source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SC-07 job is queued by the push and is not awaited
status_change: SC-07 source slice is implemented. ProjectTrustSnapshot and DepartmentSnapshot now provide strict server-scoped trust/role catalog values; SecurityAuthoritySnapshot joins those snapshots with server principal and ResolvedAssignment, rejects foreign principal/project/role/department/epoch and provides an explicit untrusted-effect gate. DaemonHost context_from_assignment and project_trust_snapshot use these values before the existing company assignment guard, without adding a second execution path.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; AssignmentDirectory and trust authority remain in-memory/local compatibility adapters, role/project revocation and department refresh lack durable CAS/replay, local-user is not external authentication, SecurityContext does not yet carry the full authority snapshot on every request, and Grant/Policy/Approval/Secret/redaction/TOCTOU/external/live/physical enforcement remains SC-08+
reviewer: Codex root implementation review plus SC-07 server-owned trust/assignment/department/epoch reconciliation; no runtime test reviewer
```

### SC-08 authority epoch and session fence evidence (2026-09-17)

```text
source_snapshot: 30a5d18 + SC-08 working-tree slice; kiana-domain/src/{fencing,ids,contracts,security_reasons,lib}.rs; kiana-domain/tests/sc08_fence.rs; kiana-core/src/{security_fence,authority,lib}.rs; kiana-core/tests/sc08_fence_guard.rs; kiana-protocol/src/lib.rs; .github/workflows/sc08-fence.yml; docs/roadmap/security-fence-baseline.md; docs/roadmap/security-compliance.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: SC-08 strict AuthorityFence with authority/session/policy/config revisions, monotonic parent digest/sequence and TTL, ControlPlane read/recheck helpers, stable stale/rollback/expiry reasons, CI fixtures/source guard and roadmap/status overlays are scoped to this step; no durable permit store, automatic retry/revocation, second authority source or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/fencing.rs kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/security_reasons.rs kiana-domain/src/lib.rs kiana-domain/tests/sc08_fence.rs kiana-core/src/security_fence.rs kiana-core/src/authority.rs kiana-core/src/lib.rs kiana-core/tests/sc08_fence_guard.rs kiana-protocol/src/lib.rs .github/workflows/sc08-fence.yml docs/roadmap/security-fence-baseline.md docs/roadmap/security-compliance.md docs/roadmap/README.md docs/roadmap.md CURRENT_STATUS.md
  rg -n 'AuthorityFence|issue_authority_fence|validate_authority_fence|authority_fence_snapshot|validate_current|validate_successor|PolicyAuthorityEpoch(Rollback|Stale)|AuthSessionGenerationStale|PolicyConfigRevisionStale|UnknownFenceExpired|FACT_FENCE_MISMATCH' kiana-domain/src kiana-domain/tests kiana-core/src kiana-core/tests kiana-protocol/src docs/roadmap/security-fence-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain/core test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/sc08_fence.rs genesis/successor/digest-chain, epoch/session/policy/config drift, expiry and parent rollback fixtures; kiana-core/tests/sc08_fence_guard.rs source guard for EventLog authority reads and no effect path; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SC-08 job is queued by the push and is not awaited
status_change: SC-08 source slice is implemented. AuthorityFence now binds scope/session generation, authority epoch, policy/config digests, sequence/parent and short TTL; ControlPlane exposes read-only issue/validate/snapshot helpers backed by the existing authority stream. Stale/rollback/expired/fence-mismatch inputs return stable reason codes and do not fall back to prior snapshots or dispatch paths.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; fence issuance is not a durable permit or revocation event, current session generation/policy/config are caller-supplied snapshot inputs until SC-09/10/12 bind them atomically, authority facts remain JSONL/compatibility scoped, late observations and handler effect-time fence/reconcile are not globally wired, and external/live/physical safety is unproven
reviewer: Codex root implementation review plus SC-08 monotonic epoch/session/policy/config fence and no-resurrection reconciliation; no runtime test reviewer
```

### SC-09 GrantScope intersection evidence (2026-09-17)

```text
source_snapshot: 049aabc + SC-09 working-tree slice; kiana-policy/src/{grant_scope,lib}.rs; kiana-policy/tests/sc09_grant_scope.rs; kiana-domain/src/{scope,contracts,lib}.rs; kiana-core/tests/sc09_grant_scope_guard.rs; .github/workflows/sc09-grant-scope.yml; docs/roadmap/security-grant-scope-baseline.md; docs/roadmap/security-compliance.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: SC-09 strict GrantScope, ScopeSet multi-dimensional intersection/subset, capability/secret/external/delegation/expiry/epoch narrowing, legacy grant adapter, CI fixtures/source guard and roadmap/status overlays are scoped to this step; no Broker/handler/approval execution, second authorization path or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-policy/src/grant_scope.rs kiana-policy/src/lib.rs kiana-policy/tests/sc09_grant_scope.rs kiana-domain/src/scope.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-core/tests/sc09_grant_scope_guard.rs .github/workflows/sc09-grant-scope.yml docs/roadmap/security-grant-scope-baseline.md docs/roadmap/security-compliance.md docs/roadmap/README.md docs/roadmap.md CURRENT_STATUS.md
  rg -n 'GrantScope|intersect_all|capability_intersection_empty|principal_mismatch|project_mismatch|allow_secret|allow_external|delegation_allowed|ScopeSet::intersect|is_subset_of|grant_scope_.*(invalid|mismatch|empty)' kiana-policy/src kiana-policy/tests kiana-domain/src kiana-core/tests docs/roadmap/security-grant-scope-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; policy/core test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-policy/tests/sc09_grant_scope.rs parent/child intersection, no-union/cross-scope, capability/secret/external mixing, request path/risk/expiry and legacy adapter fixtures; kiana-core/tests/sc09_grant_scope_guard.rs source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SC-09 job is queued by the push and is not awaited
status_change: SC-09 source slice is implemented. GrantScope now derives child permissions only by intersection of ScopeSet dimensions and explicit capability/secret/external/delegation/expiry/epoch values, rejects cross-principal/project/epoch and empty/mixed grants, proves subset containment, and adapts legacy CapabilityGrant without widening. Policy exports the contract and no Broker/handler/second authorization loop was added.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; GrantScope is not yet the durable Cell/Grant/Approval authority, existing MemoryCellRegistry and permits retain compatibility paths, parent/template/department/project/packet/approval layers are not atomically persisted, effect-time TOCTOU/egress/SecretStore/revocation/recovery and external/live/physical proof remain SC-10+
reviewer: Codex root implementation review plus SC-09 intersection/no-union/capability-boundary reconciliation; no runtime test reviewer
```

### SC-10 approval binding and Human Inbox evidence (2026-09-17)

```text
source_snapshot: 141652c + SC-10 working-tree slice; kiana-core/src/{approval_binding,approvals,lib}.rs; kiana-core/tests/sc10_approval_binding.rs; kiana-core/tests/sc10_approval_guard.rs; kiana-domain/src/{contracts,security_reasons}.rs; kiana-policy/src/grant_scope.rs; .github/workflows/sc10-approval.yml; docs/roadmap/security-approval-baseline.md; docs/roadmap/security-compliance.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: SC-10 strict ApprovalBinding exact subject/digest/scope/epoch/policy/expiry contract, HumanInboxItem status and one-shot consume/self-approval guard, existing ApprovalStore source anchors, CI fixtures/source guard and roadmap/status overlays are scoped to this step; no Broker/handler/automatic retry, second approval loop or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-core/src/approval_binding.rs kiana-core/src/approvals.rs kiana-core/src/lib.rs kiana-core/tests/sc10_approval_binding.rs kiana-core/tests/sc10_approval_guard.rs kiana-domain/src/contracts.rs kiana-domain/src/security_reasons.rs kiana-policy/src/grant_scope.rs .github/workflows/sc10-approval.yml docs/roadmap/security-approval-baseline.md docs/roadmap/security-compliance.md docs/roadmap/README.md docs/roadmap.md CURRENT_STATUS.md
  rg -n 'ApprovalBinding|HumanInboxItem|target_digest|payload_digest|scope_digest|PolicySelfApprovalForbidden|PolicyApprovalBindingMismatch|PolicyApprovalExpired|decide_approval_with_proof|validate_with_proof|prepare_capability_action|consume' kiana-core/src kiana-core/tests kiana-domain/src kiana-policy/src docs/roadmap/security-approval-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; core test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/sc10_approval_binding.rs exact digest/subject drift, strict serde, self-approval, one-shot consume, denial/expiry fixtures; kiana-core/tests/sc10_approval_guard.rs source guard for existing ApprovalStore/ControlPlane path; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SC-10 job is queued by the push and is not awaited
status_change: SC-10 source slice is implemented. ApprovalBinding now carries only typed principal/session/project/Grant references and exact command/target/payload/scope/policy/authority/expiry digests; request drift, cross-context mismatch and expiry cannot reuse an approval. HumanInboxItem enforces independent approver, explicit Pending→Approved/Denied→Consumed status and one-shot terminal semantics; malformed/raw fields fail closed. Existing ApprovalStore prepare/validate/decide CAS remains the execution authority, and no new dispatch path was added.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; ApprovalBinding/HumanInboxItem are not yet wired as the durable cross-entrypoint inbox projector or atomically persisted with every legacy approval record, external human identity and GrantScope/authority/permit CAS remain partial, reason fields remain compatibility diagnostics, and Secret/redaction/TOCTOU/recovery/external/live/physical proof remains SC-11+
reviewer: Codex root implementation review plus SC-10 exact approval subject/self-approval/one-shot/replay boundary reconciliation; no runtime test reviewer
```

### SC-11 entrypoint and adapter parity evidence (2026-09-17)

```text
source_snapshot: 9411160 + SC-11 working-tree slice; kiana-core/src/{entrypoint_parity,parity,lib}.rs; kiana-core/tests/sc11_entrypoint_parity.rs; kiana-core/tests/sc11_entrypoint_guard.rs; kiana-domain/src/{parity,contracts,lib}.rs; kiana-protocol/src/lib.rs; kiana-daemon/src/lib.rs; .github/workflows/sc11-entrypoint-parity.yml; docs/roadmap/security-entrypoint-baseline.md; docs/roadmap/security-compliance.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: SC-11 strict EntrypointCommand/EntrypointParityMatrix, expanded entrypoint labels, optional RequestMetadata label, fixed DaemonHost→ControlPlane route, denied-handler zero-effect invariant, CI fixtures/source guards and roadmap/status overlays are scoped to this step; no local authority/second execution path or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-core/src/entrypoint_parity.rs kiana-core/src/parity.rs kiana-core/src/lib.rs kiana-core/tests/sc11_entrypoint_parity.rs kiana-core/tests/sc11_entrypoint_guard.rs kiana-domain/src/parity.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-protocol/src/lib.rs kiana-daemon/src/lib.rs .github/workflows/sc11-entrypoint-parity.yml docs/roadmap/security-entrypoint-baseline.md docs/roadmap/security-compliance.md docs/roadmap/README.md docs/roadmap.md CURRENT_STATUS.md
  rg -n 'EntrypointCommand|EntrypointParityMatrix|ENTRYPOINT_ROUTE|daemonhost\.controlplane|EntrypointDecision|entrypoint_parity_denied_handler_effect|entrypoint|SecurityContext|DaemonHost|ControlPlane' kiana-core/src kiana-core/tests kiana-domain/src kiana-protocol/src kiana-daemon/src docs/roadmap/security-entrypoint-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; core test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/sc11_entrypoint_parity.rs multi-entry command digest/route, deny zero-effect, unknown/alternate-route/digest mismatch fixtures; kiana-core/tests/sc11_entrypoint_guard.rs source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SC-11 job is queued by the push and is not awaited
status_change: SC-11 source slice is implemented. EntrypointCommand and EntrypointParityMatrix now normalize CLI/TTY/Web/Workbench/Desktop/scheduler/swarm/connector requests to one fixed DaemonHost→ControlPlane route with common request/arguments/context digests; Denied handler_calls cannot be nonzero, alternate routes and mismatched command identities fail closed. RequestMetadata entrypoint is optional correlation only; existing EventLog parity projection remains the authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; historical adapters do not all yet populate the optional label or matrix, transport/session/HumanInbox/receipt/redaction/unknown parity is not durable, scheduler/swarm/connector effect routes still require later integration, and external/live/physical safety remains unproven
reviewer: Codex root implementation review plus SC-11 single-route/zero-effect/entrypoint-label parity reconciliation; no runtime test reviewer
```

### CP-07 JSONL transaction frame and recovery reader evidence (2026-09-17)

```text
source_snapshot: ed226ee + CP-07 working-tree slice; kiana-domain/src/journal.rs; kiana-domain/tests/cp07_journal_frame.rs; kiana-eventlog/src/{jsonl,journal_core}.rs; kiana-core/src/dispatch.rs; kiana-core/tests/cp07_journal_guard.rs; .github/workflows/cp07-journal.yml; docs/roadmap/control-plane-journal-baseline.md; docs/roadmap/control-plane.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: CP-07 complete JournalFrame body/receipt/read-set/ID/sequence validation, logical event expansion, existing JsonlEventLog checksum/header/version/sync/repair/O_NOFOLLOW writer boundary, CI fixtures/source guard and roadmap/status overlays are scoped to this step; no second EventLog/store, core disk backend, automatic retry or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/journal.rs kiana-domain/tests/cp07_journal_frame.rs kiana-eventlog/src/jsonl.rs kiana-eventlog/src/journal_core.rs kiana-core/src/dispatch.rs kiana-core/tests/cp07_journal_guard.rs .github/workflows/cp07-journal.yml docs/roadmap/control-plane-journal-baseline.md docs/roadmap/control-plane.md docs/roadmap/README.md docs/roadmap.md CURRENT_STATUS.md
  rg -n 'JournalFrame|JournalFramePayload|body_sha256|logical_events|journal_frame_integrity_failed|eventlog_legacy_writer_after_upgrade|eventlog_repair_failed|eventlog_recovery_sync_failed|commit_transition|commit_confirmed|sync_all|O_NOFOLLOW' kiana-domain/src kiana-domain/tests kiana-eventlog/src kiana-core/src kiana-core/tests docs/roadmap/control-plane-journal-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain/core test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp07_journal_frame.rs complete transition/event frame, receipt/read-set, checksum, strict unknown/nil event and legacy sequence fixtures; kiana-core/tests/cp07_journal_guard.rs source guard; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-07 job is queued by the push and is not awaited
status_change: CP-07 source slice is implemented. JournalFrame now validates complete transition receipts/read sets and legacy event identity before logical expansion; JsonlEventLog continues the single-writer complete-frame/sync/repair/version boundary and rejects legacy writers after v2 upgrade. Committed/replayed/conflict/unknown outcomes remain explicit and Core still gates dispatch through commit_confirmed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; actual power-loss durability and multi-process crash/recovery are not proven, legacy compatibility remains reader-scoped, EventStore capabilities vary by adapter, Unknown commit responses still require explicit read_command/reconciliation, and CP-08+ / ER/PD/DEP must bind authority/projectors/backup/retention before any durable claim
reviewer: Codex root implementation review plus CP-07 complete-frame/receipt/read-set/recovery-boundary reconciliation; no runtime test reviewer
```

### CP-08 Grant authority and revocation epoch evidence (2026-09-17)

```text
source_snapshot: b4da8aa + CP-08 working-tree slice; kiana-domain/src/{grant_authority,authority,scope,ids,contracts,lib}.rs; kiana-domain/tests/cp08_grant_ledger.rs; kiana-core/src/{authority,cell_registry}.rs; kiana-core/tests/cp08_grant_guard.rs; kiana-protocol/src/lib.rs; .github/workflows/cp08-grant.yml; docs/roadmap/control-plane-grant-baseline.md; docs/roadmap/control-plane.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: CP-08 strict GrantAuthorityEnvelope/GrantLedgerSnapshot/GrantLedger root-child binding, ScopeSet subset, sequence/revision/epoch/revocation checks, snapshot restore, existing authority/cell source guard and roadmap/status overlays are scoped to this step; no durable GrantStore/permit consumer, second authority source/execution path or unrelated WIP was reverted; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/grant_authority.rs kiana-domain/src/authority.rs kiana-domain/src/scope.rs kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-domain/tests/cp08_grant_ledger.rs kiana-core/src/authority.rs kiana-core/src/cell_registry.rs kiana-core/tests/cp08_grant_guard.rs kiana-protocol/src/lib.rs .github/workflows/cp08-grant.yml docs/roadmap/control-plane-grant-baseline.md docs/roadmap/control-plane.md docs/roadmap/README.md docs/roadmap.md CURRENT_STATUS.md
  rg -n 'GrantAuthorityEnvelope|GrantLedger(Snapshot)?|root_run|parent_grant_id|grant_ledger_child_scope_widened|active_grant|GrantAuthorityStatus::Revoked|AuthorityLedger::rebuild|authority_epoch|CellRegistryPort' kiana-domain/src kiana-domain/tests kiana-core/src kiana-core/tests kiana-protocol/src docs/roadmap/control-plane-grant-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain/core test targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp08_grant_ledger.rs root/child subset, duplicate/gap/foreign/widening, ancestor revoke and snapshot restore fixtures; kiana-core/tests/cp08_grant_guard.rs source guard for authority/cell boundaries; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-08 job is queued by the push and is not awaited
status_change: CP-08 source slice is implemented. GrantAuthorityEnvelope binds root/child principal/project/issuer/scope/expiry/revision/authority epoch; GrantLedger accepts only sequential unique additions whose child scope is a subset of an existing parent, and ancestor revocation fences descendants at current epoch. Snapshot validation/restore preserves canonical order and revoked history without resurrecting active authority; existing AuthorityLedger remains the EventLog-backed authority source.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; GrantLedger is an in-memory pure reducer not yet atomically persisted with all Cell/Approval/Permit transitions, authority/assignment revocation and cross-process recovery remain partial, ScopeSet does not prove OS/TOCTOU/egress or SecretStore safety, and external/live/physical effect proof is absent
reviewer: Codex root implementation review plus CP-08 root/child grant subset/revocation/epoch/recovery reconciliation; no runtime test reviewer
```

### CP-09 approval subject and execution material evidence (2026-09-17)

```text
source_snapshot: 15faaa1 + CP-09 working-tree slice; kiana-domain/src/{approval_journal,contracts,lib}.rs; kiana-daemon/src/journal_approvals.rs; kiana-protocol/src/lib.rs; kiana-core/src/{approvals,approval_binding}.rs; kiana-domain/tests/cp09_approval_material.rs; kiana-core/tests/cp09_approval_guard.rs; .github/workflows/cp09-approval-material.yml; docs/roadmap/control-plane-approval-material-baseline.md; docs/roadmap/control-plane.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: CP-09 strict ApprovalExecutionMaterial/ApprovalMaterialState binds exact payload and redacted preview digests with expiry/state; JournalApprovalStore stages only a redacted challenge plus digest-only material, keeps unrecoverable payload in a bounded volatile cache, removes it on failed commit, and rechecks material/request/authority before activation, decision, and consumption; protocol re-export and roadmap/status evidence are scoped to this step; static verification is complete and commit/push are pending
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp09_approval_material.rs digest/strict/redaction fixtures; kiana-core/tests/cp09_approval_guard.rs preview/volatile/authority/continuation source guard; GitHub Actions CP-09 workflow runs the fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-09 is triggered by the eventual push and is not awaited
status_change: CP-09 source slice is implemented. Approval subject retains final capability request/action, caller/session/project/role binding, policy version, target/path scope, expiry and independent random nonce; preview is display-only, secret/raw material is never serialized into the staged journal event, and lost volatile material fails closed instead of replaying a redacted preview.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: volatile payload remains process-local (not a durable SecretStore/protected archive), old approval records without material require explicit reauthorization, Human Inbox is still the existing approve/deny projection, standing/input/cancel decisions remain unsupported, and durable pending/decision/consume CAS, Broker effect, cross-process recovery, external/live/physical proof remain CP-10+ work
reviewer: Codex root implementation review plus CP-09 subject/material/redaction/replay reconciliation; no runtime test reviewer
```

### CP-10 approval facts and single-consumption evidence (2026-09-17)

```text
source_snapshot: edd745a + CP-10 working-tree slice; kiana-domain/src/{approval_journal,capabilities,contracts,lib}.rs; kiana-ports/src/lib.rs; kiana-protocol/src/lib.rs; kiana-client/src/lib.rs; kiana-daemon/src/{journal_approvals,lib}.rs; kiana-core/src/{approvals,recovery,platform}.rs; kiana-domain/tests/cp10_approval_facts.rs; kiana-core/tests/cp10_approval_guard.rs; .github/workflows/cp10-approval-facts.yml; docs/roadmap/control-plane-approval-facts-baseline.md; docs/roadmap/control-plane.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: CP-10 strict ApprovalDecisionFact/ApprovalConsumptionFact are embedded in the existing EventLog approval aggregate; decision facts bind subject request/hash, actor, command, authority version, expiry and expected aggregate version, while consumption facts separately bind the Approved→Consumed dispatch command; Core and protocol expose optional expected-version OCC and server pending views; no second approval store or execution path was added; static verification is complete and commit/push are pending
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp10_approval_facts.rs strict fact/digest/version fixtures; kiana-core/tests/cp10_approval_guard.rs journal-CAS/replay/expected-version/standing-rule source guard; GitHub Actions CP-10 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-10 is triggered by the eventual push and is not awaited
status_change: CP-10 source slice is implemented. Approve/deny transitions now carry validated immutable decision facts and command identities; stale expected versions fail before a new decision, one CAS winner is retained under contention, retries replay the recorded decision, and Approved remains distinct from the later dispatch-time Consumed fact. Pending views preserve server-owned approve/deny decisions and may carry the current journal version.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: old MemoryApprovalStore remains compatibility/test migration input and does not receive the new fact contract, external approver identity/self-approval policy is still bounded by existing local ingress, standing/input/cancel scopes remain unsupported, CP-13 still owns the full permit/budget/lease atomicity, and cross-process crash/recovery, SecretStore, Broker effect and external/live/physical proof remain open
reviewer: Codex root implementation review plus CP-10 fact/CAS/idempotency/single-consumption reconciliation; no runtime test reviewer
```

### CP-11 unified model/tool budget evidence (2026-09-17)

```text
source_snapshot: 8572b0d + CP-11 working-tree slice; kiana-domain/src/{budget_contracts,work_packets,lib,contracts}.rs; kiana-core/src/{cell_registry,model_budget}.rs; kiana-ports/src/lib.rs; kiana-runner/src/harness.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/cp11_budget_contracts.rs; kiana-core/tests/cp11_budget_guard.rs; .github/workflows/cp11-budget-contracts.yml; docs/roadmap/control-plane-budget-baseline.md; docs/roadmap/control-plane.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: CP-11 adds strict BudgetScope/ReservationFact/SettlementFact contracts and shared BudgetLease model-call accounting; CellRegistry and JournalModelBudget changes remain on the existing control-plane/eventlog spine, with no second budget ledger or execution loop; static verification is complete and commit/push are pending
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp11_budget_contracts.rs shared model/tool lease, scope and known/unknown settlement fixtures; kiana-core/tests/cp11_budget_guard.rs source guard for reserve-before-provider, execution idempotency and conservative unknown usage; GitHub Actions CP-11 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-11 is triggered by the eventual push and is not awaited
status_change: CP-11 source slice is implemented. BudgetLease now accounts model-call count and token usage in the same ledger as tool/effect consumption; strict reservation/settlement facts bind Run/Execution/Request/Lease, five scope classes, parent linkage, authority epoch, expected version and expiry; JournalModelBudget embeds and validates these facts and rejects duplicate settlements while CellRegistry uses the shared model consumption method.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: Project/Run/Turn/parent-child budgets are not yet one durable cross-process projector, model/tool sibling reservations remain adapter-local, provider tokenizer/pricing and refunds are not claimed, old snapshots use compatibility defaults, and lease/fence/permit atomicity, Secret/egress, crash recovery and external/live/physical proof remain CP-12+ / CP-13/14 work
reviewer: Codex root implementation review plus CP-11 model/tool reservation, hierarchy, overflow and unknown-usage reconciliation; no runtime test reviewer
```

### CP-12 resource lease and fencing evidence (2026-09-17)

```text
source_snapshot: 99b1c09 + CP-12 working-tree slice; kiana-domain/src/{resource_leases,paths,contracts,lib}.rs; kiana-ports/src/lib.rs; kiana-core/src/{sessions,resource_leases,cell_registry,lib}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/cp12_resource_lease.rs; kiana-core/tests/cp12_resource_guard.rs; .github/workflows/cp12-resource-leases.yml; docs/roadmap/control-plane-resource-lease-baseline.md; docs/roadmap/control-plane.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: CP-12 strict ResourceLease binds canonical resource/owner/fence/epoch/sequence/TTL/digests; path-lock acquisition now rejects invalid write-set entries before the existing kernel O_NOFOLLOW+LOCK_NB boundary; CapabilityLease carries a server-generated fencing token and Core exposes authority-backed issue/validate helpers; no second lock/execute path was added; static verification is complete and commit/push are pending
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp12_resource_lease.rs owner/coverage/successor/old-token/canonical-path/tamper fixtures; kiana-core/tests/cp12_resource_guard.rs kernel-lock/O_NOFOLLOW/authority/fencing source guard; GitHub Actions CP-12 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-12 is triggered by the eventual push and is not awaited
status_change: CP-12 source slice is implemented. Resource leases now fail closed on expired/stale epoch/reused token/owner mismatch and carry a monotonic successor chain; canonical_resource_set no longer drops malformed paths, and Cell capability settlement requires the exact generated fencing token.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: ResourceLease is an immutable observation and does not itself hold an OS lock; path locks remain adapter/process scoped, approval waits still retain existing Cell resources, durable lease/projector and cross-process stale-writer recovery are open, effect-time Patch/MCP/permit integration and cancellation remain CP-13+/CP-16, and Secret/egress/external/live/physical proof is absent
reviewer: Codex root implementation review plus CP-12 canonical write-set, kernel lock, owner/epoch/token and successor reconciliation; no runtime test reviewer
```

### CP-13 execution permit and dispatch barrier evidence (2026-09-17)

```text
source_snapshot: 90c2ae1 + CP-13 working-tree slice; kiana-domain/src/{dispatch,contracts,lib}.rs; kiana-core/src/{dispatch,capabilities}.rs; kiana-ports/src/lib.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/cp13_dispatch_permit.rs; kiana-core/tests/cp13_dispatch_guard.rs; .github/workflows/cp13-dispatch-permit.yml; docs/roadmap/control-plane-dispatch-permit-baseline.md; docs/roadmap/control-plane.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: CP-13 strict DispatchPermit now carries schema/version/digest and exact action/project/authority bindings; ControlPlane prepares it only after current read-set and cancellation/policy checks, JournalPermitVerifier validates the opaque permit and CASes invocation.dispatching before the existing invocation.executing handler boundary; no second broker or execution loop was added; static verification is complete and commit/push are pending
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp13_dispatch_permit.rs exact action/expiry/digest/read-set fixtures; kiana-core/tests/cp13_dispatch_guard.rs committed-permit/cancel/Broker barrier source guard; GitHub Actions CP-13 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-13 is triggered by the eventual push and is not awaited
status_change: CP-13 source slice is implemented. Forged authorization strings cannot mint a permit; changed action/project identity, expiry, duplicate authority read-set, missing permit and replayed execution streams fail closed, and only a committed execution.prepared transition can reach invocation.dispatching and the Broker's permit verifier.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: full budget/lease/fence/cancel/result transaction remains split across CP-11/12 and CP-14+/16, external services cannot receive an exactly-once claim, cross-process crash/recovery and stale worker fencing remain open, and Secret/egress/external/live/physical proof is absent
reviewer: Codex root implementation review plus CP-13 permit digest/action/read-set/dispatch-barrier reconciliation; no runtime test reviewer
```

### CP-14 result receipt and reconciliation evidence (2026-09-17)

```text
source_snapshot: 8ea0684 + CP-14 working-tree slice; kiana-domain/src/{capabilities,contracts,lib}.rs; kiana-core/src/{capabilities,dispatch,events}.rs; kiana-ports/src/lib.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/cp14_result_receipt.rs; kiana-core/tests/cp14_result_guard.rs; .github/workflows/cp14-result-receipt.yml; docs/roadmap/control-plane-result-receipt-baseline.md; docs/roadmap/control-plane.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: CP-14 strict digest-only CapabilityResultReceipt now fixes request/execution/invocation/attempt, success, process/effect/stop, effect/unknown/zero-effect/fenced and committed dimensions; direct/Harness/approval finalizer and execution.result_committed include the receipt, while existing committed-only delivery and Unknown settlement paths remain the single spine; static verification is complete and commit/push are pending
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp14_result_receipt.rs success/unknown/digest/flag contradiction fixtures; kiana-core/tests/cp14_result_guard.rs finalizer/result persistence/settlement/delivery source guard; GitHub Actions CP-14 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-14 is triggered by the eventual push and is not awaited
status_change: CP-14 source slice is implemented. Normalized results cannot claim success with unknown effect or conflicting started/zero-effect dimensions; receipt persistence/Cell settlement failures remain ResultUnknown, and runner delivery is still gated by committed result facts and a delivery CAS.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: receipt digest is not provider/external effect confirmation, budget/lease reconciliation remains split until later transactions, cancellation/handler stop and cross-process recovery are incomplete, and Secret/egress/external/live/physical proof is absent
reviewer: Codex root implementation review plus CP-14 result dimensions, redaction, unknown and delivery-order reconciliation; no runtime test reviewer
```

### CP-15 cancellation state and stop barrier evidence (2026-09-17)

```text
source_snapshot: 640bc15 + CP-15 working-tree slice; kiana-domain/src/{cancellation,states,event_contracts,contracts,lib}.rs; kiana-core/src/{lifecycle,events,approvals,dispatch,sessions}.rs; kiana-runner-protocol/src/lib.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/cp15_cancellation.rs; kiana-core/tests/cp15_cancellation_guard.rs; .github/workflows/cp15-cancellation.yml; docs/roadmap/control-plane-cancellation-baseline.md; docs/roadmap/control-plane.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: strict RunCancellationFact now separates durable cancellation intent from the existing in-process watch signal, binds a run.cancel command digest and canonical Run/Invocation targets, and requires stop confirmation for Cancelled while retaining ResultUnknown for unconfirmed stop; cancel_run persists run.cancelling through the same EventLog transition/CAS boundary before signal/Runner Cancel, closes pending approvals with not_executed synthetic results, and terminal events rebuild/validate the nested cancellation fact; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cp15_cancellation.rs stopping/cancelled/unknown/target-order/digest/strict-field fixtures; kiana-core/tests/cp15_cancellation_guard.rs persist-before-signal, command conflict, terminal guard, approval/queue and no-second-loop source guard; GitHub Actions CP-15 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CP-15 is triggered by the eventual push and is not awaited
status_change: CP-15 source slice is implemented. Cancellation intent is durable before process signalling; replayed same command/digest is accepted, changed cancellation payload conflicts, queued approvals receive explicit not-executed results, and only confirmed stop may reach run.cancelled; unconfirmed/inconsistent stop remains run.result_unknown.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: the existing bounded watch is not typed handler StopReport or physical stop evidence; Shell/Patch/MCP descendant/file/remote effect boundaries, physical lock release, cross-process recovery, revocation propagation and Unknown reconciliation remain CP-16/17/19/20 and ER/PD/SC work, and Secret/egress/external/live/physical proof is absent
reviewer: Codex root implementation review plus CP-15 cancellation state, command/CAS barrier, queue/approval terminalization and result-unknown reconciliation; no runtime test reviewer
```

### ER-05 JSONL v2 frame, lock and corruption evidence (2026-09-17)

```text
source_snapshot: 38bba4a + ER-05 working-tree slice; kiana-domain/src/{journal,contracts,lib}.rs; kiana-eventlog/src/{jsonl,journal_core,event_store_core}.rs; kiana-eventlog/tests/er05_jsonl_v2.rs; kiana-eventlog/JOURNAL.md; .github/workflows/er05-jsonl.yml; docs/roadmap/event-receipt-jsonl-baseline.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: JournalHeader now exposes strict schema/writer validation and JournalFrame rejects over-sized or malformed body length/digest before logical event exposure; JsonlEventLog keeps required v2 header upgrade, complete transition frames, whole-commit cursor pages, Unix flock/dirfd/O_NOFOLLOW/identity/sync barriers, legacy-after-v2 refusal, and bounded torn-tail repair versus malformed/checksum failure; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-eventlog/tests/er05_jsonl_v2.rs atomic multi-event page/reopen, malformed first line, checksum tamper, torn-tail repair, legacy-after-v2 refusal and source lock/sync guard; GitHub Actions ER-05 workflow runs integration fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-05 is triggered by the eventual push and is not awaited
status_change: ER-05 source slice is implemented. Required v2 writer headers and checksummed bounded frames are the only post-upgrade writer format; readers publish only complete validated frames, cursors stop at transaction boundaries, known torn tails repair narrowly, and malformed/unknown/checksum/identity/sync failures remain fail-closed or Unknown.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: CI-only fixtures have not been executed locally; power-loss, NFS/cross-host locking, async worker/backpressure/shutdown ack, projector/checkpoint, backup/retention, and external/live/physical effect proof remain ER-06+ / PD/DEP/SC work
reviewer: Codex root implementation review plus ER-05 frame integrity, writer upgrade, lock/no-follow, sync/identity and corruption classification; no runtime test reviewer
```

### ER-06 async writer, backpressure and shutdown acknowledgement evidence (2026-09-17)

```text
source_snapshot: ee54b33 + ER-06 working-tree slice; kiana-domain/src/{journal,contracts,lib}.rs; kiana-ports/src/lib.rs; kiana-eventlog/src/{jsonl,stream}.rs; kiana-core/src/receipts.rs; kiana-daemon/src/{lib,run_stream}.rs; kiana-eventlog/tests/er06_async_lifecycle.rs; .github/workflows/er06-async-lifecycle.yml; docs/roadmap/event-receipt-async-baseline.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: EventStorePort now exposes explicit flush/health/last_durable_cursor/close acknowledgements and strict EventStoreHealth; JsonlEventLog keeps bounded spawn_blocking admission, rejects queue/worker/closed calls, serializes close-in-progress state, and confirms file/parent sync before returning a cursor/close ack; ControlPlane/DaemonHost and stream wrappers delegate the same contract without adding authority; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-eventlog/tests/er06_async_lifecycle.rs flush/health/cursor/close acknowledgement and closed rejection fixtures plus bounded worker/lifecycle source guard; GitHub Actions ER-06 workflow runs integration fixture and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-06 is triggered by the eventual push and is not awaited
status_change: ER-06 source slice is implemented. Async file work remains bounded by a finite worker semaphore, lifecycle acknowledgements are structured/digest-bound, durable cursor is reported only after the adapter sync boundary, and close rejects subsequent work instead of treating task cancellation as success.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: CI-only fixtures have not been executed locally; slow-disk/worker-panic/terminal-shutdown integration, cross-host/power-loss durability, projector/checkpoint, backup/retention and external/live/physical effect proof remain ER-07+ / PD/DEP/SC work
reviewer: Codex root implementation review plus ER-06 bounded worker, health digest, durable cursor, close state and daemon delegation reconciliation; no runtime test reviewer
```

### ER-07 replay projector and checkpoint evidence (2026-09-17)

```text
source_snapshot: 0e6f9b0 + ER-07 working-tree slice; kiana-domain/src/{projection_contracts,contracts,lib}.rs; kiana-core/src/{projection_checkpoint,lib}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/er07_projection_checkpoint.rs; kiana-core/tests/er07_replay_guard.rs; .github/workflows/er07-replay-checkpoint.yml; docs/roadmap/event-receipt-replay-baseline.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: strict ProjectionCheckpoint binds projector/version/source cursor/sorted event IDs/state digest/checkpoint digest; ReplayProjection provides pure from-zero and validated checkpoint+tail folds, skips duplicate event IDs, rejects cursor/projector/fold mismatch, and has no EventStore/Broker/authorization capability; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/er07_projection_checkpoint.rs strict/tamper/version/empty-anchor fixtures; kiana-core/tests/er07_replay_guard.rs from-zero/checkpoint+tail duplicate-id equivalence, mismatch/fold failure and read-only source guard; GitHub Actions ER-07 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-07 is triggered by the eventual push and is not awaited
status_change: ER-07 source slice is implemented. Checkpoints are replaceable read optimizations: invalid or stale checkpoints fail closed and callers can rebuild from zero; duplicate source events do not reapply state, and projection failure cannot mutate authority or issue execution.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: CI-only fixtures have not been executed locally; no production projector runner/checkpoint store or cross-process checkpoint transaction exists, and Run/Turn/Invocation/Approval projection recovery, backup/retention and external/live/physical proof remain ER-08+ / PD work
reviewer: Codex root implementation review plus ER-07 checkpoint digest/cursor compatibility, duplicate replay, fold failure and no-authority boundary; no runtime test reviewer
```

### ER-08 Run/Turn projection and terminal evidence (2026-09-17)

```text
source_snapshot: 47f4bce + ER-08 working-tree slice; kiana-core/src/{projection,lifecycle}.rs; kiana-domain/src/states.rs; kiana-core/tests/er08_run_projection.rs; kiana-core/tests/er08_projection_guard.rs; .github/workflows/er08-run-projection.yml; docs/roadmap/event-receipt-run-projection-baseline.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: existing project_run_state reducer is now covered by dedicated CI fixtures/source guard: same-stream versions or durable order are folded with event-ID deduplication, approval/cancelling phases are explicit, a new run.prompt is the only turn reset, same terminal replay is idempotent, conflicting terminal kinds fail closed, and events after a terminal are ignored until a new turn; projection remains read-only with no Broker/Runner/authorization path; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/er08_run_projection.rs approval/cancelling/terminal/idempotent/conflicting/late-effect/new-turn fixtures; kiana-core/tests/er08_projection_guard.rs terminal reducer/order/no-authority source guard; GitHub Actions ER-08 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-08 is triggered by the eventual push and is not awaited
status_change: ER-08 source slice is implemented. RunState is derived only from committed facts, terminal conflicts never become a success, and a late event cannot resurrect a completed/cancelled/failed/unknown turn without a new prompt turn fact.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: CI-only fixtures have not been executed locally; durable projector runner/checkpoint persistence, Invocation/Execution/Attempt and Approval/Budget/Lease projection, snapshot/restart/resume, and external/live/physical proof remain ER-09+ / CP/PD work
reviewer: Codex root implementation review plus ER-08 phase ordering, terminal conflict/idempotence, late-event suppression and no-authority boundary; no runtime test reviewer
```

### ER-09 Invocation/Execution/Attempt projection evidence (2026-09-17)

```text
source_snapshot: d79d59c + ER-09 working-tree slice; kiana-core/src/{invocation_projection,capability_attempt_projection}.rs; kiana-domain/src/{states,observability}.rs; kiana-core/tests/er09_invocation_projection.rs; .github/workflows/er09-invocation-projection.yml; docs/roadmap/event-receipt-invocation-baseline.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: InvocationProjection and CapabilityAttemptRecord reducers fold committed request/approval/permit/dispatch/executing/result/cancel facts with typed request/turn/invocation/execution/attempt identity, action digest, effect_known, stop_confirmed, fenced, source IDs and terminal signatures; dispatch without a result remains Unknown, foreign/digest/multiple terminal conflicts fail closed, duplicate event IDs do not reapply, and retry attempts remain separate; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/er09_invocation_projection.rs dispatch-without-result, terminal/digest conflict, unknown stop/fencing and retry-attempt fixtures; source guard fixes reducer markers and no Broker/Runner/retry boundary; GitHub Actions ER-09 workflow runs fixture and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-09 is triggered by the eventual push and is not awaited
status_change: ER-09 source slice is implemented. Replayed or incomplete execution facts cannot become success, attempt identity drift is visible as conflict, unknown effect stays fenced, and terminal results are never merged across different signatures.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: CI-only fixtures have not been executed locally; external/provider receipts and handler stop/effect observation, durable projection persistence/checkpoints, restart/recovery and controlled Unknown retry remain ER-10+/ER-14/CP-20/PD work, with no external/live/physical proof
reviewer: Codex root implementation review plus ER-09 attempt identity/digest, terminal precedence, Unknown/fence and no-execution projection boundary; no runtime test reviewer
```

### ER-10 approval/budget/lease/cell recovery projection evidence (2026-09-17)

```text
source_snapshot: 6bc30f3 + ER-10 working-tree slice; kiana-domain/src/{recovery_resources,budget_contracts,resource_leases,states,contracts,lib}.rs; kiana-core/src/{resource_projection,recovery,cell_registry,lib}.rs; kiana-daemon/src/journal_approvals.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/er10_recovery_resources.rs; kiana-core/tests/er10_resource_projection.rs; .github/workflows/er10-recovery-resources.yml; docs/roadmap/event-receipt-resource-projection-baseline.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: strict RecoveryResourceSnapshot binds source cursor/event IDs and bounded pending approval, reserved budget, active/fenced lease and Cell IDs; project_recovery_resources folds only committed EventLog facts with approval state transitions, typed BudgetReservation/Settlement linkage, lease lifecycle and Cell lifecycle, skips duplicate event IDs, distinguishes empty/unsupported sources, and never consumes authority or executes work; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/er10_recovery_resources.rs snapshot strict/tamper fixtures; kiana-core/tests/er10_resource_projection.rs pending approval, budget settlement/orphan, lease/cell lifecycle, duplicate/empty source and read-only/cache-miss guards; GitHub Actions ER-10 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-10 is triggered by the eventual push and is not awaited
status_change: ER-10 source slice is implemented. Restart-facing pending/resource state is derived from facts rather than memory maps; staged/expired/settled/released entries are not exposed as executable pending state, orphan settlement and malformed transitions fail closed, and unknown source support is never treated as an empty ledger.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: CI-only fixtures have not been executed locally; the reducer is a read-only source projection and does not persist projector checkpoints, rebuild OS locks, replace durable Cell/Lease stores, perform atomic multi-resource release/consume, or prove cross-process/power-loss/external/live/physical behavior; those remain ER-11+ / CP/PD/DEP work
reviewer: Codex root implementation review plus ER-10 pending approval, budget linkage, lease/cell lifecycle, duplicate/empty source and no-authority boundary; no runtime test reviewer
```

### ER-11 receipt DTO and redacted view evidence (2026-09-17)

```text
source_snapshot: b3280dc + ER-11 working-tree slice; kiana-domain/src/{receipt_contracts,contracts,lib}.rs; kiana-core/src/{receipts,capability_attempt_projection}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/er11_receipt_contracts.rs; kiana-core/tests/er11_receipt_guard.rs; .github/workflows/er11-receipt-dto.yml; docs/roadmap/event-receipt-receipt-dto-baseline.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: strict RunReceipt and ExecutionReceipt now bind schema/version, owner/project/status, source cursor/event IDs, redaction profile, feature/proof levels, result/receipt digests and fenced effect dimensions; compatibility receipt_from_events keeps the legacy run-result view while attaching typed receipts built only from persisted events and attempt projections, with projection failures surfaced as result_unknown/error markers and no raw prompt/args/secret/output; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/er11_receipt_contracts.rs strict owner/source/redaction, unknown effect/fence and unknown/tamper/version fixtures; kiana-core/tests/er11_receipt_guard.rs typed integration/redaction/projection-error and no raw/authority source guard; GitHub Actions ER-11 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-11 is triggered by the eventual push and is not awaited
status_change: ER-11 source slice is implemented. Receipt status is derived from committed run facts, missing/unknown results remain visible, owner/scope and source provenance are explicit, and the typed projection cannot issue permission or claim an external effect.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: CI-only fixtures have not been executed locally; cost/files/evidence aggregation, provider/external receipts, artifact read failure, durable projector/checkpoint/restart, and multi-entrypoint parity remain ER-12+ / PD/CP work, with no external/live/physical proof
reviewer: Codex root implementation review plus ER-11 strict receipt schema, owner/source/redaction/proof fields, Unknown/fence and compatibility projection boundaries; no runtime test reviewer
```

### ER-12 receipt aggregation and evidence evidence (2026-09-17)

```text
source_snapshot: 047c6d6 + ER-12 working-tree slice; kiana-domain/src/{receipt_aggregation,receipt_contracts,contracts,lib}.rs; kiana-core/src/receipts.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/er12_receipt_aggregation.rs; kiana-core/tests/er12_receipt_aggregation.rs; .github/workflows/er12-receipt-aggregation.yml; docs/roadmap/event-receipt-aggregation-baseline.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: strict ReceiptAggregation now binds source cursor/event IDs, model turns, committed executions, known/unknown usage tokens, normalized files, memory hits, hashed evidence/provider refs, verification and cost-estimate separation; aggregate_receipt_facts reads only run-scoped persisted facts, deduplicates event IDs, marks usage/effect/commit uncertainty, and compatibility receipts include the aggregate without settling budget or exposing raw refs; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/er12_receipt_aggregation.rs strict estimate/path/ref/digest fixtures; kiana-core/tests/er12_receipt_aggregation.rs committed token/file/memory/evidence aggregation, unknown usage/effect, empty/foreign source and no-settlement source guard; GitHub Actions ER-12 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-12 is triggered by the eventual push and is not awaited
status_change: ER-12 source slice is implemented. Uncommitted/invalid files and incomplete usage do not become cost/file success, effect uncertainty remains Unknown, refs are digest-only, and no provider estimate is used for budget settlement.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: CI-only fixtures have not been executed locally; provider rate cards/billing, ArtifactStore provenance/retention/revoke, changeset durability, result delivery/recovery and external/live/physical proof remain ER-13+ / PD/INT/BQ work
reviewer: Codex root implementation review plus ER-12 committed-fact filtering, usage uncertainty, path/ref redaction, cost separation and no-authority boundary; no runtime test reviewer
```

### ER-13 result commit and delivery evidence (2026-09-17)

```text
source_snapshot: 421baf7 + ER-13 working-tree slice; kiana-core/src/{dispatch,capabilities,approvals,lifecycle}.rs; kiana-runner/src/{harness,protocol_runner}.rs; kiana-domain/src/capabilities.rs; kiana-core/tests/er13_result_delivery.rs; .github/workflows/er13-result-delivery.yml; docs/roadmap/event-receipt-result-delivery-baseline.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: direct/Harness/approval-resume paths share finalize_capability_action; dispatch commits execution.result_committed before result.delivery_claimed, binds delivery/run versions and result digest, rejects replayed/different claim or terminal/cancelled run, and maps callback uncertainty/unknown effect to Unknown; Runner only receives committed result and has no second model loop; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/er13_result_delivery.rs finalizer/shared-path, result-commit-before-delivery, delivery CAS/replay/terminal fence and no-repeat source guard; GitHub Actions ER-13 workflow runs source guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-13 is triggered by the eventual push and is not awaited
status_change: ER-13 source slice is implemented. A committed result is the only input to delivery; claim replay does not dispatch another effect, terminal/cancel fences block late delivery, and an uncertain callback cannot be reported as successful completion.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: CI-only source guard has not been executed locally; runner callback crash/replay behavior, provider receipts, terminal shutdown/flush, cross-process delivery recovery and external/live/physical exactly-once proof remain ER-14+ / CP/PD work
reviewer: Codex root implementation review plus ER-13 shared finalizer, commit-before-delivery, CAS/replay, terminal fence and callback-unknown boundary; no runtime test reviewer
```

### ER-14 effect observation and provider receipt evidence (2026-09-17)

```text
source_snapshot: 4849f5b + ER-14 working-tree slice; kiana-domain/src/{effect_observation,contracts,lib}.rs; kiana-daemon/src/connectors.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/er14_effect_observation.rs; kiana-core/tests/er14_effect_observation_guard.rs; .github/workflows/er14-effect-observation.yml; docs/roadmap/event-receipt-effect-observation-baseline.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: strict EffectObservation binds execution/invocation/attempt, owner/audience/idempotency digests, optional provider receipt/query/evidence refs, observed time and ConfirmedSuccess/Failure/NoEffect/Unknown state; local connector invoke/reconcile emits the observation, exact prior receipt/binding/idempotency/payload checks remain required, and Unknown cannot be upgraded by the DTO; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/er14_effect_observation.rs provider success/unknown/no-effect/scope/strict-field fixtures; kiana-core/tests/er14_effect_observation_guard.rs connector observation/exact-reconcile/no-retry source guard; GitHub Actions ER-14 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-14 is triggered by the eventual push and is not awaited
status_change: ER-14 source slice is implemented. Provider receipts now have a typed, owner/audience-bound observation envelope; timeout/unknown remains unresolved, no-effect needs query evidence, and non-idempotent or mismatched reconcile cannot become success.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: repository connectors remain local_fixture-only; no external transport/provider query, credential/account lease, real timeout/retry/reconcile, handler stop, durable observation store or external/live/physical proof exists; those remain ER-15+ / INT/BQ/CP work
reviewer: Codex root implementation review plus ER-14 observation state, owner/audience/idempotency binding, connector integration and Unknown/no-effect boundary; no runtime test reviewer
```

### ER-15 adapter result boundary evidence (2026-09-17)

```text
source_snapshot: 323789b + ER-15 working-tree slice; kiana-domain/src/{adapter_result,capabilities,contracts,lib}.rs; kiana-daemon/src/{harness_capabilities,harness_memory,harness_mcp,pre_tool_hooks,apply_patch}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/er15_adapter_result.rs; kiana-core/tests/er15_adapter_result_guard.rs; .github/workflows/er15-adapter-result.yml; docs/roadmap/event-receipt-adapter-result-baseline.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: strict AdapterResult binds adapter kind, bounded output/evidence digests, process/effect/stop dimensions, adapter-local commit state and reconciliation fence; shell/patch, Memory search/write/review, MCP success/unknown/cancel and Hook decision facts attach the same envelope, while Hook updated-input mutation is rejected, MCP uncertainty retains workspace, Memory reports sync boundary and Patch retains prepared transaction/rollback evidence; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/er15_adapter_result.rs strict/digest/bounded/Unknown/cancelled/Hook fixtures; kiana-core/tests/er15_adapter_result_guard.rs adapter wiring, Hook mutation deny, MCP retain/Unknown, Memory sync and Patch transaction/rollback source guard; GitHub Actions ER-15 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-15 is triggered by the eventual push and is not awaited
status_change: ER-15 source slice is implemented. Adapter-local result boundaries are explicit and bounded before ControlPlane result commit; unknown/partial/cancelled work remains fenced and no adapter metadata grants authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: adapter envelopes are source/runtime-shape evidence only; no cross-process adapter checkpoint, power-loss proof, external Hook/MCP effect, provider/live receipt, or physical exactly-once evidence exists; terminal uniqueness, snapshots and real integration remain ER-16+ / INT/PD/DEP work
reviewer: Codex root implementation review plus ER-15 common envelope, adapter commit/stop/effect mapping, Hook/MCP/Memory/Patch negative boundaries and no-authority boundary; no runtime test reviewer
```

### ER-16 terminal uniqueness and delivery evidence (2026-09-17)

```text
source_snapshot: 89caa33 + ER-16 working-tree slice; kiana-core/src/{events,projection,receipts,lifecycle}.rs; kiana-daemon/src/lib.rs; kiana-core/tests/er16_terminal_uniqueness.rs; .github/workflows/er16-terminal.yml; docs/roadmap/event-receipt-terminal-baseline.md; docs/roadmap/event-receipt-recovery.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: record_terminal_event now rejects non-terminal kinds, makes same-kind replay idempotent, returns run_terminal_conflict for cross-kind terminal facts, keeps stable run.terminal command/idempotency CAS and maps exhausted contention to result_unknown:terminal_append_unconfirmed; result_unknown still appends resource quarantine when possible; DaemonHost shutdown orders EventStore flush, observability drain and EventStore close; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/er16_terminal_uniqueness.rs same-kind duplicate, cross-kind conflict, late-event suppression and terminal/shutdown source fixtures; GitHub Actions ER-16 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions ER-16 is triggered by the eventual push and is not awaited
status_change: ER-16 source slice is implemented. Terminal facts now have an explicit kind/conflict/idempotency/CAS boundary, uncertain terminal append remains Unknown, and shutdown has an ordered flush/close acknowledgement path.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: in-memory and JSONL source checks do not prove power-loss/cross-host terminal durability, complete resource quarantine when EventStore itself fails, process-tree stop/reap, restart projection or external/live/physical outcome; those remain ER-17+ / PD/DEP/INT work
reviewer: Codex root implementation review plus ER-16 terminal kind validation, stable CAS/idempotency, conflict/Unknown handling, quarantine and shutdown ordering; no runtime test reviewer
```

### CAP-05 permit verification and single dispatch evidence (2026-09-17)

```text
source_snapshot: 02d9e1a + CAP-05 working-tree slice; kiana-core/src/dispatch.rs; kiana-capability-broker/src/lib.rs; kiana-domain/src/dispatch.rs; kiana-ports/src/lib.rs; kiana-core/tests/cap05_permit.rs; .github/workflows/cap05-permit.yml; docs/roadmap/capability-permit-baseline.md; docs/roadmap/capability.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: JournalPermitVerifier now rejects missing/empty permit identities, distinguishes an already-consumed execution_permit stream, validates strict DispatchPermit/request/project/expiry, rechecks every authority_versions dependency immediately before CAS consumption, and returns old_epoch_permit_rejected on drift; only committed invocation.dispatching reaches the existing handler path, while concurrent/unknown consumption remains fenced; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/cap05_permit.rs MemoryEventLog prepared permit, concurrent single-consumer and opaque/empty authorization fixtures plus commit-before-handler source guard; GitHub Actions CAP-05 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CAP-05 is triggered by the eventual push and is not awaited
status_change: CAP-05 source slice is implemented. Broker authorization is now a server-verified opaque permit with authority epoch recheck and durable single-consume CAS; an unconfirmed dispatch cannot invoke a handler.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: fixture uses in-memory EventStore and does not prove JSONL power-loss/cross-process races, OS spawn crash recovery, process/effect exactly-once, approval continuation, provider receipts or external/live/physical outcomes; those remain CAP-06+/CP/ER/PD/INT work
reviewer: Codex root implementation review plus CAP-05 permit identity, authority read-set epoch recheck, single-consume CAS and no-handler-before-commit boundary; no runtime test reviewer
```

### CAP-06 approval final-plan binding evidence (2026-09-17)

```text
source_snapshot: 2ad919a + CAP-06 working-tree slice; kiana-domain/src/{approval_preview,capabilities,contracts,lib}.rs; kiana-core/src/{recovery,approvals}.rs; kiana-daemon/src/journal_approvals.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/cap06_approval_preview.rs; kiana-core/tests/cap06_approval_guard.rs; .github/workflows/cap06-approval.yml; docs/roadmap/capability-approval-baseline.md; docs/roadmap/capability.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: strict ApprovalPlanPreview binds approval/request/capability/operation/risk, redacted preview, payload/preview/scope/environment digests, expiry and payload availability; pending views derive it from server-owned scope, while approve/consume paths reload protected material and re-run action/policy/gate/permission/authority/expiry/cancel/version checks before dispatch; preview cannot grant authority or become execution material; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/cap06_approval_preview.rs strict/redaction/digest/unknown/payload-availability fixtures; kiana-core/tests/cap06_approval_guard.rs pending preview, protected material, action/policy/gate/expiry/version revalidation and no-preview-execution source guard; GitHub Actions CAP-06 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CAP-06 is triggered by the eventual push and is not awaited
status_change: CAP-06 source slice is implemented. Human-facing approval projection now describes the exact prepared plan without exposing raw material, and the execution path revalidates the protected subject before one-time permit consumption.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: ApprovalPlanPreview is a redacted projection; in-memory/JSONL material and local Human Inbox do not prove cross-process approval persistence, external authenticator, OS/provider effect or live/physical outcomes; CAP-07+ / CP/ER/PD/INT remain responsible for those boundaries
reviewer: Codex root implementation review plus CAP-06 preview redaction, material/action/scope revalidation and no-authority boundary; no runtime test reviewer
```

### H-06 stream normalizer evidence (2026-09-17)

```text
source_snapshot: 870a777 + H-06 working-tree slice; kiana-domain/src/model.rs; kiana-runner/src/{stream_normalizer,harness,lib,model}.rs; kiana-runner/tests/h06_stream_normalizer.rs; kiana-runner/tests/h06_stream_guard.rs; .github/workflows/h06-stream-normalizer.yml; docs/roadmap/harness-stream-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: ModelStreamAccumulator is the single bounded per-attempt assembly path for Text/ToolArguments/Usage/Stop fragments; it enforces attempt-local delta/text/tool limits, tool identity and JSON validation, usage monotonicity, duplicate/conflicting stop rejection, EOF-without-stop failure and late-delta cancellation fence; KianaHarness sends callback deltas through the accumulator and validates the complete output before tool requests; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-runner/tests/h06_stream_normalizer.rs interleaved tool/text, invalid split JSON, EOF without stop and late-cancel fixtures; kiana-runner/tests/h06_stream_guard.rs single accumulator/limits/no-partial-dispatch source guard; GitHub Actions H06 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H06 is triggered by the eventual push and is not awaited
status_change: H06 source slice is implemented. Stream and buffered model paths now share one attempt-bound accumulator; incomplete or conflicting streams cannot produce tool dispatch or completed turn output.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: only source/cassette adapters are covered; real provider frame/reconnect/slow-consumer behavior, durable attempt ledger, billing/retry integration, terminal/recovery and external/live/physical proof remain H07+ / P4 / ER/PD/INT work
reviewer: Codex root implementation review plus H06 accumulator identity/limits/JSON/stop/cancel boundaries and Harness integration; no runtime test reviewer
```

### H-07 harness budget evidence (2026-09-17)

```text
source_snapshot: dfa9b4d + H-07 working-tree slice; kiana-runner/src/{budget,harness,lib}.rs; kiana-domain/src/{model,usage}.rs; kiana-core/src/lifecycle.rs; kiana-ports/src/lib.rs; kiana-daemon/src/lib.rs; kiana-runner/tests/h07_budget.rs; kiana-runner/tests/h07_budget_guard.rs; .github/workflows/h07-budget.yml; docs/roadmap/harness-budget-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: HarnessBudgetConfig validates constructor/environment values fail-closed and records default/environment/role source; BudgetLedger keeps project+role task-chain counters for attempts, tools, repairs, compactions and tokens, reserves before provider and settles after usage; unknown usage consumes the reservation; ModelAssignment authority runtime budget and CP-11 ModelBudgetPort remain in the admission chain; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-runner/tests/h07_budget.rs task-chain Continue reset, provider retry attempt budget, invalid effective budget and cross-project role isolation fixtures; kiana-runner/tests/h07_budget_guard.rs source ordering/bypass guard; GitHub Actions H07 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H07 is triggered by the eventual push and is not awaited
status_change: H07 source slice is implemented. Runner budgets model steps/attempts/tool calls/repair/compaction/time/tokens separately, and a failed or unknown model attempt cannot be retried for free or reset by Continue.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: task-chain ledger is process-local and project+role keyed; restart/cross-process/parallel sibling merge still relies on future durable projector work, CP-11 authority remains the final lease boundary, provider tokenizer/billing and human-wait TTL are not proven, and external/live/physical proof remains H08+ / CP/PD/INT work
reviewer: Codex root implementation review plus H07 config source/priority, pre-provider reservation, conservative settlement and task-chain isolation; no runtime test reviewer
```

### H-08 deadline and cancellation propagation evidence (2026-09-17)

```text
source_snapshot: e8e9032 + H-08 working-tree slice; kiana-ports/src/model.rs; kiana-runner/src/harness.rs; kiana-runner/tests/h08_cancellation.rs; kiana-runner/tests/h08_cancellation_guard.rs; kiana-daemon/src/{harness_capabilities,mcp_stdio}.rs; kiana-daemon/tests/daemon_host.rs; kiana-core/src/dispatch.rs; .github/workflows/h08-cancellation.yml; docs/roadmap/harness-cancellation-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: RunCancellation now exposes a watch signal alongside its typed reason; every model attempt uses cancellable admitted/prepared port wrappers and the existing Harness deadline/select/race fence; shell and MCP paths retain bounded process-group/read/write/stop confirmation and result_unknown boundaries; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-runner/tests/h08_cancellation.rs silent model deadline and retry-backoff cancellation fixtures; kiana-runner/tests/h08_cancellation_guard.rs source guard; GitHub Actions H08 workflow runs runner fixtures/source guard and existing daemon mid-stream/process-group cancellation fixtures
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H08 is triggered by the eventual push and is not awaited
status_change: H08 source slice is implemented. Cancellation now reaches the model port before response completion, silent model attempts are bounded by deadline, and cancellation races cannot turn a completed/unknown effect into a guessed success.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: process-group confirmation and MCP behavior are covered by existing source/contracts and CI fixtures but real OS/provider/network fault injection, cross-process cancellation durability, full P0-J1 state vocabulary and external/live/physical proof remain future CP/P4/PD/INT work
reviewer: Codex root implementation review plus H08 watch propagation, model deadline fence, retry cancellation and shell/MCP stop-confirmation boundaries; no runtime test reviewer
```

### H-09 versioned tool catalog evidence (2026-09-17)

```text
source_snapshot: c37d5e1 + H-09 working-tree slice; kiana-domain/src/{tool_authority,tool_catalog,actions,model}.rs; kiana-runner/src/{tools,harness,lib}.rs; kiana-provider/src/request.rs; kiana-provider/tests/oa08_provider_telemetry.rs; kiana-daemon/src/lib.rs; kiana-runner/tests/h09_tool_catalog.rs; kiana-runner/tests/h09_tool_catalog_guard.rs; .github/workflows/h09-tool-catalog.yml; docs/roadmap/harness-tool-catalog-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: ToolCatalogSnapshot/ToolDescriptor now pins schema/version/digest, wire/alias, capability/risk, argument/result schema, output/replay metadata; PreparedModelCall tool hash, runner checkpoint, action catalog and daemon configuration revision bind the snapshot; mapper and provider wire resolver use the same descriptor; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-runner/tests/h09_tool_catalog.rs unadvertised tool/wire collision, catalog drift and mapper consistency fixtures; kiana-runner/tests/h09_tool_catalog_guard.rs version/digest/pin/no-fallback source guard; GitHub Actions H09 workflow runs catalog fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H09 is triggered by the eventual push and is not awaited
status_change: H09 source slice is implemented. A model-visible tool list is now an explicit versioned snapshot, and schema/alias/handler drift invalidates prepared requests, checkpoints, actions and configuration pins before dispatch.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: current five-tool descriptor dataset remains the supported baseline, provider-specific catalog migrations and remote handler parity are not proven, approval durable recovery still depends on later steps, and external/live/physical proof remains H10+ / CP/PD/P4/INT work
reviewer: Codex root implementation review plus H09 snapshot validation, alias/wire uniqueness, request/checkpoint/action pinning and mapper/provider resolver consistency; no runtime test reviewer
```

### H-10 invocation identity evidence (2026-09-17)

```text
source_snapshot: 30961cb + H-10 working-tree slice; kiana-runner/src/{harness,tools,lib}.rs; kiana-domain/src/{model,execution_identity}.rs; kiana-ports/src/lib.rs; kiana-core/src/dispatch.rs; kiana-runner/tests/h10_invocation_identity.rs; kiana-runner/tests/h10_invocation_identity_guard.rs; .github/workflows/h10-invocation-identity.yml; docs/roadmap/harness-invocation-identity-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: complete assistant tool batches are validated before mapping/dispatch; each request ID is derived once from run/turn/step/assistant item/ordinal, explicit mapper identity is reused for emit and checkpoint restore, and restore rejects catalog or tuple identity drift; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-runner/tests/h10_invocation_identity.rs invalid second call, duplicate call ID, queue/checkpoint/restore identity fixtures; kiana-runner/tests/h10_invocation_identity_guard.rs source guard; GitHub Actions H10 workflow runs identity fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H10 is triggered by the eventual push and is not awaited
status_change: H10 source slice is implemented. Invalid assistant batches cannot partially dispatch, and a restored queued request preserves the same server-derived identity instead of minting a new request ID.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: full Invocation declaration/dispatch and result persistence remain H11–H13, approval recovery/CAS and provider/external effect proof remain later CP/PD/INT work, and provider call IDs remain correlation metadata only
reviewer: Codex root implementation review plus H10 whole-batch validation, deterministic identity derivation, explicit mapping and checkpoint restore fencing; no runtime test reviewer
```

### H-11 tool observation evidence (2026-09-18)

```text
source_snapshot: 2fdefbd + H-11 working-tree slice; kiana-domain/src/{capabilities,contracts,errors}.rs; kiana-runner/src/harness.rs; kiana-runner/tests/h11_tool_observation.rs; kiana-runner/tests/h11_tool_observation_guard.rs; .github/workflows/h11-tool-observation.yml; docs/roadmap/harness-tool-observation-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: ToolObservation v1 classifies capability results from shared dimensions/error policy into succeeded/failed_known/denied/cancelled_not_started/unknown/pending, carries bounded redacted summary/identity/exit/digest/ref and untrusted=true; runner blocks deny/unknown/cancel retry and feeds only known repairable failures as Tool-role data; schema registry entry and CI workflow are present, static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-runner/tests/h11_tool_observation.rs unknown no-retry, untrusted output, known failure repair and bounded observation fixtures; kiana-runner/tests/h11_tool_observation_guard.rs source guard; GitHub Actions H11 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H11 is triggered by the eventual push and is not awaited
status_change: H11 source slice is implemented. Tool output is now a typed data-only observation; authorization-like fields cannot grant permissions, and only explicitly repairable known failures can continue the model loop.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: pending is a schema state not yet produced by the runner, automatic repair remains bounded to the next model step, batch persistence/Invocation outcome/recovery and external/live/physical proof remain H12+ / CP/PD/INT work
reviewer: Codex root implementation review plus H11 status/error policy mapping, summary bounds/redaction, no-retry gates and model observation data boundary; no runtime test reviewer
```

### H-12 serial tool batch evidence (2026-09-18)

```text
source_snapshot: 30a5cb0 + H-12 working-tree slice; kiana-runner/src/harness.rs; kiana-runner/tests/h12_serial_batch.rs; kiana-runner/tests/h12_serial_batch_guard.rs; .github/workflows/h12-serial-batch.yml; docs/roadmap/harness-serial-batch-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: pending tool entries now carry queued/dispatched/settled phase; full batch mapping completes before queue admission, result ID/phase is checked before pop, and wrong result preserves pending state; only the front call dispatches, successful/known observations unlock the next, and cancel drains remaining queued calls into replay-safe not_executed events; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-runner/tests/h12_serial_batch.rs wrong result, cancelled batch and three serial calls fixtures; kiana-runner/tests/h12_serial_batch_guard.rs phase/order/no-bypass source guard; GitHub Actions H12 workflow runs serial fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H12 is triggered by the eventual push and is not awaited
status_change: H12 source slice is implemented. Ordered tool batches no longer consume pending state on wrong replies or dispatch queued siblings after cancellation; the next model step starts only after the batch is drained.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: batch declaration/outcome is still process-local runner state, durable Invocation ledger and crash/recovery windows remain H13+, and external/live/physical effect proof is not claimed
reviewer: Codex root implementation review plus H12 phase transitions, wrong-result fencing, serial dispatch barrier and cancellation drain semantics; no runtime test reviewer
```

### H-13 invocation ledger evidence (2026-09-18)

```text
source_snapshot: c0b6db6 + H-13 working-tree slice; kiana-core/src/{dispatch,capabilities,lifecycle,invocation_projection,recovery}.rs; kiana-domain/src/capabilities.rs; kiana-core/tests/h13_invocation_ledger.rs; kiana-core/tests/h13_invocation_ledger_guard.rs; .github/workflows/h13-invocation-ledger.yml; docs/roadmap/harness-invocation-ledger-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: Core records request/decision/prepared/dispatching/executing/result/delivery boundaries through EventLog CAS; result commit and delivery claim now carry receipt/outcome_ready/outcome_state metadata; broker execution remains after committed invocation boundary, and projection turns dispatch without outcome into Unknown; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/h13_invocation_ledger.rs persisted outcome/dispatch-without-outcome projection fixtures; kiana-core/tests/h13_invocation_ledger_guard.rs event-order/CAS/no-reexecute source guard; GitHub Actions H13 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H13 is triggered by the eventual push and is not awaited
status_change: H13 source slice is implemented. Invocation facts and result receipts are committed before runner delivery, and durable projections fail closed on missing or conflicting outcomes.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: EventLog adapter crash/power-loss and cross-process CAS durability, approval resume and external effect reconciliation remain H14+ / CP/PD/INT work; no exactly-once external effect is claimed
reviewer: Codex root implementation review plus H13 invocation event ordering, result receipt/outcome metadata, delivery CAS and unknown projection boundaries; no runtime test reviewer
```

### H-14 approval pause and original invocation resume evidence (2026-09-18)

```text
source_snapshot: 61103ad + H-14 working-tree slice; kiana-domain/src/{invocation_resume,capabilities,contracts,event_contracts,lib}.rs; kiana-runner/src/harness.rs; kiana-core/src/{approvals,capabilities,lifecycle,recovery}.rs; kiana-domain/tests/h14_invocation_resume.rs; kiana-core/tests/h14_approval_resume.rs; .github/workflows/h14-approval-resume.yml; docs/roadmap/harness-approval-resume-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: Runner emits server-owned Turn/Step and ordered pending-batch digest with each tool request; Core stores a strict InvocationResumeBinding beside the pending approval and RunSnapshot/event facts; approval waits remain AwaitingApproval without a fake tool failure; approve revalidates binding/material/path/scope/authority/policy/budget/cancel before the existing permit/dispatch and result-delivery path; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/h14_invocation_resume.rs binding round-trip and changed-parameter/owner rejection; kiana-core/tests/h14_approval_resume.rs expiry/change, duplicate-decision, and identity source guards; GitHub Actions H14 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H14 is triggered by the eventual push and is not awaited
status_change: H14 source slice is implemented. Pending approval now carries an explicit owner/identity/parameter/catalog/authority/sandbox/batch binding, and same-process approval resumes the original invocation through the existing Runner result and next-step path; denial/cancel/expiry and binding drift close the pending run without dispatch.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: PendingInvocation and Runner checkpoint ownership remain process-local; cross-process hydration, power-loss CAS/reconcile, path-lock/resource-version projector, external effect observation and exactly-once effects remain H24/H25/PD/INT work
reviewer: Codex root implementation review plus H14 binding, proof, pre-dispatch revalidation, idempotent decision and original Turn/Step/Invocation continuity; no runtime test reviewer
```

### H-15 bounded output and paged reference evidence (2026-09-18)

```text
source_snapshot: 533771f + H-15 working-tree slice; kiana-domain/src/{execution_output,actions,contracts,lib}.rs; kiana-daemon/src/{harness_capabilities,execution_control}.rs; kiana-core/src/capabilities.rs; kiana-domain/tests/h15_output_ref.rs; kiana-daemon/tests/h15_output_limits.rs; .github/workflows/h15-output-bounds.yml; docs/roadmap/harness-output-bounds-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: shell stdout/stderr capture keeps chunked byte/line/observed limits and drain timeout; complete redacted output is stored outside model/event payloads behind strict ExecutionOutputRef with output/run/invocation/content/scope/expiry digests; operator-only execution.output.read rechecks owner/run/data epoch/reference/content and returns bounded UTF-8 pages with stable cursor; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/h15_output_ref.rs strict reference round-trip/tamper fixtures; kiana-daemon/tests/h15_output_limits.rs bounded capture, cross-run denial, expiry and paging source guards; GitHub Actions H15 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H15 is triggered by the eventual push and is not awaited
status_change: H15 source slice is implemented. Large output is bounded before buffering, safe complete content is referenced rather than injected into model history, and paged reads cannot cross owner/run/epoch/expiry/content boundaries.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: output storage remains a local adapter without cross-process ArtifactStore lifecycle, retention/deletion propagation or backup proof; MCP/connector result bounds and external/live/physical effect proof remain ER/PD/INT work
reviewer: Codex root implementation review plus H15 streaming limits, typed output reference integrity, owner/run/epoch/expiry fencing and cursor page semantics; no runtime test reviewer
```

### H-16 parallel-read groups and exclusive barrier evidence (2026-09-18)

```text
source_snapshot: 9db12d6 + H-16 working-tree slice; kiana-domain/src/{tool_authority,tool_scheduling,lib}.rs; kiana-runner/src/harness.rs; kiana-core/src/{dispatch,capabilities}.rs; kiana-domain/tests/h16_tool_scheduling.rs; kiana-core/tests/h16_parallel_barriers.rs; .github/workflows/h16-parallel-barriers.yml; docs/roadmap/harness-parallel-barriers-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: ToolCatalog descriptors now carry scheduling class, coarse resource claims and bounded max_parallelism; deterministic ToolBatchPlan groups adjacent read-only calls and makes every side-effecting call an exclusive barrier; Runner validates the plan before pending admission, while every request continues through independent ControlPlane authorization/permit/result CAS; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/h16_tool_scheduling.rs read-group/barrier planning fixtures; kiana-core/tests/h16_parallel_barriers.rs write barrier, revocation, H13 outcome and source-order guards; GitHub Actions H16 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H16 is triggered by the eventual push and is not awaited
status_change: H16 source slice is implemented. Batch scheduling metadata and deterministic barrier semantics are now part of the versioned tool catalog; a batch-level allow cannot authorize an unstarted sibling.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: Harness external behavior remains serialized while the safe plan contract is introduced; bounded worker-pool overlap, cross-process resource CAS, queue-head pressure and external/live/physical effect proof remain later CAP/ER/PD/INT work
reviewer: Codex root implementation review plus H16 scheduling metadata, source-order planner, exclusive barrier, per-call revalidation and H13 no-rerun handoff; no runtime test reviewer
```

### H-17 long-running JobHandle evidence (2026-09-18)

```text
source_snapshot: 16305b7 + H-17 working-tree slice; kiana-domain/src/{job_handle,contracts,lib}.rs; kiana-daemon/src/execution_control.rs; kiana-core/src/capabilities.rs; kiana-domain/tests/h17_job_handle.rs; kiana-daemon/tests/h17_job_handle.rs; .github/workflows/h17-job-handle.yml; docs/roadmap/harness-job-handle-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: process.start now records a strict JobHandle after spawn, binding start request/invocation, optional Run/Turn, owner/session, project digest, authority epoch, process-group evidence and TTL; poll/stdin/resize/stop validate the handle and each operation remains a separate ControlPlane invocation; restart paths return persisted outcome for poll only and never attach by PID; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/h17_job_handle.rs handle round-trip/tamper fixtures; kiana-daemon/tests/h17_job_handle.rs foreign owner, PID reuse, no-restart and distinct-operation source guards; GitHub Actions H17 workflow runs fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H17 is triggered by the eventual push and is not awaited
status_change: H17 source slice is implemented. Long-running process continuations now require a typed, owner/scoped/authority-bound JobHandle with positive process-group evidence and expiry; no restart path can silently reattach a process.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: process map and JobHandle projector remain process-local; cross-process process-group/handle recovery, complete output stream durability, kill-9/power-loss reconciliation and external/live/physical proof remain H24/H25/PD/INT work
reviewer: Codex root implementation review plus H17 handle identity, owner/run/turn/authority/TTL fencing, restart no-attach policy and operation-level invocation boundaries; no runtime test reviewer
```

### H-18 checkpointed Inbox and input receipt evidence (2026-09-18)

```text
source_snapshot: 55bb304 + H-18 working-tree slice; kiana-domain/src/{ids,contracts,inbox_contract,lib}.rs; kiana-runner/src/{inbox,harness}.rs; kiana-runner/tests/h18_inbox.rs; kiana-core/tests/h18_inbox_guard.rs; .github/workflows/h18-inbox.yml; docs/roadmap/harness-inbox-baseline.md; docs/roadmap/harness.md; docs/roadmap.md; docs/roadmap/README.md
worktree_status: Inbox messages now carry server-owned InputId, source, target, target turn and monotonic received sequence; NextTurn/NextStep queues and claimed ledger are bounded, duplicate/claimed replays are idempotent, queue overflow returns backpressure, and InputReceipt provides accepted/duplicate/claimed ACK projection; HarnessCheckpoint serializes/restores inbox and claim state, while claim rejects cross-turn steering; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-runner/tests/h18_inbox.rs duplicate/claim, checkpoint order and cross-turn fixtures; kiana-core/tests/h18_inbox_guard.rs claim/checkpoint/backpressure source guards; GitHub Actions H18 workflow runs runner fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H18 is triggered by the eventual push and is not awaited
status_change: H18 source slice is implemented. Input identity, ACK/claim semantics, bounded backpressure and checkpoint round-trip are now explicit Runner contracts; no duplicate input is consumed twice.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: accepted/consumed input facts are not yet atomically projected by every ControlPlane entrypoint; large payload Artifact refs, reconnect/steer protocol and cross-process Inbox projector remain H19/PD/ER work
reviewer: Codex root implementation review plus H18 InputId/receipt contract, queue bounds, duplicate/claim ledger, target-turn fence and checkpoint persistence; no runtime test reviewer
```

### H-19 Continue / Steer / Inject product wiring evidence (2026-09-18)

```text
source_snapshot: 31a4488 + H-19 working-tree slice; kiana-protocol/src/lib.rs; kiana-client/src/lib.rs; kiana-daemon/src/lib.rs; kiana-core/src/lifecycle.rs; kiana-runner-protocol/src/lib.rs; kiana-runner/src/{harness,protocol_runner}.rs; kiana-domain/src/event_contracts.rs; kiana-{protocol,runner,core}/tests/h19_*; .github/workflows/h19-steer-inject.yml; docs/roadmap/harness-steer-inject-baseline.md
worktree_status: additive Steer/Inject requests now use the same versioned client→DaemonHost→ControlPlane route; ControlPlane validates bounded source/target and expected turn, records input.accepted facts (and a rejected claim when Runner refuses delivery), and returns an InputReceipt; Runner defers inputs received while ActiveRun is temporarily out of the map and consumes them at the next safe model-step boundary; Continue v1 remains unchanged; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-protocol/tests/h19_steer_inject.rs additive strict wire round-trip/unknown-field fixtures; kiana-runner/tests/h19_steer_inject.rs blocked first model call with one deferred steer observed exactly once by the next step; kiana-core/tests/h19_steer_inject_guard.rs sandbox/model immutability and stale-turn source guards; GitHub Actions H19 workflow runs all fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions H19 is triggered by the eventual push and is not awaited
status_change: H19 source slice is implemented. Continue/Steer/Inject are no longer test-only helpers: additive wire/client/daemon routing, ControlPlane input.accepted facts/ACK with rejected-delivery closure, stale-turn fencing, and in-flight deferred delivery are explicit.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: accepted/claimed facts and deferred queues remain process-local around cross-process crash/restart; large payloads are bounded inline rather than Artifact-backed; terminal race arbitration, provider-native stream guarantees, durable projector/recovery and external/live/physical proof remain open for later H/PD/ER/provider steps
reviewer: Codex root implementation review plus H19 protocol, route, turn-fence and deferred-input invariants; no runtime test reviewer
```

### P0-G-03 explicit resume entrypoint evidence (2026-09-18)

```text
source_snapshot: 42865d8 + P0-G-03 evidence slice; kiana-core/src/recovery.rs; kiana-core/src/lifecycle.rs; kiana-protocol/src/lib.rs; kiana-client/src/lib.rs; kiana-daemon/src/lib.rs; kiana-core/tests/p0_g03_resume_guard.rs; .github/workflows/p0-g03-resume.yml; docs/roadmap/p0-g03-resume-baseline.md
worktree_status: existing ResumeRequest and DaemonHost→ControlPlane route are now explicitly reconciled as the P0-G-03 source slice; resume rebuilds the EventLog snapshot/invocation projection, rechecks owner/scope/authority/data epoch and snapshot freshness, claims run.resume_prepared at the observed stream version, restores the same Runner, and sends approved continuation through the shared drive_run; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p0_g03_resume_guard.rs resume route/shared-drive and missing-snapshot/stale-scope source guards; existing core/daemon resume fixtures remain CI-only; GitHub Actions P0-G-03 workflow runs the guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-G-03 is triggered by the eventual push and is not awaited
status_change: P0-G-03 source slice is implemented/reconciled. Explicit Resume is additive, fail-closed and routed through the existing lifecycle/Runner path; no startup auto-resume or PID attachment is introduced.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: Runner/checkpoint and pending approval hydration remain process-local around crash/restart; full durable projector, power-loss reconciliation, automatic recovery policy and external/live/physical proof remain P0-F-03/H24/H25/PD/ER work
reviewer: Codex root implementation review plus P0-G-03 ResumeRequest, snapshot/CAS, scope revalidation and shared drive_run invariants; no runtime test reviewer
```

### P0-F-01 approval surfaces evidence (2026-09-18)

```text
source_snapshot: f7ac080 + P0-F-01 evidence slice; kiana-protocol/src/lib.rs; kiana-client/src/lib.rs; kiana-core/src/recovery.rs; kiana-core/src/approvals.rs; kiana-daemon/src/lib.rs; kiana-entrypoints/src/{harness_run,workbench_chat,web,product_command}.rs; kiana-entrypoints/tests/p0_f01_approval_surfaces.rs; .github/workflows/p0-f01-approval.yml; docs/roadmap/p0-f01-approval-baseline.md
worktree_status: Workbench/TTY, Web and one-shot product CLI now explicitly share the server-owned pending_approvals view and proof-bound approval decision envelope; challenge ID/hash/nonce, available decisions and expiry remain server supplied, while surfaces do not auto-approve or start a second execution loop; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-entrypoints/tests/p0_f01_approval_surfaces.rs shared query/decision and no-auto-approval source guards; GitHub Actions P0-F-01 workflow runs the guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-F-01 is triggered by the eventual push and is not awaited
status_change: P0-F-01 source slice is implemented/reconciled. Three product surfaces now expose the same pending approval contract and response path without granting capability authority in the UI/CLI layer.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: transport remains local/in-process compatibility, durable decision facts and single-use OCC are P0-F-02, restart continuation is P0-F-03, and external human authentication/live/physical proof remain later SC/PD work
reviewer: Codex root implementation review plus P0-F-01 surface parity, server challenge ownership and no-auto-approval invariants; no runtime test reviewer
```

### P0-F-02 approval decision and single-use evidence (2026-09-18)

```text
source_snapshot: 215ac03 + P0-F-02 evidence slice; kiana-core/src/approvals.rs; kiana-daemon/src/journal_approvals.rs; kiana-domain/src/approval_journal.rs; kiana-protocol/src/lib.rs; kiana-core/tests/control_plane.rs; kiana-core/tests/p0_f02_approval_guard.rs; .github/workflows/p0-f02-approval.yml; docs/roadmap/p0-f02-approval-decision-baseline.md
worktree_status: existing approval decision path is explicitly reconciled as P0-F-02; approved/denied/consumed facts bind original subject request/hash/nonce, actor/scope, expiry and expected version, while repeated, expired or conflicting decisions fail closed; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p0_f02_approval_guard.rs typed decision/consumption, proof/expiry/conflict and original-subject source guards; existing serialized core/daemon approval fixtures remain CI-only; GitHub Actions P0-F-02 workflow runs the guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-F-02 is triggered by the eventual push and is not awaited
status_change: P0-F-02 source slice is implemented/reconciled. Durable approval decision and single-use consumption contracts are explicit and separate from cancellation or the original execution request.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: durable cross-process journal/power-loss recovery, external approver authentication, pending continuation hydration and full receipt projection remain P0-F-03/SC/PD work
reviewer: Codex root implementation review plus P0-F-02 decision/consumption facts, proof binding, expiry and conflict fencing; no runtime test reviewer
```

### P0-F-03 resume material and RunSnapshot evidence (2026-09-18)

```text
source_snapshot: c1136a5 + P0-F-03 evidence slice; kiana-core/src/{recovery,approvals,lifecycle}.rs; kiana-domain/src/capabilities.rs; kiana-runner/src/harness.rs; kiana-daemon/src/journal_approvals.rs; kiana-core/tests/control_plane.rs; kiana-core/tests/p0_f03_resume_guard.rs; .github/workflows/p0-f03-resume.yml; docs/roadmap/p0-f03-resume-material-baseline.md
worktree_status: approval pauses persist a redacted RunSnapshot with runner-state digest, authority/data revisions and pending invocation material; fresh core rebuilds the pending projection from EventLog, requires explicit Resume, rechecks scope/policy/gate/approval, claims run.resume_prepared at the observed stream version and restores the same Runner before the shared approval→drive_run continuation; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p0_f03_resume_guard.rs fresh-process reconstruction, explicit-resume and policy/gate/approval recheck source guards; existing serialized core/daemon resume fixtures remain CI-only; GitHub Actions P0-F-03 workflow runs the guards and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-F-03 is triggered by the eventual push and is not awaited
status_change: P0-F-03 source slice is implemented/reconciled. Restarted pending approvals now have explicit, redacted, digest-bound material and a fail-closed Resume entrypoint; startup does not auto-resume.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: checkpoint and pending maps remain process-local around crash/power loss, no full durable Runner/projector or reconciliation queue is claimed, and external human authn/effect/live/physical proof remains later H24/H25/PD/SC/INT work
reviewer: Codex root implementation review plus P0-F-03 snapshot integrity, explicit-resume, stream CAS and authorization recheck invariants; no runtime test reviewer
```

### P0-J1-01 cancellation state vocabulary evidence (2026-09-18)

```text
source_snapshot: c4c21e6 + P0-J1-01 evidence slice; kiana-domain/src/{states,cancellation}.rs; kiana-core/src/{lifecycle,events,projection}.rs; kiana-runner/src/harness.rs; kiana-domain/tests/p0_j1_01_cancellation.rs; kiana-core/tests/p0_j1_01_cancellation_guard.rs; .github/workflows/p0-j1-01-cancellation.yml; docs/roadmap/p0-j1-01-cancellation-baseline.md
worktree_status: current cancellation authority is the EventLog RunCancellationFact/domain transition table; core watch and Runner signal are adapters only; ExecutionStatus exposes accepted/queued/cancelling intermediates, stop confirmation distinguishes cancelled from result_unknown, and late terminal reopening is fenced; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/p0_j1_01_cancellation.rs state transition/wire fixtures; kiana-core/tests/p0_j1_01_cancellation_guard.rs fact/signal/terminal source guards; GitHub Actions P0-J1-01 workflow runs both fixture targets and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-J1-01 is triggered by the eventual push and is not awaited
status_change: P0-J1-01 source slice is implemented/reconciled. Cancellation intermediate vocabulary and irreversible terminal semantics are explicit without introducing a second cancellation loop.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: queued tool drain, OS process-group stop confirmation, mid-stream cancellation race and cross-process/power-loss recovery remain P0-J1-02/03/04 and CP/PD/INT work
reviewer: Codex root implementation review plus P0-J1-01 fact/state/signal ordering and terminal-fence invariants; no runtime test reviewer
```

### P0-J1-02 queued cancellation drain evidence (2026-09-18)

```text
source_snapshot: f49c114 + P0-J1-02 evidence slice; kiana-runner/src/harness.rs; kiana-core/src/{lifecycle,capabilities}.rs; kiana-runner/tests/{h12_serial_batch,p0_j1_02_cancellation}.rs; kiana-core/tests/p0_j1_02_cancellation_guard.rs; .github/workflows/p0-j1-02-cancellation.yml; docs/roadmap/p0-j1-02-cancellation-drain-baseline.md
worktree_status: serial Runner batches preserve the first dispatched boundary and drain queued siblings into bounded ToolCancelled results marked not_executed/replay_safe; core cancellation helpers retain those facts before terminal cancellation/unknown projection; synthetic results cannot authorize or claim external effects; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-runner/tests/p0_j1_02_cancellation.rs queue-drain/replay-safe source guards; existing h12_serial_batch cancellation runtime fixture; kiana-core/tests/p0_j1_02_cancellation_guard.rs core handoff guard; GitHub Actions P0-J1-02 workflow runs guards and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-J1-02 is triggered by the eventual push and is not awaited
status_change: P0-J1-02 source slice is implemented/reconciled. Unstarted queued siblings now receive explicit replay-safe synthetic results instead of being silently dropped or reported as successful effects.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: the first already-dispatched effect still needs stop/effect confirmation; mid-stream late-delta fencing, process-group proof, cross-process recovery and power-loss reconciliation remain P0-J1-03/04 and CP/PD/INT work
reviewer: Codex root implementation review plus P0-J1-02 serial drain, not-executed/replay-safe labeling and core terminal handoff invariants; no runtime test reviewer
```

### P0-J1-03 process-group stop confirmation evidence (2026-09-18)

```text
source_snapshot: e0ec416 + P0-J1-03 evidence slice; kiana-daemon/src/{harness_capabilities,harness_mcp,mcp_stdio,execution_control}.rs; kiana-daemon/tests/daemon_host.rs; kiana-daemon/tests/p0_j1_03_process_group.rs; .github/workflows/p0-j1-03-process-group.yml; docs/roadmap/p0-j1-03-process-group-baseline.md
worktree_status: shell/MCP/long-process adapters create isolated process groups, terminate and poll descendants, drain bounded output, and report stop_confirmed only after positive absence confirmation; failure to confirm is structured result_unknown/reconciliation rather than cancelled success; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-daemon/tests/p0_j1_03_process_group.rs process-group/stop-evidence source guards; existing daemon_host cancel_stops_in_flight_shell_before_it_writes runtime fixture; GitHub Actions P0-J1-03 workflow runs the guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-J1-03 is triggered by the eventual push and is not awaited
status_change: P0-J1-03 source slice is implemented/reconciled. `stop_confirmed` now represents positive adapter evidence and unconfirmed termination remains Unknown.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: kernel/descendant races, mid-stream late-delta fencing, durable stop/reconciliation and cross-process/power-loss proof remain P0-J1-04 and CP/PD/INT work
reviewer: Codex root implementation review plus P0-J1-03 process-group lifecycle, confirmation and Unknown fallback invariants; no runtime test reviewer
```

### P0-J1-04 cancellation race evidence (2026-09-18)

```text
source_snapshot: 3274ec0 + P0-J1-04 evidence slice; kiana-daemon/tests/daemon_host.rs; kiana-daemon/src/harness_capabilities.rs; kiana-core/src/{lifecycle,events}.rs; kiana-daemon/tests/p0_j1_04_cancel_race.rs; .github/workflows/p0-j1-04-cancel-race.yml; docs/roadmap/p0-j1-04-cancel-race-baseline.md
worktree_status: existing held-stream daemon race preserves cancel-before-signal ordering, prevents late model delta/completion, records exactly one terminal cancellation fact, and keeps result_unknown available for uncertain effects; source guard and CI wiring retain the prior runtime assertion unchanged; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-daemon/tests/p0_j1_04_cancel_race.rs race/terminal source guards; existing daemon_host::cancelling_mid_stream_never_completes_or_emits_a_late_delta runtime fixture; GitHub Actions P0-J1-04 workflow runs the serialized regression and guard
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-J1-04 is triggered by the eventual push and is not awaited
status_change: P0-J1-04 source slice is implemented/reconciled. Cancellation races cannot report a wrong completion or late stream result, and terminal uniqueness remains explicit.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: external/provider and kernel descendant races, cross-process/power-loss reconciliation, and physical effect proof remain CP/PD/INT work
reviewer: Codex root implementation review plus P0-J1-04 cancel ordering, late-result fencing and terminal uniqueness invariants; no runtime test reviewer
```

### P0-M1-01 Workbench terminal-state parity evidence (2026-09-18)

```text
source_snapshot: d082a5c + P0-M1-01 evidence slice; kiana-entrypoints/src/{workbench_chat,stream_render,web}.rs; kiana-daemon/src/run_stream.rs; contrib/desktop/main.js; kiana-entrypoints/tests/p0_m1_workbench.rs; .github/workflows/p0-m1-workbench.yml; docs/roadmap/p0-m1-workbench-baseline.md
worktree_status: Workbench/TTY, CLI stream renderer and Web ledger/SSE consume the same run-scoped Terminal/ExecutionStatus projection; Desktop launches the same Web/DaemonHost surface; cancelled and result_unknown remain visible, and no surface writes terminal facts; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-entrypoints/tests/p0_m1_workbench.rs cross-surface terminal/status source guards; existing workbench/stream/web unit fixtures remain CI-only; GitHub Actions P0-M1-01 workflow runs the guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-M1-01 is triggered by the eventual push and is not awaited
status_change: P0-M1-01 source slice is implemented/reconciled. All four product surfaces now derive terminal state from the shared server-owned run response/stream rather than a UI-specific terminal fact.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: versioned UiSnapshot/action/reconnect projector, desktop package runtime evidence, accessibility checks and cross-process stream recovery remain UI-01+/P2-M2/P2-M5/DEP work
reviewer: Codex root implementation review plus P0-M1-01 terminal/status parity and no-UI-authority invariants; no runtime test reviewer
```

### P1-C-02 Cell lifecycle and retirement evidence (2026-09-18)

```text
source_snapshot: 0bf2f1d + P1-C-02 evidence slice; kiana-core/src/{cell_registry,collaboration}.rs; kiana-domain/src/{work_packets,states}.rs; kiana-core/tests/control_plane.rs; kiana-core/tests/p1_c02_cell_lifecycle.rs; .github/workflows/p1-c02-cell-lifecycle.yml; docs/roadmap/p1-c02-cell-lifecycle-baseline.md
worktree_status: MemoryCellRegistry reserve/commit/terminal/retire validates scoped template/grant/budget/supervision/path identities; active capabilities fence retirement; release_resources returns only the owning budget reservation, removes still-owned path locks idempotently, and emits grant/supervision/budget retirement identity; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p1_c02_cell_lifecycle.rs reserve/commit/retire ownership and release source guards; existing serialized control_plane Cell cancellation/path-lock fixtures remain CI-only; GitHub Actions P1-C-02 workflow runs the guards and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-C-02 is triggered by the eventual push and is not awaited
status_change: P1-C-02 source slice is implemented/reconciled. Cell resource lifecycle and exactly-once owned release are explicit, with no cross-Cell cleanup.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: MemoryCellRegistry remains process-local; durable Cell/Grant/Budget/Lease projection, restart recovery, scheduler/Swarm claims and cross-process fencing remain AUT/SW/ER/PD/CP work
reviewer: Codex root implementation review plus P1-C-02 reserve/commit/terminal/retire and owned-resource-release invariants; no runtime test reviewer
```

### P1-E-02 Symposium decision replay evidence (2026-09-18)

```text
source_snapshot: 0d41775 + P1-E-02 evidence slice; kiana-domain/src/symposiums.rs; kiana-core/src/{collaboration,events}.rs; kiana-core/tests/control_plane.rs; kiana-core/tests/p1_e02_symposium.rs; .github/workflows/p1-e02-symposium.yml; docs/roadmap/p1-e02-symposium-baseline.md
worktree_status: Symposium validates department/chair/attendee/round boundaries, runs bounded private speaker sessions or explicit anti-meeting, preserves blackboard alternatives and typed DecisionRecord, writes department artifact and appends durable symposium.closed under the EventLog request stream; existing core regressions remain selected in CI; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p1_e02_symposium.rs decision close/replay and chair/alternative source guards; existing control_plane multi-round/private-session and department anti-meeting runtime fixtures; GitHub Actions P1-E-02 workflow runs guards and selected serialized regressions
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-E-02 is triggered by the eventual push and is not awaited
status_change: P1-E-02 source slice is implemented/reconciled. Symposium/DecisionRecord and durable close event are now explicitly tracked with bounded speaker and anti-meeting paths.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: cross-process decision projector, department memory/RAG admission, external human decision authn and full durable replay/recovery remain P4-E-03/PD/ER/SC work
reviewer: Codex root implementation review plus P1-E-02 Symposium contract, chair/attendee fences, DecisionRecord preservation and durable event handoff; no runtime test reviewer
```

### P1-J2-01 typed context sections evidence (2026-09-18)

```text
source_snapshot: fa81cae + P1-J2-01 evidence slice; kiana-domain/src/prompts.rs; kiana-daemon/src/harness_skills.rs; kiana-core/src/lifecycle.rs; kiana-domain/tests/p1_j2_01_context.rs; kiana-core/tests/p1_j2_01_context_guard.rs; .github/workflows/p1-j2-01-context.yml; docs/roadmap/p1-j2-01-context-sections-baseline.md
worktree_status: PromptSection carries name/order/text/source/authority, render_prompt sorts deterministically, per-section provenance hashes exact text, and PromptBundle partitions Product versus Context sections; core/daemon harness assembly consumes the typed bundle without granting authority from context text; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/p1_j2_01_context.rs deterministic order/provenance fixtures; kiana-core/tests/p1_j2_01_context_guard.rs typed assembly source guard; GitHub Actions P1-J2-01 workflow runs domain fixtures, guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-J2-01 is triggered by the eventual push and is not awaited
status_change: P1-J2-01 source slice is implemented/reconciled. Typed context sections, deterministic rendering and provenance are explicit foundations for later budgeting and immutable step snapshots.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: complete ContextPlan selection/omission, provider tokenizer budget, role-pack source, immutable StepContext and retrieval/index durability remain P1-J2-02/03/04, H20/CM work
reviewer: Codex root implementation review plus P1-J2-01 section typing, deterministic render, authority separation and provenance invariants; no runtime test reviewer
```

### P1-J2-02 context budget evidence (2026-09-18)

```text
source_snapshot: 9d3620d + P1-J2-02 evidence slice; kiana-domain/src/{prompts,model}.rs; kiana-ports/src/model.rs; kiana-runner/src/harness.rs; kiana-domain/tests/p1_j2_02_budget.rs; kiana-core/tests/p1_j2_02_budget_guard.rs; .github/workflows/p1-j2-02-budget.yml; docs/roadmap/p1-j2-02-budget-baseline.md
worktree_status: TokenBudget now has explicit message/system/tool-schema/reserved-output components and conservative framing reserve; ModelRequestContext and PreparedModelCall share the same budget, and zero/overflow limits fail closed before provider invocation; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/p1_j2_02_budget.rs component accounting/overflow fixtures; kiana-core/tests/p1_j2_02_budget_guard.rs prepared-call/system-tool source guard; GitHub Actions P1-J2-02 workflow runs fixtures, guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-J2-02 is triggered by the eventual push and is not awaited
status_change: P1-J2-02 source slice is implemented/reconciled. Tool schemas and system prompt are no longer omitted from the prepared request budget, and exhaustion is a structured pre-send refusal.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: accounting remains a conservative local UTF-8/framing estimate, not provider-specific tokenizer or billing proof; role prompt provider wire, cache/compaction, immutable step snapshot and usage reconciliation remain P1-J2-03/04/H20/H21/P4/CP work
reviewer: Codex root implementation review plus P1-J2-02 budget component coverage, shared prepared-call validation and fail-closed overflow invariants; no runtime test reviewer
```

### P1-J2-03 role prompt provider wiring evidence (2026-09-18)

```text
source_snapshot: 1f34d13 + P1-J2-03 evidence slice; kiana-domain/src/{prompts,roles}.rs; kiana-core/src/lifecycle.rs; kiana-runner/src/harness.rs; kiana-daemon/src/model_client.rs; kiana-core/tests/p1_j2_03_role_prompt_guard.rs; .github/workflows/p1-j2-03-role-prompt.yml; docs/roadmap/p1-j2-03-role-prompt-baseline.md
worktree_status: ControlPlane builds a RoleSpec-bound PromptBundle, Runner carries its encoded bundle, and daemon ProviderModelClient decodes the exact Product-authority system prompt and sends it in the provider system field; product safety remains before role text and environment additions stay policy-subject; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p1_j2_03_role_prompt_guard.rs role/system/safety source guards; existing daemon harness prompt bundle fixture remains CI-only; GitHub Actions P1-J2-03 workflow runs the guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-J2-03 is triggered by the eventual push and is not awaited
status_change: P1-J2-03 source slice is implemented/reconciled. Assigned role prompt now reaches provider system content through the existing harness path without becoming capability authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: provider-native framing/tokenizer and live route proof, role-pack loading/trust/invalidation, immutable StepContext and exact snapshot/cache remain P1-J2-04/H20/H21/EXT work
reviewer: Codex root implementation review plus P1-J2-03 RoleSpec/PromptBundle/provider system routing, safety ordering and authority separation; no runtime test reviewer
```

### P1-J2-04 role-pack source and hash evidence (2026-09-18)

```text
source_snapshot: fd35cfe + P1-J2-04 evidence slice; kiana-domain/src/{roles,prompts}.rs; kiana-domain/role-packs/*.md; kiana-daemon/src/harness_skills.rs; kiana-core/src/{lifecycle,recovery,receipts}.rs; kiana-domain/tests/p1_j2_04_role_pack.rs; kiana-core/tests/p1_j2_04_role_pack_guard.rs; .github/workflows/p1-j2-04-role-pack.yml; docs/roadmap/p1-j2-04-role-pack-baseline.md
worktree_status: all nine RoleSpec factories load compile-time role-pack files and derive stable prompt_hash; PromptBundle validates exact role content, lifecycle/receipt/snapshot facts carry the hash, and ProjectTrust-gated skills/extensions remain bounded Context sections; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/p1_j2_04_role_pack.rs nine role-pack/hash/bundle fixtures; kiana-core/tests/p1_j2_04_role_pack_guard.rs receipt/resume/trust source guard; GitHub Actions P1-J2-04 workflow runs fixtures, guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-J2-04 is triggered by the eventual push and is not awaited
status_change: P1-J2-04 source slice is implemented/reconciled. Role-pack source and prompt hash are now explicit and reproducible across PromptBundle, assignment, snapshot and receipt boundaries.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: role packs remain compile-time bundled without signed/hot updates; provider exact framing/cache, immutable StepContext and cross-process catalog recovery remain EXT/SC/DEP/H20/H21 work
reviewer: Codex root implementation review plus P1-J2-04 role-pack source, hash propagation, ProjectTrust context boundary and resume drift fence; no runtime test reviewer
```

### P1-J3-02 memory retrieval and ACL evidence (2026-09-18)

```text
source_snapshot: 9ffe7e2 + P1-J3-02 evidence slice; kiana-daemon/src/{harness_memory,memory_retrieval}.rs; kiana-domain/src/{context_scope,roles,memory}.rs; kiana-core/src/receipts.rs; kiana-daemon/tests/daemon_host.rs; kiana-daemon/tests/p1_j3_02_memory.rs; .github/workflows/p1-j3-02-memory.yml; docs/roadmap/p1-j3-02-memory-retrieval-baseline.md
worktree_status: server-derived MemoryScope and RoleSpec knowledge grants reject unauthorized collections before file reads; searchable/revoked/scratch filters feed deterministic sparse/dense/RRF/MMR ranking with score components, and receipt projection attaches retrieval event/request/query/role provenance; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-daemon/tests/p1_j3_02_memory.rs ACL/ranking/receipt source guards; existing daemon_host project-hit and unauthorized-collection runtime fixtures; GitHub Actions P1-J3-02 workflow runs the guard, selected receipt regression and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-J3-02 is triggered by the eventual push and is not awaited
status_change: P1-J3-02 source slice is implemented/reconciled. Memory hits now carry explicit relevance/provenance and remain bounded by server ACLs through both scope and role grants.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: durable index/cache/projector, exact hybrid model health, context selection/omission and deletion/revocation propagation remain P1-J3-04/CM/PD/SC work; no network embedding service is introduced
reviewer: Codex root implementation review plus P1-J3-02 ACL-before-read, ranking metadata, provenance and receipt projection invariants; no runtime test reviewer
```

### P1-J3-03 memory proposal and admission evidence (2026-09-18)

```text
source_snapshot: a9d1e1e + P1-J3-03 evidence slice; kiana-domain/src/{memory_proposals,memory_distillation}.rs; kiana-core/src/{memory_distillation,memory_proposals}.rs; kiana-daemon/src/harness_memory.rs; kiana-domain/tests/p1_j3_03_memory_proposal.rs; kiana-core/tests/p1_j3_03_memory_proposal_guard.rs; .github/workflows/p1-j3-03-memory-proposal.yml; docs/roadmap/p1-j3-03-memory-proposal-baseline.md
worktree_status: terminal/meeting output enters a bounded distillation job; strict MemoryProposal validates evidence quotes, ADD/UPDATE/DELETE targets and top-three similar records; extraction failure emits an incident without rewriting the source run; memory.review is the only materialization path from Candidate/Draft to Qualified/Active or Rejected, with scratch as the explicit ephemeral exception; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/p1_j3_03_memory_proposal.rs evidence/similar/target validation fixtures; kiana-core/tests/p1_j3_03_memory_proposal_guard.rs distillation/review source guard; GitHub Actions P1-J3-03 workflow runs fixtures, guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-J3-03 is triggered by the eventual push and is not awaited
status_change: P1-J3-03 source slice is implemented/reconciled. Memory extraction proposals are evidence-bound candidates and require the existing human review path for persistent admission.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: provider-generated extraction is still bounded by the local distillation adapter; cross-process queue/projector, durable proposal recovery, hybrid index and deletion/revocation propagation remain P1-J3-04/CM/PD/SC work
reviewer: Codex root implementation review plus P1-J3-03 evidence/target validation, bounded similar records and three-tier admission invariants; no runtime test reviewer
```

### P1-J3-04 hybrid retrieval evidence (2026-09-18)

```text
source_snapshot: 335e39d + P1-J3-04 evidence slice; kiana-daemon/src/{memory_retrieval,harness_memory}.rs; kiana-domain/src/memory.rs; kiana-core/src/receipts.rs; kiana-daemon/tests/p1_j3_04_hybrid_retrieval.rs; .github/workflows/p1-j3-04-hybrid-retrieval.yml; docs/roadmap/p1-j3-04-hybrid-retrieval-baseline.md; docs/roadmap.md
worktree_status: J3-02 ACL-filtered records now use one deterministic CJK-aware BM25/sparse plus optional pinned local token-vectors cosine channel, RRF-60 and MMR-0.7 ranking; model manifest/schema/dimensions/format/file/hash checks fail closed, missing model reports sparse degraded, and hit/harness/receipt provenance carries model and algorithm metadata; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-daemon/src/memory_retrieval.rs private pinned-model fixture test; kiana-daemon/tests/p1_j3_04_hybrid_retrieval.rs fail-closed/source contract guard; GitHub Actions P1-J3-04 workflow runs the deterministic fixture, guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-J3-04 is triggered by the eventual push and is not awaited
status_change: P1-J3-04 source slice is implemented/reconciled. Hybrid ranking is deterministic for a pinned local fixture model, with explicit sparse degradation and hash-bound model identity; no network embedding path was introduced.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: ONNX/ort inference and production model registry/download remain deferred; token-vectors fixture is not live provider evidence; durable index/cache, selection/sent/cited layering, deletion/revocation propagation and cross-process model/index recovery remain CM/PD/SC work
reviewer: Codex root implementation review plus BM25/CJK, pinned hash, RRF/MMR determinism, degradation and receipt provenance invariants; no runtime test reviewer
```

### P1-J4-01 MCP lifecycle evidence (2026-09-18)

```text
source_snapshot: bdca70a + P1-J4-01 evidence slice; kiana-domain/src/actions.rs; kiana-capability-broker/src/lib.rs; kiana-daemon/src/{harness_mcp,mcp_stdio}.rs; kiana-daemon/tests/p1_j4_01_mcp.rs; .github/workflows/p1-j4-01-mcp.yml; docs/roadmap/p1-j4-01-mcp-baseline.md; docs/roadmap.md
worktree_status: server-owned descriptors bind mcp.discover/mcp.call to Network risk/resource/schema/binding/reconciliation/idempotency metadata; stdio-only registry pins config/executable/argument files and environment, operator-only discovery commits protocol/tool catalog/version/health facts, prepared calls recheck trust/scope/pins/schema/drift, and per-invocation process groups report cancellation/stop/Unknown without false success; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: existing kiana-daemon/src/harness_mcp.rs schema/result/size/unknown fixtures; kiana-daemon/tests/p1_j4_01_mcp.rs descriptor/trust/stdio/health/process source guard; GitHub Actions P1-J4-01 workflow runs MCP boundary fixtures, guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-J4-01 is triggered by the eventual push and is not awaited
status_change: P1-J4-01 source slice is implemented/reconciled. MCP discovery and call now expose traceable protocol/tool catalog/version/health facts while preserving the single ControlPlane→Broker→stdio adapter path and fail-closed Unknown/cancellation semantics.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: HTTP/SSE/WS and legacy service client remain outside the product path; health is a successful local initialize/tools-list snapshot, not a remote business-health or external-effect receipt; durable health heartbeat/pool, cross-process discovery recovery, external authorization and live/physical outcome proof remain CAP/PD/ER/SC/INT work
reviewer: Codex root implementation review plus descriptor binding, trust/scope pin, schema/result, health/version, process-group stop and Unknown boundary review; no runtime test reviewer
```

### P1-J8-01 observability receipt evidence (2026-09-18)

```text
source_snapshot: b04bbad + P1-J8-01 evidence slice; kiana-core/src/{receipts,capability_attempt_projection,model_attempt_projection,span_projection,redaction}.rs; kiana-domain/src/observability.rs; kiana-policy/src/security.rs; kiana-core/tests/p1_j8_01_observability.rs; .github/workflows/p1-j8-01-observability.yml; docs/roadmap/p1-j8-01-observability-baseline.md; docs/roadmap.md
worktree_status: committed EventLog-only run receipts now include bounded observability projection for source cursor/event IDs and persistence revision, typed model/provider/usage/retry, capability admission/effect/stop/action digest, run/turn/invocation spans, policy/gate verdict/reason, tool args hash and cancellation reason; existing redaction boundary is retained and projection invokes no Broker/Provider/Runner; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p1_j8_01_observability.rs receipt/projection/redaction source guard; existing kiana-core/tests/oa07_span_lifecycle.rs, oa08_model_instrumentation.rs and oa09_capability_instrumentation.rs CI fixtures; GitHub Actions P1-J8-01 workflow runs the guard, OA-08/OA-09 projection fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-J8-01 is triggered by the eventual push and is not awaited
status_change: P1-J8-01 source slice is implemented/reconciled. Run receipts now make provider/model, policy/gate, action/args digest, usage/retry/cancel and persistence/source provenance jointly queryable without exposing secret-bearing payloads.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: observability and typed projections remain re-buildable from EventLog without durable projector checkpoint; tool_args_hash is normalized action/args fingerprint rather than external receipt, persistence_revision is a source digest rather than cross-store fsync transaction, and metric/trace exporters, provider invoice/cost reconciliation, cross-process query and live/physical telemetry remain OA/PD/ER/SC work
reviewer: Codex root implementation review plus source-only receipt/projection/redaction and no-execution-boundary review; no runtime test reviewer
```

### P1-K5-01 cost/capacity evidence (2026-09-18)

```text
source_snapshot: 30e83ef + P1-K5-01 evidence slice; kiana-domain/src/{usage,budget_contracts,company}.rs; kiana-core/src/{model_budget,cell_registry,receipts}.rs; kiana-runner/src/budget.rs; kiana-domain/tests/p1_k5_01_cost_capacity.rs; kiana-core/tests/p1_k5_01_cost_capacity_guard.rs; .github/workflows/p1-k5-01-cost-capacity.yml; docs/roadmap/p1-k5-01-cost-capacity-baseline.md; docs/roadmap.md
worktree_status: UsageRecord/CostLedger preserve known token totals and unknown cost; Company keeps ProjectBudget, RuntimeBudget and Quota separate; Runner/Core/CP-11 reserve and settle bounded model/tool budgets before provider/effect and keep unknown usage conservative; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/p1_k5_01_cost_capacity.rs UsageRecord/CostLedger and Runtime/Project/Quota separation fixtures; kiana-core/tests/p1_k5_01_cost_capacity_guard.rs source guard; existing CP-11 and H-07 fixtures; GitHub Actions P1-K5-01 workflow runs all listed fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-K5-01 is triggered by the eventual push and is not awaited
status_change: P1-K5-01 source slice is implemented/reconciled. Runtime usage, project budget and quota semantics are typed and separated; missing usage/cost remains Unknown rather than zero or paid.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: pricing/rate cards, provider invoice/correction/refund, durable quota projector, cross-process atomic settlement, capacity backend and external/live/physical billing evidence remain BQ/CP/PD/ER work; CostLedger is an observation projection and does not prove payment or provider acceptance
reviewer: Codex root implementation review plus typed budget separation, conservative usage, provider-before-reservation and no-second-ledger boundary review; no runtime test reviewer
```

### P1-L1-01 GoldenTrace evidence (2026-09-18)

```text
source_snapshot: 10d682a4 + P1-L1-01 evidence slice; kiana-core/src/{versioning,receipts,eval}.rs; kiana-domain/src/quality.rs; kiana-core/tests/p1_l1_01_golden_trace.rs; kiana-domain/tests/eq03_eval_objects.rs; .github/workflows/p1-l1-01-golden-trace.yml; docs/roadmap/p1-l1-01-golden-trace-baseline.md; docs/roadmap.md
worktree_status: trace.capture binds an authenticated/trusted owner/project/run to controlled source manifest/hash, input/events/runtime version and receipt, appending a new golden_trace.captured fact; trace.replay validates owner/project/event digest/data revocation and only folds history/invocations with side_effects=false/provider_calls=0; strict domain GoldenTrace remains the typed contract; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p1_l1_01_golden_trace.rs capture/replay source guard; kiana-domain/tests/eq03_eval_objects.rs strict GoldenTrace object fixtures; existing core eval_baseline/provider-independent replay regression; GitHub Actions P1-L1-01 workflow runs guard, object fixtures, eval baseline and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-L1-01 is triggered by the eventual push and is not awaited
status_change: P1-L1-01 source slice is implemented/reconciled. GoldenTrace capture/replay now has explicit source/input/version/receipt binding and a no-side-effect replay contract.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: legacy capture JSON and strict GoldenTrace are not yet unified through durable EvalStore/FixtureStore, normalizer/diff, isolated runner, Judge, experiment/result/gate or promote/rollback; replay proves local projection behavior only, not model quality, business Outcome, payment, external effect or live/physical evidence
reviewer: Codex root implementation review plus owner/source/hash/revocation/no-side-effect replay boundary review; no runtime test reviewer
```

### P1-L4-01 code intelligence evidence (2026-09-18)

```text
source_snapshot: cba747de + P1-L4-01 evidence slice; kiana-query/src/{repo_map,index}.rs; kiana-daemon/src/context_query.rs; kiana-core/src/context_query.rs; kiana-query/tests/p1_l4_01_code_intelligence.rs; .github/workflows/p1-l4-01-code-intelligence.yml; docs/roadmap/p1-l4-01-code-intelligence-baseline.md; docs/roadmap.md
worktree_status: RepoMap/ContextIndex/Search/Vector/Pack/Artifact graph retain exact content hashes, canonical relative paths and bounded source fields; daemon render_output adds source_snapshot, local_workspace provenance, captured_at_read freshness and runtime version for JSON/text; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-query/tests/p1_l4_01_code_intelligence.rs RepoMap/Index/Search/Vector/Pack/provenance source guard; GitHub Actions P1-L4-01 workflow runs the guard and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-L4-01 is triggered by the eventual push and is not awaited
status_change: P1-L4-01 source slice is implemented/reconciled. Code intelligence outputs now expose content/source snapshot and read-time freshness without changing visibility or creating an execution path.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: source_snapshot is a read-time hash summary rather than immutable workspace snapshot; generation/atomic index switch, cross-process locking/recovery, semantic provider index, deletion/retention propagation, chunk provenance and selected/sent/cited layers remain CM/PD/OA/SC work
reviewer: Codex root implementation review plus canonical-root, hash/provenance/freshness and no-authority/no-provider-boundary review; no runtime test reviewer
```

### P2-J5-01 workflow replay evidence (2026-09-18)

```text
source_snapshot: 789b0425 + P2-J5-01 evidence slice; kiana-domain/src/automation.rs; kiana-workflow/src/{lib,durable}.rs; kiana-core/src/automation.rs; kiana-workflow/tests/p2_j5_01_replay.rs; kiana-core/tests/p2_j5_01_workflow_guard.rs; .github/workflows/p2-j5-01-workflow.yml; docs/roadmap/p2-j5-01-workflow-baseline.md; docs/roadmap.md
worktree_status: versioned WorkflowDefinition/node/instance contracts and pure plan_command enforce DAG, role/budget/deadline, input digest, approval/signal/cancel/retry/compensation/Unknown boundaries; ControlPlane loads/replays workflow EventLog, commits CAS/idempotent command facts and dispatches only after durable reservation through the existing execution spine; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-workflow/tests/p2_j5_01_replay.rs deterministic fixed-authority/definition/command replay fixture; kiana-core/tests/p2_j5_01_workflow_guard.rs single-spine/CAS/idempotency/source guard; existing workflow state and automation baseline fixtures; GitHub Actions P2-J5-01 workflow runs all listed fixtures and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P2-J5-01 is triggered by the eventual push and is not awaited
status_change: P2-J5-01 source slice is implemented/reconciled. Workflow definition versions are immutable, replay uses the same pure planner and idempotency/CAS facts, and only post-commit effects reach the existing ControlPlane/Broker path.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: scheduler/queue/claim/ClockPort, automatic occurrence recovery, cross-process projector/power-loss reconciliation and external workflow backend remain AUT/SW/PD/ER work; WorkflowEffect and local EventLog wiring do not prove provider/Broker/external outcome or compensation success/live/physical behavior
reviewer: Codex root implementation review plus definition-version, planner determinism, CAS/idempotency, evidence/terminal and single-execution-spine boundary review; no runtime test reviewer
```

### P2-K3-01 Human Inbox evidence (2026-09-18)

```text
source_snapshot: 08349bf2 + P2-K3-01 evidence slice; kiana-core/src/platform.rs; kiana-core/src/{commands,approvals,company}.rs; kiana-domain/src/platform.rs; kiana-core/tests/p2_k3_01_human_inbox.rs; kiana-core/tests/sc10_approval_binding.rs; .github/workflows/p2-k3-01-human-inbox.yml; docs/roadmap/p2-k3-01-human-inbox-baseline.md; docs/roadmap.md
worktree_status: Core human_items aggregates pending approvals, Company review/acceptance/delivery/change/cancel, Company/failure incidents, reconciliation and feedback candidates as stable sorted HumanInboxItem kinds; human.resolve requires inbox digest, item/action IDs, required fields and idempotency, then routes to the original approval/company/failure/feedback authority; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p2_k3_01_human_inbox.rs six-kind/list digest/resolve source guard; kiana-core/tests/sc10_approval_binding.rs self-approval/expiry/one-shot/raw-field fixtures; GitHub Actions P2-K3-01 workflow runs guard, approval regression and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P2-K3-01 is triggered by the eventual push and is not awaited
status_change: P2-K3-01 source slice is implemented/reconciled. Human decisions now share one bounded inbox projection and resolve route without granting the Inbox independent authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: HumanInboxItem/list revision is rebuilt from Approval/Company/EventLog projections; durable NotificationStore/read-state/outbox/delivery/recipient channel, cross-process inbox checkpoint, external human authentication, retention/withdraw/supersede and live/physical delivery remain NM/UI/PD/SC work
reviewer: Codex root implementation review plus six-kind aggregation, stale/action-field fences, original-authority routing and no-second-loop boundary review; no runtime test reviewer
```

### P2-K4-01 artifact checkpoint evidence (2026-09-18)

```text
source_snapshot: 25ac3ddb + P2-K4-01 evidence slice; kiana-domain/src/platform.rs; kiana-core/src/workspace_checkpoints.rs; kiana-core/src/{dispatch,lifecycle}.rs; kiana-daemon/src/{workspace_checkpoints,apply_patch}.rs; kiana-core/tests/p2_k4_01_checkpoint.rs; .github/workflows/p2-k4-01-checkpoint.yml; docs/roadmap/p2-k4-01-artifact-checkpoint-baseline.md; docs/roadmap.md
worktree_status: WorkspaceCheckpoint binds project/actor/session/role, run/invocation, transcript offset, path allow, files/data epoch and workspace revision; capture runs before input/write, preview is read-only, restore rechecks exact snapshot/revision/company/path/data/approval and reuses apply_patch transaction, invalidating old approvals/runs and recording workspace.restored; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p2_k4_01_checkpoint.rs checkpoint field/restore invalidation source guard; existing P1-H03 path and ER-07 replay checkpoint fixtures; GitHub Actions P2-K4-01 workflow runs guard, path/replay regressions and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P2-K4-01 is triggered by the eventual push and is not awaited
status_change: P2-K4-01 source slice is implemented/reconciled. Edit-level checkpoint capture/preview/restore is bound to transcript/workspace/invocation facts and invalidates stale approval/run context before restore writes.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: cross-process durable CheckpointService, ArtifactStore version graph, power-loss atomicity, projector checkpoint, backup/restore and external/database/generated-artifact undo remain PD/ER/DEP work; captured file snapshots do not prove external provider/MCP/DB effects or live/physical rollback
reviewer: Codex root implementation review plus snapshot/revision/data/path/TOCTOU, approval/run invalidation and descriptor-relative transaction boundary review; no runtime test reviewer
```

### P2-K6-01 reliability and reconciliation evidence (2026-09-18)

```text
source_snapshot: c0723fa8 + P2-K6-01 evidence slice; kiana-domain/src/platform.rs; kiana-domain/tests/p2_k6_01_reliability.rs; kiana-core/src/platform.rs; kiana-core/src/commands.rs; kiana-core/tests/p2_k6_01_reliability.rs; kiana-core/tests/oa19_incident_projection.rs; kiana-core/tests/er10_resource_projection.rs; kiana-daemon/src/lib.rs; .github/workflows/p2-k6-01-reliability.yml; docs/roadmap/p2-k6-01-reliability-baseline.md; docs/roadmap.md
worktree_status: FailureClass covers crash/timeout/cancel/disk-full/MCP failure/provider Unknown; each class has a bounded RecoveryPlan and FailureIncident projection. failure.incidents rebuilds committed failure facts, unresolved Unknown enters the Human Inbox reconciliation view, failure.reconcile requires explicit resolution plus owned event evidence/revision/CAS/idempotency, and failure.release requires prior reconciliation plus confirmed stop before resource release; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/p2_k6_01_reliability.rs six FailureClass recovery fixture; kiana-core/tests/p2_k6_01_reliability.rs incident/recovery source guard; existing OA-19 observability incident and ER-10 resource quarantine projections; GitHub Actions P2-K6-01 workflow runs fixtures, guards, regressions and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P2-K6-01 is triggered by the eventual push and is not awaited
status_change: P2-K6-01 source slice is implemented/reconciled. All six failure classes now have explicit incident/recovery contracts; result_unknown remains reconciliation-required and cannot be automatically retried, released or rewritten as success.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: Incident/Recovery and reconciliation queue are EventLog/platform projections, not an independent cross-process IncidentStore or background worker; no automatic provider query, external receipt, power-loss durability, cross-process physical stop/release proof, external business outcome, live connector/provider or physical evidence is claimed
reviewer: Codex root implementation review plus six-class classification, Unknown/effect/stop fences, evidence/CAS/idempotency, release quarantine and no-second-loop boundary review; no runtime test reviewer
```

### P2-K7-01 data governance and deletion propagation evidence (2026-09-18)

```text
source_snapshot: 4ff8d5e9 + P2-K7-01 evidence slice; kiana-domain/src/governance.rs; kiana-domain/tests/p2_k7_01_data_governance.rs; kiana-core/src/data_governance.rs; kiana-core/src/workspace_checkpoints.rs; kiana-core/tests/p2_k7_01_data_governance.rs; kiana-core/tests/oa20_data_governance_projection.rs; kiana-daemon/src/data_governance.rs; kiana-daemon/src/harness_memory.rs; kiana-daemon/src/context_query.rs; kiana-domain/tests/oa20_data_governance.rs; .github/workflows/p2-k7-01-data-governance.yml; docs/roadmap/p2-k7-01-data-governance-baseline.md; docs/roadmap.md
worktree_status: DataPolicy binds DataClass/Purpose/ProcessingGrant/Retention, policy digest/revision and data epoch; revoke cascades parent grants and increments epoch. DataGovernanceSnapshot preserves source cursor/event IDs and propagates Available/Expired/Revoked/Unknown to receipt/audit/artifact/memory/index/cache/export. daemon data.governance persists revocation before cleaning governed memory/cache/index/artifact-store/compaction, and checkpoint/runner paths reject stale data epochs; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/p2_k7_01_data_governance.rs revoke/expiry/seven-store propagation fixture; kiana-core/tests/p2_k7_01_data_governance.rs governance source guard; existing OA-20 domain/core snapshot fixtures; GitHub Actions P2-K7-01 workflow runs fixtures, guards, regressions and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P2-K7-01 is triggered by the eventual push and is not awaited
status_change: P2-K7-01 source slice is implemented/reconciled. Revocation, expiry and deletion now carry explicit policy/data-epoch semantics and propagate conservatively across governed derived stores; pending or uncertain propagation remains Unknown/Revoked and never becomes an unverified success.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: EventLog facts remain immutable and the snapshot is a rebuildable projection; no independent cross-process governance store/retention worker, power-loss recovery, external DB/provider/MCP/backup/log erasure, generated-artifact cleanup, or live/physical deletion proof is claimed
reviewer: Codex root implementation review plus policy/grant/retention validation, cursor/digest/epoch fences, pending-vs-committed propagation, path/hash/lock safety and no-second-fact-source boundary review; no runtime test reviewer
```

### P2-L2-01 feedback and candidate evidence (2026-09-18)

```text
source_snapshot: 73660c91 + P2-L2-01 evidence slice; kiana-domain/src/platform.rs; kiana-domain/src/quality.rs; kiana-core/src/platform.rs; kiana-core/src/versioning.rs; kiana-core/tests/p2_l2_01_feedback.rs; kiana-core/tests/p2_k3_01_human_inbox.rs; .github/workflows/p2-l2-01-feedback.yml; docs/roadmap/p2-l2-01-feedback-baseline.md; docs/roadmap.md
worktree_status: feedback.submit validates bounded category/observation/proposal, evidence, candidate id and platform revision/idempotency before appending FeedbackCandidate; feedback.review requires independent reviewer/sponsor session, evidence and quality outcome, rejects duplicate/self review, and appends candidate-only review with authority_changes_applied=false. Feedback remains a projection/candidate path and does not mutate Role/Grant/Policy/Approval/Receipt or historical facts; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p2_l2_01_feedback.rs candidate-only source guard; existing P2-K3 Human Inbox feedback routing guard; GitHub Actions P2-L2-01 workflow runs source guard, inbox regression and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P2-L2-01 is triggered by the eventual push and is not awaited
status_change: P2-L2-01 source slice is implemented/reconciled. Feedback now produces evidence-bound candidates and independently reviewed candidate-only facts; no direct authority or policy mutation is exposed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: candidate facts live in the protected human-operations stream; no independent FeedbackStore, automatic diagnosis/promotion worker, quality evaluator, rollback path, external effect or live model-quality outcome is claimed; promotion remains an EQ/ER/SC gate concern
reviewer: Codex root implementation review plus candidate lifecycle, evidence/revision/idempotency, independent review, candidate-only marker and no-policy/history-mutation boundary review; no runtime test reviewer
```

### P2-M2-01 UI projection evidence (2026-09-18)

```text
source_snapshot: 130e4d97 + P2-M2-01 evidence slice; kiana-protocol/src/{ui_contracts,lib}.rs; kiana-protocol/tests/ui01_dto.rs; kiana-daemon/src/{run_stream,lib}.rs; kiana-daemon/tests/p2_m2_01_ui_projection.rs; kiana-entrypoints/src/{web.rs,web_page.html,workbench_chat.rs}; .github/workflows/p2-m2-01-ui-projection.yml; docs/roadmap/p2-m2-01-ui-projection-baseline.md; docs/roadmap.md
worktree_status: versioned UI snapshot/feed/action/result DTOs validate schema, digest, cursor/epoch, revision, pending action and unknown fields; DaemonHost projects owner-scoped EventLog session/run/status/pending facts, RunStreamBus atomically rejects stale/replayed UI actions, and subscribe_after/Web/Workbench signal gaps and require snapshot hydration; UI state remains projection/precondition and never a second authority; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-daemon/src/run_stream.rs stale_ui_action_is_rejected_by_epoch unit fixture; kiana-daemon/tests/p2_m2_01_ui_projection.rs cross-surface source guard; existing kiana-protocol/tests/ui01_dto.rs versioned DTO fixture; GitHub Actions P2-M2-01 workflow runs stale-action fixture, source guard, DTO regression and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P2-M2-01 is triggered by the eventual push and is not awaited
status_change: P2-M2-01 source slice is implemented/reconciled. Unified UI snapshot/action/cursor/epoch contracts and stale/gap fencing now cover protocol, DaemonHost, Web and Workbench projections.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: RunStreamBus cursor and replay keys remain bounded process-local state; durable cross-process UI cursor/instance/read-state, notification delivery, multi-tab/live reconnect, browser/OS effect and external human authentication are not claimed and remain UI/NM/PD/SC work
reviewer: Codex root implementation review plus schema/digest/cursor/epoch, owner-scoped snapshot, stale/replay action, gap hydration and no-second-loop authority boundary review; no runtime test reviewer
```

### P2-M3-01 human action cards evidence (2026-09-18)

```text
source_snapshot: 35e9e9eb + P2-M3-01 evidence slice; kiana-domain/src/platform.rs; kiana-protocol/src/{ui_contracts,lib}.rs; kiana-core/src/platform.rs; kiana-core/tests/p2_m3_01_action_cards.rs; kiana-daemon/src/{lib,run_stream}.rs; kiana-daemon/tests/p2_m2_01_ui_projection.rs; kiana-entrypoints/src/{web.rs,workbench_chat.rs,cli.rs,product_command.rs,command_dispatch.rs}; .github/workflows/p2-m3-01-action-cards.yml; docs/roadmap/p2-m3-01-action-card-baseline.md; docs/roadmap.md
worktree_status: Core HumanAction and protocol HumanActionCard carry target/command/owner/epoch/revision/allowed decisions/required fields/digest metadata. Human Inbox/resolve and approval challenge paths expose server-issued action metadata; Web, Workbench and CLI submit through the same DaemonHost/ControlPlane authority, while stale/owner/expiry/field/scope/idempotency checks remain server-side; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-daemon/tests/p2_m2_01_ui_projection.rs action metadata/cursor source guard; kiana-core/src/platform.rs HumanAction aggregation and resolve path; GitHub Actions P2-M3-01 workflow runs action-card source guard, P2-M2 UI regression and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P2-M3-01 is triggered by the eventual push and is not awaited
status_change: P2-M3-01 source slice is implemented/reconciled. Approval/Review/Acceptance/Incident action metadata is generated by the server and reused by the entrypoint paths; surfaces cannot expand available decisions or execute outside ControlPlane.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: action metadata and replay keys are currently bounded projections/challenges, with durable cross-process action-card/read-state, external human authentication/delivery, full three-surface runtime parity and live/physical proof remaining UI/NM/PD/SC work
reviewer: Codex root implementation review plus action target/owner/epoch/revision/digest, server-issued decision set, stale/expiry/field/idempotency and no-second-authority boundary review; no runtime test reviewer
```

### P2-M4-01 run/artifact detail evidence (2026-09-18)

```text
source_snapshot: b6c555e0 + P2-M4-01 evidence slice; kiana-protocol/src/ui_contracts.rs; kiana-core/src/{receipts,invocation_projection,artifacts,company_business}.rs; kiana-core/tests/p2_m4_01_run_artifact_detail.rs; kiana-daemon/src/lib.rs; kiana-entrypoints/src/{web.rs,web_thread.rs,workbench_chat.rs,cli.rs}; .github/workflows/p2-m4-01-run-artifact-detail.yml; docs/roadmap/p2-m4-01-run-artifact-detail-baseline.md; docs/roadmap.md
worktree_status: receipt_from_events/aggregate_receipt_facts and invocation/capability projections retain run/invocation/event/source cursor, execution receipt, files, artifact/evidence/provider refs and redacted aggregation; Web web_thread, Workbench and CLI/DaemonHost receipt paths render the same committed facts, with owner mismatch/terminal conflict/Unknown preserved; no detail surface writes facts or executes Broker/Provider/Runner; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p2_m4_01_run_artifact_detail.rs cross-location source guard; existing P2-K4 checkpoint and Web thread projection regressions; GitHub Actions P2-M4-01 workflow runs guard, receipt/checkpoint, Web thread regressions and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P2-M4-01 is triggered by the eventual push and is not awaited
status_change: P2-M4-01 source slice is implemented/reconciled. Run timeline, invocation/execution, file/diff, evidence/artifact references and receipt are connected by committed run/event/invocation identifiers and shared owner-scoped projections.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: no independent typed RunDetail endpoint, durable detail index, artifact diff store or cross-process UI cache exists; large evidence/artifact pagination, full four-surface visual/runtime parity, external human auth and live/physical effect or rollback proof remain UI/PD/ER/DEP/SC work
reviewer: Codex root implementation review plus shared receipt aggregation, invocation/source refs, artifact/evidence linkage, redaction/owner/Unknown and no-second-loop detail boundary review; no runtime test reviewer
```

### P2-M5-01 Web snapshot sync evidence (2026-09-18)

```text
source_snapshot: 4d445730 + P2-M5-01 evidence slice; kiana-entrypoints/src/{web.rs,web_page.html}; kiana-entrypoints/tests/cli_web.rs; kiana-entrypoints/tests/p2_m5_01_web_sync.rs; kiana-daemon/src/run_stream.rs; kiana-protocol/src/lib.rs; .github/workflows/p2-m5-01-web-sync.yml; docs/roadmap/p2-m5-01-web-sync-baseline.md; docs/roadmap.md
worktree_status: Web snapshot-first hydration obtains /api/state and waits for EventSource onopen before /api/run; /api/events uses epoch/sequence/last-event-id and DaemonHost subscribe_after, emits machine-readable gap/error for initial attach, cursor gap, lag, epoch change or closed stream, and retains only bounded terminal replay. Browser refresh generations/cursors reject stale or duplicate deltas, mark incomplete and require receipt; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-entrypoints/tests/cli_web.rs web_sse_reconnect_emits_stream_gap_without_replaying_delta_items loopback reconnect fixture; kiana-entrypoints/tests/p2_m5_01_web_sync.rs source guard; existing Web gap/stream error unit regressions; GitHub Actions P2-M5-01 workflow runs reconnect acceptance, source guard, gap regression and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P2-M5-01 is triggered by the eventual push and is not awaited
status_change: P2-M5-01 source slice is implemented/reconciled. Snapshot hydration, cursor/epoch/gap handling, refresh ordering and no-duplicate-delta reconnect semantics are fixed across Web/DaemonHost/RunStreamBus.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: stream retention/cursors/action keys remain bounded daemon-process projections; durable cross-process cursor/read-state, notification/multi-tab delivery, browser/OS transport, external/live provider effects and physical proof are not claimed and remain UI/NM/PD/SC work
reviewer: Codex root implementation review plus snapshot-first ordering, last-event-id parsing, epoch/sequence gap, terminal replay, refresh generation and receipt-authoritative boundary review; no runtime test reviewer
```

### P2-M7-01 accessible fallback evidence (2026-09-18)

```text
source_snapshot: 5204cc0b + P2-M7-01 evidence slice; kiana-entrypoints/src/{web_page.html,web.rs,workbench_chat.rs,workbench.rs}; kiana-entrypoints/tests/p2_m7_01_accessibility.rs; contrib/desktop/main.js; .github/workflows/p2-m7-01-accessibility.yml; docs/roadmap/p2-m7-01-accessibility-baseline.md; docs/roadmap.md
worktree_status: Web/TTY/Desktop fallback exposes status/error/receipt/incomplete/result_unknown text, ARIA labels/live regions, focus-visible/skip-link/focus restoration, keyboard Enter/Ctrl-Enter/Esc/Ctrl-C, narrow vertical layout, high-contrast/forced-colors and reduced-motion rules; Desktop reuses Web shell and no independent execution/status authority was added; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-entrypoints/tests/p2_m7_01_accessibility.rs accessibility source guard; existing Web page status/focus/stream tests and Workbench key tests; GitHub Actions P2-M7-01 workflow runs guard, Web regressions, TTY regression and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P2-M7-01 is triggered by the eventual push and is not awaited
status_change: P2-M7-01 source slice is implemented/reconciled. Safety-relevant state remains reachable without color, animation or a live stream, with text/ARIA/focus/keyboard and narrow/high-contrast/reduced-motion fallback.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: no browser/screen-reader/device automation, OS accessibility/notification packaging, external human auth, cross-process durable UI state or live/physical proof is claimed; fallback projections remain non-authoritative and defer to EventLog/Receipt/ControlPlane
reviewer: Codex root implementation review plus status-without-color, keyboard/focus, ARIA/live, narrow/high-contrast/reduced-motion and Desktop reuse boundary review; no runtime test reviewer
```

### P3-I-02 company command and event freeze evidence (2026-09-18)

```text
source_snapshot: a9b69edb + P3-I-02 evidence slice; kiana-domain/src/{company.rs,company_replay.rs,company_policy.rs}; kiana-domain/tests/co08_replay.rs; kiana-protocol/src/lib.rs; kiana-core/src/company.rs; kiana-core/tests/{p3_i02_company_commands,co08_replay_guard}.rs; .github/workflows/p3-i02-company-commands.yml; docs/roadmap/p3-i02-company-commands-baseline.md; docs/roadmap.md
worktree_status: nine foundational Company command/event pairs are fixed under versioned COMPANY_COMMAND_SCHEMA/COMPANY_EVENT_SCHEMA: Objective, Project, Milestone, Packet, Run reservation/observation, Acceptance request/decision, Project close and Outcome. Strict CompanyCommandRequest/CompanyEvent, CompanyCommandPolicy, aggregate stream metadata, revision/idempotency CAS and CompanyReplayReducer validate migration, gaps, duplicates, conflicts and role authority before state transition; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p3_i02_company_commands.rs nine-pair/schema/replay source guard; existing kiana-domain/tests/co08_replay.rs deterministic reducer/legacy v0 migration/unknown schema fixtures; kiana-core/tests/co08_replay_guard.rs reducer wiring; GitHub Actions P3-I-02 workflow runs all guards/regressions and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P3-I-02 is triggered by the eventual push and is not awaited
status_change: P3-I-02 source slice is implemented/reconciled. The foundational Company command/event vocabulary is versioned and frozen; start_run remains a two-phase reservation→observed-start fact rather than an unverified success claim.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: Company aggregate/replay remains local EventLog projection; full Objective→Project→Packet→Run→Acceptance→Delivery rebuild, cross-process durable index/power-loss recovery, migration beyond explicit v0 adapter, external business outcome and live/physical proof remain P3-I-03+ / CO/ER/PD/DEP work
reviewer: Codex root implementation review plus nine-pair vocabulary, strict schema/unknown-field, aggregate/revision/idempotency, replay/migration/gap/duplicate and start-run reservation-vs-observation boundary review; no runtime test reviewer
```

### P3-I-03 company chain replay evidence (2026-09-18)

```text
source_snapshot: 26ad2f63 + P3-I-03 evidence slice; kiana-domain/src/{company.rs,company_replay.rs}; kiana-core/src/{company.rs,receipts.rs,artifacts.rs}; kiana-core/tests/p3_i03_company_chain.rs; kiana-domain/tests/{co06_artifact,co07_receipt,co08_replay}.rs; kiana-core/tests/oa27_company_governance.rs; .github/workflows/p3-i03-company-chain.yml; docs/roadmap/p3-i03-company-chain-baseline.md; docs/roadmap.md
worktree_status: ControlPlane::load_company reads only the protected company aggregate stream and rebuilds CompanyState via CompanyReplayReducer; company_snapshot/view and runtime receipts project Objective/Project/Milestone/Packet/Run/Acceptance/Review/Delivery/ClosingReceipt from committed facts. Artifact/evidence refs verify path/hash/immutability/ownership/revocation, duplicate/gap/schema/revision/idempotency/terminal/approval/claim conflicts fail closed, and replay never dispatches execution; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p3_i03_company_chain.rs chain/reducer/artifact/evidence source guard; existing co08 replay, co06 artifact, co07 receipt and OA-27 Company governance fixtures; GitHub Actions P3-I-03 workflow runs guard, replay/artifact/receipt/governance regressions and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P3-I-03 is triggered by the eventual push and is not awaited
status_change: P3-I-03 source slice is implemented/reconciled. A fresh ControlPlane can rebuild the Company lifecycle and receipts from EventLog plus governed artifact/evidence references without treating reservations, runtime completion or projections as business outcome.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: aggregate/replay is local EventLog projection with explicit v0 adapter; no independent cross-process aggregate index/snapshot checkpoint, full historical upcast, power-loss recovery, external provider/MCP/DB/notification effect proof or live/physical business outcome is claimed
reviewer: Codex root implementation review plus aggregate stream/reducer/migration, object-chain/ref linkage, owner/revision/idempotency, artifact hash/revocation, terminal/Unknown and no-execution-on-replay boundary review; no runtime test reviewer
```

### P3-I-04 acceptance snapshot and independent review evidence (2026-09-18)

```text
source_snapshot: cd811cb9 + P3-I-04 evidence slice; kiana-domain/src/company.rs; kiana-core/src/{company.rs,company_business.rs,company_governance.rs,collaboration.rs}; kiana-core/tests/p3_i04_acceptance_review.rs; kiana-domain/tests/p3_i01_company_objects.rs; kiana-core/tests/{oa27_company_governance,control_plane}.rs; .github/workflows/p3-i04-acceptance-review.yml; docs/roadmap/p3-i04-acceptance-review-baseline.md; docs/roadmap.md
worktree_status: RequestAcceptance captures immutable CriteriaSnapshot from project/milestone/packet versions and criteria, binding Builder run/session/evidence. RecordReview requires a different reviewer session, exact criteria keys and inherited evidence; DecideAcceptance rechecks acceptance/review IDs, snapshot, author run/session, reviewer role/session and decision-specific evidence. Business review and Company governance preserve the same independent identity and frozen target boundaries; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p3_i04_acceptance_review.rs acceptance/review source guard; existing domain Company object, OA-27 governance and ControlPlane fresh-review fixtures; GitHub Actions P3-I-04 workflow runs guard, object/governance/fresh-review regressions and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P3-I-04 is triggered by the eventual push and is not awaited
status_change: P3-I-04 source slice is implemented/reconciled. Acceptance criteria are frozen at request time and independent Review/Decision cannot mutate Builder run, evidence, criteria or historical CompanyEvent.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: acceptance/review facts remain local Company EventLog projection; no cross-process durable AcceptanceStore, external reviewer authentication, semantic quality guarantee, external delivery confirmation, power-loss recovery or live/physical business outcome proof is claimed
reviewer: Codex root implementation review plus criteria snapshot/version maps, author/reviewer session separation, criterion/evidence completeness, role/assignment/decision recheck, immutable Builder facts and no-second-authority boundary review; no runtime test reviewer
```

### P3-I-05 closeout and outcome evidence (2026-09-18)

```text
source_snapshot: a0503f3e + P3-I-05 evidence slice; kiana-domain/src/{company.rs,company_closeout.rs}; kiana-core/src/{company.rs,company_governance.rs}; kiana-core/tests/p3_i05_closeout_outcome.rs; kiana-domain/tests/p3_i01_company_objects.rs; kiana-core/tests/oa27_company_governance.rs; .github/workflows/p3-i05-closeout-outcome.yml; docs/roadmap/p3-i05-closeout-outcome-baseline.md; docs/roadmap.md
worktree_status: Company CloseProject requires accepted project/Acceptance, independent review, same-project confirmed Delivery with handoff receipt, resolved incidents and unique ClosingReceipt; business closeout requires human actor, stopped/known effects, acceptance/delivery obligations and named waiver where applicable. DeliveryUnknown/Reconcile preserves Incident/evidence. RecordOutcome and business measurement plan/observation/assessment bind objective snapshot, owner, window, finite values, evidence and minimum samples; only Realized outcome can Achieve, never runtime Completed/Receipt/model text; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; test targets compiled only and no test or smoke binary executed locally
fixture or cassette: kiana-core/tests/p3_i05_closeout_outcome.rs closeout/outcome source guard; existing kiana-domain/tests/p3_i01_company_objects.rs object/measurement fixtures and kiana-core/tests/oa27_company_governance.rs runtime-vs-business/closed-chain regressions; GitHub Actions P3-I-05 workflow runs guard, object and governance regressions and workspace compile
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P3-I-05 is triggered by the eventual push and is not awaited
status_change: P3-I-05 source slice is implemented/reconciled. Delivery, ClosingReceipt and measured Outcome gates are enforced by CompanyState/Business closeout; runtime success remains separate from business outcome.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: closeout/outcome facts are local Company EventLog projections; no external recipient/provider receipt, durable cross-process store, semantic KPI validity, power-loss recovery, external business confirmation or live/physical outcome proof is claimed
reviewer: Codex root implementation review plus acceptance/delivery/incident/closing prerequisites, waiver/Unknown, measurement window/owner/evidence/minimum sample, Realized-only achievement and no-runtime-success-as-outcome boundary review; no runtime test reviewer
```

### CI-04 protected daemon ingress evidence (2026-09-16)

```text
source_snapshot: d1c8241 + CI-04 working-tree slice; kiana-domain/src/{identity_contracts,contracts,lib}.rs; kiana-protocol/src/lib.rs; kiana-daemon/src/lib.rs; kiana-domain/tests/ci04_ingress.rs; kiana-daemon/tests/ci04_ingress.rs; kiana-core/tests/ci04_ingress_guard.rs; .github/workflows/ci04-ingress.yml; docs/roadmap/daemon-ingress-baseline.md; docs/roadmap.md
worktree_status: CI-04 protected ingress metadata validation and explicit local-user migration fact are scoped to this step; legacy fields remain readable, no external auth provider or second execution loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/identity_contracts.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-protocol/src/lib.rs kiana-daemon/src/lib.rs kiana-domain/tests/ci04_ingress.rs kiana-daemon/tests/ci04_ingress.rs kiana-core/tests/ci04_ingress_guard.rs .github/workflows/ci04-ingress.yml docs/roadmap/daemon-ingress-baseline.md
  rg -n 'validate_protected_ingress|loopback_authority|ingress_origin_not_loopback|ingress_host_not_loopback|ingress_protected_credentials_required|identity_mode|credential_ref|IdentityMigration|legacy_local_user_migration' kiana-domain/src kiana-protocol/src kiana-daemon/src kiana-domain/tests/ci04_ingress.rs kiana-daemon/tests/ci04_ingress.rs kiana-core/tests/ci04_ingress_guard.rs docs/roadmap/daemon-ingress-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain/protocol/daemon ingress fixture/source guard compiled only; no test or smoke command executed locally
fixture or cassette: ci04_ingress domain migration fixture, daemon bad Origin/Host/protected-credential negative fixture and opaque loopback acceptance fixture; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CI-04 job is queued by the push and is not awaited
status_change: CI-04 source slice is implemented. DaemonHost validates optional protected loopback metadata before ControlPlane, rejects malformed/non-loopback origin/host and missing protected credentials, while keeping legacy local-user compatibility explicit through IdentityMigration.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; optional metadata is not a cryptographic bearer/Unix peer verifier, no OS/keyring/OAuth/tenant identity or durable migration/session store exists, and cross-process owner/recovery proof remains CI-05+ and CP/SC/PD work
reviewer: Codex root implementation review plus CI-04 ingress/migration source-boundary review; no runtime test reviewer
```

### CI-03 identity/config/credential ports evidence (2026-09-16)

```text
source_snapshot: f4cab0d + CI-03 working-tree slice; kiana-ports/src/lib.rs; kiana-domain/src/identity_contracts.rs; kiana-ports/tests/ci03_ports.rs; kiana-core/tests/ci03_ports_guard.rs; .github/workflows/ci03-ports.yml; docs/roadmap/ports-identity-baseline.md; docs/roadmap.md
worktree_status: CI-03 IdentityResolver/CredentialResolver/ConfigSnapshotStore/CredentialRotationPort layering and secret-free CredentialResolution contract are scoped to this step; no production secret adapter or second authority was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-ports/src/lib.rs kiana-domain/src/identity_contracts.rs kiana-ports/tests/ci03_ports.rs kiana-core/tests/ci03_ports_guard.rs .github/workflows/ci03-ports.yml docs/roadmap/ports-identity-baseline.md
  rg -n 'IdentityResolver|CredentialResolver|ConfigSnapshotStore|CredentialRotationPort|RotationRevokePort|CredentialResolution|CredentialState|resolved_digest|raw secret' kiana-ports/src kiana-domain/src/identity_contracts.rs kiana-ports/tests/ci03_ports.rs kiana-core/tests/ci03_ports_guard.rs docs/roadmap/ports-identity-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; ports fixture/source guard compiled only; no test or smoke command executed locally
fixture or cassette: ci03_ports::ports_never_return_raw_secret_to_core plus compile-only fake Identity/Credential/Config/Rotation adapters; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CI-03 job is queued by the push and is not awaited
status_change: CI-03 source slice is implemented. Ports now separate identity/authority resolution, non-secret config snapshots, credential status/ref resolution and generation-aware rotation/revoke; CredentialResolution never carries raw secret bytes/strings and unsupported adapters fail closed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; no production adapter, SecretStore, CredentialLease, OAuth/PKCE, durable identity/config store or provider transport is implemented, and Available metadata does not prove credential validity or action authorization
reviewer: Codex root implementation review plus CI-03 ports/secret-boundary source review; no runtime test reviewer
```

### CI-02 identity and authority contract evidence (2026-09-16)

```text
source_snapshot: 85b8375 + CI-02 working-tree slice; kiana-domain/src/{identity_contracts,identity,assignment,ids,contracts,lib}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/ci02_identity_contracts.rs; kiana-core/tests/ci02_identity_contract_guard.rs; .github/workflows/ci02-identity-contracts.yml; docs/roadmap/identity-contracts-baseline.md; docs/roadmap.md
worktree_status: CI-02 typed Principal/Membership/SecretRef/ProviderAccount/ServiceIdentity/ConfigSnapshot/AuthoritySnapshot contracts and stable IDs are scoped to this step; raw secret values remain absent and no SecretStore or second authority was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/identity_contracts.rs kiana-domain/src/identity.rs kiana-domain/src/assignment.rs kiana-domain/src/ids.rs kiana-domain/src/contracts.rs kiana-domain/src/lib.rs kiana-protocol/src/lib.rs kiana-domain/tests/ci02_identity_contracts.rs kiana-core/tests/ci02_identity_contract_guard.rs .github/workflows/ci02-identity-contracts.yml docs/roadmap/identity-contracts-baseline.md
  rg -n 'Principal|Membership|SecretRef|ProviderAccount|ServiceIdentity|ConfigSnapshot|AuthoritySnapshot|PrincipalId|ProviderAccountId|ServiceIdentityId|contains_raw_secret|validate_current_epoch|deny_unknown_fields' kiana-domain/src kiana-protocol/src kiana-domain/tests/ci02_identity_contracts.rs kiana-core/tests/ci02_identity_contract_guard.rs docs/roadmap/identity-contracts-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; identity contract fixture/source guard compiled only; no test or smoke command executed locally
fixture or cassette: ci02_identity_contracts covers typed ID/schema/digest round trips, SecretRef redaction shape, ConfigSnapshot raw-secret rejection and AuthoritySnapshot epoch rollback/stale rejection; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CI-02 job is queued by the push and is not awaited
status_change: CI-02 source slice is implemented. Domain now exposes stable identity IDs and strict Principal/Membership/SecretRef/ProviderAccount/ServiceIdentity/ConfigSnapshot/AuthoritySnapshot contracts; snapshots bind digest/revision/expiry/authority epoch, and raw secret values are rejected before they can enter configuration metadata.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; contracts are value objects without durable identity store, SecretStore/CredentialLease, OAuth/PKCE, assignment revoke projection, cross-process recovery or provider effect evidence; CI-03+ and CP/SC/PD still own those boundaries
reviewer: Codex root implementation review plus CI-02 identity/secret/config/authority contract source-boundary review; no runtime test reviewer
```

### P3-I-01 Company object contract evidence (2026-09-16)

```text
source_snapshot: 8b91c3c + P3-I-01 working-tree slice; kiana-domain/src/company.rs; kiana-core/src/company.rs; kiana-domain/tests/p3_i01_company_objects.rs; kiana-core/tests/p3_i01_company_objects_guard.rs; .github/workflows/p3-i01-company-objects.yml; docs/roadmap/company-object-baseline.md; docs/roadmap.md
worktree_status: P3-I-01 ten Company object contracts, explicit state transitions, strict Acceptance/Review/MetricObservation criteria/measurement validation and core boundary guard are scoped to this step; no command/event freeze or second state authority was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/company.rs kiana-core/src/company.rs kiana-domain/tests/p3_i01_company_objects.rs kiana-core/tests/p3_i01_company_objects_guard.rs .github/workflows/p3-i01-company-objects.yml docs/roadmap/company-object-baseline.md
  rg -n 'pub struct (Objective|Initiative|Project|Milestone|Acceptance|Delivery|Outcome|ChangeRequest|Risk|Incident)|states!\(|impl Acceptance|impl CompanyReview|impl MetricObservation|criteria_snapshot_version_required|review_criteria_snapshot_mismatch|metric_observation_invalid|CompanyState|handle_company_command' kiana-domain/src/company.rs kiana-core/src/company.rs kiana-domain/tests/p3_i01_company_objects.rs kiana-core/tests/p3_i01_company_objects_guard.rs docs/roadmap/company-object-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain object fixture/source guard compiled only; no test or smoke command executed locally
fixture or cassette: p3_i01_company_objects::company_objects_expose_invariants covers ten object validators, state/version/evidence/measurement rules and Acceptance unknown fields; core source guard checks CompanyState/ControlPlane boundary; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P3-I-01 job is queued by the push and is not awaited
status_change: P3-I-01 source slice is implemented. Ten Company domain objects and their state graphs are explicit; Acceptance/CriteriaSnapshot/CompanyReview/MetricObservation now reject unknown fields and validate identity, versions, criteria keys, finite measurements and evidence. CompanyState remains the only state transition authority consumed by core.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; object facts still use existing CompanyState/EventStore adapters, command/event version freeze, durable projector/upcast/recovery and real business outcomes remain P3-I-02+ and CO/ER/PD work
reviewer: Codex root implementation review plus P3-I-01 Company object/state source-boundary review; no runtime test reviewer
```

### P1-H-03 shared path containment evidence (2026-09-16)

```text
source_snapshot: cd672b7 + P1-H-03 working-tree slice; kiana-domain/src/paths.rs; kiana-core/src/{cell_registry,events,workspace_checkpoints}.rs; kiana-daemon/src/{apply_patch,execution_control,execution_workspace,harness_capabilities,workspace_checkpoints}.rs; kiana-domain/tests/p1_h03_path_containment.rs; kiana-core/tests/p1_h03_path_containment_guard.rs; .github/workflows/p1-h03-path-containment.yml; docs/roadmap/path-containment-baseline.md; docs/roadmap.md
worktree_status: P1-H-03 shared lexical path/root containment helper and side-effecting boundary migrations are scoped to this step; filesystem no-follow checks remain in adapters and no second execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/paths.rs kiana-core/src/cell_registry.rs kiana-core/src/events.rs kiana-core/src/workspace_checkpoints.rs kiana-daemon/src/apply_patch.rs kiana-daemon/src/execution_control.rs kiana-daemon/src/execution_workspace.rs kiana-daemon/src/harness_capabilities.rs kiana-daemon/src/workspace_checkpoints.rs kiana-domain/tests/p1_h03_path_containment.rs kiana-core/tests/p1_h03_path_containment_guard.rs .github/workflows/p1-h03-path-containment.yml docs/roadmap/path-containment-baseline.md
  rg -n 'enforce_path_containment|enforce_root_containment|path_not_relative|path_outside_scope|harness_workdir_outside_project|apply_patch_path_outside_project|package_path_denied|checkpoint_path_denied' kiana-domain/src kiana-core/src kiana-daemon/src kiana-domain/tests/p1_h03_path_containment.rs kiana-core/tests/p1_h03_path_containment_guard.rs docs/roadmap/path-containment-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain containment fixture/source guard compiled only; no test or smoke command executed locally
fixture or cassette: p1_h03_path_containment::path_containment_is_shared_by_every_side_effecting_tool covers relative normalization, parent/absolute/escape rejection and root-prefix trap; core source guard covers patch/shell/package/checkpoint/Cell/event consumers; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-H-03 job is queued by the push and is not awaited
status_change: P1-H-03 source slice is implemented. Domain now owns lexical path/root containment, and daemon/core side-effect boundaries consume the same helper before their adapter-specific symlink/hardlink/TOCTOU checks; no existing fail-closed errors were relaxed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; lexical checks do not prove fd-relative/no-follow or external rename resistance, MCP/Memory external scope and full network/secret containment remain CAP/SC/ER/PD work, and legacy allow_list_covers remains a compatibility API
reviewer: Codex root implementation review plus P1-H-03 path containment/source-boundary review; no runtime test reviewer
```

### P1-H-01 single tool authority registry evidence (2026-09-16)

```text
source_snapshot: eb35690 + P1-H-01 working-tree slice; kiana-domain/src/{tool_authority,tool_catalog,contracts,lib}.rs; kiana-runner/src/tools.rs; kiana-daemon/src/lib.rs; kiana-domain/tests/cap01_registry.rs; kiana-core/tests/p1_h01_tool_authority_guard.rs; .github/workflows/p1-h01-tool-authority.yml; docs/roadmap/tool-authority-baseline.md; docs/roadmap.md
worktree_status: P1-H-01 typed five-tool ToolSpec registry, Runner canonical mapping and DaemonHost composition validation are scoped to this step; no sixth model-visible tool or second execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/tool_authority.rs kiana-domain/src/tool_catalog.rs kiana-domain/src/contracts.rs kiana-runner/src/tools.rs kiana-daemon/src/lib.rs kiana-domain/tests/cap01_registry.rs kiana-core/tests/p1_h01_tool_authority_guard.rs .github/workflows/p1-h01-tool-authority.yml docs/roadmap/tool-authority-baseline.md
  rg -n 'ToolSpec|TOOL_SPECS|tool_spec|validate_tool_authority|model_tool_name|let canonical = model_tool_name|match canonical|TOOL_SHELL =>|TOOL_APPLY_PATCH =>|TOOL_MCP =>|tool-authority.v1' kiana-domain/src kiana-runner/src kiana-daemon/src kiana-domain/tests/cap01_registry.rs kiana-core/tests/p1_h01_tool_authority_guard.rs docs/roadmap/tool-authority-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain registry fixture/source guard compiled only; no test or smoke command executed locally
fixture or cassette: cap01_registry::tool_authority_covers_every_model_visible_tool checks all five canonical tools/aliases/action descriptors; core source guard checks Runner/daemon use the same registry; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-H-01 job is queued by the push and is not awaited
status_change: P1-H-01 source slice is implemented. `TOOL_SPECS` is the domain authority for the five model-visible tools; aliases normalize through `tool_spec`, Runner maps only canonical names, DaemonHost validates the registry before composing capabilities, and schema/alias drift fails closed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; ToolSpec metadata does not grant capabilities or approvals, provider wire aliases remain an adapter concern, and full path containment/handler/entrypoint parity remains P1-H-02/03 and CAP/CP work
reviewer: Codex root implementation review plus P1-H-01 tool authority/source-boundary review; no runtime test reviewer
```

### P1-E-01 typed communication and accountability evidence (2026-09-16)

```text
source_snapshot: bb90094 + P1-E-01 working-tree slice; kiana-domain/src/{communication,handoff,contracts,event_contracts,lib}.rs; kiana-ports/src/lib.rs; kiana-core/src/{communication,commands}.rs; kiana-protocol/src/lib.rs; kiana-domain/tests/p1_e01_communication.rs; kiana-core/tests/p1_e01_communication_guard.rs; .github/workflows/p1-e01-communication.yml; docs/roadmap/communication-baseline.md; docs/roadmap.md
worktree_status: P1-E-01 typed seven-way communication contract, server sender check, Chat authority denial, Handoff ACK boundary and ControlPlane formal event route are scoped to this step; no notification delivery bus or second execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/communication.rs kiana-domain/src/handoff.rs kiana-domain/src/contracts.rs kiana-domain/src/event_contracts.rs kiana-domain/src/lib.rs kiana-ports/src/lib.rs kiana-core/src/communication.rs kiana-core/src/commands.rs kiana-protocol/src/lib.rs kiana-domain/tests/p1_e01_communication.rs kiana-core/tests/p1_e01_communication_guard.rs .github/workflows/p1-e01-communication.yml docs/roadmap/communication-baseline.md
  rg -n 'CommunicationMessageKind|Chat|Command|Handoff|Decision|StatusReport|Evidence|Incident|grants_authority|communication.send|communication_sender_mismatch|CommunicationPort|handoff_ack_reason_required|COMMUNICATION_FIELDS|communication.chat' kiana-domain/src kiana-ports/src kiana-core/src kiana-protocol/src kiana-domain/tests/p1_e01_communication.rs kiana-core/tests/p1_e01_communication_guard.rs docs/roadmap/communication-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain message fixture/source guard compiled only; no test or smoke command executed locally
fixture or cassette: p1_e01_communication::free_chat_never_grants_authority and handoff_is_directed_and_requires_ack; core typed communication source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-E-01 job is queued by the push and is not awaited
status_change: P1-E-01 source slice is implemented. Seven message kinds are explicit; Chat cannot carry action/ACK authority fields and all messages report grants_authority=false. ControlPlane validates server-owned sender/project trust and appends communication.* facts, while Handoff requires a distinct target session/role, expiry and explicit ACK reason.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; CommunicationPort has no durable production adapter, notification outbox/delivery/read-state and cross-process delivery remain NM/ER/PD/INT work, and a communication fact never proves the referenced business command was approved or executed
reviewer: Codex root implementation review plus P1-E-01 communication/hand-off authority source-boundary review; no runtime test reviewer
```

### P1-D-03 claim and lease recovery evidence (2026-09-16)

```text
source_snapshot: efd81cc + P1-D-03 working-tree slice; kiana-domain/src/{packet_graph,company}.rs; kiana-core/src/company.rs; kiana-domain/tests/p1_d03_lease.rs; kiana-core/tests/p1_d03_lease_guard.rs; .github/workflows/p1-d03-lease.yml; docs/roadmap/lease-recovery-baseline.md; docs/roadmap.md
worktree_status: P1-D-03 owner-bound PacketClaim renewal, bounded expiry scan, CompanyCommand reclaim and no-double-dispatch guards are scoped to this step; no scheduler/second execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/packet_graph.rs kiana-domain/src/company.rs kiana-core/src/company.rs kiana-domain/tests/p1_d03_lease.rs kiana-core/tests/p1_d03_lease_guard.rs .github/workflows/p1-d03-lease.yml docs/roadmap/lease-recovery-baseline.md
  rg -n 'PacketClaim|lease_expires_at|heartbeat_at|CompanyCommand::RenewPacketClaim|CompanyCommand::ReclaimPacketClaim|company_claim_worker_not_stopped|reclaim_packet_leases|renew_company_claim|expired_lease_is_reclaimed_without_double_dispatch' kiana-domain/src kiana-core/src kiana-domain/tests/p1_d03_lease.rs kiana-core/tests/p1_d03_lease_guard.rs docs/roadmap/lease-recovery-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain lease fixture/source guard compiled only; no test or smoke command executed locally
fixture or cassette: p1_d03_lease::expired_lease_is_reclaimed_without_double_dispatch and heartbeat_renewal_requires_the_current_owner_and_live_lease; core scan/renew/reclaim source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-D-03 job is queued by the push and is not awaited
status_change: P1-D-03 source slice is implemented. Packet claims now reject foreign/expired/non-monotonic renewal, runtime turns renew only the current owner, and bounded reclaim commands clear stale claims through the Company state transition. In-flight expired claims require terminal worker evidence; ResultUnknown is incident/reconciliation state and cannot be dispatched again.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; claim/Company state remains limited to current adapters, no cross-process worker death or durable lease projection/queue fairness/backoff/scheduler heartbeat is proven, and no automatic retry follows an expired Unknown result
reviewer: Codex root implementation review plus P1-D-03 claim/lease/recovery source-boundary review; no runtime test reviewer
```

### P1-D-02 dependency graph validation evidence (2026-09-16)

```text
source_snapshot: f60df32 + P1-D-02 working-tree slice; kiana-domain/src/{packet_graph,company}.rs; kiana-core/src/company.rs; kiana-domain/tests/p1_d02_dependency_graph.rs; kiana-core/tests/p1_d02_dependency_guard.rs; .github/workflows/p1-d02-dependency-graph.yml; docs/roadmap/dependency-graph-baseline.md; docs/roadmap.md
worktree_status: P1-D-02 explicit dependency/DAG validation and candidate-packet approval fence are scoped to this step; graph errors fail before Company fact append and no scheduler/second execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/packet_graph.rs kiana-domain/src/company.rs kiana-core/src/company.rs kiana-domain/tests/p1_d02_dependency_graph.rs kiana-core/tests/p1_d02_dependency_guard.rs .github/workflows/p1-d02-dependency-graph.yml docs/roadmap/dependency-graph-baseline.md
  rg -n 'validate_dependency_dag|packet_dependency_cycle|packet_dependency_missing|packet_dependency_duplicate|cycle.rotate_left|company_packet_dependency_graph_invalid|dependency_cycle_is_rejected_deterministically' kiana-domain/src kiana-core/src kiana-domain/tests/p1_d02_dependency_graph.rs kiana-core/tests/p1_d02_dependency_guard.rs docs/roadmap/dependency-graph-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain graph fixture/source guard compiled only; no test or smoke command executed locally
fixture or cassette: p1_d02_dependency_graph::dependency_cycle_is_rejected_deterministically and dependency_missing_and_duplicate_edges_fail_closed; CompanyState candidate approval source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-D-02 job is queued by the push and is not awaited
status_change: P1-D-02 source slice is implemented. `validate_dependency_dag` now remains the single deterministic graph authority and CompanyState validates the full project graph, including a candidate packet, before persisting approval; missing, duplicate and cyclic dependencies fail closed with stable diagnostics.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; graph validation does not itself reclaim leases or change blocked status, EventLog durable migration/replay and scheduler/queue recovery remain P1-D-03/AUT/ER/PD work
reviewer: Codex root implementation review plus P1-D-02 dependency/DAG source-boundary review; no runtime test reviewer
```

### P1-D-01 single WorkPacket ready predicate evidence (2026-09-16)

```text
source_snapshot: 260fd34 + P1-D-01 working-tree slice; kiana-domain/src/{packet_graph,company}.rs; kiana-core/src/company.rs; kiana-tasks/{Cargo.toml,src/project_board.rs,src/lib.rs}; kiana-tasks/tests/p1_d01_ready_predicate.rs; kiana-core/tests/p1_d01_ready_predicate_guard.rs; .github/workflows/p1-d01-ready-predicate.yml; docs/roadmap/ready-predicate-baseline.md; docs/roadmap.md
worktree_status: P1-D-01 canonical domain readiness predicate, core/Company consumers and legacy task wrapper are scoped to this step; no second WorkPacket readiness algorithm or scheduler execution path was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/packet_graph.rs kiana-domain/src/company.rs kiana-core/src/company.rs kiana-tasks/Cargo.toml kiana-tasks/src/project_board.rs kiana-tasks/src/lib.rs kiana-tasks/tests/p1_d01_ready_predicate.rs kiana-core/tests/p1_d01_ready_predicate_guard.rs .github/workflows/p1-d01-ready-predicate.yml docs/roadmap/ready-predicate-baseline.md
  rg -n 'pub fn ready_packets|kiana_domain::ready_packets|crate::ready_packets|packet_claimed|dependency_incomplete|packet_deadline_expired|single_ready_predicate_agrees_across_three_callers' kiana-domain/src kiana-core/src kiana-tasks/src kiana-tasks/tests/p1_d01_ready_predicate.rs kiana-core/tests/p1_d01_ready_predicate_guard.rs docs/roadmap/ready-predicate-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain/tasks/core readiness fixture/source guard compiled only; no test or smoke binary executed locally
fixture or cassette: p1_d01_ready_predicate::single_ready_predicate_agrees_across_three_callers compares domain canonical output with kiana-tasks wrapper across dependency, active/expired claim and status cases; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-D-01 job is queued by the push and is not awaited
status_change: P1-D-01 source slice is implemented. Domain `ready_packets` validates identity/DAG and deterministically returns ready, blocked and expired-claim sets; CompanyState/ControlPlane consume it directly, while kiana-tasks exposes only a delegating compatibility adapter so its legacy ProjectBoard cannot become a second WorkPacket authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; readiness remains a read-only calculation, dependency-cycle admission/lease reclaim/durable queue/scheduler dispatch and legacy board completion semantics remain P1-D-02/03 and AUT/PD work
reviewer: Codex root implementation review plus P1-D-01 readiness/source-boundary review; no runtime test reviewer
```

### P1-C-03 role model routing evidence (2026-09-16)

```text
source_snapshot: 763e89a + P1-C-03 working-tree slice; kiana-domain/src/{roles,model,prompts}.rs; kiana-core/src/lifecycle.rs; kiana-provider/src/{lib,config}.rs; kiana-provider/tests/co04_role_model_routes.rs; kiana-core/tests/p1_c03_model_routing_guard.rs; .github/workflows/p1-c03-model-routing.yml; docs/roadmap/role-model-routing-baseline.md; docs/roadmap.md
worktree_status: P1-C-03 role catalog/profile route fixture and source guard are scoped to this step; ControlPlane remains the sole ModelAssignment authority and ProviderGateway consumes assignment.profile; no new model-visible tool or second execution loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/roles.rs kiana-domain/src/model.rs kiana-domain/src/prompts.rs kiana-core/src/lifecycle.rs kiana-provider/src/lib.rs kiana-provider/src/config.rs kiana-provider/tests/co04_role_model_routes.rs kiana-core/tests/p1_c03_model_routing_guard.rs .github/workflows/p1-c03-model-routing.yml docs/roadmap/role-model-routing-baseline.md
  rg -n 'model_profile|RoleCatalog|DepartmentCatalog|ModelAssignment|profile: role.model_profile|assignment.profile|model_profile_unconfigured|planning_and_execution_roles_can_use_different_models' kiana-domain/src kiana-core/src kiana-provider/src kiana-provider/tests/co04_role_model_routes.rs kiana-core/tests/p1_c03_model_routing_guard.rs docs/roadmap/role-model-routing-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; provider fixture/source guard compiled only; no test or smoke binary executed locally
fixture or cassette: p1-c03 provider route fixture uses explicit planning/executing/quality loopback profiles and checks distinct model/connection selection; CO-04 role catalog/provenance fixtures remain the shared compatibility contract; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-C-03 job is queued by the push and is not awaited
status_change: P1-C-03 source slice is implemented. The five-department/role catalog supplies fixed model_profile values, ControlPlane records them in server-owned ModelAssignment, and ProviderGateway requires that assignment before selecting a configured connection; profile/role drift and unknown profiles remain fail-closed.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; the catalog is a built-in snapshot, profile configuration is process-local/non-durable, no live provider request or model quality is proven, and per-attempt permit/usage/retry/reconciliation remain P4-J7-11+ work
reviewer: Codex root implementation review plus P1-C-03 role/profile/provider source-boundary review; no runtime test reviewer
```

### P1-C-01 organization and Cell contract evidence (2026-09-16)

```text
source_snapshot: 7169492 + P1-C-01 working-tree slice; kiana-domain/src/{work_packets,capabilities}.rs; kiana-core/src/cell_registry.rs; kiana-domain/tests/p1_c01_contract.rs; kiana-core/tests/p1_c01_contract_guard.rs; .github/workflows/p1-c01-contract.yml; docs/roadmap/cell-contract-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: P1-C-01 six versioned AgentTemplate/CellSpec/SpawnPlan/BudgetLease/CapabilityGrant/SupervisionLease contracts, unknown-field fences, template/parent scope checks and CI-only fixtures are scoped to this step; no durable scheduler or second execution loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/work_packets.rs kiana-domain/src/capabilities.rs kiana-core/src/cell_registry.rs kiana-domain/tests/p1_c01_contract.rs kiana-core/tests/p1_c01_contract_guard.rs .github/workflows/p1-c01-contract.yml docs/roadmap/cell-contract-baseline.md
  rg -n 'AgentTemplate|CellSpec|SpawnPlan|BudgetLease|CapabilityGrant|SupervisionLease|deny_unknown_fields|template_version|contains\(&self, child|spawn_grant_not_contained|spawn_delegation_denied' kiana-domain/src kiana-core/src kiana-domain/tests/p1_c01_contract.rs kiana-core/tests/p1_c01_contract_guard.rs docs/roadmap/cell-contract-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; domain fixture/source guard compiled only; no test or smoke binary executed locally
fixture or cassette: p1_c01_contract::child_grant_cannot_exceed_parent_grant and templates_pin_version_and_default_to_non_delegable; core CellRegistry source guard; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P1-C-01 job is queued by the push and is not awaited
status_change: P1-C-01 source slice is implemented. Domain now marks all six Cell contracts as deny-unknown-field DTOs; template IDs/versions are checked against Cell specs, RoleSpec templates default to non-delegable, and CapabilityGrant containment enforces capability/operation/resource/path/expiry/delegation subset. MemoryCellRegistry repeats template, grant, budget and supervision checks for reserve and snapshot admission.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; MemoryCellRegistry remains process-local, durable Cell/Grant/Budget/Lease projection and cross-process lease fencing are absent, and full scheduler/Swarm lifecycle and all alternate entrypoint coverage remain P1-C-02/SW/AUT/ER/PD work
reviewer: Codex root implementation review plus P1-C-01 contract/subset source-boundary review; no runtime test reviewer
```

### P0-K1-01 server identity and authority epoch evidence (2026-09-16)

```text
source_snapshot: 4f324f3 + P0-K1-01 working-tree slice; kiana-core/src/{authority,sessions,lifecycle}.rs; kiana-daemon/src/lib.rs; kiana-domain/src/{identity,assignment}.rs; kiana-protocol/src/lib.rs; kiana-daemon/tests/p0_k1_identity.rs; kiana-core/tests/p0_k1_identity_guard.rs; .github/workflows/p0-k1-identity.yml; docs/roadmap/identity-authority-baseline.md; docs/module-map.md; docs/roadmap/README.md; docs/roadmap.md
worktree_status: P0-K1-01 server-owned ingress identity, canonical ProjectIdentity, CAS SessionAssignment and authority stream epoch wiring are scoped to this step; wire actor/role/department cannot rewrite an existing assignment; no external auth provider or second execution loop was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-core/src/authority.rs kiana-core/src/sessions.rs kiana-core/src/lifecycle.rs kiana-daemon/src/lib.rs kiana-domain/src/identity.rs kiana-domain/src/assignment.rs kiana-protocol/src/lib.rs kiana-daemon/tests/p0_k1_identity.rs kiana-core/tests/p0_k1_identity_guard.rs .github/workflows/p0-k1-identity.yml docs/roadmap/identity-authority-baseline.md
  rg -n 'AuthenticatedPrincipal::local|project_authority|principal_role_not_authorized|synchronize_authority|bind_session_assignment|authority_epoch|SessionAssignment|append_expected|session_assignment_mismatch' kiana-domain/src kiana-core/src kiana-daemon/src kiana-protocol/src kiana-daemon/tests/p0_k1_identity.rs kiana-core/tests/p0_k1_identity_guard.rs docs/roadmap/identity-authority-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; rustc/cargo 1.97.1; locked offline dependency resolution; identity ingress test/source guard compiled only; no test or smoke binary executed locally
fixture or cassette: p0_k1_identity::wire_actor_cannot_grant_role_or_department establishes a server assignment then submits forged actor/role/department; cp01_identity typed principal/project fixture remains the compatibility contract; GitHub Actions only
exit_code: 0 for source hashes, format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-K1-01 job is queued by the push and is not awaited
status_change: P0-K1-01 source slice is implemented. DaemonHost overwrites wire actor with its authenticated local principal, rechecks ProjectTrust and canonical project identity, and refuses role/department drift against the CAS-bound SessionAssignment before ControlPlane/Broker. The authority stream's monotonic version is now recorded as numeric authority_epoch in SessionAssignment and run.authorized, while authority revision digest remains separate.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; principal is still fixed local-user/local_os, OS/OAuth/tenant auth and durable Membership/Assignment/Grant ledgers are absent, authority epoch persistence is limited to the current EventStore adapter, non-effectful queries do not mint assignments, and complete cross-process/effect-time revocation proof remains in CI/CP/SC/PD follow-up steps
reviewer: Codex root implementation review plus P0-K1-01 identity/assignment/epoch source-boundary review; no runtime test reviewer
```

### P0-B-01 state transition matrix evidence (2026-09-16)

```text
source_snapshot: ff7076b (P4-J7-10 parent; P0-B-01 source files listed below); kiana-domain/src/{states,company}.rs; kiana-core/src/{lifecycle,dispatch}.rs; kiana-runner/src/state_driver.rs; kiana-domain/tests/p0_b01_state_machine.rs; kiana-core/tests/p0_b01_state_machine_guard.rs; .github/workflows/p0-b01-state-machine.yml; docs/roadmap/state-machine-baseline.md; docs/roadmap.md
worktree_status: P0-B-01 verifies the single domain state transition/terminal contract consumed by core lifecycle/dispatch and Runner driver; terminal/result_unknown cannot reopen or become success, and no second state machine was added; static verification is complete and commit/push are pending
command_argv:
  sha256sum kiana-domain/src/states.rs kiana-domain/src/company.rs kiana-core/src/lifecycle.rs kiana-core/src/dispatch.rs kiana-runner/src/state_driver.rs kiana-domain/tests/p0_b01_state_machine.rs kiana-core/tests/p0_b01_state_machine_guard.rs .github/workflows/p0-b01-state-machine.yml docs/roadmap/state-machine-baseline.md
  rg -n 'can_transition_to|transition_via|is_terminal|ResultUnknown|InvalidStateTransition|company_illegal_state_transition|DriverTransition' kiana-domain/src kiana-core/src kiana-runner/src kiana-domain/tests kiana-core/tests docs/roadmap/state-machine-baseline.md
  cargo fmt --all
  cargo fmt --all --check
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; locked offline dependency resolution; domain/core/runner state fixture targets compiled only; no test or smoke binary executed locally
fixture or cassette: kiana-domain/tests/p0_b01_state_machine.rs illegal/terminal/result_unknown fixtures and kiana-core/tests/p0_b01_state_machine_guard.rs consumer source checks; GitHub Actions only
exit_code: 0 for format, workspace test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P0-B-01 job is queued by the next push and is not awaited
status_change: P0-B-01 source slice is implemented. Approval, capability, cancellation, packet, execution and Company state graphs expose explicit transition edges and terminal predicates; core/dispatch and Runner consume these contracts, and result_unknown remains terminal/uncertain with explicit reconciliation rather than automatic success/retry.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live or physical promotion
limitations: CI result was intentionally not awaited; no local test or smoke command was run; formal matrix does not prove every later Company object/legacy projector caller lacks ad-hoc inference, and durable cross-process transition/recovery remains ER/PD/CO scope
reviewer: Codex root implementation review plus P0-B-01 transition/terminal source-boundary review; no runtime test reviewer
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
### P3-I-06 fake-model Company golden loop evidence (2026-09-18)

```text
source_snapshot: 79639b57 + P3-I-06 working-tree slice; kiana-domain/src/{company.rs,company_replay.rs,company_closeout.rs}; kiana-core/src/{company.rs,company_business.rs,collaboration.rs}; kiana-daemon/src/{lib.rs,model_client.rs}; kiana-daemon/tests/p3_i06_company_golden.rs; kiana-core/tests/p3_i06_company_golden.rs; .github/workflows/p3-i06-company-golden.yml; docs/roadmap/p3-i06-company-golden-baseline.md; docs/roadmap.md
worktree_status: real protocol-entry fixture now drives a trusted ScriptedModel through Sponsor Objective/Project, PM Milestone/Packet, Builder spawn_from_packet, independent Reviewer/Acceptance, local Delivery and Closer CloseProject; the fixture asserts OUTPUT.txt, EventLog evidence, revision/CAS/idempotency and a complete CompanyClosingReceipt; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check -p kiana-daemon --test p3_i06_company_golden -p kiana-core --test p3_i06_company_golden --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-daemon/tests/p3_i06_company_golden.rs uses ScriptedModel apply_patch output and real DaemonHost::handle CompanyCommand envelopes; kiana-core/tests/p3_i06_company_golden.rs checks runtime/business evidence separation, replay, rework/pause/cancel/failure/Unknown boundaries and no second model/capability loop; GitHub Actions P3-I-06 workflow runs the fixture, source guard, closeout/governance regressions and workspace compile
exit_code: 0 for format, focused test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P3-I-06 is triggered by the eventual push and is not awaited
status_change: P3-I-06 source slice is implemented. The fake-model Company path now exercises Objective→Project→Planner→Builder→Reviewer→Acceptance→Delivery→Closer through one DaemonHost/ControlPlane spine and emits a queryable CompanyClosingReceipt with linked file/evidence facts.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: ScriptedModel is a deterministic offline fixture, not a live provider; Company aggregate/EventLog/Artifact storage remain the current local composition; rejection/Unknown/cancel/failure paths are source-guarded but not executed locally; external recipient confirmation, semantic KPI outcome, cross-process power-loss recovery, four-entry Company UX and live/physical business proof remain CO-35..48/EQ/ER/PD/DEP/SC work
reviewer: Codex root implementation review plus Objective/Project/PM packet/Builder spawn/Reviewer criteria/Acceptance/Delivery/Closer receipt linkage, evidence ownership, CAS/idempotency, Unknown/replay and no-second-loop boundary review; no runtime test reviewer
```
### P4-E-03 department symposium memory evidence (2026-09-18)

```text
source_snapshot: 5a3101c0 + P4-E-03 working-tree slice; kiana-domain/src/{roles.rs,symposiums.rs,memory_proposals.rs}; kiana-core/src/{collaboration.rs,memory_proposals.rs,memory_distillation.rs}; kiana-daemon/src/harness_memory.rs; kiana-domain/tests/p4_e03_department_memory.rs; kiana-core/tests/p4_e03_department_memory.rs; .github/workflows/p4-e03-department-memory.yml; docs/roadmap/p4-e03-department-memory-baseline.md; docs/roadmap.md
worktree_status: all five built-in departments now have a shared bounded Symposium/DecisionRecord path; public symposium.closed facts feed the existing decision distillation queue and produce department-scoped Candidate proposals with source evidence, while memory.review remains the explicit ACL/approval promotion boundary; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check -p kiana-domain --test p4_e03_department_memory -p kiana-core --test p4_e03_department_memory --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-domain/tests/p4_e03_department_memory.rs validates initiating/planning/executing/monitoring/closing meeting contracts and kind=decision department Candidate evidence; kiana-core/tests/p4_e03_department_memory.rs guards derive/queue/memory.review, ACL, bounded citations, automatic_execution=false and no direct memory write; GitHub Actions P4-E-03 workflow runs fixtures, Symposium/memory regressions and workspace compile
exit_code: 0 for format, focused test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-E-03 is triggered by the eventual push and is not awaited
status_change: P4-E-03 source slice is implemented. Five departments can produce explicit DecisionRecord facts, and each published resolution is routed to a department:<id> Candidate path without silently granting memory or execution authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: Candidate proposals are not Active/Qualified memory and are not automatically persisted through memory.write; Builder persistent write permissions remain unchanged; cross-tenant department ACL, durable projector/recovery, production index/vector search, external notifications and live/physical business outcomes remain CM/PD/SC/NM/ER work
reviewer: Codex root implementation review plus five-department catalog, chair/attendee/round, public-decision-only evidence, Candidate/admission, ACL/promotion and no-second-loop boundary review; no runtime test reviewer
```
### P4-J3-05 terminal distillation evidence (2026-09-18)

```text
source_snapshot: 8756a882 + P4-J3-05 working-tree slice; kiana-domain/src/{memory_distillation.rs,memory_proposals.rs}; kiana-core/src/{events.rs,lifecycle.rs,memory_distillation.rs,memory_proposals.rs}; kiana-daemon/src/harness_memory.rs; kiana-domain/tests/p4_j3_05_distillation.rs; kiana-core/tests/p4_j3_05_distillation.rs; .github/workflows/p4-j3-05-distillation.yml; docs/roadmap/p4-j3-05-distillation-baseline.md; docs/roadmap.md
worktree_status: terminal run outcomes now queue at-most-once memory distillation jobs; explicit consume uses a read-only deny-tools Runner, strict schema/quote/evidence validation produces only department lesson Candidate proposals, and memory.review remains the promotion boundary; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check -p kiana-domain --test p4_j3_05_distillation -p kiana-core --test p4_j3_05_distillation --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-domain/tests/p4_j3_05_distillation.rs validates retain/discard, exact source quote, malformed quote rejection and Candidate lesson output; kiana-core/tests/p4_j3_05_distillation.rs guards terminal queue/claim/start/finish/failure/Unknown/idempotency, memory.review and no direct Broker/Provider boundary; GitHub Actions P4-J3-05 workflow runs fixtures, proposal regression and workspace compile
exit_code: 0 for format, focused test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-J3-05 is triggered by the eventual push and is not awaited
status_change: P4-J3-05 source slice is implemented. Run terminal facts can produce bounded, evidence-cited lesson candidates in the source department collection without changing the source outcome or granting automatic knowledge authority.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: Scripted/fixture validation does not establish LLM semantic quality or production throughput; candidate queue and memory JSONL remain local adapters, cross-process worker/power-loss recovery, durable index/vector recall, retention/revocation and real business impact remain CM/PD/ER/PD/SC/EQ work
reviewer: Codex root implementation review plus terminal trigger, at-most-once source key, deny-tools internal run, strict output/quote/evidence, Candidate-only admission, memory.review ACL and Unknown/no-second-loop boundary review; no runtime test reviewer
```
### P4-J6-01 bounded Swarm evidence (2026-09-18)

```text
source_snapshot: b905935e + P4-J6-01 working-tree slice; kiana-domain/src/{swarm.rs,swarm_graph.rs,swarm_reducer.rs}; kiana-core/src/{swarm.rs,collaboration.rs,company.rs}; kiana-domain/tests/p4_j6_01_bounded_swarm.rs; kiana-core/tests/p4_j6_01_bounded_swarm.rs; .github/workflows/p4-j6-01-bounded-swarm.yml; docs/roadmap/p4-j6-01-bounded-swarm-baseline.md; docs/roadmap.md
worktree_status: bounded Swarm now has explicit fan-out/fan-in fixture and source guard: graph projection is deterministic; WorkFingerprint/path/limit checks reject duplicate or overlapping work; Controller Cell/Grant/Supervision/budget and child lifecycle reuse ControlPlane Company StartRun; commit-before-dispatch, Reconcile/Unknown/cancel, independent review-gated Merge and parent retire remain explicit; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check -p kiana-domain --test p4_j6_01_bounded_swarm -p kiana-core --test p4_j6_01_bounded_swarm --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-domain/tests/p4_j6_01_bounded_swarm.rs repeats `SwarmWorkGraph::projection`, simulates reducer ReadyToMerge→Completed(review), and asserts overlap/duplicate/no-review rejection; kiana-core/tests/p4_j6_01_bounded_swarm.rs guards bounds, child identity, commit-before-dispatch, Company reuse, Unknown/cancel/retire and no second loop; GitHub Actions P4-J6-01 workflow runs fixtures, graph/reducer regressions and workspace compile
exit_code: 0 for format, focused test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-J6-01 is triggered by the eventual push and is not awaited
status_change: P4-J6-01 source slice is implemented. Bounded fan-out/fan-in contracts and deterministic review-gated merge are now covered by a focused fixture and ControlPlane source evidence.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: CellRegistry and portions of Swarm aggregate/reconciliation remain process-local; event-before-dispatch is an explicit recovery window, not cross-process durability; no real concurrent worker/power-loss/Integrator conflict or external/live/physical effect proof is claimed; follow-up SW-04..18/CO-43..48/PD/ER/DEP remains
reviewer: Codex root implementation review plus WorkGraph/WorkFingerprint/path/budget/concurrency/TTL, Controller Cell, child Company dispatch, replay/Unknown/cancel and independent Merge/retire boundary review; no runtime test reviewer
```
### P4-J7-02 stream sequence/epoch evidence (2026-09-18)

```text
source_snapshot: e9687329 + P4-J7-02 working-tree slice; kiana-protocol/src/lib.rs; kiana-daemon/src/run_stream.rs; kiana-protocol/tests/p4_j7_02_sequence.rs; kiana-daemon/tests/p2_m2_01_ui_projection.rs; kiana-entrypoints/src/{web.rs,web_page.html}; kiana-entrypoints/tests/p2_m5_01_web_sync.rs; .github/workflows/p4-j7-02-stream-sequence.yml; docs/roadmap/p4-j7-02-stream-sequence-baseline.md; docs/roadmap.md
worktree_status: additive RunStreamEnvelope epoch/sequence/ui_cursor and RunStreamBus per-run monotonic cursor are covered by a real delta→terminal→late-replay bus fixture; protocol cursor rejects gaps/epoch changes and treats duplicate delivery idempotently; Web SSE/Last-Event-ID and snapshot fallback remain on the same projection path; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check -p kiana-protocol --test p4_j7_02_sequence -p kiana-daemon --lib --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-daemon/src/run_stream.rs `run_stream_sequence_is_monotonic` publishes two deltas and terminal then replays terminal to a late cursor; kiana-protocol/tests/p4_j7_02_sequence.rs covers duplicate/gap/epoch-change; GitHub Actions P4-J7-02 workflow runs protocol/daemon fixtures and UI stream regressions
exit_code: 0 for format, focused test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-J7-02 is triggered by the eventual push and is not awaited
status_change: P4-J7-02 source slice is implemented. Stream envelopes now carry additive monotonic sequence/epoch metadata without changing PROTOCOL_SCHEMA, and stale/gapped display updates fail closed or request snapshot hydration.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: RunStreamBus cursor/terminal retention and UI state are process-local best-effort projections; no cross-process cursor durability, guaranteed network delivery, external provider stream, device synchronization or live/physical proof is claimed
reviewer: Codex root implementation review plus additive wire/schema, per-run sequence, epoch fencing, duplicate/gap handling, terminal replay, SSE Last-Event-ID and EventLog/Receipt authority boundary review; no runtime test reviewer
```
### P4-J7-03 terminal replay evidence (2026-09-18)

```text
source_snapshot: aa45af69 + P4-J7-03 working-tree slice; kiana-protocol/src/lib.rs; kiana-daemon/src/run_stream.rs; kiana-entrypoints/src/{web.rs,web_page.html}; kiana-core/tests/p4_j7_03_terminal_replay.rs; .github/workflows/p4-j7-03-terminal-replay.yml; docs/roadmap/p4-j7-03-terminal-replay-baseline.md; docs/roadmap.md
worktree_status: committed RunStream projections now cover Usage/ToolCall/ApprovalRequested/Error and unknown events remain ignorable; each run retains one bounded terminal envelope, late cursors receive terminal replay with gap signaling, caught-up cursors do not duplicate; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check -p kiana-protocol --test p4_j7_02_sequence -p kiana-daemon --lib --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-daemon/src/run_stream.rs `terminal_is_replayed_to_late_subscriber` projects usage/tool/approval/error, publishes terminal and replays it to a late subscriber; kiana-core/tests/p4_j7_03_terminal_replay.rs guards projection mappings, unknown/gap/epoch and no-provider boundary; GitHub Actions P4-J7-03 workflow runs daemon/protocol fixtures and stream regression
exit_code: 0 for format, focused test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-J7-03 is triggered by the eventual push and is not awaited
status_change: P4-J7-03 source slice is implemented. Terminal responses are replayable to late subscribers without replaying deltas or re-running execution, and committed event kinds have explicit display projections.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: broadcast/terminal retention and gap detection remain process-local best-effort; no cross-process durable stream, network exactly-once delivery, provider-native stream or live/physical effect proof is claimed; EventLog/Receipt remain authoritative
reviewer: Codex root implementation review plus event-kind mapping, additive unknown handling, per-run terminal retention, late-cursor replay, gap/epoch fencing and no-second-loop/no-provider boundary review; no runtime test reviewer
```
### P4-K2-01 trigger authority evidence (2026-09-18)

```text
source_snapshot: 3f1e1d8b + P4-K2-01 working-tree slice; kiana-domain/src/automation.rs; kiana-workflow/src/{lib.rs,durable.rs}; kiana-core/src/{automation.rs,commands.rs}; kiana-workflow/tests/p4_k2_01_trigger.rs; kiana-core/tests/p4_k2_01_trigger.rs; .github/workflows/p4-k2-01-trigger.yml; docs/roadmap/p4-k2-01-trigger-baseline.md; docs/roadmap.md
worktree_status: TriggerDefinition/Fire/Tick now have a focused pure-planner fixture: Fire creates a trigger-bound WorkflowInstance without a capability effect, concurrent Reject is explicit, and only Advance returns a dispatch effect; ControlPlane commits workflow facts before Company/Capability dispatch and rechecks owner/role/approval/expiry/idempotency; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check -p kiana-workflow --test p4_k2_01_trigger -p kiana-core --test p4_k2_01_trigger --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-workflow/tests/p4_k2_01_trigger.rs registers a capability workflow, fires a manual trigger and proves Fire has no effect while Advance produces WorkflowEffect::Dispatch; kiana-core/tests/p4_k2_01_trigger.rs guards trigger bounds, evidence/CAS/idempotency and commit-before-dispatch/no-second-loop; GitHub Actions P4-K2-01 workflow runs fixture, automation baseline and workspace compile
exit_code: 0 for format, focused test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-K2-01 is triggered by the eventual push and is not awaited
status_change: P4-K2-01 source slice is implemented. Triggers can create bounded workflow/run intent but cannot execute capabilities directly; execution remains a later ControlPlane-authorized effect.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: trigger/workflow state and scheduling are local EventLog/inline projections without durable worker/timer lease or cross-process power-loss proof; no production scheduler fairness, external connector/provider receipt or live/physical effect is claimed; legacy watcher remains compatibility-only
reviewer: Codex root implementation review plus trigger owner/role/approval/expiry/firing/concurrency bounds, pure Fire/Advance separation, workflow CAS/idempotency, commit-before-dispatch and no direct capability/no-second-loop boundary review; no runtime test reviewer
```
### P4-K8-01 connector control-plane evidence (2026-09-18)

```text
source_snapshot: 4372c3ba + P4-K8-01 working-tree slice; kiana-domain/src/{connectors.rs,actions.rs,effect_observation.rs}; kiana-core/src/{connectors.rs,commands.rs}; kiana-daemon/src/connectors.rs; kiana-domain/tests/p4_k8_01_connector.rs; kiana-core/tests/p4_k8_01_connector.rs; .github/workflows/p4-k8-01-connector.yml; docs/roadmap/p4-k8-01-connector-baseline.md; docs/roadmap.md
worktree_status: connector definitions/bindings now have a focused scoped contract fixture; core normalizes connector.manage/invoke and re-enters authorize_and_execute, daemon consumes prepared permits against local_fixture hashes and records ProviderReceipt/EffectObservation/Unknown/reconcile facts with idempotency; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check -p kiana-domain --test p4_k8_01_connector -p kiana-core --test p4_k8_01_connector --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-domain/tests/p4_k8_01_connector.rs checks read/write risk, scope, revoke/transport failure and strict wire fields; kiana-core/tests/p4_k8_01_connector.rs guards trust/owner/snapshot/approval/payload/rate/idempotency, receipt/reconcile/Unknown and ControlPlane/Broker/no-network boundary; GitHub Actions P4-K8-01 workflow runs connector/effect/integration fixtures and workspace compile
exit_code: 0 for format, focused test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-K8-01 is triggered by the eventual push and is not awaited
status_change: P4-K8-01 source slice is implemented. Connector operations cannot bypass ControlPlane authorization or claim external success; only bounded local_fixture observations produce receipts, and Unknown remains reconciliable.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: only local_fixture/local_only transport exists; no external HTTP/account/OAuth/webhook/A2A, production SecretStore, cross-process durable registry/worker, notification delivery or live/physical effect proof is claimed; ProviderReceipt is an adapter observation, not external confirmation
reviewer: Codex root implementation review plus definition/binding identity, scope/risk derivation, trust/approval, prepared permit, fixture hash, rate/idempotency, receipt/effect observation, Unknown/reconcile and no-direct-network/no-second-loop boundary review; no runtime test reviewer
```
### P4-L3-01 version governance/drift evidence (2026-09-18)

```text
source_snapshot: e3e53651 + P4-L3-01 working-tree slice; kiana-domain/src/versioning.rs; kiana-domain/src/{model.rs,prompts.rs}; kiana-services/src/api/provider.rs; kiana-core/src/versioning.rs; kiana-domain/tests/p4_l3_01_versioning.rs; kiana-core/tests/p4_l3_01_versioning.rs; .github/workflows/p4-l3-01-versioning.yml; docs/roadmap/p4-l3-01-versioning-baseline.md; docs/roadmap.md
worktree_status: domain-owned RouteDecision/DriftBucket/DriftReport are strict and canonical; RouteDecision binds provider/model/profile, PromptBundle prompt hash, route digest/configuration revision, budget schema and runtime version; ControlPlane projects only owned committed run.model_turn facts into deterministic BTreeMap buckets, deduplicates event IDs and validates observed-turn/unknown-cost semantics with automatic_model_switch=false; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check -p kiana-domain --test p4_l3_01_versioning -p kiana-core --test p4_l3_01_versioning --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-domain/tests/p4_l3_01_versioning.rs changes model profile to prove distinct canonical buckets, records shared-version/error counters, round-trips strict report and rejects unknown fields/automatic switch; kiana-core/tests/p4_l3_01_versioning.rs guards ModelProfile/PromptBundle/RouteDecision/DriftReport dimensions, EventLog-only projection and no provider/broker/model-switch boundary; GitHub Actions P4-L3-01 workflow runs domain/core fixtures and workspace compile
exit_code: 0 for format, focused test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-L3-01 is triggered by the eventual push and is not awaited
status_change: P4-L3-01 source slice is implemented. Version governance now preserves route/prompt/profile/budget/runtime attribution in typed drift buckets without selecting or switching model versions.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: provider ModelProfile catalog remains service-owned and the report is an EventLog query projection; no durable EvalStore, online drift alert, quality causality, billing reconciliation, Promote/Rollback, automatic model switching or live/physical proof is claimed; follow-up EQ/ER/PD/DEP/SC remains
reviewer: Codex root implementation review plus strict serde/domain validation, canonical digest/key consistency, profile/prompt/route/budget/runtime coverage, owner-scoped committed-event projection, deduplicated counters and no-authority/no-provider boundary review; no runtime test reviewer
```
### P4-L5-01 extension/skill authority evidence (2026-09-18)

```text
source_snapshot: 0dffc13a + P4-L5-01 working-tree slice; kiana-skills/src/{types.rs,loader.rs}; kiana-daemon/src/harness_skills.rs; kiana-domain/src/{extensions.rs,extension_contracts.rs}; kiana-capability-broker/src/lib.rs; kiana-domain/tests/p4_l5_01_skill_allowed_tools.rs; kiana-core/tests/p4_l5_01_skill_allowed_tools.rs; .github/workflows/p4-l5-01-extension-skills.yml; docs/roadmap/p4-l5-01-extension-skills-baseline.md; docs/roadmap.md
worktree_status: Skill `allowed-tools` is parsed with bounds and rendered only as JSON display-only Context metadata; ExtensionExecutionContract remains an additional intersection and Broker rechecks effect/handler/required-capability/role/network before dispatch; a read-only extension requesting shell/process/write behavior is rejected with extension_read_only_write_denied; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check -p kiana-daemon --lib --test p4_l5_01_skill_allowed_tools -p kiana-domain --test p4_l5_01_skill_allowed_tools -p kiana-core --test p4_l5_01_skill_allowed_tools --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-daemon/src/harness_skills.rs `skill_allowed_tools_cannot_grant_shell` proves shell.exec is display-only Context; kiana-domain/tests/p4_l5_01_skill_allowed_tools.rs proves a read-only extension cannot classify shell/process as executable while read-only memory.search remains admissible; kiana-core/tests/p4_l5_01_skill_allowed_tools.rs guards parser/prompt/broker/control-plane boundaries and no allowed_tools broker lookup; GitHub Actions P4-L5-01 workflow runs daemon/domain/source fixtures and workspace compile
exit_code: 0 for format, focused test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-L5-01 is triggered by the eventual push and is not awaited
status_change: P4-L5-01 source slice is implemented. Skill declarations cannot grant shell or expand policy; extension effect and capability checks are enforced at the broker boundary.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: allowed-tools is explanatory metadata rather than an authorization fact; full catalog/activation/resource/invocation lifecycle and durable snapshots remain EXT-06..10, non-Skill adapters and supply-chain verification remain P4-L6/INT/PD/ER/SC, and no external/live/physical effect is claimed
reviewer: Codex root implementation review plus strict frontmatter bounds, Context-only prompt authority, JSON display escaping, read-only handler classification, required capability/role/network intersection and no-policy/no-second-loop boundary review; no runtime test reviewer
```
### P4-L6-01 extension supply-chain evidence (2026-09-18)

```text
source_snapshot: 6ce43b54 + P4-L6-01 working-tree slice; kiana-domain/src/extensions.rs; kiana-daemon/src/{extensions.rs,local_packages.rs}; kiana-daemon/tests/p4_l6_01_supply_chain.rs; kiana-domain/tests/p4_l6_01_supply_chain.rs; kiana-core/tests/p4_l6_01_supply_chain.rs; .github/workflows/p4-l6-01-supply-chain.yml; docs/roadmap/p4-l6-01-supply-chain-baseline.md; docs/roadmap.md
worktree_status: ExtensionRegistry now exposes a pure trusted-key Ed25519 verifier used before package activation; install/upgrade/rollback validate manifest, signature, canonical package content hash and migration/rollback references before EventLog CAS, and receipt carries license/signature/capability diff/previous package; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check -p kiana-daemon --lib --test p4_l6_01_supply_chain -p kiana-domain --test p4_l6_01_supply_chain -p kiana-core --test p4_l6_01_supply_chain --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-daemon/src/extensions.rs `extension_signature_is_verified_before_install` signs canonical unsigned manifest with a fixed Ed25519 seed and rejects manifest drift/untrusted publisher; kiana-domain/tests/p4_l6_01_supply_chain.rs checks strict package round-trip, capability/license diff and invalid algorithm/path; kiana-core/tests/p4_l6_01_supply_chain.rs guards verification order, immutable cache/CAS, migration/rollback and no-direct-network boundary; GitHub Actions P4-L6-01 workflow runs daemon/domain/source fixtures and workspace compile
exit_code: 0 for format, focused test-target compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-L6-01 is triggered by the eventual push and is not awaited
status_change: P4-L6-01 source slice is implemented. Extension installation and rollback now require auditable signature/content/compatibility verification before lifecycle facts can activate a package.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: trust roots remain local environment key configuration; no production KMS/transparent log/SBOM/advisory quarantine, cross-process registry store, non-Skill adapter activation, external/live/physical effect or distribution proof is claimed; follow-up EXT-20..31/SC-25..30/DEP-39 remains
reviewer: Codex root implementation review plus strict manifest/signing-bytes contract, trusted publisher lookup, signature-before-content verification order, immutable cache/CAS/idempotency, migration/rollback reference validation and no-script/no-external-effect boundary review; no runtime test reviewer
```
### P4-M6-01 desktop shell evidence (2026-09-18)

```text
source_snapshot: e4259e89 + P4-M6-01 working-tree slice; contrib/desktop/{main.js,preload.js,welcome.html}; contrib/desktop/lib/{worker.js,workspace.js,find-kiana.js,parse-url.js,close-policy.js}; contrib/desktop/tests/p4_m6_01_desktop.test.js; .github/workflows/p4-m6-01-desktop.yml; docs/roadmap/p4-m6-01-desktop-baseline.md; docs/roadmap.md
worktree_status: Electron preload exposes bounded workspace state/open/new/continue; welcome onboarding creates only ~/.kiana/workspaces scratch projects, starts loopback DaemonHost after readiness URL, and reuses tray/background path; worker stop owns a dedicated process group, confirms tree exit after SIGTERM, uses bounded SIGKILL/taskkill fallback and before-quit shutdownComplete fencing; static verification is complete and commit/push follow this evidence update
command_argv:
  git diff --check
  node --check contrib/desktop/main.js
  node --check contrib/desktop/lib/worker.js
  node --check contrib/desktop/tests/p4_m6_01_desktop.test.js
cwd/environment: repository root; Linux x86_64; Node syntax-only checks; CI-only runtime process-tree test; no local Node test, Rust test or smoke binary executed
fixture or cassette: contrib/desktop/tests/p4_m6_01_desktop.test.js `desktop_safe_close_leaves_no_orphan_process` starts a detached child that ignores SIGTERM and verifies bounded fallback leaves no process group, plus source assertions for onboarding/health/tray/background/close; GitHub Actions P4-M6-01 runs the fixture from contrib/desktop
exit_code: 0 for source checks and diff checks; local tests deliberately not run per user instruction; GitHub Actions P4-M6-01 is triggered by the eventual push and is not awaited
status_change: P4-M6-01 source slice is implemented. Desktop workspace lifecycle and safe-close process-tree boundary are covered without adding a second runtime or authority path.
proof-level_change: source plus static syntax evidence only; no local_behavior, durable, live, or physical promotion
limitations: no real desktop session/tray OS integration, macOS backend, notification delivery, signed installer/upgrade, cross-restart DaemonHost recovery or live/physical effect proof is claimed; worker confirmation covers process tree only and EventLog/Receipt remain DaemonHost/ControlPlane authority
reviewer: Codex root implementation review plus preload IPC scope, workspace containment/onboarding, loopback readiness, tray/background behavior, shutdown idempotency, process-group descendant cleanup and bounded unconfirmed-stop failure review; no runtime test reviewer
```
### CM-05 Memory EventStore projection evidence (2026-09-18)

```text
source_snapshot: 57a1b48e + CM-05 working-tree slice; kiana-domain/src/memory_journal.rs; kiana-daemon/src/{harness_memory.rs,lib.rs}; kiana-eventlog/src/{event_store_core.rs,journal_core.rs,jsonl.rs,memory.rs}; kiana-ports/src/lib.rs; kiana-domain/tests/cm05_memory_eventstore.rs; kiana-daemon/tests/cm05_memory_eventstore.rs; kiana-core/tests/cm05_memory_eventstore.rs; .github/workflows/cm05-memory-eventstore.yml; docs/roadmap/cm05-memory-eventstore-baseline.md; docs/roadmap/context-memory.md; docs/roadmap.md
worktree_status: production MemoryScope now carries the DaemonHost EventStore; memory write/review completes server mutation preflight then appends idempotent/CAS `memory.fact` with bounded body ref before JSONL projection, while search/review compare committed stream replay against file and fail with memory_projection_unjournaled or memory_projection_lag; project_memory_facts rebuilds latest records by contiguous stream version and rejects duplicate/gap/hash drift; production proposal batch materialization is explicitly blocked until journal support; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-domain/tests/cm05_memory_eventstore.rs covers no-unjournaled projection, deterministic replay, latest-record replacement, duplicate/gap and body hash failure; kiana-daemon/tests/cm05_memory_eventstore.rs checks journal-before-JSONL ordering and lag guards; kiana-core/tests/cm05_memory_eventstore.rs binds EventStore/CAS/source boundaries; GitHub Actions CM-05 workflow runs domain fixture, daemon/core guards and workspace compile
exit_code: 0 for format, workspace test-target static compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CM-05 is triggered by the eventual push and is not awaited
status_change: CM-05 source slice is implemented. EventStore is the only committed memory fact source in the production handler path; JSONL remains a rebuildable projection with explicit lag/unjournaled refusal.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: EventStore commit and JSONL projection are ordered but not one cross-storage atomic transaction; projection repair is explicit future work, multi-target accept_proposal is blocked in production, and no cross-process worker/power-loss/durable index-generation/processing-grant/source-dependency/live/physical proof is claimed; CM-06/PD/SC/ER remain
reviewer: Codex root implementation review plus EventStore stream identity/CAS/idempotency, mutation-before-journal ordering, bounded body reference, replay reconstruction, projection lag/un-journaled refusal, torn/duplicate/gap handling and no-second-fact-source boundary review; no runtime test reviewer
```
### CM-06 source dependency graph evidence (2026-09-18)

```text
source_snapshot: 2f2a4d02 + CM-06 working-tree slice; kiana-domain/src/source_dependencies.rs; kiana-domain/src/{context_scope.rs,governance.rs,memory.rs}; kiana-core/src/data_governance.rs; kiana-domain/tests/cm06_source_dependencies.rs; kiana-core/tests/cm06_source_dependencies.rs; .github/workflows/cm06-source-dependencies.yml; docs/roadmap/cm06-source-dependencies-baseline.md; docs/roadmap/context-memory.md; docs/roadmap.md
worktree_status: domain-owned SourceDependencyGraph deterministically indexes Memory/Evidence/Event/Artifact/File/ContextPlan/Index/Summary/Plan derived-to-dependency edges; reverse BFS returns affected closure for a source, revoke_source advances data_epoch and emits SourceInvalidation, while unknown/self/duplicate edge, digest and epoch rollback fail closed; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-domain/tests/cm06_source_dependencies.rs builds ordered/reversed graphs, checks deterministic digest/closure, unrelated source isolation, epoch invalidation/rollback and strict unknown/missing/duplicate edges; kiana-core/tests/cm06_source_dependencies.rs guards graph/governance integration markers and no authority bypass; GitHub Actions CM-06 workflow runs domain fixture, core guard and workspace compile
exit_code: 0 for format, workspace test-target static compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CM-06 is triggered by the eventual push and is not awaited
status_change: CM-06 source slice is implemented. Source revocation now has a typed, deterministic impact closure and monotonic governance epoch without deleting or rewriting authoritative facts.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: graph is currently a domain projection contract, not an independently durable EventStore aggregate or UI/query command; DataGovernance cleanup/delete workers, artifact/index/cache integration, cross-process repair and power-loss evidence remain CM-07+/PD/SC/ER; no semantic recall or external/live/physical effect proof is claimed
reviewer: Codex root implementation review plus node/edge bounded validation, canonical ordering/digest, reverse dependency closure, unrelated scope isolation, monotonic data_epoch and no-delete/no-authority mutation boundary review; no runtime test reviewer
```
### CI-06 single ConfigResolver evidence (2026-09-18)

```text
source_snapshot: 3faf9219 + CI-06 working-tree slice; kiana-provider/src/{resolver.rs,config.rs,lib.rs}; kiana-domain/src/{identity_contracts.rs,provider_config.rs}; kiana-daemon/src/model_client.rs; kiana-provider/tests/ci06_config_resolver.rs; kiana-daemon/tests/ci06_config_resolver.rs; kiana-core/tests/ci06_config_resolver.rs; .github/workflows/ci06-config-resolver.yml; docs/roadmap/ci06-config-resolver-baseline.md; docs/roadmap/config-credentials-identity-baseline.md; docs/roadmap.md
worktree_status: ProviderGateway now constructs connections and ProviderConfigSnapshot only through ConfigResolver; trusted workspace overlays are parsed by strict versioned DTO after project_trusted, reject unknown schema/fields, oversized or invalid profiles/env refs/endpoints, and can produce secret-free ConfigSnapshot with canonical config/trust revisions; daemon's alternate profile parser remains under cfg(test) legacy_fixtures only; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-provider/tests/ci06_config_resolver.rs rejects untrusted workspace, unknown major/field, userinfo/query URL, unknown profile and oversize input, then round-trips canonical secret-free snapshot and resolver-backed gateway; kiana-daemon/tests/ci06_config_resolver.rs guards legacy parser isolation; kiana-core/tests/ci06_config_resolver.rs guards trust/revision/no-provider boundary; GitHub Actions CI-06 workflow runs provider fixture, daemon/core guards and workspace compile
exit_code: 0 for format, workspace test-target static compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CI-06 is triggered by the eventual push and is not awaited
status_change: CI-06 source slice is implemented. Product provider configuration has one resolver entrypoint with explicit trust, schema, endpoint and revision fences.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: Config/Connection still hold raw credential only inside provider adapter until CI-07 SecretStore/CredentialLease; workspace overlay is a pure API not yet dynamically loaded per project, no cross-process ConfigSnapshotStore/atomic reload or OAuth/provider policy/live/physical evidence is claimed; legacy compatibility parser remains test-only
reviewer: Codex root implementation review plus resolver single-entrypoint wiring, strict workspace schema and trust ordering, endpoint/redirect-safe URL checks, profile/env bounds, canonical secret-free snapshot, revision fencing and cfg(test)-only legacy boundary review; no runtime test reviewer
```

### CI-07 SecretStore and CredentialLease evidence (2026-09-18)

```text
source_snapshot: 6aedc6f0 + CI-07 working-tree slice; kiana-domain/src/{credentials.rs,contracts.rs,lib.rs,identity_contracts.rs}; kiana-ports/src/lib.rs; kiana-provider/src/{credentials.rs,config.rs,transport.rs,lib.rs}; kiana-capability-broker/src/lib.rs; kiana-domain/tests/ci07_credential_lease.rs; kiana-provider/tests/ci07_secret_store.rs; kiana-capability-broker/tests/ci07_credential_lease.rs; kiana-core/tests/ci07_secret_store.rs; .github/workflows/ci07-secret-store.yml; docs/roadmap/ci07-secret-store-baseline.md; docs/roadmap.md
worktree_status: `CredentialLease` is a strict opaque domain contract with bounded time window, provider/purpose/audience/endpoint binding, digest and one-shot replay rejection; provider `Connection` no longer has a raw credential field and instead carries `SecretRef` plus an adapter handle. Env and explicit-inline adapters materialize raw value only after capacity/deadline checks, consume the lease immediately before header construction, and clear `SecretMaterial` on drop; keyring/file/OS adapters fail closed as explicit unsupported backends. Broker exposes metadata-only lease revalidation/consumption, while snapshots/catalogs remain ref/digest-only; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-domain/tests/ci07_credential_lease.rs covers opaque strict JSON, expiry, provider/endpoint binding and one-shot replay; kiana-provider/src/credentials.rs unit fixtures cover fake inline issue/consume and explicit protected-backend denials; kiana-provider/tests/ci07_secret_store.rs checks snapshot/catalog sentinel exclusion and missing env failure; kiana-capability-broker/tests/ci07_credential_lease.rs checks exact binding/replay; kiana-core/tests/ci07_secret_store.rs guards no-raw-Connection, adapter and Broker effect-boundary markers; GitHub Actions CI-07 workflow runs all fixtures and workspace compile
exit_code: 0 for format, workspace test-target static compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CI-07 is triggered by the eventual push and is not awaited
status_change: CI-07 source slice is implemented. Provider credential material is no longer retained directly on `Connection`; effect-time adapters issue short opaque leases, enforce exact binding and consume once before outbound authentication, with unsupported protected stores refusing rather than falling back. Domain/ports/Broker/snapshot boundaries carry references, digests and lease metadata only.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: explicit `ProviderConfig.api_key` remains a compatibility inline store; keyring/file/OS, cross-process rotation/revocation and generation CAS, OAuth refresh, restart fencing, physical memory erasure and external provider/live effect are not implemented; request clients may internally copy header values, and no durable lease or secret deletion proof is claimed
reviewer: Codex root implementation review plus SecretRef/lease strictness, expiry/replay and purpose/audience/provider/endpoint binding, adapter fail-closed behavior, no-raw snapshot/catalog/port/Broker boundary, effect-time injection/cleanup and no-second-loop/no-network expansion review; no runtime test reviewer
```

### CI-08 ProviderGateway route admission evidence (2026-09-18)

```text
source_snapshot: 6ed3e27d + CI-08 working-tree slice; kiana-domain/src/model.rs; kiana-ports/src/model.rs; kiana-provider/src/{lib.rs,config.rs,request.rs,transport.rs,telemetry.rs,credentials.rs}; kiana-core/src/model_budget.rs; kiana-daemon/src/model_client.rs; kiana-domain/tests/ci08_route_admission.rs; kiana-provider/tests/ci08_route_admission.rs; kiana-core/tests/ci08_route_admission.rs; .github/workflows/ci08-route-admission.yml; docs/roadmap/ci08-route-admission-baseline.md; docs/roadmap.md
worktree_status: `PreparedModelCall` and `ModelCallPermit` now carry optional migration-compatible route digest, configuration revision, authority revision, credential revision and opaque provider-account binding; `JournalModelBudget::reserve_prepared` copies the binding into the committed `model.prepared` fact, while `ProviderGateway::complete_admitted` validates it before budget consume and checks the current SecretStore revision. Transport then rechecks the credential revision after capacity and sends only one opaque account metadata header plus effect-time authentication; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-domain/tests/ci08_route_admission.rs covers exact binding, route/config/authority/credential/account drift, expiry and secret-free permit JSON; kiana-provider/tests/ci08_route_admission.rs runs a loopback fake HTTP provider, asserts one opaque account header/one Authorization injection/no body sentinel and parses a bounded reply; kiana-core/tests/ci08_route_admission.rs guards validation-before-consume, actor-blind provider and revision markers; GitHub Actions CI-08 workflow runs domain/provider/core fixtures and workspace compile
exit_code: 0 for format, workspace test-target static compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CI-08 is triggered by the eventual push and is not awaited
status_change: CI-08 source slice is implemented. Provider admission is now bound to the frozen route, configuration, authority, credential and opaque account revisions before the existing budget/EventLog consume point; env/inline credential changes are detected before network effect, redirects/proxies remain disabled, and the fake provider path demonstrates a single bounded injection without secret projection.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: permit binding fields remain optional for legacy cassette/adapter decoding but networked ProviderGateway requires them; authority freshness still depends on the existing JournalModelBudget/EventStore path, dynamic reload and cross-process permit recovery are absent, provider account is a local digest rather than external identity, custom metadata/header acceptance is only fixture-proven, and no real provider receipt, billing, DNS/host pinning or live/physical effect is claimed
reviewer: Codex root implementation review plus route/config/authority/credential revision ordering, permit/EventLog binding, pre-consume drift rejection, current env revision fence, actor-blind provider metadata, redirect/proxy boundary and no-secret body/telemetry projection review; no runtime test reviewer
```

### CI-09 OAuth and workload identity lifecycle evidence (2026-09-18)

```text
source_snapshot: 4e876446 + CI-09 working-tree slice; kiana-domain/src/{oauth.rs,contracts.rs,lib.rs}; kiana-provider/src/{oauth.rs,lib.rs}; kiana-domain/tests/ci09_oauth_contracts.rs; kiana-core/tests/ci09_oauth_guard.rs; .github/workflows/ci09-oauth-lifecycle.yml; docs/roadmap/ci09-oauth-lifecycle-baseline.md; docs/roadmap.md
worktree_status: domain OAuth contracts keep only flow/callback/token metadata, User/Workload subject, scope, expiry, generation and digests; provider `OAuthManager` owns pending state/verifier and raw tokens, builds S256 PKCE authorization URLs, validates exact redirect/state once, parses bounded strict token responses, performs Notify single-flight refresh with cooldown, classifies transient/permanent/revoked outcomes, and applies generation-CAS rotation/revoke. Optional provider token files use strict envelopes, symlink/permission checks, create-new temporary files, sync and atomic rename; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo check --workspace --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-domain/tests/ci09_oauth_contracts.rs covers User/Workload flow metadata, strict callback, scope canonicalization, expiry, digest and generation/revoke CAS; kiana-provider/src/oauth.rs fixtures cover PKCE/state/redirect replay, strict unknown/oversize/malformed/insufficient token responses, concurrent single-flight, transient preservation, permanent reauth, in-flight rotation CAS and Unix 0600 atomic file; kiana-core/tests/ci09_oauth_guard.rs guards provider ownership, no raw token fields in domain, PKCE/refresh/error/file markers and no kiana-services dependency; GitHub Actions CI-09 workflow runs domain/provider/core fixtures and workspace compile
exit_code: 0 for format, workspace test-target static compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CI-09 is triggered by the eventual push and is not awaited
status_change: CI-09 source slice is implemented. OAuth and workload identities now have bounded PKCE/state/callback contracts and provider-local refresh/rotation/revoke fencing; malformed or insufficient token responses, state/redirect mismatch, refresh replay/generation races, broad file permissions and unknown fields fail closed, while valid tokens can refresh early with single-flight semantics.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: OAuthManager is not yet wired to a real IdP HTTP client or ProviderGateway credential backend; token file intentionally remains provider-secret storage and is only best-effort memory/file hygiene, not HSM/physical erasure; workload attestation, scope policy, browser callback listener, cross-process durable projector/revocation, OAuth receipt/audit and external/live provider effect remain CI-10/11, PD/SC/ER follow-up
reviewer: Codex root implementation review plus strict PKCE/state/redirect binding and one-shot callback, token response bounds/scope, early-skew single-flight, transient/permanent/revoked behavior, generation CAS race, symlink/0600 atomic persistence and domain/provider raw-token boundary review; no runtime test reviewer
```

### CI-10 Provider policy and credential diagnostics evidence (2026-09-18)

```text
source_snapshot: b98072c2 + CI-10 working-tree slice; kiana-domain/src/{credentials.rs,contracts.rs,lib.rs}; kiana-policy/src/{provider.rs,lib.rs}; kiana-protocol/src/lib.rs; kiana-entrypoints/src/{provider_diagnostics.rs,lib.rs,cli.rs,tui.rs}; kiana-policy/tests/ci10_provider_policy.rs; kiana-protocol/tests/ci10_provider_probe.rs; kiana-entrypoints/tests/ci10_provider_diagnostics.rs; kiana-core/tests/ci10_provider_policy_guard.rs; .github/workflows/ci10-provider-policy.yml; docs/roadmap/ci10-provider-policy-baseline.md; docs/roadmap.md
worktree_status: `ProviderPolicyBundle` is a server-owned, strict, digest-bound default-deny policy with deterministic precedence/declaration selection and fail-closed invalid-request handling; configured credentials are necessary but never sufficient, and missing/expired/reauth/scope-insufficient/revoked/unsupported/unknown states have distinct reasons.  Protocol probe request/response and policy-view DTOs carry only SecretRef/digest/generation/expiry/status metadata with canonical scopes and strict unknown-field/digest checks.  CLI, TUI and direct loopback app-server auth status now pass compatibility output through one presence-only sanitizer that redacts key/token/preview fields; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-protocol --tests --locked --offline
  cargo check -p kiana-policy --tests --locked --offline
  cargo check -p kiana-entrypoints --tests --locked --offline
  cargo check -p kiana-core --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-policy/tests/ci10_provider_policy.rs covers default deny, precedence/last declaration, distinct scope/missing reasons, wrong operation, duplicate IDs and strict unknown fields; kiana-protocol/tests/ci10_provider_probe.rs covers canonical SecretRef-only request, digest-bound response, scope status, tamper and unknown fields; kiana-entrypoints/tests/ci10_provider_diagnostics.rs covers nested token/key redaction and non-auth passthrough; kiana-core/tests/ci10_provider_policy_guard.rs guards policy/probe/sanitizer markers and CLI/TUI/HTTP wiring; GitHub Actions CI-10 workflow runs all fixtures and workspace compile
exit_code: 0 for format, focused workspace test-target static compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions CI-10 is triggered by the eventual push and is not awaited
status_change: CI-10 source slice is implemented.  Provider.use evaluation is now an explicit server-owned deny-first boundary, read-only credential probes preserve scope/expiry/re-auth distinctions without authorization side effects, and user-facing auth diagnostics are presence-only.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: policy bundles are not yet loaded from a durable per-project store and ProviderGateway admission does not consume this bundle; probe DTOs are not wired to live IdP/provider health calls; compatibility auth command still owns provider-local credential handling behind the sanitizer; CI-11 remains responsible for durable audit/redaction/rotation/recovery projection; no external/live/physical effect proof is claimed
reviewer: Codex root implementation review plus default-deny/precedence and invalid-policy fail-closed semantics, distinct credential status reasons, SecretRef/digest-only wire DTOs, strict scope/unknown-field checks, CLI/TUI/HTTP presence-only projection and no-second-loop/no-secret boundary review; no runtime test reviewer
```

### SW-04 Child grant admission evidence (2026-09-18)

```text
source_snapshot: 5635a9b1 + SW-04 working-tree slice; kiana-policy/src/grant_scope.rs; kiana-policy/tests/sw04_grant_scope_materialization.rs; kiana-domain/src/swarm.rs; kiana-core/src/{capabilities.rs,cell_registry.rs,collaboration.rs,swarm.rs,lib.rs}; kiana-core/tests/sw04_child_grant.rs; kiana-core/tests/sw04_child_grant_guard.rs; .github/workflows/sw04-child-grant.yml; docs/roadmap/sw04-child-grant-baseline.md; docs/roadmap.md
worktree_status: `derive_swarm_child_grant` now intersects explicit parent/template/department/project/packet/approval GrantScope layers and materializes only a bounded non-delegable `coding`/`builder.packet` CapabilityGrant.  It rejects invalid/stale authority, role/template drift, cross-project packets, path supersets, secret/provider capability substitution and expired packet scope before CellRegistry admission.  SwarmController stores server-generated authority epoch and opaque principal/project IDs; current owner/epoch are rechecked before child lookup.  CellRegistry repeats grant containment and budget subset checks for live reservation and snapshot restore; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-policy --tests --locked --offline
  cargo check -p kiana-core --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-policy/tests/sw04_grant_scope_materialization.rs covers path intersection, non-delegation, coding-only materialization and secret substitution denial; kiana-core/tests/sw04_child_grant.rs covers narrow partition grant, superset path, cross-project packet and stale epoch; kiana-core/tests/sw04_child_grant_guard.rs guards all six layers, controller fencing and CellRegistry budget/grant rechecks; GitHub Actions SW-04 workflow runs policy/core fixtures and workspace compile
exit_code: 0 for format, focused test-target static compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions SW-04 is triggered by the eventual push and is not awaited
status_change: SW-04 source slice is implemented.  Child capability grants are now formed from an explicit authority intersection and cannot widen parent paths, budgets, roles, projects, providers or secret access; the controller remains an admission/supervision-only Cell.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: principal/project values remain local opaque compatibility IDs, SharingGrant and durable authority projector are not yet consumed by this pure derivation, DispatchIntent/QueueEntry atomic reservation and cross-process recovery remain SW-05+, and child fresh Session/Run/Attempt materialization remains SW-07; no external/live/physical effect proof is claimed
reviewer: Codex root implementation review plus six-layer GrantScope intersection, capability/operation/secret/external conversion fence, owner/authority epoch check, packet project/path validation, parent grant/budget subset and snapshot restore recheck, controller no-model boundary and no-second-loop review; no runtime test reviewer
```

### AUT-02 Trusted ClockPort evidence (2026-09-18)

```text
source_snapshot: 0c6654f7 + AUT-02 working-tree slice; kiana-domain/src/{clock.rs,contracts.rs,lib.rs}; kiana-ports/src/lib.rs; kiana-core/src/automation.rs; kiana-domain/tests/aut02_clock_observation.rs; kiana-ports/tests/aut02_clock_port.rs; kiana-core/tests/aut02_clock_guard.rs; .github/workflows/aut02-clock.yml; docs/roadmap/automation-clock-baseline.md; docs/roadmap.md
worktree_status: strict `ClockObservation`/`ClockTrust` now records bounded wall and monotonic samples, source, previous values, revision and digest; zero/overflow samples, revision regressions, trusted rollback mismatch and tamper fail closed.  `ClockPort` samples the observation and shared `require_trusted_deadline` refuses untrusted/expired deadlines, while the old evaluation-only `Clock` trait remains compatibility-only; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-domain --tests --locked --offline
  cargo check -p kiana-ports --tests --locked --offline
  cargo check -p kiana-core --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-domain/tests/aut02_clock_observation.rs covers trusted/rollback observations, zero/overflow/revision/digest rejection; kiana-ports/tests/aut02_clock_port.rs covers deterministic fake sampling, persisted round-trip and untrusted deadline denial; kiana-core/tests/aut02_clock_guard.rs guards the automation boundary and no second scheduler loop; GitHub Actions AUT-02 workflow runs all fixtures and workspace compile
exit_code: 0 for format, focused test-target static compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions AUT-02 is triggered by the eventual push and is not awaited
status_change: AUT-02 source slice is implemented.  Automation now has a versioned wall/monotonic clock contract and a reusable fail-closed deadline gate before scheduler/trigger/lease wiring.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: existing `kiana-core::automation` still uses its compatibility process-local `SystemTime` helper until a later composition change injects ClockPort; no durable observation store, timer worker, trigger occurrence, lease/approval integration, cross-restart scheduler or live/physical proof is claimed
reviewer: Codex root implementation review plus wall/monotonic zero/overflow and rollback trust semantics, revision/digest/persisted observation checks, ClockPort object-safe adapter and deadline hard-deny boundary, compatibility trait isolation and no-second-loop review; no runtime test reviewer
```

### AUT-03 Workflow definition admission evidence (2026-09-18)

```text
source_snapshot: fa9a0c02 + AUT-03 working-tree slice; kiana-domain/src/{automation.rs,contracts.rs}; kiana-workflow/src/durable.rs; kiana-workflow/tests/aut03_definition.rs; kiana-core/src/automation.rs; kiana-core/tests/aut03_definition_guard.rs; .github/workflows/aut03-definition.yml; docs/roadmap/automation-definition-baseline.md; docs/roadmap.md
worktree_status: `WorkflowDefinition` now has a canonical `kiana.workflow-definition.v1` digest/validate_digest contract while retaining the historical wire shape; pure planner admission rejects malformed identity/keys/projects, noncanonical schemas, unknown role, invalid node budget/timeout, dependency cycle/missing refs, recursive/missing subdefinitions and artifact dependency gaps. Existing `(definition_id, version)` insertion remains immutable and all effects still return to ControlPlane after commit; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-workflow --tests --locked --offline
  cargo check -p kiana-core --tests --locked --offline
  cargo check -p kiana-domain --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-workflow/tests/aut03_definition.rs covers stable/tampered digest, cycle, missing artifact dependency, malformed project and immutable version/unknown fields; kiana-core/tests/aut03_definition_guard.rs guards domain digest, planner fail-closed markers and pure/no-I/O/no-second-loop boundary; GitHub Actions AUT-03 workflow runs workflow/core fixtures and workspace compile
exit_code: 0 for format, focused test-target static compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions AUT-03 is triggered by the eventual push and is not awaited
status_change: AUT-03 source slice is implemented. Workflow definitions now have a stable versioned identity and fail-closed structural admission before a workflow fact or effect intent is accepted.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: digest is computed at admission but not yet stored as a separate field on every historical instance/event projection; trigger source/occurrence/authority binding and durable CAS/cursor query remain AUT-04/05, and no scheduler or external/live/physical effect proof is claimed
reviewer: Codex root implementation review plus canonical digest/version identity, unknown-field/role/project/key bounds, DAG and artifact dependency no-panic checks, subdefinition immutability, planner purity and ControlPlane-only effect routing review; no runtime test reviewer
```

### AUT-04 Trigger definition/source admission evidence (2026-09-18)

```text
source_snapshot: 69e0db17 + AUT-04 working-tree slice; kiana-domain/src/{automation.rs,contracts.rs}; kiana-workflow/src/durable.rs; kiana-workflow/tests/aut04_trigger_definition.rs; kiana-core/src/automation.rs; kiana-core/tests/aut04_trigger_guard.rs; .github/workflows/aut04-trigger.yml; docs/roadmap/automation-trigger-baseline.md; docs/roadmap.md
worktree_status: `TriggerDefinition` now has schema-registered shape/digest validation for owner/role/approval references, bounded input JSON, expiry/firing budget, source kind and interval arithmetic. The pure planner invokes it before registering a trigger, then checks owner role, owned approval evidence, definition role/input coverage and existing Manual/Event/Interval source proof; firing keys, duplicate occurrences, concurrency, quota and interval catch-up remain deny-first. Static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-workflow --tests --locked --offline
  cargo check -p kiana-core --tests --locked --offline
  cargo check -p kiana-domain --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-workflow/tests/aut04_trigger_definition.rs covers Manual/Event/Interval shape, digest, malformed source/input/expiry/approval and unknown fields; kiana-core/tests/aut04_trigger_guard.rs guards owner/approval/source evidence, occurrence key, quota/concurrency and pure/no-second-loop boundary; GitHub Actions AUT-04 workflow runs workflow/core fixtures and workspace compile
exit_code: 0 for format, focused test-target static compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions AUT-04 is triggered by the eventual push and is not awaited
status_change: AUT-04 source slice is implemented. Trigger registration and firing now have a bounded, digestable domain shape and source/approval/owner deny-first checks before instance creation.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: project/authority/policy revisions remain authenticated ControlPlane context and approval evidence rather than dedicated TriggerOccurrence fields; durable occurrence/event envelope, CAS/cursor query and multi-scheduler dedup remain AUT-05, with no scheduler or external/live/physical proof claimed
reviewer: Codex root implementation review plus trigger schema/digest and input bound, expiry/interval overflow, source kind/event_ref exactness, owner/role/approval evidence, duplicate firing/concurrency/quota and planner purity/no-second-loop review; no runtime test reviewer
```

### AUT-05 Automation event envelope/CAS evidence (2026-09-18)

```text
source_snapshot: c30b35ae + AUT-05 working-tree slice; kiana-domain/src/{automation.rs,contracts.rs}; kiana-ports/src/lib.rs; kiana-core/src/automation.rs; kiana-domain/tests/aut05_event_envelope.rs; kiana-core/tests/aut05_event_guard.rs; .github/workflows/aut05-event-envelope.yml; docs/roadmap/automation-event-baseline.md; docs/roadmap.md
worktree_status: workflow command facts now receive additive strict `AutomationEventEnvelope` metadata immediately before protected EventStore CAS: workflow aggregate/id, stream version/cursor, request/idempotency key, command/payload digests and envelope digest. Replay validates the envelope against the RuntimeEvent stream cursor and reconstructed request/proof; legacy envelope-less facts remain migration-readable. `EventStorePort::read_stream_after` provides bounded committed cursor reads without granting claim/authority; static verification is complete and commit/push follow this evidence update
command_argv:
  cargo fmt --all
  cargo fmt --all --check
  cargo check -p kiana-domain --tests --locked --offline
  cargo check -p kiana-ports --tests --locked --offline
  cargo check -p kiana-core --tests --locked --offline
  git diff --check
cwd/environment: repository root; Linux x86_64; stable Rust toolchain; locked offline Cargo dependency cache; CI-only runtime tests; no local test or smoke binary executed
fixture or cassette: kiana-domain/tests/aut05_event_envelope.rs covers envelope round-trip, digest binding, cursor/payload drift and unknown fields; kiana-core/tests/aut05_event_guard.rs guards envelope stamping/replay validation, aggregate CAS/idempotency and bounded cursor port; GitHub Actions AUT-05 workflow runs domain/core fixtures and workspace compile
exit_code: 0 for format, focused test-target static compilation and diff checks; local tests deliberately not run per user instruction; GitHub Actions AUT-05 is triggered by the eventual push and is not awaited
status_change: AUT-05 source slice is implemented. Automation events now expose an inspectable aggregate/CAS/idempotency/cursor envelope while preserving the single ControlPlane→EventStore execution spine.
proof-level_change: source plus static compile evidence only; no local_behavior, durable, live, or physical promotion
limitations: legacy facts without envelopes remain compatibility-readable, read_stream_after defaults to bounded filtering for adapters without indexed queries, and durable indexed projector/torn-tail/multi-scheduler recovery remain PD/ER/AUT-08+; no scheduler or external/live/physical effect proof is claimed
reviewer: Codex root implementation review plus strict envelope schema/unknown-field/digest binding, stream cursor/idempotency/request payload consistency, stamping-before-CAS and replay checks, bounded read-only cursor query and legacy migration boundary review; no runtime test reviewer
```
