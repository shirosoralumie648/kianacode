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
