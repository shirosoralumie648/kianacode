# EXT-22 Extension Lifecycle Baseline

The extension lifecycle boundary now has a typed source contract for the inspect → stage →
install → enable path:

- mutations carry server-owned action, extension/project identity, expected registry version,
  idempotency key, actor, reason and trust revision;
- stage/install/enable require an exact package hash and dependency snapshot, while enable also
  requires approval and a configuration snapshot;
- configuration is represented as redacted value digests plus optional secret handles, with
  host/user/project/run precedence and source provenance retained per field;
- the existing ControlPlane `extension.manage` route and daemon append-only lifecycle CAS remain
  the only mutation path; config and dependency declarations do not authorize effects.

The domain fixture is `kiana-domain/tests/ext22_extension_lifecycle.rs`; the product-path guard is
`kiana-core/tests/ext22_extension_lifecycle_guard.rs`. Current-head local focused receipts are
recorded below. Full durable stage/install/enable projection,
approval persistence and multi-surface UI commands remain follow-up integration work.

## 2026-10-06 focused local receipt

The Core guard checks the domain-owned `EXTENSION_MANAGE_OPERATION` constant used by the
ControlPlane instead of a stale literal. This confirms the route marker without claiming that the
typed lifecycle snapshot/mutation is consumed by the runtime manage path.

```text
source_snapshot: current dirty worktree; extension lifecycle domain contract, ControlPlane guard and daemon lifecycle source
worktree_status: roadmap row 357 remains 🔄; guard-only marker correction, no Stage command or lifecycle runtime wiring
command_argv: bounded serial cargo test -p kiana-domain --test ext22_extension_lifecycle --locked --offline -- --test-threads=1; cargo test -p kiana-core --test ext22_extension_lifecycle_guard --locked --offline -- --test-threads=1
cwd·environment: repository root Linux/bash; one Cargo/test worker, 32 MiB stack, stripped/non-incremental profile; domain max RSS 82804 KiB, Core 84508 KiB, swaps 0
fixture·cassette: domain 3/3; Core guard 1/1; typed stage/config/approval denial and constant-backed ControlPlane route marker
exit_code: both commands 0; logs/resources `/tmp/kiana-roadmap-20261005-evidence/ext22-{domain,core}.{log,resources}`
status_change: selected EXT-22 contract/guard behavior is local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: selected domain/guard source → local_behavior; no durable/live/physical promotion
limitations: typed lifecycle/config snapshots have no production consumers, no Stage command, install auto-enables and runtime enable does not consume dependency/config/approval snapshots; durable CAS/recovery and UI/multi-surface integration remain open
reviewer: integration agent with EXT-22 read-only audit; no human release reviewer
```

## 2026-10-08 install staging fence

The daemon no longer treats a verified Skill package as enabled during install, upgrade or
rollback. `activation_state` returns `staged` for every package; only the explicit enable action
can move a record into the enabled visibility set, so installation cannot inject Skill context or
remove the Activate action before approval/configuration wiring is present.

```text
source_snapshot: current dirty worktree; kiana-daemon/src/extensions.rs; kiana-core/tests/ext22_extension_lifecycle_guard.rs
worktree_status: roadmap row 357 remains 🔄 / feature_status=partial / proof_level=source; selected install-state boundary is locally verified
command_argv: RED and GREEN private daemon selector; Core EXT-22 guard; broader daemon `extensions::tests` module regression
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, 32 MiB test stack, stripped/non-incremental profile; RED 90492 KiB / 24% CPU; GREEN 915464 KiB / 65% CPU; Core guard 129576 KiB / 39% CPU; module regression 90992 KiB / 53% CPU; all runs reported 0 swaps
fixture·cassette: existing Skill manifest fixture; RED expected `staged` assertion observed `enabled`; GREEN daemon 1/1 and Core guard 1/1; broader module 1/2 fails on pre-existing signature error prefix (`port_failed:extension_signature_verification_failed`)
exit_code: RED 101 expected; daemon/Core guard 0; broader module regression 101 baseline failure
status_change: selected install staging moves source → local_behavior; whole EXT-22 remains partial/source
proof-level_change: install-state source → local_behavior; no durable/live/physical promotion
limitations: typed approval/config/dependency/trust snapshots still have no complete protocol-to-daemon consumer; the broader daemon extension unit module retains a pre-existing signature error-prefix failure; durable CAS/recovery and UI/multi-surface integration remain open
reviewer: root integration agent with EXT-22 lifecycle audit; focused install-state reviewer
```
