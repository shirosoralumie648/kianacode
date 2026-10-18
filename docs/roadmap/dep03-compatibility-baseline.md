# DEP-03 · Deployment compatibility baseline

> Snapshot date: 2026-10-18 (first slice 2026-09-27). This slice defines the version axes and a
> deterministic compatibility matrix. The current local focused verification is recorded below;
> remote CI results were not awaited.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-03`](../roadmap.md#step-dep-03) |
| source snapshot | historical implementation anchor `b3129bd9`; focused verification on the DEP-03 worktree (2026-10-18) |
| feature_status | `partial` for the read-only compatibility matrix; admission integration remains open |
| proof_level | `local_behavior` for focused matrix fixtures; no durable/live/physical promotion |
| canonical path | current/candidate server snapshots → version axes → compatibility matrix → later admission/migration gate |
| schema registration | `kiana.deployment-compatibility.v1` is registered in `kiana_domain::SCHEMA_CONTRACTS` (Domain, owner `kiana-domain`, unknown fields denied, backward-compatible minor policy) |

`DeploymentVersionAxes` carries app, protocol, domain, store, projection, workflow, provider,
extension, config, authority epoch, data epoch and generation fields. `DeploymentCompatibilityMatrix`
recomputes stable reasons and status. Major mismatches, store/epoch/generation downgrades and
workflow/provider/extension drift block the candidate. Config revision text is also a blocking
axis: `config_revision` is compared and drift yields `config_revision_drift`. App build text is
not compared by the reason function; it stays a digest-bound, non-blocking axis (see the
app_build decision below).

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| unknown major | protocol/domain/projection major mismatch is blocked |
| registry boundary | `kiana.deployment-compatibility.v1` is registered as a Domain schema and an unknown major fails closed there too |
| store format | a layout above `MAX_SUPPORTED_STORE_FORMAT` (`JOURNAL_WRITER_VERSION`) is blocked as `store_format_unsupported`, on either side; downgrade is `store_format_downgrade` |
| downgrade | store format and authority/data/generation rollback are blocked |
| projection minor lag | candidate projection minor below current is blocked before new-work admission |
| workflow/provider/extension drift | route or definition drift is explicit and blocked |
| config drift | a different `config_revision` is blocked as `config_revision_drift` |
| forged status/reasons | recomputation and digest binding reject tampering |
| differing app build | a different `app_build` with a recomputed axes digest stays `Compatible` with empty reasons; without the digest recompute it fails `deployment_compatibility_axes_digest_mismatch` |
| stable matrix | equal axes produce Compatible and strict JSON round-trip |
| source boundary | no migration, process, Broker or EventLog effect is added |

## Store/schema mismatch decision (2026-10-18)

The card asks for an explicit store/schema mismatch decision instead of a silent accept. The
matrix now applies a supported-format bound:

- `MAX_SUPPORTED_STORE_FORMAT` is `kiana_domain::JOURNAL_WRITER_VERSION` (currently `2`), the
  JSONL layout this workspace actually writes and reads. It is not a caller-supplied constant.
- Any snapshot — current or candidate — whose `store_format` is above that bound is `Blocked`
  with the stable reason `store_format_unsupported`. Equal-to-equal no longer launders a future
  layout into `Compatible`.
- `candidate.store_format < current.store_format` keeps the stable `store_format_downgrade`
  reason. When both facts hold the reasons appear in that fixed order
  (`store_format_unsupported`, then `store_format_downgrade`).
- Domain schema majors are unchanged: `domain_schema_major_mismatch` covers an unknown or
  different major. No data is migrated, no store is opened; this layer only names the refusal.

## app_build decision (2026-10-18)

`app_build` is **digest-bound and non-blocking**. It is part of `DeploymentVersionAxes::digest()`,
so changing it without recomputing `axes_digest` fails validation with
`deployment_compatibility_axes_digest_mismatch`. The reason function deliberately never reads it,
so a different build over identical data-facing axes evaluates to `Compatible` with empty
reasons. Replacing the process is a rollout/admission concern for DEP-10/DEP-12; this matrix must
not claim data compatibility on the strength of a build string, and must not block a build swap
that the data axes allow. The card-explicit fixture
`differing_app_build_stays_compatible_with_empty_reasons` is the last (success) fixture in
`kiana-domain/tests/dep03_compatibility.rs`.

## CI and limitations

GitHub Actions is wired through `.github/workflows/dep03-compatibility.yml` with `workflow_dispatch`
only and `permissions: contents: read`. The lane fetches locked dependencies
(`cargo fetch --locked`), gates formatting, runs the Core guard and the domain fixtures serially, and
compiles both targets with `--no-run`. The local receipt below is the current focused proof; remote
workflow results were not awaited.

This matrix does not inspect a store, migrate data, verify a release artifact, acquire a lease or
decide a provider outcome. Compatibility windows, schema adapters, startup admission and durable
operation facts remain later DEP steps. The roadmap card stays open until the compatibility result
is enforced before startup/new-work admission, replayable fixtures cover the remaining admission
mapping, and successful CI evidence is available. The store-format bound and the app_build decision
above are resolved inside the matrix; they do not imply admission wiring (reserved to DEP-10/DEP-12).

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

## 2026-10-18 completion slice receipt (store bound, app_build, registration)

```text
source_snapshot: DEP-03 worktree D:/code/kiana-steps/dep03 (branch step/dep03-20261018-1); kiana-domain/src/deployment_compatibility.rs; kiana-domain/src/contracts.rs; kiana-domain/tests/dep03_compatibility.rs; kiana-core/tests/dep03_compatibility_guard.rs; .github/workflows/dep03-compatibility.yml; scripts/ci/doc-reference-exemptions.txt
worktree_status: DEP-03 row 608 remains 🔄 / feature_status=partial / proof_level=local_behavior; store-format bound, app_build decision, schema registration, guard markers, lane extension and exemption pruning are locally verified; no startup/new-work admission wiring
command_argv: RED `cargo test -p kiana-domain --test dep03_compatibility --offline -- --test-threads=1` after temporarily reverting the bound and the registry entry; GREEN same command; GREEN `cargo test -p kiana-core --test dep03_compatibility_guard --offline -- --test-threads=1`; static `cargo check -p kiana-domain --tests --offline`; `bash scripts/ci/validate-doc-references.sh`
cwd·environment: Windows/Git Bash at the worktree root; CARGO_TARGET_DIR=D:/code/kiana-steps/target-dep03; CARGO_BUILD_JOBS=4; RUST_MIN_STACK=33554432; --offline; one test thread
fixture·cassette: RED 7 passed / 2 failed (exit 101) — `schema must be registered` and `store_format_unsupported` returned `Compatible`; GREEN domain 9/9 (exit 0) — registry boundary, unsupported store format on either side plus downgrade ordering, unknown major, downgrade, drift, config drift, projection lag, epoch/schema-major rollback, forged status, identical round-trip, and the card-explicit `differing_app_build_stays_compatible_with_empty_reasons`; GREEN Core guard 5/5 (exit 0)
exit_code: RED 101 expected; GREEN domain 0; GREEN Core 0; cargo check 0; doc-reference validator 0
status_change: whole DEP-03 remains 🔄 / partial; this slice adds local_behavior evidence for the store/schema decision, the app_build decision and registry registration
proof-level_change: store-format refusal and app_build non-blocking compatibility source → local_behavior for the focused fixtures; no durable/live/physical promotion
limitations: matrix is read-only and not enforced by startup/new-work admission (DEP-10/DEP-12); no migration adapter, store inspection, lease, provider, durable replay, live/physical or remote-CI proof; admission mapping of `Blocked` to rejection remains open; `registered_schemas_have_valid_owners` in `kiana-domain/src/contracts.rs` fails on this branch for the pre-existing `kiana-client` owner entries (verified identical on unmodified source), so the registry test group was not green and the new entry only had its own guard coverage
reviewer: DEP-03 step agent with focused domain/Core review
```
