# CAP-32 Windows backend baseline (partial)

> Snapshot date: 2026-09-28. This slice adds the backend *selection* contract shared with CAP-31.
> It does not add a Windows implementation and cannot prove Windows behaviour. Local Cargo
> test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions owns fixtures
> and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`CAP-32`](capability.md#step-cap-32) |
| source snapshot | master plus this CAP-31/32 selection slice |
| feature_status | `partial`, unchanged from before this slice |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | requested backend + server report -> fixed-order decision -> refusal or selection |

The product execution path in this checkout is Unix/Linux oriented. There is no Job Object
implementation, no restricted-token or AppContainer path, no ACL or handle-inheritance code, and no
target-machine receipt. Linux compilation cannot prove Windows behaviour.

`kiana-domain/src/backend_selection.rs` is the same contract CAP-31 uses, reached through the
existing `PlatformTarget` / `PlatformBackendDisposition` / `PlatformBackendReport` vocabulary
rather than a parallel one. Windows is a different target and a different backend name, not a
different mechanism.

## What the card rejects, and how

| Rejected | How |
|---|---|
| a reparse point, UNC path, ADS suffix or case alias resolving outside scope | there is no Windows path code in this checkout, and the backend cannot be selected here at all, so no Linux path routine can be reached through a "Windows" label. A forged `requested_target: Windows` on a Linux host is `Blocked` (`backend_selection_target_mismatch`) |
| a child breaking away from a job | `PlatformBackendDisposition::Blocked` is a distinct outcome from `TargetOnly`, so "present but unusable" is not conflated with "not on this platform"; and `Implemented` requires `behavior_verified = true`, which requires a target receipt |
| a cross-process lock that is neither real nor denied | the card allows "real lock **or** operation denied". The denial half is implemented in source; the real-lock half is not claimed, and a backend that has not produced target evidence cannot be selected |
| an empty implementation returning success to satisfy a cross-platform trait | `BackendSelectionDecision` has no `Ok`-shaped value that means "nothing to do". Every refusal carries a `reason` and an empty `effective_runtime`, and `validate_against` rejects a decision that names a substitute |
| Unix quoting semantics applied to Windows arguments | nothing in this slice produces a Windows argv, so the question does not arise here; PowerShell/cmd/argv semantics remain unverified and unclaimed |

## Failure-first fixture matrix

`kiana-domain/tests/cap32_windows_backend.rs` — one test per rejected-first item on the card.

| Fixture | Assertion |
|---|---|
| `windows_reparse_or_unc_path_cannot_escape_scope` | a target-only Windows report on Linux is `TargetOnly` and names no substitute; a forged Windows target on a Linux host is `Blocked`; a report-less request is `NotSupported`, not a permissive default |
| `windows_child_cannot_break_away_from_job` | `Implemented` without behaviour is refused; a digest-sealed report cannot have its disposition flipped; a `Blocked` backend is refused and is distinguishable from `TargetOnly` |
| `windows_cross_process_lock_is_real_or_operation_is_denied` | the denial half is implemented; a tampered decision fails `validate_against`; only a behaviour-verified report is selectable, and it still grants no execution authority |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.

**This proves nothing about Windows.** No Job Object is created, no `CREATE_SUSPENDED`/
`AssignProcessToJobObject` sequence runs, no breakaway flag is checked, no handle is duplicated or
inherited, no ACL is applied or revoked, no reparse point, UNC path, device path or ADS suffix is
resolved, and no cross-process lock is taken. The three rejected-first items name behaviours; this
slice proves only that those behaviours cannot yet be claimed and that no path exists by which
they could be faked.

The last fixture constructs a `behavior_verified = true` report in order to show that `Selected` is
reachable at all. That report is fixture data, not evidence: nothing in this repository observes
Windows behaviour, and the fixture's existence must not be read as a Windows receipt.

The card's `-a` support-matrix prototype (restricted token vs. AppContainer vs. account/ACL),
its `-b` Job Object and minimal-handle implementation, and its `-c` target run are all absent. The
existing GitHub Windows job compiles the workspace and runs a source guard only; the earlier
`CARGO_TARGET_DIR` path-length remediation in the changelog is unrelated to this slice and is
unchanged.

## Registration

Same two lines as CAP-31; the module is shared:

```text
mod backend_selection;
pub use backend_selection::*;
```
