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
lock, create a backup or verify a cryptographic signature. The signature digest is only a binding
until a release manifest/signature service is implemented by later DEP steps.

DEP-27 therefore remains partial with source/static evidence only. MigrationRunnerPort, durable
registry storage, release manifest production, cryptographic signature verification and GitHub CI
runtime receipt remain open.
