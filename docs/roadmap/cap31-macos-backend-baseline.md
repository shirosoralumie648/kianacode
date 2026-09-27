# CAP-31 macOS backend baseline (partial)

> Snapshot date: 2026-09-28. This slice adds the backend *selection* contract. It does not add a
> Seatbelt implementation and cannot prove macOS behaviour. Local Cargo test/build/check/clippy/
> smoke commands are intentionally not run; GitHub Actions owns fixtures and affected-target
> checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`CAP-31`](capability.md#step-cap-31) |
| source snapshot | master plus this CAP-31/32 selection slice |
| feature_status | `partial`, unchanged from before this slice |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | requested backend + server report -> fixed-order decision -> refusal or selection |

The product execution backend in this checkout is Linux/bwrap (`SANDBOX_BACKEND` in
`kiana-daemon/src/harness_sandbox.rs`). There is no Seatbelt implementation, no macOS
`sandbox-exec` profile compiler, and no target-machine receipt anywhere in the repository. Linux
cfg compilation is not macOS evidence and is not treated as such.

What this slice adds is `kiana-domain/src/backend_selection.rs`: the rule for what happens when a
caller selects a macOS backend on a host where it does not exist. The rule is a refusal, never a
substitution.

## What the card rejects, and how

| Rejected | How |
|---|---|
| a missing backend silently falling back to the host | `BackendSelectionDecision` has no field that could name a substitute for a refused selection, and `validate_against` rejects a decision that carries one (`backend_selection_substitute_backend_named`) |
| a macOS backend reported as available on a non-macOS host | `PlatformBackendDisposition::TargetOnly` for a report whose target is not the host yields `BackendSelection::TargetOnly`, and `usable()` is false |
| a caller forging its target so a Linux build answers for macOS | a `requested_target` that differs from `host_target` is `Blocked` with `backend_selection_target_mismatch`, before the backend's own disposition is consulted |
| `Implemented` claimed without target-machine evidence | `PlatformBackendReport::validate` refuses `Implemented` with `behavior_verified = false` (`platform_backend_implemented_requires_behavior`); `backend_selection.rs` never constructs a report, so it can only forward one that already passed that gate |
| a host fallback on any disposition | `no_host_fallback` is rejected at construction for every disposition (`platform_backend_host_fallback_forbidden`) |
| a refusal with no stated reason | `TargetOnly`/`NotSupported`/`Blocked` reports require at least one limitation (`platform_backend_limitation_required`) |
| a tampered report or decision | both are digest-sealed; `validate_against` re-derives the decision and re-binds the request digest, so an edited field is refused |

The decision order is: target agreement, then whether the server knows the backend, then report
validity, then report/host target agreement, then disposition. Target agreement comes first because
a caller that believes it is on macOS while the host is Linux is itself the evidence of a
misconfigured or forged environment, and answering `Selected` there would convert a configuration
bug into a confinement lie.

## Failure-first fixture matrix

`kiana-domain/tests/cap31_macos_backend.rs` — one test per rejected-first item on the card.

| Fixture | Assertion |
|---|---|
| `macos_missing_backend_has_no_host_fallback` | an unknown backend is `NotSupported`; a target-only macOS report on a Linux host is `TargetOnly`; neither names the host's real backend; a forged macOS target on a Linux host is `Blocked`; a decision edited to name a substitute fails its own validation |
| `macos_backend_denies_host_secrets_and_gui_escape` | an unverified backend cannot be presented as `Implemented` under any name; host fallback is refused for every non-implemented disposition; a missing limitation list is refused |
| `macos_descendant_escape_or_stop_failure_is_visible` | an edited report is refused as `Blocked`, not silently re-read as a different capability; a digest-sealed report cannot have `behavior_verified` flipped; the Windows backend obeys the same rule through the same vocabulary |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.

**This proves nothing about macOS.** No Seatbelt profile is compiled, no `sandbox-exec` is run, no
GUI or Apple Events request is made or refused, no host secret is read or hidden, no descendant is
forked or escaped, and no process is supervised or stopped. The three rejected-first items on the
card name *behaviours* — `macos_backend_denies_host_secrets_and_gui_escape`,
`macos_descendant_escape_or_stop_failure_is_visible` — and this slice proves only the *decision
that the behaviour cannot be claimed yet*. A macOS backend that is selected on Linux and "succeeds"
would be a lie; refusing is the correct behaviour and is what this contract enforces and tests.

`host_target` is a caller-supplied fact by design. This module does not inspect the host, so a
composition root that passes the wrong value gets a decision consistent with that wrong value. The
guard below asserts that the current product backend constant and the Linux `cfg` gate in
`harness_sandbox.rs` are still what they are, which is the available substitute for a host probe.

The macOS and Windows cards' `-b` work — Seatbelt file identity, process supervision, Job Objects,
restricted tokens, ACL application, reparse/UNC/ADS resolution, minimal handle inheritance — is
entirely absent. So is the `-c` target run. The existing GitHub macOS and Windows jobs compile the
workspace and run a source guard only.

## Registration

`kiana-domain/src/lib.rs` must gain:

```text
mod backend_selection;        # alphabetically before `mod capabilities;`
pub use backend_selection::*; # alphabetically before `pub use backup_lifecycle::*;`
```
