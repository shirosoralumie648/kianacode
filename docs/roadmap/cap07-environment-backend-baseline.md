# CAP-07 EnvironmentPort and verifiable backend baseline

CAP-07 defines the environment boundary as `probe → plan → prepare → execute →
quiesce → dispose`. Scope, owner, backend/version and plan digest are server-owned
inputs; the port defaults fail-closed when a phase or backend is unsupported. A pure
plan never starts a process.

The current Linux backend is the bwrap plan in `harness_sandbox`: it resolves an
explicit executable or PATH candidate, pins file identity, rejects missing/wrong or
non-Unix backends, requests unshare/proc/dev/tmpfs/cap-drop/clearenv/network isolation,
filters environment variables and refuses path/symlink/private-scope escapes. The
execution-control inspection reports backend and limits without claiming
`behavior_verified`; host enforcement is not inferred from `KIANA_SANDBOX_BACKEND`.

GitHub Actions runs CAP-07 source guards plus existing sandbox/environment and scope
fixtures and workspace target compilation. Local runtime tests and smoke commands are
intentionally not run.

This is source/static evidence only; real kernel/userns enforcement, non-Linux backends,
cross-host process isolation and physical sandbox guarantees remain open.

## 2026-10-05 focused local receipt

The two harness unit assertions were aligned with the current descriptor-pinned mount arguments
(`--ro-bind-fd`/`--bind-fd`); this preserves the FD identity boundary instead of reintroducing
path-only bind assertions.

```text
source_snapshot: current dirty worktree; harness_sandbox FD mount plan, EnvironmentPort guard and container adapter fixtures
worktree_status: roadmap row 237 remains 🔄; stale unit assertions corrected, no DaemonHost/Broker adapter wiring added
command_argv: bounded serial cargo test -p kiana-daemon --lib harness_sandbox --locked --offline -- --test-threads=1; cargo test -p kiana-core --test cap07_environment_backend_guard --locked --offline -- --test-threads=1; cargo test -p kiana-daemon --test cap33_container --locked --offline -- --test-threads=1; cargo test -p kiana-daemon --test dep36_container_adapter --locked --offline -- --test-threads=1
cwd·environment: repository root Linux/bash; one Cargo/test worker, 32 MiB test stack, stripped/non-incremental profile; harness max RSS 917940 KiB, Core guard 133220 KiB, container 944132 KiB, adapter 569784 KiB, swaps 0
fixture·cassette: harness sandbox 5/5; EnvironmentPort guard 2/2; container 2/2; adapter 3/3; bwrap plan remains no-host-fallback and FD-pinned
exit_code: all commands 0; logs/resources `/tmp/kiana-roadmap-20261005-evidence/cap07-{harness,core,container,adapter}.{log,resources}`
status_change: selected CAP-07 plan/adapter behavior is local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: selected fixtures source → local_behavior; no kernel/live/physical promotion
limitations: bwrap is not wired through full EnvironmentPort lifecycle or DaemonHost/Broker; executable-bit probe, kernel/userns/network enforcement, non-Linux/cross-host and physical guarantees remain open
reviewer: integration agent with CAP-07 read-only audit; no human release reviewer
```

## 2026-10-06 executable-bit probe receipt

`canonicalize_file` now rejects a canonical regular file with no Unix execute bit before a bwrap
plan is returned. Both the explicit `KIANA_BWRAP` path and PATH candidates share this helper;
non-Unix builds retain the regular-file check. This is local probe evidence and does not claim
kernel enforcement of the complete isolation profile.

```text
source_snapshot: current dirty worktree; kiana-daemon/src/harness_sandbox.rs and CAP-07 guard
worktree_status: roadmap row 237 remains 🔄 / feature_status=partial / proof_level=source; no manifest/lockfile or automatic commit/push/merge/rebase
command_argv: bounded serial `cargo test -p kiana-daemon --lib harness_sandbox --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test cap07_environment_backend_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; CARGO_BUILD_JOBS=1, RUST_TEST_THREADS=1, 32 MiB test stack, stripped/non-incremental profile; harness 6/6, 22.61s, max RSS 913160 KiB (~891.8 MiB); Core guard 2/2, max RSS 134592 KiB (~131.4 MiB); swaps 0; `/tmp/kiana-roadmap-20261006-cap07-exec/{harness,core-guard}.{log,resources}`
fixture·cassette: existing bwrap plan/environment fixtures plus `canonicalize_file_rejects_regular_file_without_execute_bit`; no host fallback or external process started
exit_code: daemon target 0; formatting and diff checks 0
status_change: executable-bit probe moved from open limitation to tested local_behavior; whole CAP-07 remains partial/source
proof-level_change: selected probe source → local_behavior; no kernel/live/physical promotion
limitations: full EnvironmentPort lifecycle and DaemonHost/Broker wiring, kernel/userns/network enforcement, non-Linux/cross-host behavior and physical isolation remain open
reviewer: root integration agent; focused CAP-07 runtime reviewer
```

## 2026-10-07 container required-dimension enforcement receipt

The container adapter now treats `EnvironmentProbe.enforced` as part of the required-dimension
admission check. A capability marked true without an enforced marker, or with an explicit false
marker, is denied before a plan digest is created. The gVisor dimension uses the same conjunction
when the selected runtime is `runsc`.

```text
source_snapshot: current dirty worktree; kiana-daemon/src/container_environment.rs; kiana-daemon/tests/dep36_container_adapter.rs; kiana-core/tests/cap07_environment_backend_guard.rs
worktree_status: roadmap row 237 remains 🔄 / feature_status=partial / proof_level=source; container plan deny boundary is locally verified; no manifest/lockfile or automatic commit/push/merge/rebase
command_argv: RED `cargo test -p kiana-daemon --test dep36_container_adapter --locked --offline -- --test-threads=1`; GREEN same target; Core guard `cargo test -p kiana-core --test cap07_environment_backend_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo worker/test thread; `RUST_MIN_STACK=33554432`; stripped/non-incremental test profile; RED max RSS 944948 KiB (~922.8 MiB), GREEN max RSS 945336 KiB (~923.2 MiB); CPU 84%/82%; swaps 0
fixture·cassette: RED two new plan fixtures failed because capability-only support was accepted; GREEN adapter 5/5 covers explicit unenforced and missing enforced markers; Core CAP-07 guard 2/2 includes the `enforced`/dimension conjunction markers; no runtime container was started
exit_code: RED 101 expected; GREEN adapter/Core/fmt/diff 0
status_change: selected container required-dimension denial moves source → local_behavior; whole CAP-07 remains feature_status=partial, proof_level=source
proof-level_change: container plan enforcement and missing-marker refusal are locally verified; no kernel/live/physical promotion
limitations: the adapter probe currently reports enforcement conservatively false, full EnvironmentPort lifecycle and DaemonHost/Broker wiring remain open; kernel/userns/network enforcement, non-Linux/cross-host behavior and physical isolation are unproven
reviewer: root integration agent with CAP-07 read-only audit; focused container deny-path reviewer
```

