# DEP-03 · Deployment compatibility baseline

> Snapshot date: 2026-09-27. This slice defines the version axes and a deterministic compatibility
> matrix. The current local focused verification is recorded below; remote CI results were not awaited.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-03`](../roadmap.md#step-dep-03) |
| source snapshot | historical implementation anchor `b3129bd9`; focused verification on current dirty worktree (2026-10-05) |
| feature_status | `partial` for the read-only compatibility matrix; admission integration remains open |
| proof_level | `local_behavior` for focused matrix fixtures; no durable/live/physical promotion |
| canonical path | current/candidate server snapshots → version axes → compatibility matrix → later admission/migration gate |

`DeploymentVersionAxes` carries app, protocol, domain, store, projection, workflow, provider,
extension, config, authority epoch, data epoch and generation fields. `DeploymentCompatibilityMatrix`
recomputes stable reasons and status. Major mismatches, store/epoch/generation downgrades and
workflow/provider/extension drift block the candidate; app build/config text remains inspectable
inputs rather than caller authority.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| unknown major | protocol/domain/projection major mismatch is blocked |
| downgrade | store format and authority/data/generation rollback are blocked |
| projection minor lag | candidate projection minor below current is blocked before new-work admission |
| workflow/provider/extension drift | route or definition drift is explicit and blocked |
| forged status/reasons | recomputation and digest binding reject tampering |
| stable matrix | equal axes produce Compatible and strict JSON round-trip |
| source boundary | no migration, process, Broker or EventLog effect is added |

## CI and limitations

GitHub Actions is wired through `.github/workflows/dep03-compatibility.yml` with domain fixtures,
the Core source guard and affected-target compilation. The local receipt below is the current
focused proof; remote workflow results were not awaited.

This matrix does not inspect a store, migrate data, verify a release artifact, acquire a lease or
decide a provider outcome. Compatibility windows, schema adapters, startup admission and durable
operation facts remain later DEP steps. The roadmap card stays open until the compatibility result
is enforced before startup/new-work admission, schema/store mismatches are explicit, replayable
fixtures exist, and successful CI evidence is available.

## Local focused verification receipt (2026-10-05)

```text
source_snapshot: current dirty worktree after DEP-00/DEP-01/DEP-02 closure
worktree_status: DEP-03 remains 🔄; local matrix evidence recorded without whole-card promotion; no automatic commit/push/merge/rebase
command_argv: CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0 cargo test -p kiana-domain --test dep03_compatibility -p kiana-core --test dep03_compatibility_guard --locked --offline -- --test-threads=1
cwd·environment: repository root Linux/bash; serial Cargo/test threads; 32 MiB test-thread stack; stripped, non-incremental test profile
fixture·cassette: `kiana-domain/tests/dep03_compatibility.rs` 3/3; `kiana-core/tests/dep03_compatibility_guard.rs` 1/1; unknown major, store/epoch/generation rollback, workflow/provider/extension drift, forged status/reasons/digest and stable round-trip
exit_code: 0
status_change: no roadmap status promotion; focused matrix source proof advanced from unverified local state to local_behavior
proof-level_change: local matrix contract `source` → `local_behavior`; whole DEP-03 card remains partial/open
limitations: matrix is read-only and not wired into startup or new-work admission; app/config fields are not compared; schema/store compatibility is not explicit; no durable replay fixture or successful CI run was observed; no migration, lease, provider, durable/live or physical operation proof
reviewer: integration agent with read-only DEP-03 audit; focused local contract reviewer
```

## 2026-10-07 config revision drift receipt

The compatibility reason matrix now compares the already validated `config_revision` axis. A
candidate with a recomputed digest but a different configuration revision is blocked before any
startup or new-work admission integration is attempted.

```text
source_snapshot: current dirty worktree; kiana-domain/src/deployment_compatibility.rs; kiana-domain/tests/dep03_compatibility.rs; kiana-core/tests/dep03_compatibility_guard.rs
worktree_status: DEP-03 roadmap row 608 remains 🔄 / feature_status=partial; config drift is locally verified; no migration/startup/admission wiring or manifest/lockfile change
command_argv: RED `cargo test -p kiana-domain --test dep03_compatibility config_revision_drift_is_blocked --locked --offline -- --test-threads=1`; GREEN `cargo test -p kiana-domain --test dep03_compatibility --locked --offline -- --test-threads=1`; Core `cargo test -p kiana-core --test dep03_compatibility_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo worker/test thread; `RUST_MIN_STACK=33554432`; stripped/non-incremental profile; domain GREEN max RSS 4034712 KiB, Core guard max RSS 4049724 KiB; swaps 0; cached rerun resources in `/tmp/kiana-roadmap-20261007-corrective/dep03-*.resources`
fixture·cassette: RED returned `Compatible` for current `config-r1` versus candidate `config-r2`; GREEN domain 4/4 covers config drift, major/downgrade/digest drift, forged matrix state and round-trip; Core guard 1/1
exit_code: RED 101 expected; GREEN domain/Core/fmt/diff 0
status_change: selected config-revision deny behavior moves source → local_behavior; whole DEP-03 remains partial/source
proof-level_change: config drift reason and status are locally verified; no durable/live/physical promotion
limitations: matrix is read-only and not enforced by startup/new-work admission; schema/store mismatch, migration, durable replay, CI, lease/provider or external operation proof remain open
reviewer: root integration agent with DEP-03 read-only audit; focused compatibility reviewer
```

## 2026-10-08 projection minor lag receipt

The matrix now treats a candidate projection minor below the current snapshot as a blocked
`projection_version_downgrade`. A recomputed candidate digest cannot turn a lagging projection into
`Compatible`; the later admission layer still needs to map this result to rebuilding and reject new
business commands.

```text
source_snapshot: current dirty worktree; kiana-domain/src/deployment_compatibility.rs; kiana-domain/tests/dep03_compatibility.rs; kiana-core/tests/dep03_compatibility_guard.rs
worktree_status: DEP-03 row 608 remains 🔄 / feature_status=partial / proof_level=source; selected projection lag boundary is locally verified
command_argv: RED and GREEN projection-lag selector; full domain DEP-03 target; Core guard
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, 32 MiB test stack, stripped/non-incremental profile; RED 4090176 KiB / 87% CPU; selector GREEN 4089756 KiB / 84% CPU; full domain 82956 KiB / 14% CPU; Core guard 4031540 KiB / 90% CPU; all runs reported 0 swaps
fixture·cassette: current 1.3 and candidate 1.2 projection versions with candidate axes digest recomputed; RED returned Compatible; GREEN domain 5/5 and Core guard 1/1 return stable `projection_version_downgrade`
exit_code: RED 101 expected; selector/full domain/Core guard 0
status_change: selected projection minor lag denial moves source → local_behavior; whole DEP-03 remains partial/source
proof-level_change: pure matrix lag handling source → local_behavior; no startup/admission, rebuild, migration or durable/live/physical proof
limitations: no startup/new-work admission consumer, rebuilding executor, durable migration/replay, CI/live/physical evidence or provider/lease enforcement
reviewer: root integration agent with DEP-03 compatibility audit; focused projection-version reviewer
```
