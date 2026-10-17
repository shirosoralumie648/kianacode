# SC-26 ExtensionManifest and CapabilityCatalog baseline

## Delivered source slice

- `kiana-domain::extension_manifest_digest` binds the complete validated manifest, including
  signature metadata, effect, version, requirements and capability declarations.
- `CapabilityCatalog` records requested versus effective capabilities for each manifest. Effective
  capabilities are the intersection of host availability, parent/Grant scope and exact approval;
  `provided_capabilities` or skill `allowed-tools` are not authorization inputs.
- ReadOnly manifests cannot request write-capable operations. Missing intersection members,
  duplicate extension identities, digest drift and widened effective sets fail closed.
- The catalog is a value-only snapshot; it does not load packages, execute tools or call Broker.

## Boundary and proof ceiling

The source/ProjectTrust and extension lifecycle adapters still supply filesystem/signature and
activation facts. This step does not claim package installation, signed-key trust, sandboxing,
runtime revocation, durable catalog storage or external effect safety; those belong to SC-25/27+
and the existing ControlPlane/Broker path.

## Current-head local receipt (2026-10-06)

```text
source_snapshot: current dirty worktree; kiana-domain/src/capability_catalog.rs; sc26_capability_catalog fixtures
worktree_status: SC-26 roadmap row 304 remains 🔄; corrected ReadOnly/write-request denial clears effective capabilities while retaining requested capabilities for audit
command_argv: `cargo test -p kiana-domain --test sc26_capability_catalog --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0`; final cached run 0.21s, max RSS 81880 KiB (~80 MiB), 0 swaps; `/tmp/kiana-roadmap-20261006-final-receipts/sc26.{log,resources}`
fixture·cassette: 3/3 focused catalog tests; ReadOnly write denial, effective/requested separation and existing digest/intersection contracts
exit_code: 0
status_change: none; this closes one current-head defect and does not complete SC-26 lifecycle or trust acceptance
proof-level_change: selected ReadOnly denial source → local_behavior; no whole-card durable/live/physical promotion
limitations: project trust loading, signed-key trust, package lifecycle, runtime revocation, durable catalog, Broker execution and external effects remain outside this fixture
reviewer: root integration agent; focused SC-26 test review
```

## 2026-10-17 completion slice (CI-only)

- `CapabilityCatalogEntry::authorize` only hands out a capability after the entry self-validates
  and only from its effective set; denied entries keep their deny reason
  (`capability_catalog_entry_denied:<reason>`) while unrequested or non-effective capabilities
  return `capability_not_effective:<capability>` and malformed requests return
  `capability_catalog_request_invalid`.
- `CapabilityCatalogEntry::check_manifest_drift` plus
  `CapabilityCatalog::{entry_for_manifest, authorize_manifest}` reject an unknown extension id and
  every drift class before anything is handed out: identity
  (`capability_catalog_extension_identity_drift`), version
  (`capability_catalog_manifest_version_drift`), capability set
  (`capability_catalog_manifest_capability_drift`) and digest
  (`capability_catalog_manifest_digest_drift`, which still binds signature metadata).
- `kiana-domain/tests/sc26_capability_catalog.rs` grows from three to seven deny-first fixtures:
  skill `allowed_tools` stay descriptive even when host availability, Grant and approval all offer
  the tool, drift cannot reuse an older entry, denied and unrequested capabilities never
  authorize, and duplicate identity, invalid manifests, tampered reasons, a bumped generation and
  a resealed entry digest all fail closed. The successful column pins order-independent catalog
  digests, serde round-trips and the re-signed-manifest digest change.
- `kiana-domain/tests/sc26_capability_catalog_guard.rs` keeps the no-execution/no-filesystem
  negative assertions, adds the `allowed_tools` negative marker and a wiring fixture
  (`sc26_deny_first_fixtures_are_wired_into_the_catalog_lane`) binding the seven fixture names to
  `.github/workflows/sc26-capability-catalog.yml` and `scripts/ci/test-shards.json`.
- The manual `.github/workflows/sc26-capability-catalog.yml` lane runs `cargo fmt --all --check`, both targets with
  `--test-threads=1`, every fixture individually with `--exact`, and two `--no-run` compile steps.
  The unified `ci.yml` `kiana-domain-s4/4` shard still owns both targets.

Limitations: the catalog remains a value-only snapshot (no package loading, signature-key trust,
registry/ProjectTrust read, Broker call or runtime revocation); this record is source-only and CI
pending because no local build, test or clippy run happened in this session.
