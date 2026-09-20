# EXT-20 Extension Supply Chain Baseline

The existing ExtensionRegistry now has a CI-locked supply-chain boundary:

- canonical Ed25519 signing bytes are verified against daemon-trusted publisher keys;
- cached package bytes are rehashed, content hash is recomputed over canonical files, and package
  file count/encoded size/content size/UTF-8/path/migration limits are enforced;
- license/publisher/compatibility/revocation and required migration references are checked before
  lifecycle state changes;
- cache entries are inert until an `extension.lifecycle` event commits the state; package scripts
  are never executed during verification.

The source guard is `kiana-core/tests/ext20_extension_supply_chain_guard.rs`. Local tests are
intentionally not run; GitHub Actions is the validation surface. External marketplace, SBOM and
live signature-service evidence remain deferred.
