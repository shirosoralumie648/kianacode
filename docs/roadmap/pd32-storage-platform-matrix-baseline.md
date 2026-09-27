# PD-32 platform and filesystem matrix baseline

> Snapshot date: 2026-09-28. This slice owns the *capability decisions* for each
> `(platform, filesystem)` pair and the pre-install check that refuses a root placed on a cell
> whose semantics it cannot rely on. Local Cargo test/build/check/clippy/smoke commands are
> intentionally not run; GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`PD-32`](persistence-data-layer.md#step-pd-32) |
| source snapshot | master plus this PD-32 platform-matrix slice |
| feature_status | `partial` for source-level disposition/negotiation contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | `(platform, filesystem)` -> `StoragePlatformCell` -> `install_preflight` / `negotiated_capabilities` |
| source | `kiana-ports/src/storage_platform_matrix.rs` |
| fixture | `kiana-ports/tests/pd32_storage_platform_matrix.rs` |
| guard | `kiana-ports/tests/pd30_32_34_storage_matrices_guard.rs` |

The card asks for a platform and filesystem matrix — Linux ext4/tmpfs, Windows/macOS fallback, a
network filesystem, and clock/encoding — plus capability negotiation and a pre-install check, with
independent limitations per platform.

**This suite ran on Linux, in one filesystem, at one clock reading.** It therefore asserts the
**disposition** of every cell, never an observed filesystem behaviour. It did not compare ext4
against tmpfs, did not mount a network filesystem, did not start a process on Windows, and did not
read a macOS `fsync` result, because none of those could be observed from this host. The module
docstring says the same thing in the source.

## Scope + proof-ceiling table

The matrix is a set of cells, each with an explicit disposition per primitive and a proof ceiling
that may never outrank its own dispositions.

| Target | Filesystem | `rename` | `fsync` | `lock` | perm | link | Clock | Encoding | Ceiling | Observed here? |
|---|---|---|---|---|---|---|---|---|---|---|
| `linux` | `ext4` | supported | supported | supported | supported | supported | monotonic_wall | utf8 | `local_behavior` | **no** — declared from the adapter's existing code path |
| `linux` | `tmpfs` | supported | **degraded** | supported | supported | supported | monotonic_wall | utf8 | `source` | **no** |
| `linux` | `network_fs` | **unsupported** | **unsupported** | **unsupported** | supported | **degraded** | adjustable_wall | utf8 | `source` | **no** |
| `windows` | `ext4` | **degraded** | **degraded** | **unsupported** | **unsupported** | **unsupported** | adjustable_wall | non_utf8 | `source` | **no** — no Windows backend ships |
| `macos` | `ext4` | supported | **degraded** | supported | supported | supported | adjustable_wall | utf8 | `source` | **no** — no macOS backend ships |
| `other_unix` | `unknown` | **unsupported** | **unsupported** | **unsupported** | **unsupported** | **unsupported** | absent_wall | non_utf8 | `source` | **no** — the catch-all |

Every row carries its own `limitations` list. No platform is described by another's prose, and a
cell that is not fully supported may not have an empty one.

## What the card rejects, and how

| Rejected | How |
|---|---|
| **an unsupported `rename` / `fsync` / `lock` semantic treated as an equivalent durable one** | `StoragePlatformDisposition` has three values and only `Supported` satisfies `admits_durable_claim`. `Degraded` is not a weaker `Supported`: `negotiated_capabilities` **omits** it rather than returning `false`, so a caller cannot read a weaker primitive as present. `install_preflight` names the primitive and the platform (`storage_platform_semantic_unsupported:<platform>:<semantic>`) |
| a degraded primitive carrying a durable ceiling | a `LocalBehavior` cell must be `Supported` on all five primitives *and* have a monotonic wall clock (`storage_platform_ceiling_outranks_dispositions`) |
| a ceiling the negotiation cannot certify at all | `storage_platform_ceiling_not_negotiable` refuses anything above `LocalBehavior`, reusing PD-31's `ProofCeiling::certifiable_by_source_suite` |
| an unrecognised Unix inheriting the Linux row | `OtherUnix` must stay at `Source` (`storage_platform_other_unix_ceiling`) and every semantic is `Unsupported` |
| an unidentified filesystem credited with an atomic rename | `storage_platform_unknown_filesystem_supported` — `Unknown` + `Supported` is unconstructible |
| **a clock rollback breaking a TTL or a retention deadline** | `admit_platform_expiry` refuses any deadline on a cell whose `clock` is not `MonotonicWall` (`storage_platform_expiry_refused:<clock_shape>`), and refuses an untrusted or rolled-back observation even on the one cell that can expire (`storage_platform_expiry_refused:untrusted_clock`). An elapsed deadline is `storage_platform_expiry_elapsed`, never silently extended |
| a root placed where a TTL is undecidable | `install_preflight` applies the same rule (`storage_platform_clock_not_expirable:<platform>`) |
| a non-UTF-8 path transliterated into a different path | `install_preflight` refuses `NonUtf8` (`storage_platform_encoding_refused:<platform>`) |
| a cell claiming a guard the platform cannot enforce | `validate_against_capabilities` cross-checks against PD-28's `StorageSecurityCapabilities` (`storage_platform_guard_not_enforceable:<guard>`, `storage_platform_security_platform_mismatch`) and against `StorageCapabilities`' own `fsync` bit, which is the record that already refuses `durable_commits` without it |
| a matrix that quietly omits a platform | every `StoragePlatformTarget::ALL` entry must have a row (`storage_platform_target_missing:<platform>`, `storage_platform_cell_duplicate`) |
| a tampered cell | `validate` re-seals the digest (`storage_platform_cell_digest_mismatch`) |

## The vocabulary that was reused, not reinvented

| Reused | From | Why |
|---|---|---|
| `ProofCeiling` + `certifiable_by_source_suite()` | PD-31 `adapter_conformance` | One durable/local_behavior/physical ladder in the workspace, not a second scale |
| `StorageSecurityCapabilities` | PD-28 `storage_security` | The authority for which guards a platform can enforce. A cell cannot claim a guard its own record disallows |
| `StorageCapabilities::fsync` | `kiana-domain::storage_health` | The one fsync claim in the workspace, rather than a new bit on the cell |
| `CapacityEnvelope` | DEP-17 | The PD-34 budget slice and this matrix read the *same* envelope; the fixture asserts the frame bound agrees |

`StorageSemantic::ALL` is the closed list of durability-affecting primitives: `rename`, `fsync`,
`advisory_lock`, `permission_guard`, `link_guard`. The first three are the ones the card names; the
last two are the ones PD-28 already gates. `AdvisoryLock` deliberately has **no** PD-28 guard
mapping, because nothing in the domain reports whether a platform's lock is advisory or mandatory —
the cell's own disposition is the only statement about it, and it says so in the source.

## Failure-first fixture matrix

`kiana-ports/tests/pd32_storage_platform_matrix.rs`:

| Fixture | Assertion |
|---|---|
| `the_matrix_names_every_platform_and_filesystem_the_card_lists` | all four targets and Linux's ext4/tmpfs/network_fs rows are present, every cell has its own limitations, and `STORAGE_PLATFORM_BUILD_TARGET` is a compile-time constant rather than a probe |
| `the_install_preflight_refuses_every_cell_that_is_not_fully_supported` | Linux/ext4 passes; tmpfs, network_fs, Windows and the catch-all are each refused with the primitive named |
| `an_unsupported_semantic_is_absent_from_the_negotiation_not_downgraded` | a degraded or unsupported semantic is missing from the granted set rather than present-and-false |
| `a_cell_is_cross_checked_against_the_pd28_platform_record` | Linux/ext4 agrees with both capability records; a Windows cell validated against a unix record is refused |
| `the_matrix_does_not_claim_durable_on_any_unverified_platform` | no cell exceeds the source-certifiable ceiling, and only Linux/ext4 exceeds `source` |
| `a_degraded_semantic_may_not_claim_a_durable_ceiling` | tmpfs at `LocalBehavior` is unconstructible; an adjustable clock is refused even fully supported |
| `a_cell_may_not_claim_a_ceiling_above_what_negotiation_can_certify` | `ProofCeiling::Durable` is unconstructible |
| `an_unknown_filesystem_is_never_credited_with_a_rename` | `Unknown` + `Supported` is refused |
| `a_matrix_missing_a_platform_is_refused` | removing the Windows row is refused by name |
| `a_tampered_cell_digest_is_refused` | an edited disposition is refused rather than adopted |
| `a_clock_that_can_move_backwards_refuses_every_expiry` | `AdjustableWall` and `AbsentWall` cells refuse an expiry, naming the clock shape |
| `a_wall_clock_deadline_is_refused_where_the_clock_can_move_backwards` | `admit_platform_expiry` refuses an untrusted clock, an elapsed deadline and a zero sample; a live deadline on Linux/ext4 is admitted |
| `a_non_utf8_cell_is_not_a_usable_root` | `NonUtf8` is not a usable root and the catch-all preflight is refused |
| `the_platform_matrix_and_the_capacity_budget_read_the_same_envelope` | the PD-34 budget's `max_frame_bytes` bound is read from the DEP-17 envelope, and a declared budget reports `Bounded`, not `Measured` |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.
No local `cargo test`, build, check, clippy or smoke command was run; only static compilation of
the affected targets and `rustfmt` on the files this slice touched.

**This does NOT prove the following, and no claim here should be read as proving it:**

- **Nothing in the matrix was observed on the filesystem it describes.** `linux`/`ext4` reads
  `supported` because that is what this checkout's JSONL writer already implements, not because a
  probe of the running mount was taken. The module contains no `statfs`, no `mount`, no `flock` and
  no clock read, and the guard asserts their absence.
- **No ext4-vs-tmpfs comparison happened.** Both rows are declarations. tmpfs is marked
  `fsync`-degraded because its bytes are RAM by construction, not because a tmpfs mount was
  measured here.
- **No network filesystem was contacted.** The `network_fs` row refuses rename/fsync/lock on the
  grounds that they are server-dependent. That is a conservative refusal, not an observation that a
  particular NFS or SMB server lacks them.
- **Windows and macOS were never run.** There is no Windows backend (CAP-32) or macOS backend
  (CAP-31) in this checkout. Both rows are declarations derived from each platform's documented
  semantics, and the fixture's `windows_fallback` / `macos_fallback` cells exist to exercise the
  *refusal* logic, not the platform.
- **A non-UTF-8 refusal is a policy, not a measurement.** `NonUtf8` is refused for `windows` and
  `other_unix`; nothing here observed a path encoding.
- **`STORAGE_PLATFORM_BUILD_TARGET` is not a probe.** It is the compile-time constant `"linux"`. A
  Linux binary whose `KIANA_HOME` sits on a network filesystem still gets that cell's refusal, and a
  cross-compiled build would report the wrong row until someone sets it.
- **The ceiling is `local_behavior` at best, and only for one cell.** Even `linux`/`ext4` at
  `local_behavior` means "this process produced the outcomes it declared" — not that a power loss,
  a page drop or a torn tail was survived. PD-30's fault matrix and PD-06/PD-08's recovery path
  own that, and neither ran here.
- **This does not replace runtime capability negotiation.** The matrix is a published record of
  dispositions. The daemon's preflight (PD-07 / `storage_preflight`) is where a real root is
  checked before use, and this slice does not wire into it.
