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

The workflow also runs formatting and workspace test-target compilation. This is `source` proof
with CI wiring only: it does not claim a production signer, registry transparency, live GitHub
release, cryptographic key custody, reproducible builds on every runner, or physical artifact
distribution. SC-41 remains responsible for integrating the verifier into the final release gate.
