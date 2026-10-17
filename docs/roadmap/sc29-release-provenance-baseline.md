# SC-29 · Release artifact signing and provenance baseline

SC-29 adds a strict evidence boundary for release artifacts. A release filename or Git tag is
only an input: the exact artifact bytes must match a digest in `ReleaseManifest`, and that
manifest digest must be repeated by SLSA-style `ReleaseProvenance` and the externally-produced
`ReleaseSignatureAttestation`.

## Source contract

- `kiana-domain::ReleaseManifest` binds release ID/tag, source revision/tree digest, Cargo.lock
  digest, toolchain digest, builder identity, SBOM digest and a bounded unique artifact subject
  list. Artifact names are single path components and each subject carries size and SHA-256.
- `ReleaseProvenance` requires an SLSA build type, builder/source/toolchain/invocation digests,
  unique materials and the exact manifest subject digest. Source, builder or toolchain drift is
  blocked before a verification report is marked verified.
- `ReleaseSignatureAttestation` carries algorithm, signer/key/verifier identity, signature digest,
  manifest subject digest, external verification status and a transparency-log entry digest.
  Missing or unverified external checks remain `Unknown`; this domain contract never performs
  cryptography or contacts a transparency service.
- `ReleaseVerificationReport` performs deny-first tag, source, builder, toolchain, manifest,
  provenance, signature-subject and transparency binding. Unknown and blocked reasons remain
  structured and digest-bound.
- `scripts/verify-release-provenance.py` verifies the same contract for actual artifact paths,
  rejects unknown JSON fields and digest/size/name drift, and has a deterministic forged-tag
  self-test. It is offline and read-only; release signing and publication stay outside this step.

## CI-only fixture catalog

`.github/workflows/sc29-release-provenance.yml` runs the following on GitHub Actions; no runtime
tests are run as a local development step:

- `kiana-domain/tests/sc29_release_attestation.rs`: exact success binding, tag/builder/toolchain/
  subject drift, unverified or missing transparency, strict serde and artifact path/digest checks.
- `scripts/verify-release-provenance.py --self-test`: valid manifest/provenance/signature fixture,
  actual artifact bytes and forged-tag rejection.
- `kiana-core/tests/sc29_release_attestation_guard.rs`: source, verifier and workflow boundary
  guard, including no network client in the verifier.

## 2026-10-17 completion slice (CI-only)

The card's rejection paths now produce machine readable, deny-first evidence on both the Rust
contract and the offline verifier instead of relying on prose or a single smoke check:

| rejection path | reason code(s) | fixture |
|---|---|---|
| `文件名/tag 伪造` (filename claims another version) | `release_manifest_artifact_tag_mismatch` | `forged_artifact_filenames_cannot_claim_a_different_version_than_the_tag`, `test_forged_tag_and_filename_are_refused` |
| `文件名/tag 伪造` (forged tag / path smuggling) | `release_verification_tag_mismatch`, `release_manifest_tag_invalid`, `release_artifact_name_invalid` | `test_forged_tag_and_filename_are_refused` |
| `builder/source/toolchain 不匹配` | `release_verification_builder_mismatch`, `release_verification_source_revision_mismatch`, `release_verification_toolchain_mismatch`, `release_verification_source_tree_digest_mismatch`, `release_verification_cargo_lock_digest_mismatch` | `builder_source_and_toolchain_drift_are_blocked_before_subject_binding`, `test_builder_source_and_toolchain_drift_are_refused` |
| `subject digest 不符` | `release_verification_provenance_subject_mismatch`, `release_verification_signature_subject_mismatch`, `release_artifact_digest_mismatch`, `release_artifact_size_mismatch` | `subject_digest_mismatch_is_blocked_for_provenance_signature_and_artifact_bytes`, `test_subject_and_recomputed_digest_mismatch_are_refused`, `test_artifact_bytes_must_match_name_size_and_digest` |
| signature / transparency not verified | `release_verification_signature_unverified`, `release_verification_transparency_missing` (both stay `Unknown`, never `Verified`) | `unverified_or_missing_transparency_stays_unknown_and_unknown_fields_fail`, `test_unverified_or_unlogged_signatures_are_refused` |

- `kiana-domain/src/release_attestation.rs` now also refuses an artifact filename whose
  version token contradicts the signed tag, checks that manifest and provenance source/toolchain
  digests agree before a report can be `Verified`, requires a SLSA build type on provenance, and
  exposes `ReleaseArtifactSubject::verify_bytes` so observed bytes bind name, digest and size
  with distinct stable codes.
- `scripts/tests/test_sc29_release_provenance.py` is the deny-first fixture harness: it drives
  `scripts/verify-release-provenance.py` offline over synthetic evidence, proves every denial
  above before the single success fixture, asserts the verifier output is deterministic and
  read-only, and keeps the forged-tag self-test runnable.
  `scripts/validate-sc29-release-provenance.sh` runs the fixtures plus the self-test and pins the
  marker/reason-code matrix, so the unified `ci` gate and the manual lane execute the same
  evidence.
- `kiana-core/tests/sc29_release_attestation_guard.rs` pins the deny-first ordering, the refusal
  code matrix, the fixture names, the manual lane steps and the shard registration
  (`kiana-domain-s4/4`, `kiana-core-s6/6`), and asserts the verifier has no process or network
  edge and the domain contract grows no signer/publisher.

### Evidence boundary

- **feature_status:** `implemented` for the release manifest, provenance, signature/transparency
  binding contracts, the offline verifier hardening, the deny-first fixtures and the CI wiring.
- **proof_level:** `source`; GitHub CI is wired but its result is intentionally unobserved and no
  local test was run.
- **not claimed:** a production signer or key custody, a real transparency log, registry
  provenance, reproducible build proof, a live GitHub release, or physical artifact
  distribution. External signature verification remains an input (`verified`) that this step
  binds but cannot produce.
- **limitations:** fixtures use synthetic digests and files, so digest recomputation is proven
  for the contract but not against real signed bytes; SC-41 still owns final release-gate
  integration, and the manual lane is not automatically dispatched.
- **reviewer:** source review of the manifest/provenance/signature binding order, the stable
  reason codes, the version-token filename guard, the offline verifier constraints and the
  workflow wiring; no local runtime test reviewer and no CI receipt.

The workflow also runs formatting and workspace test-target compilation. This is `source` proof
with CI wiring only: it does not claim a production signer, registry transparency, live GitHub
release, cryptographic key custody, reproducible builds on every runner, or physical artifact
distribution. SC-41 remains responsible for integrating the verifier into the final release gate.
