# EQ-10 Evaluation Provider Baseline

## Scope

EQ-10 adds `kiana-daemon::eval_runtime::{FakeProviderAdapter, FakeProviderScenario}` as a
strict, deterministic implementation of the existing `ModelClient` port.  A cassette can return
a complete reply, replay text in bounded chunks, return a tool-call declaration, or surface an
explicit malformed-stream/provider error.  The adapter records call count so fixtures can prove
that normalization does not create hidden retries or execute a tool.

The scenario is an offline value object: it has no endpoint, credential, environment lookup,
network client, runner, Broker, scheduler, or background task.  Tool calls remain model output;
they are not dispatched by the fake provider.  Stream callbacks receive only typed `ModelDelta`
values and the complete response remains the returned aggregate.

## Evidence and limits

- `kiana-daemon/tests/eq10_fake_provider.rs` covers complete, complete-streaming text delta,
  chunked stream, accumulator normalization, tool-call, malformed/error, callback stop,
  strict unknown-field, and bounded scenario behavior (10/10).
- `kiana-core/tests/eq10_fake_provider_guard.rs` protects the offline provider-port boundary.
  GitHub Actions runs the fixtures; bounded local tests are also recorded below.

This slice is `feature_status=implemented`, `proof_level=local_behavior`: it does not yet connect the
fake adapter to a DaemonHost target, deny-by-default Broker, fixture store, event/receipt capture,
fault/restart plan, or any real/live/physical provider or external effect.  Those remain EQ-11+.

## 2026-10-05 focused local receipt

The Core source guard was narrowed to the `FakeProviderScenario` source interval so the separate
EQ-11 deny-by-default broker declaration in `eval_runtime.rs` does not create a false EQ-10
boundary failure.

```text
source_snapshot: current dirty worktree; kiana-daemon eval runtime/fixtures and kiana-core fake-provider guard
worktree_status: roadmap row 182 remains 🔄; guard-only correction, no provider or DaemonHost production integration
command_argv: bounded serial cargo test -p kiana-daemon --test eq10_fake_provider --locked --offline -- --test-threads=1; cargo test -p kiana-core --test eq10_fake_provider_guard --locked --offline -- --test-threads=1; cargo fmt --all --check; git diff --check
cwd·environment: repository root Linux/bash; CARGO_BUILD_JOBS=1, RUST_TEST_THREADS=1, RUST_MIN_STACK=33554432, stripped non-incremental tests; daemon max RSS 91360 KiB, Core guard 130340 KiB, fmt 150756 KiB, swaps 0
fixture·cassette: daemon 5/5 (complete, bounded stream, tool declaration without execution, malformed/provider error, strict/bounded scenario); Core guard 1/1
exit_code: all commands 0; logs/resources `/tmp/kiana-roadmap-20261005-evidence/eq10-{daemon-fixed,core-fixed}.{log,resources}`
status_change: selected EQ-10 adapter/guard behavior is local_behavior; whole card remains feature_status=implemented, proof_level=local_behavior
proof-level_change: selected fixtures/guard source → local_behavior; no durable/live/physical promotion
limitations: fake adapter remains outside DaemonHost/ControlPlane/Broker/EventLog/receipt/recovery; malformed partial-stream semantics, live providers and external effects remain open to later EQ cards
reviewer: integration agent with EQ-10 read-only audit; no human release reviewer
```

## 2026-10-07 focused local receipt

