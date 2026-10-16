# DEP-02 · Release bundle baseline

> Snapshot date: 2026-09-27. This slice binds the existing release manifest, provenance,
> signature and verification contracts to a deployment profile. Local Cargo test/build/check/
> clippy/smoke commands are intentionally not run; GitHub Actions owns fixtures and compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-02`](../roadmap.md#step-dep-02) |
| source snapshot | historical implementation anchor `ff3904e5`; focused closure on current dirty worktree (2026-10-05) |
| feature_status | `implemented` for profile-bound release evidence contracts |
| proof_level | `local_behavior` for deny-first focused fixtures; no durable/live/physical promotion |
| canonical path | CI/build provenance → ReleaseManifest/Provenance/Signature → profile-bound release bundle → later admission |

`ReleaseManifest`, `ReleaseProvenance`, `ReleaseSignatureAttestation` and
`ReleaseVerificationReport` remain the existing evidence contracts. `DeploymentReleaseBundle`
requires their exact subject digests, verified status, toolchain/source/Cargo.lock/artifact facts,
and a secret-free payload before a deployment profile can consume the evidence. It never signs,
publishes, installs or starts a release.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| unverified signature | Unknown/failed verification cannot enter a deployment bundle |
| subject mismatch | manifest/provenance/signature/report digest drift is rejected |
| secret marker | API key/token/password/private-key markers are rejected |
| digest/round-trip | stable bundle digest and strict JSON round-trip are required |
| source boundary | no filesystem, process, Broker, EventLog or publication path is added |

## CI and limitations

GitHub Actions is wired through `.github/workflows/dep02-release-bundle.yml` with domain fixtures,
the Core source guard and affected-target compilation. The current local focused receipt below
supersedes the historical no-local-test note; remote workflow results were not awaited.

The bundle is evidence-only. It does not build artifacts, verify a real signature/transparency log,
contact a registry, install a binary, select a supervisor or prove durable/live/physical release
provenance; those effects remain outside this source slice.

## Local focused closure receipt (2026-10-05)

The current worktree ran the release-bundle deny-first fixtures and source guard with serial
Cargo/test threads and a 32 MiB test-thread stack:

```text
source_snapshot: current dirty worktree after DEP-00/DEP-01 closure
worktree_status: DEP-02 row/card promoted to ✅; no automatic commit/push/merge/rebase
command_argv: CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0 cargo test -p kiana-domain --test dep02_release_bundle -p kiana-core --test dep02_release_bundle_guard --locked --offline -- --test-threads=1
cwd·environment: repository root Linux/bash; serial Cargo/test threads; controlled test stack and stripped test profile
fixture·cassette: `kiana-domain/tests/dep02_release_bundle.rs` 2/2; `kiana-core/tests/dep02_release_bundle_guard.rs` 1/1; unverified signature, subject digest drift, secret markers, stable digest/round-trip and evidence-only boundary
exit_code: 0
status_change: DEP-02 promoted from 🔄 to ✅ in `docs/roadmap.md`
proof-level_change: `source` → `local_behavior` for the profile-bound release evidence contract
limitations: no real artifact build, cryptographic signature/transparency-log verification, registry access, CI-generated package, install/start, durable/live/physical release proof; workflow was not awaited
reviewer: integration agent; focused local contract reviewer
```
