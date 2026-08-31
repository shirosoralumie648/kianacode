# Kiana 当前状态账本

> 本文件是“当前状态”的唯一汇总入口。  
> 更新规则：只有绑定源码快照、精确命令和证据产物后，才能提升状态或证明等级。  
> 当前工作树：包含未提交修改；以下结论不得当作干净发布基线。

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

| 能力 | 状态 | 证明等级 | 当前边界 |
|---|---|---|---|
| 本地受信项目上的受控 coding 行为 | partial | local_behavior | 受固定本机、cassette/fake-script 和现有 smoke 限制 |
| `shell` / `apply_patch` broker 主路径 | partial | local_behavior | 仍需加强 TOCTOU、原子 patch、进程树取消和输出 redaction |
| CLI / Workbench / loopback Web 共用 DaemonHost | partial | local_behavior | Web session ownership、异步状态和多标签页隔离仍有缺口 |
| 基础 trust / sandbox / role / path / memory policy | partial | local_behavior | DaemonHost 已从 stored ProjectTrust 派生项目可信度；固定本地主体、role/department assignment 和持久身份仍未完成 |
| WorkPacket / Symposium / Review 领域对象 | partial | source/local_behavior | 现有 schema 较窄，独立 Reviewer 和完整生命周期尚未完成 |
| JSONL EventLog / 基础 Receipt | partial | local_behavior | 当前按 request sequence；不是 aggregate/CAS durable authority |
| Desktop Web 壳 | partial | local_behavior | 安装、升级、worker 崩溃恢复和供应链证据未达生产级 |
| live provider | not_supported | source | provider/cassette 存在不等于 live 证明 |
| token streaming | not_supported | source | 当前 Web 明确不提供 token streaming |
| 跨进程完整 resume | deferred | source | session/run/lock/approval 仍有进程内状态 |
| 支付、外卖、打车、旅行预订 | not_supported | source | 尚无满足身份、审批、幂等、对账和证据要求的 adapter |
| 智能家居和物理设备控制 | not_supported | source | 尚无独立安全控制器、watchdog、急停和物理证据 |
| 企业租户、RBAC、远程执行 | deferred | source | 必须先完成个人本地核心并重新设计身份和租户边界 |

## 3. Gate 0 当前结果

Gate 0 整体仍未通过；本轮已关闭 app-server schema 集合缺口并修复 daemon approval actor mismatch，但 release smoke 仍被冻结的 legacy CLI/SDK 测试阻塞：

```text
当前已验证：
cargo fmt --all --check
cargo check --workspace --locked --offline
cargo clippy --workspace --all-targets --locked --offline
cargo test -p kiana-core --test dependency_boundaries --locked --offline
cargo test -p kiana-daemon --test control_plane --locked --offline -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
bash scripts/v10-workbench-smoke.sh
→ 均通过

仍未通过：
cargo test --workspace --locked --offline --no-fail-fast
bash scripts/release-smoke.sh
→ legacy entrypoints CLI/SDK tests 失败；DaemonHost CompanyOS focused path 已通过
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
| P1-06 | generic MCP 与外部生活能力的风险隔离 | target/not_supported | live/physical 前置 |
| P2-01 | aggregate event stream、CAS、重放和恢复 | partial | durable |
| P3-01 | Template/Cell/SpawnPlan/Lease/Grant/DelegationPacket | partial | durable |
| P3-02 | Planner → fresh Builder → independent Reviewer → Closer | partial | local_behavior → durable |

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


