# EXT-25 Signed Skill Server Binding Baseline

EXT-25 upgrades the extension scope carried from the signed package registry into a server-bound
execution reference:

- ExtensionExecutionScope now binds the verified package hash, publisher, registry generation,
  selected role, effect, required capabilities, capability-diff digest, network policy digest,
  secret policy digest and a bounded issue/expiry window.
- ExtensionRegistry::skill_context creates the scope from the committed registry snapshot, not
  from prompt text or model metadata. PromptBundle remains context transport only.
- Broker admission treats the scope as an untrusted hint and rechecks the live registry generation,
  enabled package hash, role, manifest policy digests, compatibility and expiry immediately before
  dispatch. Upgrade, revoke, role drift and stale generations fail closed.
- The fixed model-tool mapper does not copy _extension_scopes from model arguments; if a prepared
  request carries a server-provided scope list, the domain schema and Broker admission still
validate and recheck it.

## 2026-10-08 forged contract digest denial

`ExtensionExecutionContract` now recomputes the canonical digest of the server-owned scope
fields at every domain boundary. A static or otherwise forged contract with a valid-looking but
different `scope_digest` is rejected by `validate_at`, `matches_manifest`, and Broker-facing
`check`; the handler effect remains outside the signed scope digest.

```text
source_snapshot: current dirty worktree; kiana-domain/src/extensions.rs; kiana-domain/tests/ext25_signed_skill_scope.rs; kiana-domain/tests/p4_l5_01_skill_allowed_tools.rs; kiana-core/tests/ext25_signed_skill_scope_guard.rs
worktree_status: EXT-25 row 360 remains 🔄 / feature_status=partial / proof_level=source; no manifest/lockfile or second execution path
command_argv: RED `cargo test -p kiana-domain --test ext25_signed_skill_scope signed_skill_contract_rejects_scope_digest_drift --locked --offline -- --test-threads=1`; GREEN `cargo test -p kiana-domain --test ext25_signed_skill_scope --locked --offline -- --test-threads=1`; compatibility `cargo test -p kiana-domain --test p4_l5_01_skill_allowed_tools --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test ext25_signed_skill_scope_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`; roadmap `python3 -B scripts/tests/test_roadmap_goal.py`
cwd·environment: repository root Linux/bash; one Cargo worker/test thread; `RUST_MIN_STACK=33554432`; stripped/non-incremental profile; RED max RSS 230576 KiB / 4% CPU; final domain GREEN max RSS 83112 KiB / 30% CPU; compatibility max RSS 228116 KiB / 65% CPU; final Core guard max RSS 4055184 KiB / 86% CPU; all completed runs reported 0 swaps
fixture·cassette: RED forged `sha256:<64 hex>` scope digest was accepted before the guard; GREEN domain 3/3 covers validate_at and matches_manifest rejection, malformed policy digest shape, and future-issued/equality behavior; compatibility 1/1 recalculates hand-built valid contracts and denies forged Broker check; Core guard 1/1 locks canonical digest marker and existing Broker/Daemon recheck chain
exit_code: RED 101 expected; domain/compatibility/Core/fmt/diff/roadmap 0
status_change: selected forged-contract digest denial moves source → local_behavior; whole EXT-25 remains partial/source
proof-level_change: local domain and source-guard behavior only; no durable/live/physical promotion
limitations: no direct Broker/DaemonHost runtime admission fixture, durable scope store, cross-process recovery, cryptographic bearer token or external/live effect proof; manifest policy semantics and provider/package authority remain outside this local contract slice
reviewer: root integration agent with EXT-25 read-only audit; focused signed-scope reviewer
```

## 2026-10-06 focused local receipt

The stale source guard markers now match the actual multiline `.validate_at(...)` and `.check(...)`
calls. The domain fixture and guard pass; the runtime chain remains bounded by the source audit
ceiling.

```text
source_snapshot: current dirty worktree; extensions domain, daemon admission, broker, harness/runner and EXT-25 fixtures/guard
worktree_status: roadmap row 360 remains 🔄; guard-only marker correction, no new admission path
command_argv: bounded serial cargo test -p kiana-domain --test ext25_signed_skill_scope --locked --offline -- --test-threads=1; cargo test -p kiana-core --test ext25_signed_skill_scope_guard --locked --offline -- --test-threads=1
cwd·environment: repository root Linux/bash; one Cargo/test worker, 32 MiB stack, stripped/non-incremental profile; domain max RSS 83480 KiB, Core 84720 KiB, swaps 0
fixture·cassette: domain 2/2; Core guard 1/1; generation/role/expiry/manifest-policy drift and unknown-field denial; server-bound scope and Broker recheck markers
exit_code: both commands 0; logs/resources `/tmp/kiana-roadmap-20261005-evidence/ext25-{domain,core}.{log,resources}`
status_change: selected EXT-25 scope/guard behavior is local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: selected domain/guard source → local_behavior; no durable/live/physical promotion
limitations: no direct Broker/DaemonHost admission fixture, durable scope storage, cross-process recovery or external/live effect proof; detail-card status still conflicts with canonical roadmap table
reviewer: integration agent with EXT-25 read-only audit; no human release reviewer
```

The fixtures are `kiana-domain/tests/ext25_signed_skill_scope.rs` and
`kiana-core/tests/ext25_signed_skill_scope_guard.rs`; current-head local focused receipts are
recorded below. This step does not claim durable scope storage, real provider
effects, cross-process recovery or physical/live proof.

## 2026-10-06 future-issued scope receipt

The execution window is now fail-closed at both domain validation boundaries: a scope or derived
execution contract cannot be used before `issued_at_unix_ms`, while equality at the issue instant
remains valid. This closes a real time-window gap without changing the wire schema or adding an
admission path.

```text
source_snapshot: current dirty worktree; kiana-domain/src/extensions.rs and kiana-domain/tests/ext25_signed_skill_scope.rs
worktree_status: roadmap row 360 remains 🔄; selected future-issued deny behavior is local_behavior; no manifest/lockfile or second execution path
command_argv: RED then GREEN `cargo test -p kiana-domain --test ext25_signed_skill_scope --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test ext25_signed_skill_scope_guard --locked --offline -- --test-threads=1`; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, `RUST_MIN_STACK=33554432`, stripped/non-incremental profile; one Cargo worker; GREEN domain max RSS 4092852 KiB (~3.90 GiB), Core guard max RSS 83660 KiB (~81.7 MiB), 0 swaps
fixture·cassette: RED 1/2 failed as expected for pre-fix future-issued scope; GREEN domain 2/2 and Core guard 1/1; scope and contract future deny, issue-instant allow; logs/resources `/tmp/kiana-roadmap-20261006-ext25/{red,green}.{log,resources}` and `/tmp/kiana-roadmap-20261006-ext25-root/core.{log,resources}`
exit_code: RED 101 expected; GREEN/Core/fmt/diff 0
status_change: selected scope/contract time-window behavior is recorded as local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: selected time-window deny behavior source → local_behavior; no durable/live/physical promotion
limitations: no direct Broker/DaemonHost admission fixture, durable scope storage, cross-process recovery or external/live effect proof; detail-card/canonical roadmap status drift remains to reconcile
reviewer: root integration agent with EXT-25 read-only audit; no human release reviewer
```
