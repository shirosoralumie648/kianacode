# DEP-00 · Deployment and operations source inventory baseline

`DEP-00` records the deployment, operations and migration inputs before the
`DEP-01` profile contracts are added.  It is an inventory and source guard; it
does not claim that the listed components already provide durable deployment,
backup, migration or multi-process operations.

## Source snapshot and worktree

| field | value |
|---|---|
| source snapshot | `794b6d44` (`origin/master` at the start of this slice) |
| worktree | isolated `dep-00-deployment-baseline-20260924` |
| operating system | Linux x86_64; stable Rust workspace |
| proof ceiling | `source` plus GitHub-only inventory/source-guard wiring |
| local execution | no tests, build, check, clippy or smoke commands |

The snapshot is a review anchor only.  A later step must refresh it before
making a deployment decision; this document must not be read as a live release
manifest.

## Inventory matrix

| area | current source anchors | observed boundary at DEP-00 | later step |
|---|---|---|---|
| module map and status | `docs/module-map.md`, `CURRENT_STATUS.md` | informative map and evidence ledger are separate; status evidence is the authority | DEP-41 |
| release inputs | `scripts/release-preflight.sh`, `scripts/package-release.sh`, `scripts/sign-release-artifacts.sh`, `scripts/verify-release-provenance.py`, `.github/workflows/` | release scripts and CI gates exist, but no single deployment profile or operation journal binds them | DEP-02, DEP-34 |
| composition root | `kiana-daemon/src/lib.rs` (`DaemonHost`) | one product composition root assembles Core, Runner, Broker, EventLog and adapters | DEP-10 |
| authority and execution | `kiana-core/src/`, `kiana-runner/src/`, `kiana-capability-broker/src/` | ControlPlane is the authority path; this inventory found no deployment-specific second model loop | DEP-04, DEP-05 |
| facts and storage | `kiana-eventlog/src/{lib.rs,event_store_core.rs,journal_core.rs,jsonl.rs,artifact_store.rs}` | EventLog and artifact stores are existing sources/adapters; cross-process lease, backup and restore are not implied | DEP-06, DEP-19, DEP-20 |
| schema and migration | `kiana-domain/src/{contracts.rs,migration.rs,migration_registry.rs,migration_runner.rs}`, `kiana-protocol/src/lib.rs` | versioned contracts and migration value objects exist; startup preflight and compatibility matrix are not yet a deployment service | DEP-03, DEP-27..DEP-33 |
| configuration and trust | `kiana-daemon/src/{lib.rs,storage.rs,authn.rs}`, `kiana-policy/src/lib.rs` | local configuration and ProjectTrust checks exist; immutable deployment snapshots and root identity are not unified | DEP-01, DEP-07, DEP-08 |
| supervision | `kiana-daemon/src/process_supervisor.rs`, `kiana-capability-broker/src/` | process/capability supervision is execution-scoped; no supervisor adapter may become a second execution authority | DEP-09 |
| scripts and evidence | `scripts/*`, `docs/roadmap/*baseline.md`, `.github/workflows/*` | fixtures and evidence are per slice; no universal operation/release evidence manifest is claimed | DEP-14, DEP-15, DEP-41 |
| user WIP | `git status`, stash inventory at integration time | unrelated WIP must remain outside deployment commits; no WIP file is an authority source | integration closeout |

## Route and gap classification

The product route remains `entrypoints → client/protocol → DaemonHost →
ControlPlane → policy/gates/approval → Broker/Runner → handlers → EventLog →
Receipt`.  Direct supervisor or capability entry points found under legacy
crates are compatibility boundaries and are not used as evidence for the
product route.  DEP-00 therefore blocks any later implementation that adds a
deployment-only execution loop, lets a supervisor dispatch a capability, or
treats a UI/transcript/cache as the deployment state ledger.

| classification | examples | consequence |
|---|---|---|
| existing source contract | DaemonHost, EventLog, schema versions, release scripts | may be referenced by later steps, but requires a fresh snapshot |
| source-only partial | local storage, process supervisor, migration value objects, CI evidence | do not claim durable, cross-process, live or physical proof |
| missing deployment contract | profile/root/instance IDs, operation journal, lease/fence, health/readiness, backup/restore activation | must be implemented in DEP-01 onward before operational claims |
| compatibility/legacy | `kiana-tools`, `kiana-commands`, old SDK/runner and legacy directories | must not be wired into a new deployment path |

## CI-only fixture catalog

`.github/workflows/dep00-deployment-baseline.yml` runs the following on GitHub
Actions.  The guard only checks source boundaries and inventory references; it
does not start a daemon, write a storage root, invoke a provider or perform a
deployment:

- `kiana-core/tests/dep00_deployment_guard.rs`: verifies the single
  `DaemonHost → ControlPlane → Broker/EventLog` route markers, release-script
  references, migration anchors and explicit proof-limit wording.
- `cargo fmt --all --check` and `cargo check --workspace --tests --locked`:
  remote compilation/format gates for the source snapshot.

The workflow is evidence wiring only.  It cannot prove power-loss recovery,
cross-process fencing, backup integrity, external supervisor behavior, live
release provenance or physical deployment effects.

