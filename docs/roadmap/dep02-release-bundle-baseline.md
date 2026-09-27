# DEP-02 · Release bundle baseline

> Snapshot date: 2026-09-27. This slice binds the existing release manifest, provenance,
> signature and verification contracts to a deployment profile. Local Cargo test/build/check/
> clippy/smoke commands are intentionally not run; GitHub Actions owns fixtures and compilation.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-02`](../roadmap.md#step-dep-02) |
| source snapshot | `ff3904e5` plus this DEP-02 source slice |
| feature_status | `partial` for deployment-bound release evidence |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
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

GitHub Actions runs `.github/workflows/dep02-release-bundle.yml` with domain fixtures, the Core
source guard and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

The bundle is evidence-only. It does not build artifacts, verify a real signature/transparency log,
contact a registry, install a binary, select a supervisor or prove durable/live/physical release
provenance; those effects remain outside this source slice.
