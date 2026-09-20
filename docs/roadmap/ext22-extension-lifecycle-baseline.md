# EXT-22 Extension Lifecycle Baseline

The extension lifecycle boundary now has a typed source contract for the inspect → stage →
install → enable path:

- mutations carry server-owned action, extension/project identity, expected registry version,
  idempotency key, actor, reason and trust revision;
- stage/install/enable require an exact package hash and dependency snapshot, while enable also
  requires approval and a configuration snapshot;
- configuration is represented as redacted value digests plus optional secret handles, with
  host/user/project/run precedence and source provenance retained per field;
- the existing ControlPlane `extension.manage` route and daemon append-only lifecycle CAS remain
  the only mutation path; config and dependency declarations do not authorize effects.

The CI-only domain fixture is `kiana-domain/tests/ext22_extension_lifecycle.rs`; the product-path
guard is `kiana-core/tests/ext22_extension_lifecycle_guard.rs`. Local tests are intentionally not
run; GitHub Actions is the validation surface. Full durable stage/install/enable projection,
approval persistence and multi-surface UI commands remain follow-up integration work.
