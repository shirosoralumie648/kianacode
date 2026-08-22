---
phase: 02-reproducible-toolchain-dependency-convergence
verified: 2026-08-22T10:57:00Z
status: human_needed
score: 2/3 must-haves fully verified; success criterion 1 is local_behavior only
behavior_unverified: 1
overrides_applied: 0
reconstructed_plans: true
human_verify_mode: end-of-phase
decision_coverage:
  honored: 12
  total: 14
  not_honored:
    - "D-11 full Sigstore/release-signature.json chain remains outside Phase 2"
    - "D-14 collects no new target-environment or user-value evidence; it only keeps missing evidence blocking"
  human:
    - test: "Review --md --audience=user redaction for path/hostname leakage"
      expected: "User markdown shows category counts and check ids without host paths or env secrets"
      why_human: "Redaction sufficiency is a judgment call; grep cannot certify that every sensitive string is absent from future check titles"
    - test: "Confirm GitHub Actions release-smoke Record build inputs writes a schema-valid dist/build-inputs.json"
      expected: "CI-produced file contains live rustc version, Cargo.lock SHA-256, and 40-hex HEAD"
      why_human: "No CI run artifact is attached to this checkout; local generation is not target_environment proof"
    - test: "Decide whether Phase 2 may close with designed dist/CI/signing blockers still blocking"
      expected: "Phase checkbox stays unchecked unless a human accepts local_behavior plus fail-closed missing artifacts"
      why_human: "workflow.human_verify_mode is end-of-phase; auto_advance must not invent Phase 3 planning answers"
---

# Phase 02: Reproducible Toolchain & Dependency Convergence Verification

**Phase Goal:** 用户和发布维护者可以复现构建并透明判断产品离发布就绪还缺什么。
**Verified:** 2026-08-22T10:57:00Z
**Status:** human_needed
**Reconstruction:** `02-01-PLAN.md` and `02-02-PLAN.md` were reconstructed from `02-01-SUMMARY.md` / `02-02-SUMMARY.md` and commits `c0bd383` / `0eaaa9e` / `05cc81a`. The original PLAN files were lost with unpushed `feature/tui-mega-upgrade` after `git gc --prune=now`.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | 发布维护者可以从固定工具链、lockfile 和依赖策略复现同一构建输入，并解释版本差异。 | PRESENT_BEHAVIOR_UNVERIFIED | local_behavior landed: `rust-toolchain.toml` channel=stable + rustfmt/clippy/rust-src; `kiana.build-inputs.v1` schema; isolated record step produced schema-valid JSON (`toolchain_version=1.97.1`, `source_hash=eae099de13c7f2639e6211e064165b40359d0aa7`). Target CI job has not been observed after the 2026-08-22 env-wiring fix. |
| 2 | 用户和管理员可以查看并导出 local/external blockers、平台、签名、SBOM、license 与 acceptance readiness，敏感信息保持脱敏。 | VERIFIED | `bash scripts/commercial-release-blockers-report.sh --json` → 22 checks. `--md --audience=user` renders category counts and check ids without `dist/` host paths in the summary table. License summary script emitted `kiana.license-compliance-summary.v1` with 545 packages / 38 workspace / 0 UNKNOWN and passed schema validation. |
| 3 | 缺少目标环境或用户证据的能力保持阻塞，不能被标记为 complete / production-ready / 1.0。 | VERIFIED | Live report status=`blocked`, 17 blocking / 5 satisfied. Phase 2 generated-artifact checks `toolchain.build-inputs`, `sbom.present`, `sbom.signed`, `license.compliance-summary` are blocking without `dist/` outputs. ROADMAP Phase 2 checkbox remains `[ ]`. |

**Score:** 2/3 truths verified; truth 1 is local_behavior only (excluded from a `passed` closeout).

### Required Artifacts