```text
source_snapshot: current dirty worktree; kiana-daemon/src/eval_runtime.rs and kiana-daemon/tests/eq10_fake_provider.rs
worktree_status: roadmap row 182 remains 🔄; Complete streaming delta behavior fixed locally; no manifest/lockfile change or second execution path
command_argv: bounded serial `cargo test -p kiana-daemon --test eq10_fake_provider --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test eq10_fake_provider_guard --locked --offline -- --test-threads=1`; RED selector `fake_provider_streaming_complete_emits_non_empty_text_delta` before fix; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, `RUST_MIN_STACK=33554432`, stripped non-incremental test profile; one Cargo/test worker; daemon GREEN max RSS 946904 KiB (~924.7 MiB), Core guard 130308 KiB (~127.3 MiB), GREEN CPU 94%/63%, all observed swap counters 0
fixture·cassette: RED Complete streaming fixture 0/1 (expected no delta, exit 101); GREEN daemon 8/8 and Core guard 1/1; malformed stream emits zero deltas, callback error stops after second delta, normal stream passes ModelStreamAccumulator::finish
exit_code: RED 101 as expected; GREEN focused tests 0; format and diff checks 0; logs/resources `/tmp/kiana-roadmap-20261007-eq10/{red,green,core}.{log,resources}`
status_change: selected EQ-10 adapter/stream behavior promoted from source to local_behavior; whole card remains feature_status=implemented, roadmap status 🔄
proof-level_change: complete streaming delta, malformed deny and callback propagation fixtures source → local_behavior; no durable/live/physical promotion
limitations: no DaemonHost/ControlPlane/Broker/EventLog/receipt/recovery integration, no durable restart/fault proof, malformed partial-stream scenario and live/physical provider effects remain open
reviewer: integration agent with EQ-10 read-only audit; no human release reviewer
```

## 2026-10-08 empty streaming chunk denial

`FakeProviderScenario::Stream` now rejects an empty chunk during construction with the existing
`fake_provider_stream_invalid` error. This keeps the fake cassette strict and prevents
`complete_streaming` from producing a no-op `ModelDelta::Text`, which differs from the other
provider paths that omit empty text deltas.

