# DEP-34 release preflight baseline (partial)

The domain now evaluates a read-only release preflight over reproducible build/source state,
Cargo.lock digest, target artifacts, SBOM/checksum/signature evidence, migration registry/preflight
and verified backup. Target entries are unique and bounded; every target must carry matching lock
identity, reproducible evidence, SBOM and verified signature. Any mismatch returns a stable reason
and remediation, while publish_allowed remains false.

DEP-34 remains partial with source/static evidence only. The preflight does not build, sign,
generate an SBOM, verify a real key, inspect a real backup, run migration preflight collectors or
publish a release. No release success is inferred from a report alone.
