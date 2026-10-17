# PD-14 Unified Content-Addressed Artifact Store Baseline

## Scope

PD-14 completes the durable bytes half of the artifact boundary. `kiana-ports` now declares
`ContentAddressedArtifactStorePort` plus the immutable `ArtifactManifest` (`kiana.artifact-manifest.v1`,
sealed over schema/version/blob digest), and `kiana-core/src/artifact_store.rs` provides the one
`UnifiedArtifactStore` that serves **both** the new port and the legacy `ArtifactStorePort`, so the
two artifact surfaces cannot drift into two stores.

Layout under one explicitly injected absolute root:

```text
<root>/blobs/<content-hash hex>                                   one file per unique byte content
<root>/scopes/<owner-scope hex>/artifacts/<artifact id>/v<n>.manifest
<root>/scopes/<owner-scope hex>/artifacts/<artifact id>/v<n>.commit
```

Four decisions shape it:

- **Bytes by content, manifests by identity.** Identical bytes are stored once; each
  `(owner scope, artifact, version)` keeps its own immutable manifest, so dedup can never merge two
  artifacts into one readable object.
- **One handle, one owner scope.** Any version or reference carrying another scope is refused
  before a byte is written.
- **Atomic or failed.** Same-directory temporary file, `fsync`, no-replace `renameat2`, then
  directory `fsync`; a target that already exists is accepted only byte-for-byte. An unconfirmed
  sync is reported as failure or `result_unknown`, never as a durable success.
- **No path resolution.** The root is opened once and pinned by descriptor; every later operation
  is `openat`/`renameat2`-relative with `O_NOFOLLOW`, and stored files must be single-link regular
  files. The `ArtifactSyncFault` injection point exists so the fsync denial path is provable in a
  fixture rather than asserted in a comment.

The previous eventlog `MemoryArtifactStore` slice (see `pd14-artifact-store-baseline.md`) remains a
process-local adapter for its own fixtures; it is not the durable byte store this step adds.

## Deny-first fixtures (`kiana-core/tests/pd14_unified_artifact_store.rs`)

- `open_refuses_relative_roots_traversal_symlinks_scope_and_limits` — relative root, `..`
  traversal, symlinked root component, malformed owner scope and zero bounds are all refused while
  the descriptor is opened.
- `stage_refuses_size_digest_bound_scope_and_version_drift_before_writing` — size mismatch, digest
  mismatch, non-immutable version, store-level over-limit and foreign scope; nothing is written on
  any denial.
- `staged_content_is_unreadable_until_a_matching_commit` — staged content is not readable as
  manifest/content/verify, foreign-scope and drifted references cannot commit, an absent later
  version is `artifact_manifest_missing`, and commit is idempotent with a pinned `v<n>.commit`.
- `identical_bytes_are_deduplicated_but_scopes_never_merge` — two artifacts share one blob file,
  two scopes share one blob file yet keep separate manifests, and neither scope resolves the
  other's reference.
- `symlinked_hardlinked_and_non_regular_files_are_refused` — symlinked, hard-linked and
  non-regular blob files, and a symlinked manifest, are refused on read and verify.
- `a_replaced_root_path_cannot_redirect_an_open_store` — an open handle keeps reading the original
  directory after the root path is renamed and recreated; a store opened at the new path knows
  nothing.
- `pre_existing_blob_drift_and_conflicting_writers_fail_closed` — a planted blob whose bytes do not
  hash to its own name, two racing writers for one version (exactly one publishes; the loser cannot
  read its reference), and a sequential second proposal for a staged version.
- `file_sync_failure_denies_the_stage_and_publishes_nothing` — a file-sync fault leaves nothing
  staged, committed or readable, and the same immutable object publishes once sync succeeds.
- `directory_sync_failure_is_reported_as_unknown_and_the_retry_is_idempotent` — an unconfirmed
  directory sync after staging or committing is reported as `result_unknown`, never as durable, and
  the retry returns the first manifest.
- `legacy_artifact_port_and_manifest_surface_agree` — the legacy `ArtifactStorePort` staging/commit/
  read/verify/revision-conflict contract is served by the same implementation and resolves to the
  same manifest, blob and stable errors; `invalidate_artifact` stays explicitly unsupported.
- `reopen_preserves_the_published_manifest` — after reopening the root, manifest/content/verify and
  the blob probe return the published values, and a retry with a different creation timestamp still
  resolves to the first manifest.

## Port contract fixtures (`kiana-ports/tests/pd14_artifact_manifest_port.rs`)

- `manifest_rejects_blob_digest_schema_version_and_seal_drift`
- `manifest_accepts_both_digest_spellings_and_round_trips_through_json`
- `port_defaults_deny_with_one_stable_reason`

## Wiring

- Focused lane `.github/workflows/pd14-artifact-store.yml` (manual `workflow_dispatch` only): format
  check, both eventlog targets, the three new targets, every core fixture by `--exact` name, and
  `--no-run` compilation for all six targets.
- `kiana-core/tests/pd14_unified_artifact_store_guard.rs` binds the fixture names to the behaviour
  target, this document and the lane, and pins the store's denial reasons, the forbidden
  path/shell/in-memory markers, the shared syscall helpers and the `kiana-ports` contract.
- `scripts/ci/test-shards.json` registers `pd14_unified_artifact_store` and
  `pd14_unified_artifact_store_guard` in `kiana-core-s5/6`; the `kiana-ports` shard is whole-crate,
  so `pd14_artifact_manifest_port` is covered by the unified `ci.yml` gate.

## Limits

- `feature_status=implemented`, `proof_level=source`: no local build or test was run in this
  CI-only session; compile/run receipts are GitHub Actions and were not awaited.
- Descriptor/rename/sync acknowledgements are source-level. Power-loss and crash-recovery
  equivalence are **not** proven here; a crash fixture belongs to the fault matrix (`PD-30`) and the
  platform matrix (`PD-32`).
- Linux-only: other targets return `Unavailable("artifact_store_platform_unsupported")` instead of a
  path-based fallback that would silently change atomicity.
- Single-host concurrency only: writers are serialized by `RENAME_NOREPLACE`; there is no
  cross-process lock and no multi-writer coordination beyond that rename.
- Blobs are never erased: `invalidate_artifact` is deliberately unimplemented and orphan-blob
  reclamation stays with retention/erasure (`PD-25`/`PD-26`).
- No product caller is switched over yet: `DaemonHost`/`ControlPlane` still use their existing
  artifact adapters, so the store is exercised by fixtures, not by the product path.