```text
source_snapshot: current dirty worktree; kiana-daemon/src/eval_runtime.rs; kiana-daemon/tests/eq10_fake_provider.rs; kiana-core/tests/eq10_fake_provider_guard.rs
worktree_status: EQ-10 roadmap row 182 remains 🔄; empty stream chunks are denied at scenario validation; no DaemonHost/ControlPlane integration, manifest/lockfile or second execution path
command_argv: RED `cargo test -p kiana-daemon --test eq10_fake_provider fake_provider_rejects_empty_stream_chunk --locked --offline -- --exact --test-threads=1`; GREEN `cargo test -p kiana-daemon --test eq10_fake_provider --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test eq10_fake_provider_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, `RUST_MIN_STACK=33554432`, stripped/non-incremental test profile; RED max RSS 571652 KiB / 61% CPU; daemon GREEN max RSS 944532 KiB / 90% CPU; Core guard max RSS 128780 KiB / 53% CPU; all measured runs reported 0 swaps
fixture·cassette: RED mixed `("first", "")` stream was accepted and assertion failed; GREEN daemon 9/9 includes empty-chunk denial plus existing complete, chunked stream, callback-stop, malformed/error, tool-call and strict-bound tests; Core guard 1/1 locks the offline ModelClient boundary
exit_code: RED 101 expected; daemon/Core/fmt/diff 0
status_change: selected EQ-10 empty-stream validation moves source → local_behavior; whole card remains feature_status=implemented / proof_level=local_behavior
proof-level_change: empty-chunk refusal and existing fake-provider fixtures locally verified; no durable/live/physical promotion
limitations: no DaemonHost/ControlPlane/Broker/EventLog/receipt/recovery integration, no total-stream memory cap or restart/fault proof, and no live/physical provider effect; complete empty replies and malformed partial-stream semantics remain separate decisions
reviewer: root integration agent with EQ-10 read-only audit; focused fake-provider boundary reviewer
```

## 2026-10-08 total streaming text cap denial

`FakeProviderScenario::Stream` now computes the checked sum of all chunk byte lengths during
validation and rejects a cassette above the runner's `512 KiB` text-normalizer ceiling. The exact
`32 * 16 KiB` boundary remains valid; this prevents `chunks.concat()` from accepting a payload that
the downstream normalizer must reject after allocating the aggregate string.

```text
source_snapshot: current dirty worktree; kiana-daemon/src/eval_runtime.rs; kiana-daemon/tests/eq10_fake_provider.rs; kiana-core/tests/eq10_fake_provider_guard.rs
worktree_status: EQ-10 roadmap row 182 remains 🔄 / feature_status=implemented / proof_level=local_behavior; Stream validation rejects total text > 512 KiB with stable fake_provider_stream_invalid; no DaemonHost/ControlPlane integration, manifest/lockfile or second execution path
command_argv: RED `cargo test -p kiana-daemon --test eq10_fake_provider fake_provider_rejects_stream_over_total_text_limit --locked --offline -- --exact --test-threads=1`; GREEN `/usr/bin/time -v env CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0 cargo test -p kiana-daemon --test eq10_fake_provider --locked --offline -- --test-threads=1`; Core guard same bounded serial command; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo/test worker; stripped/non-incremental test profile; RED max RSS 953652 KiB / 91% CPU; daemon GREEN max RSS 942808 KiB / 93% CPU; Core guard max RSS 130996 KiB / 62% CPU; all measured runs reported 0 swaps
fixture·cassette: RED old validator accepted 33 chunks × 16 KiB and the exact-limit assertion failed; GREEN daemon 10/10 includes oversized denial plus 32 × 16 KiB acceptance; Core guard 1/1 pins MAX_STREAM_TEXT_BYTES, checked total fold and offline ModelClient boundary
exit_code: RED 101 expected; daemon/Core/fmt/diff 0
status_change: selected EQ-10 total-stream memory boundary moves source → local_behavior; whole card remains feature_status=implemented / proof_level=local_behavior
proof-level_change: checked fake-cassette total text bound is locally verified; no durable/live/physical promotion
limitations: no DaemonHost/ControlPlane/Broker/EventLog/receipt/recovery integration, no restart/fault proof, malformed partial-stream semantics and complete empty replies remain separate decisions, and no live/physical provider effect
reviewer: root integration agent with EQ-10 read-only audit; focused fake-provider memory-boundary reviewer
```

## 2026-10-13 DaemonHost evaluation completion path

The bounded fake provider is now exercised through a real `DaemonHost::handle` request. The
fixture uses the existing `KianaHarness` and a trusted project authority, so the request crosses
the authenticated host/control-plane path before the provider call. The test then checks the
persisted `run.authorized`, `run.completed`, and `run.receipt` facts plus the corresponding UI
projection, with request and correlation identity preserved.

```text
source_snapshot: current dirty worktree; kiana-daemon/src/eval_runtime.rs; kiana-daemon/src/lib.rs; kiana-daemon/tests/eq10_fake_provider.rs; kiana-core/tests/eq10_fake_provider_guard.rs
worktree_status: roadmap row 182 remains 🔄 / feature_status=implemented / proof_level=local_behavior; selected fake-provider completion now crosses DaemonHost → ControlPlane → KianaHarness → EventLog/receipt projection; no manifest/lockfile or second execution path
command_argv: bounded serial `cargo test -p kiana-daemon --test eq10_fake_provider --locked --offline -- --exact fake_provider_completes_through_daemon_host_control_plane_eventlog --test-threads=1`; full daemon target with the same locked/offline profile; `cargo test -p kiana-core --test eq10_fake_provider_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode`; one Cargo/compiler job and one Rust test thread; `RUST_MIN_STACK=33554432`, stripped/non-incremental profile; all completed runs reported 0 swaps
fixture·cassette: exact Host integration 1/1; complete daemon target 11/11; Core no-real-executor guard 1/1; fake provider call count is one; persisted facts include `run.authorized`, `run.completed`, `run.receipt`; UI event carries the same run/request/correlation identity
exit_code: all listed selectors 0; Host integration max RSS 4093400 KiB / 80% CPU / 1:55.98; full daemon target 3560800 KiB / 75% CPU / 2:03.77; Core guard 4058788 KiB / 82% CPU / 1:34.99; logs/resources are archived under `/tmp/kiana-roadmap-20261010-evidence/`
status_change: selected DaemonHost completion/event/receipt behavior moves source → local_behavior; whole EQ-10 card and roadmap remain open
proof-level_change: local_behavior only; no durable restart, cross-process recovery, live provider, external effect, or physical proof
limitations: the fixture uses an in-memory EventLog and a fixed trusted project authority; malformed/unknown Host paths, deny-by-default Broker composition, full evaluation evidence capture, fault/restart recovery and live/physical provider effects remain EQ-11+
reviewer: root integration agent with EQ-10 read-only audit; focused DaemonHost and Core guard reviewers
```
