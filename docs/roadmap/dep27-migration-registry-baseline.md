# DEP-27 migration registry baseline (partial)

The domain now defines a strict MigrationRegistry contract. Each registry is versioned and
checksum-bound, contains ordered forward-only steps, and binds a release manifest digest, artifact
digest and signature digest. Each step requires an owner, a verified-backup precondition, a
source schema digest, an expected source revision, an explicit checksum, an upcaster name and a
bounded compatibility window. Unknown fields, unknown digest shapes, duplicate IDs/checksums,
ordinal gaps, version gaps, missing backup requirements, owner drift and down migrations fail
closed.

The negative matrix is recorded explicitly: unknown migration, checksum drift, duplicate version,
no owner, missing backup requirement and forged down migration are rejected before any runner or
effect call. Registry validation is pure domain code and does not apply migrations, acquire a
lock, or create a backup.

## Release signature verification

`MigrationReleaseBinding.signature_digest` names the key a registry was signed with, and
`signature_value` carries the hex Ed25519 signature itself. Both are bound into the registry
digest, and the domain validates only their shape — a digest is a claim, not a proof.

Verification is the adapter's job, because `kiana-domain` has no cryptography dependency.
`kiana-daemon`'s `verify_registry_signature` verifies an Ed25519 signature over
`MigrationRegistry::signing_bytes` against a key the **caller** supplies. The signing bytes are the
canonical form of the registry with the signature binding excluded, so a signature cannot be
re-pointed at a different key, and any change to the steps, owner, checksum or release digests
invalidates it.

The trusted key set is keyed by release digest then key id and comes from the operator, never from
the registry being verified. A registry that names a key nobody trusts is refused regardless of
whether its signature is well-formed.

## Fixtures

`kiana-daemon/tests/dep27_registry_signature.rs` signs a real registry with a generated Ed25519
key and proves that a genuine signature verifies, and that a signature from a different key, a
registry tampered with after signing, a registry with no signature at all, an untrusted key id and
a malformed (non-hex) signature are each refused with a stable reason.

## Still open

DEP-27 remains partial with source/static evidence only. `MigrationRunnerPort` is still the
default unsupported port, durable registry storage and release-manifest production are not
implemented, and no migration receipt exists. The verifier proves a registry is the one that was
signed; it does not prove the release itself was authorised to sign.