| Artifact | Exists | Substantive | Wired | Status |
|----------|--------|-------------|-------|--------|
| `rust-toolchain.toml` | Yes | channel=stable, three components | CI `@master` workflows | VERIFIED |
| `docs/toolchain-upgrade-policy.md` | Yes | upgrade wait / MSRV / CI note | referenced by toolchain file comment | VERIFIED |
| `docs/schemas/kiana-build-inputs.v1.schema.json` | Yes | 7 required fields, additionalProperties false | schema-contract fixtures + CI record step | VERIFIED |
| `scripts/fixtures/valid-build-inputs.json` | Yes | valid instance | `validate-json-schema.py` OK | VERIFIED |
| `scripts/fixtures/invalid-build-inputs-missing-version.json` | Yes | missing toolchain_version | validator exit 1 as required | VERIFIED |
| `.github/workflows/release-smoke.yml` Record build inputs | Yes | exports live rustc/lock/HEAD into Python | 2026-08-22 fix removed empty `${{ env.* }}` mappings | VERIFIED local; CI unobserved |
| `.github/workflows/release.yml` / `release-tui.yml` | Yes | `dtolnay/rust-toolchain@master` | no remaining `@stable` | VERIFIED |
| `scripts/commercial-release-blockers-report.sh` | Yes | 22 checks, `--md` dual audience | `toolchain.*` / `sbom.*` / `license.compliance-summary` | VERIFIED |
| `scripts/sign-sbom.sh` | Yes | user SBOM redact + optional signing skip | blocker `sbom.signed` stays blocking without sig | VERIFIED |
| `scripts/generate-license-summary.sh` | Yes | JSON/MD export | schema valid on `/tmp` output; `dist/license-summary.json` absent | VERIFIED local_behavior |
| `docs/schemas/kiana-license-compliance-summary.v1.schema.json` | Yes | closed package table | **not** wired into `schema-contract-smoke.sh` | WARNING: schema exists, smoke coverage missing |

`gsd-tools query verify.artifacts`: 02-01 8/8 passed, 02-02 5/5 passed.
`gsd-tools query verify.key-links`: 02-01 3/3, 02-02 4/4 verified.

## Behavioral Verification

| Check | Result | Detail |
|-------|--------|--------|
| `python3` tomllib `rust-toolchain.toml` | pass | channel=stable, components rustfmt/clippy/rust-src |
| `rg dtolnay/rust-toolchain@stable .github/workflows` | pass | no matches after `release-tui.yml` alignment |
| Isolated record-build-inputs + schema validate | pass | `/tmp/tmp.rlcXyz7gZq/build-inputs.json` conforms |
| Build-inputs fixtures | pass | valid OK; missing-version fails |
| Blocker report `--json` | pass | 22 checks; `toolchain.rust-toolchain-file` satisfied |
| Blocker report `--md --audience=user` | pass | category table + Action Required list |
| `generate-license-summary.sh --json` | pass | 545/38/0 UNKNOWN; schema OK |
| `commercial-release-handoff-smoke.sh` | pass | after status-aware `source.remote` markdown assertion |
| Full `schema-contract-smoke.sh` | not used as Phase 2 closeout | reaches Phase 1 `capability-governance-smoke.sh` supervisor; Phase 2 fixture section is independent and passed |
| Workspace `cargo test` | skipped | not a Phase 2 success criterion; 5-minute suite would mix unrelated global-state tests |

## Verification-time fixes (not historical Wave 1-3 work)

1. `.github/workflows/release-smoke.yml` Record build inputs now `export`s `TOOLCHAIN_VERSION` / `CARGO_LOCK_HASH` / `SOURCE_HASH` in the same shell. The landed Wave 2 step read empty GitHub `env.toolchain_version` mappings, which would KeyError or write empty fields in CI.
2. `.github/workflows/release-tui.yml` toolchain action `@stable` → `@master` so the later TUI workflow reads `rust-toolchain.toml` (D-04 regression).
3. `scripts/schema-contract-smoke.sh` and `scripts/commercial-release-handoff-smoke.sh` no longer require `source.remote` in blocking handoff markdown when the live origin makes that check `satisfied`. JSON still requires the check and owner override.

These fixes are local_behavior repairs. They do **not** create CI, SBOM, or signing evidence.

## Designed remaining blockers (not gaps)

Phase 2 success criterion 3 requires these to stay blocking until real artifacts exist:

- `toolchain.build-inputs` — needs CI-produced `dist/build-inputs.json`
- `sbom.present` — run `scripts/generate-sbom.sh`
- `sbom.signed` — needs `KIANA_SIGNING_COMMAND` and `dist/sbom.cdx.json.sig`
- `license.compliance-summary` — needs `dist/license-summary.json` in the build flow
- External family (`signing.release-artifacts`, live provider, platform artifacts, acceptance proofs) — later phases / Phase 24

## Decision Coverage

CONTEXT D-01..D-14: honored in artifacts except D-11 (full release signature / Sigstore) which Wave 3 explicitly did not complete, and D-14 which only preserves fail-closed visibility.

## Human Verification Required

`workflow.human_verify_mode: end-of-phase`. Do not mark ROADMAP Phase 2 `[x]`, do not set `completed_phases: 2`, and do not start `$gsd-discuss-phase 3` until a human accepts this report.

Keep `wip/unlanded-extensions` and `wip/stash-before-ctrl-r-merge`.

## Status

`human_needed` — local implementation and fail-closed blockers are demonstrated; CI/signing/user-value evidence and the end-of-phase gate are not.
