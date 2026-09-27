# DEP-07 · Storage preflight baseline

> Snapshot date: 2026-09-27. This slice defines a metadata-only storage root, project trust,
> path containment and filesystem capability preflight. Local Cargo test/build/check/clippy/smoke
> commands are intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-07`](../roadmap.md#step-dep-07) |
| source snapshot | `45d0defa` plus this DEP-07 source slice |
| feature_status | `partial` for the pure storage preflight contract |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | server ProjectTrust snapshot + adapter metadata → StoragePreflightReport → later DaemonHost/root resolver |

`StoragePreflightRequest` carries an adapter supplied `project_root_digest` separately from the
storage root identity. The preflight reuses `normalize_role_path`/`enforce_path_containment`,
checks trust, owner binding, symlink/hardlink/file mode metadata, platform capability limits and
byte/inode capacity, then returns a stable Ready/Blocked report with backend, capability and
capacity facts, reasons and remediation.
An optional storage owner project is compared when present; an unscoped storage root does not
silently become a project root.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| untrusted project | ProjectTrust false blocks preflight |
| path escape | absolute/parent traversal is blocked through the shared lexical helper |
| remote filesystem | unsupported backend is reported blocked |
| symlink/hardlink | adapter metadata is blocked before any write/open |
| permissions/capacity | broad mode, insufficient bytes or inodes blocks admission |
| capability limits | missing no-follow/identity/permission/atomic replace proof stays visible |
| daemon root resolver | existing root and parent symlink components are rejected before canonicalize |
| source boundary | domain/Core contain no filesystem read/write, symlink metadata call, process, Tokio, Broker or EventLog effect |

## CI and limitations

GitHub Actions runs `.github/workflows/dep07-storage-preflight.yml` with domain fixtures, Core and
daemon source guards and affected-target compilation. Local Cargo tests, builds, checks, clippy and smoke
commands are intentionally not run, and CI results are not awaited.

The domain/Core report consumes metadata and capability claims; it does not inspect live free
space/inodes, create storage directories, acquire a lock or persist a preflight fact. The daemon
root adapter now rejects existing symlink components before canonicalize, while
`PathResolverPort`/later adapters still must perform no-follow identity and revalidation before
effect steps.