## 2026-10-08 prepared backend identity receipt

`PlannedContainer` now retains the probe backend version and required dimensions. `planned()`
rejects a prepared environment whose backend label/version or enforced-dimension claims drift
from that server-owned plan before any container operation is attempted. The fixture uses the
in-memory plan state and `execute_argv` only; no runtime process is started.

```text
source_snapshot: current dirty worktree; kiana-daemon/src/container_environment.rs; kiana-daemon/tests/dep36_container_adapter.rs; kiana-core/tests/cap07_environment_backend_guard.rs
worktree_status: CAP-07 row 237 remains 🔄 / feature_status=partial / proof_level=source; prepared backend/version/dimension binding now local_behavior; no DaemonHost/Broker lifecycle, manifest/lockfile or second execution path
command_argv: RED `cargo test -p kiana-daemon --test dep36_container_adapter forged_prepared_backend_is_denied_before_container_exec --locked --offline -- --exact --test-threads=1`; GREEN daemon `cargo test -p kiana-daemon --test dep36_container_adapter --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test cap07_environment_backend_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread, `RUST_MIN_STACK=33554432`, stripped/non-incremental tests; RED max RSS 950596 KiB / 84% CPU; daemon GREEN max RSS 948076 KiB / 96% CPU; Core guard max RSS 131112 KiB / 64% CPU; all measured runs reported 0 swaps; logs/resources `/tmp/kiana-roadmap-20261008-cap07-{red,daemon,core}.{log,resources}`
fixture·cassette: RED forged backend reached the old operation-validation path; GREEN adapter 6/6 and Core guard 2/2 return `container_environment_identity_mismatch` before runtime execution; no container process started
exit_code: RED 101 expected; GREEN daemon/Core 0
status_change: selected CAP-07 prepared identity binding moves source → local_behavior; whole card remains feature_status=partial, proof_level=source
proof-level_change: backend/version/required-dimension binding source → local_behavior; no kernel/live/physical promotion
limitations: probe still reports enforcement conservatively false, bwrap executable remains path/TOCTOU-bound, full EnvironmentPort/DaemonHost/Broker lifecycle, kernel/userns/network enforcement and physical isolation remain open
reviewer: root integration agent with CAP-07 read-only audit; focused prepared-environment identity reviewer
```

## 2026-10-08 backend version identity denial

`EnvironmentPort::plan` now validates the probe's `backend_version` with the existing bounded
identity helper before canonicalizing the workspace or creating a plan digest. Empty, whitespace,
control-character, slash-containing or oversized versions cannot become server-owned plan state.

```text
source_snapshot: current dirty worktree; kiana-daemon/src/container_environment.rs; kiana-daemon/tests/dep36_container_adapter.rs; kiana-core/tests/cap07_environment_backend_guard.rs
worktree_status: CAP-07 row 237 remains 🔄 / feature_status=partial / proof_level=source; selected backend-version input boundary is locally verified
command_argv: RED/GREEN invalid-version selector; full `dep36_container_adapter`; Core CAP-07 guard
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, 32 MiB test stack, stripped/non-incremental profile; RED 943724 KiB / 69% CPU; selector GREEN 945656 KiB / 92% CPU; full adapter 91096 KiB / 57% CPU; Core guard 132088 KiB / 60% CPU; all runs reported 0 swaps
fixture·cassette: supported probe with empty backend_version; RED plan was accepted, GREEN selector returns stable `container_backend_version_invalid`, adapter 7/7 and guard 2/2
exit_code: RED 101 expected; selector/full adapter/Core guard 0
status_change: selected backend-version identity denial moves source → local_behavior; whole CAP-07 remains partial/source
proof-level_change: plan identity input source → local_behavior; no runtime container, kernel, TOCTOU, durable/live or physical proof
limitations: full EnvironmentPort/DaemonHost/Broker lifecycle, inode identity across plan/prepare, kernel/userns/network enforcement and cross-host behavior remain open
reviewer: root integration agent with CAP-07 environment audit; focused backend-version reviewer
```
