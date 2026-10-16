# DEP-19 - Backup manifest, chunk and integrity baseline

> Snapshot date: 2026-09-27. This slice defines the verifiable description of a quiesced snapshot.
> The local-test restriction recorded for the 2026-09-27 snapshot session was superseded by the
> user's current authorization for bounded local roadmap tests; current-head evidence follows.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-19`](../roadmap.md#step-dep-19) |
| source snapshot | master plus this DEP-19 backup manifest slice |
| feature_status | `partial` for source-level backup manifest contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | quiesced snapshot facts -> typed `BackupManifest` -> redacted export |

`BackupManifest` describes what a backup contains and proves it can be replayed. It binds the
instance, storage root, deployment revision, source cursor, projection generation and the
data/authority epochs it was taken under, plus a config digest pinning the configuration in force.
`BackupChunk` addresses one piece by logical reference and digest, and records how its integrity was
established.

A manifest is restorable only when every chunk is intact. `TornTail` and `Unknown` integrity make
the whole backup unusable rather than partially restorable, so a restore can never silently
reconstruct a store from half a snapshot.

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| verifiable | a full manifest validates, is restorable, and exports a redacted set reference |
| header | a zero source cursor, projection generation, data epoch or authority epoch is rejected |
| hash | a tampered chunk digest, or a manifest digest that no longer describes its fields, fails closed |
| paths | absolute paths, `..` traversal, drive letters, `file://` and whitespace segments are rejected |
| secrets | a reference that looks like a credential is rejected as unredacted or secret-bearing |
| integrity | torn-tail and unknown integrity both make the whole manifest unrestorable |
| coverage | a manifest missing the EventLog or the migration registry cannot be restored |
| ordering | duplicate and out-of-order chunks are rejected |
| binding | a manifest taken under a different revision, data epoch or authority epoch will not activate |

`export_redacted` drops chunk references entirely and keeps only counts, per-kind totals, digests
and epochs, so an operator can attach a manifest to an incident without disclosing a path.

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate, which compiles and tests
the affected targets. This slice adds no separate workflow.

This is a source-only contract over facts a later quiesce step (DEP-20) will produce. It does not
quiesce a store, read or hash a byte, write a manifest, restore a root, verify a signature, or
schedule a backup. `kiana_ports::BackupStorePort` still takes an untyped manifest value; binding it
to this type, and producing manifests from real bytes, remains later work. No RPO/RTO claim is made
here — that belongs to DEP-24.

## Current-head local receipt (2026-10-06)

```text
source_snapshot: current dirty worktree; kiana-domain/src/redaction.rs and dep19_backup_manifest fixtures
worktree_status: DEP-19 remains roadmap row 688 🔄; shared sentinel scanner now rejects sk-prefixed API keys inside logical references
command_argv: `cargo test -p kiana-domain --test dep19_backup_manifest --locked --offline -- --test-threads=1`; `cargo test -p kiana-domain --test int13_secret_redaction --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0`; DEP-19 cached run 0.53s/max RSS 242220 KiB (~237 MiB), 0 swaps; first compile 52.75s/max RSS 4090216 KiB (~3.90 GiB), 0 swaps; `/tmp/kiana-roadmap-20261006-dep19-{final,redaction}.resources`
fixture·cassette: DEP-19 8/8 and INT-13 4/4; `artifact/sk-...` is classified as `backup_chunk_reference_secret_detected`; shared redaction regression stays green
exit_code: both commands 0; `git diff --check` 0
status_change: none; local behavior closes the shared scanner defect but not the DEP-19 backup lifecycle
proof-level_change: selected secret-reference refusal source → local_behavior; no durable/live/physical promotion
limitations: no quiesce, real-byte hashing, backup-store binding, signature verification, restore, schedule, RPO/RTO, or evidence that all secret formats are covered
reviewer: root integration agent; DEP-19 security audit agent; focused regression review
```
