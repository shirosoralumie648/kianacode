# CAP-33 Container / gVisor execution baseline (partial)

> Snapshot date: 2026-09-28. This slice adds the *environment descriptor and selection decision*.
> It does not run a container. Local Cargo test/build/check/clippy/smoke commands are intentionally
> not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`CAP-33`](capability.md#step-cap-33) |
| source snapshot | master plus this CAP-33 descriptor/decision slice |
| feature_status | `partial`, unchanged from before this slice |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | immutable descriptor + runtime observation -> fixed-order decision -> selection or refusal |

The card asks for an *optional* execution environment, not a shipped one. In source that is a
descriptor stating exactly which image, runtime, user, mounts, limits and network the environment
will have, plus a decision that refuses the environment when any of those facts are missing or
unverifiable. That is `kiana-domain/src/container_environment.rs`. The runtime-facing half is the
existing `kiana-daemon/src/container_environment.rs` `EnvironmentPort` adapter, which this slice
builds on rather than duplicates: it already has `quiesce`, `execute_argv`, label verification and
`result_unknown` handling, and nothing here re-implements them.

The container backend is optional by design. `ContainerEnvironmentDescriptor` states a pinned
image, a named runtime, a non-root user, an explicit mount list, resource ceilings and a
deny-by-default network policy; `ContainerEnvironmentDecision` decides whether the server's
observations are good enough to use it.

## What the card rejects, and how

| Rejected | How |
|---|---|
| mounting a host control socket or credential path | every mount is checked against the same host deny list the daemon adapter uses (`container_environment_host_control_mount_denied`); a mutable image reference, a root user, and a second writable mount are each refused at construction |
| a runtime failure falling back to the host | `ContainerEnvironmentDecision` has no substitute field, and a decision that names one fails its own validation (`container_environment_substitute_runtime_named`); a missing runtime, a runtime mismatch and a missing image are all `Unavailable` with an empty `effective_runtime` |
| silently running a different runtime than the descriptor names | `observed_runtime != descriptor.runtime` is `Unavailable` (`container_environment_runtime_mismatch`); the descriptor is not rewritten to match what was found |
| a missing image being pulled at execution time | `image_present = false` is `Unavailable` (`container_environment_image_unavailable`); fetching needs its own network authorization, which this decision does not grant |
| a promised enforcement dimension that was not observed | the seven `CONTAINER_REQUIRED_DIMENSIONS` must all appear in the observation; any one missing is `NotEnforced`, and `gvisor_runtime` joins them for a `runsc` descriptor |
| a cancel reported as a stop without confirmation | `ContainerStopConfirmation` has no soft-success value; `stop_disposition` maps only `Stopped` to `ContainerEffectDisposition::Stopped` and everything else to `Unknown`, with `result_unknown:` reason codes matching the daemon adapter |
| a cleanup that deletes a resource it does not own | the descriptor binds `owner_id`, `scope_digest` and `environment_id`, and the decision re-binds the descriptor digest, so a decision presented against a different environment is refused |

The decision order is: descriptor validity, then runtime availability, then runtime identity, then
image presence, then the enforcement dimensions. Descriptor validity comes first so a
misconfiguration is never reported as a missing dependency.

## Failure-first fixture matrix

`kiana-domain/tests/cap33_container_environment.rs` — one test per rejected-first item on the card.

| Fixture | Assertion |
|---|---|
| `container_never_mounts_host_control_socket_or_credentials` | nine host control and credential paths are refused as mounts; three mutable image references are refused; a root user is refused; a second writable mount is refused; the honest descriptor validates and its receipt facts carry image, runtime, user, network policy and a limits digest |
| `container_runtime_failure_never_falls_back_to_host` | missing runtime, runtime mismatch and missing image are each `Unavailable` with no substitute; an edited decision fails validation; a gVisor descriptor without the `gvisor_runtime` dimension is `NotEnforced`, and with it is selectable; each of the seven required dimensions is independently necessary |
| `container_cancel_confirms_inner_process_stop` | only a confirmed stop maps to `Stopped`; unconfirmed and not-observed map to `Unknown` with the matching `result_unknown:` codes; the container backend is reported as optional per platform through the shared `PlatformTarget` vocabulary |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.

**This runs no container and observes no runtime.** The `ContainerRuntimeObservation` in every
fixture is hand-constructed data. Nothing here pulls an image, starts a process inside a
filesystem+PID namespace, applies a seccomp profile, verifies that `--read-only` actually held, or
checks that a gVisor `runsc` runtime confines anything. The fixtures prove that the *decision*
refuses a weak or missing observation; they cannot tell a real OCI backend from an absent one.

`ContainerRuntimeKind::RunSc` exists as a configuration choice, matching the existing daemon
adapter's `--runtime runsc`. Selecting it does not assert gVisor confinement, and the fixture that
reaches `Selected` with a gVisor dimension is a statement about the decision, not a receipt.

Two card items are explicitly not established. First, the **EventLog-backed environment inventory
and lease** is still not connected: the daemon adapter's plan registry is process-local, so daemon
restart recovery is unavailable rather than inferred from a container name, and the existing
`ContainerInventoryRecord` contract in `kiana-domain/src/container_inventory.rs` has no writer.
Second, **changeset/artifact publication, disk-full fixtures, remote cleanup recovery and a target
CI runtime fixture** are absent, as is shell/MCP routing through this adapter.

The host mount deny list is duplicated between this module and
`kiana-daemon/src/container_environment.rs::is_forbidden_host_mount`, because the two crates cannot
share a private helper without moving it. The CAP-33 source guard asserts the two lists agree by
name, so a change to one without the other fails CI rather than silently widening what a container
can read.

## Registration

`kiana-domain/src/lib.rs` must gain:

```text
mod container_environment;        # before `mod container_inventory;`
pub use container_environment::*; # before `pub use container_inventory::*;`
```
