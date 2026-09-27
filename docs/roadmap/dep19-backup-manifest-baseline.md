# DEP-19 - Backup manifest, chunk and integrity baseline

> Snapshot date: 2026-09-27. This slice defines the verifiable description of a quiesced snapshot.
> Local Cargo test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions owns
> fixtures and affected-target checks.

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
