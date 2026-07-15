---
phase: 01
slug: baseline-evidence-governance
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-07-15
---

# Phase 1 - Validation Strategy

> Phase 1 的反馈采样合同。所有快速门禁均离线运行，不访问网络，也不把模块、stub、mock 或测试数量当作完成证据。

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Python 3.13 standard library + existing JSON Schema subset validator + Bash contract smoke |
| **Config file** | `scripts/validate-json-schema.py`, `scripts/schema-contract-smoke.sh` |
| **Quick run command** | `bash scripts/capability-governance-smoke.sh` |
| **Full suite command** | `bash scripts/schema-contract-smoke.sh` |
| **Estimated runtime** | Quick gate target `<30s`; first green run must record measured duration |

---

## Sampling Rate

- **After every task commit:** Run `bash scripts/capability-governance-smoke.sh`
- **After every plan wave:** Run `bash scripts/schema-contract-smoke.sh`
- **Before `$gsd-verify-work`:** Both commands must be green, followed by `git diff --check -- docs/agent-program/kiana-completion/governance docs/schemas scripts docs/reference-feature-matrix.md`
- **Max feedback latency:** 30 seconds for the quick gate

---

## Threat Coverage

| Threat Ref | Threat | Automated secure behavior |
|------------|--------|---------------------------|
| `T-01` | Repository, source, or evidence path traversal | Reject absolute paths, `..`, unsupported URI schemes, and resolved paths outside allowed roots with a stable non-zero diagnostic. |
| `T-02` | Markdown/HTML injection through source metadata or rationale | Escape table/link/control characters and reject raw unsafe HTML in generated views. |
| `T-03` | Stale or mismatched evidence reported as current completion | Require matching subject, source/target revision, freshness, proof threshold, and current evidence before counting completion. |
| `T-04` | License ambiguity silently treated as compatible | Keep unknown/conflicting license state non-current and block Adopt/Adapt completion until explicit compatibility evidence exists. |
| `T-05` | Generated view or summary diverges from canonical data | Recompute summaries and compare generated output byte-for-byte; any drift exits non-zero. |

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| `01-01-01` | 01 | 1 | `COD-01`, `DIF-11` | `T-01`, `T-04` | Four closed schemas reject unknown enum values, unsafe paths, incomplete source metadata, and ambiguous current license decisions. | contract | `bash scripts/capability-governance-smoke.sh schemas` | No - Wave 0 creates the focused smoke and fixtures | pending |
| `01-01-02` | 01 | 1 | `COD-01` | `T-03` | A frozen public snapshot covers every required journey; required children below proof/freshness gates cannot complete a journey. | semantic contract | `bash scripts/capability-governance-smoke.sh public-baseline` | No - Wave 0 creates canonical seed and negative fixtures | pending |
| `01-01-03` | 01 | 1 | `DIF-11` | `T-03`, `T-04` | Registry reconciliation proves 38 unique frozen identities and every inventoried capability has a complete Adopt/Adapt/Reject decision. | semantic contract | `bash scripts/capability-governance-smoke.sh reference-governance` | No - Wave 0 creates registry/decision fixtures | pending |
| `01-02-01` | 02 | 2 | `COD-01`, `DIF-11` | `T-01`, `T-03`, `T-04` | Duplicate IDs, dangling references, invalid Reject rationale, evidence mismatch, proof regression, stale snapshots, and wrong summaries fail closed with stable error prefixes. | negative integration | `bash scripts/capability-governance-smoke.sh semantic-negative` | No - Wave 0 creates semantic validator and fixture corpus | pending |
| `01-02-02` | 02 | 2 | `COD-01`, `DIF-11` | `T-02`, `T-05` | Two renders of identical canonical input are byte-identical, hostile text is escaped, and a hand-edited view fails `--check`. | deterministic integration | `bash scripts/capability-governance-smoke.sh generated-views` | No - Wave 0 creates generator and generated-view fixtures | pending |
| `01-03-01` | 03 | 3 | `COD-01`, `DIF-11` | `T-03`, `T-05` | Full canonical data, generated views, existing schema contracts, and migrated status-bearing documents agree on snapshot IDs and recomputed counts. | repository smoke | `bash scripts/schema-contract-smoke.sh` | Existing harness; Phase 1 invocation is added during implementation | pending |

Task IDs are the validation partition expected by planning. If the planner splits or renumbers plans, it must preserve each row's requirement, threat, command, and sampling point in the corresponding task.

---

## Wave 0 Requirements

- [ ] `scripts/capability-governance-smoke.sh` - focused offline runner with named slices `schemas`, `public-baseline`, `reference-governance`, `semantic-negative`, and `generated-views`
- [ ] `scripts/fixtures/capability-governance/valid/` - minimal cross-file graph plus one full 38-repository reconciliation fixture
- [ ] `scripts/fixtures/capability-governance/invalid/` - one focused fixture for every D-21 fail-closed invariant and threats `T-01` through `T-05`
- [ ] `scripts/validate-capability-governance.py` - semantic validation entrypoint with stable machine-readable error prefixes
- [ ] `scripts/generate-capability-governance.py` - deterministic renderer with `--check`
- [ ] First green quick run records elapsed time and proves the `<30s` budget without network or full Cargo workspace execution

No new test framework or third-party package is required.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Official Claude Code source classification and clean-room conflict resolution are semantically faithful to the cited public material. | `COD-01` | Automation can verify identity, version, hash, required fields, and snapshot coverage, but cannot establish whether a human interpretation of proprietary public behavior is accurate. | Sample every required journey category in the generated public-parity view; open its cited frozen source artifact; confirm version/applicability, capability decomposition, outcome, and any intentional-difference rationale; record reviewer, date, result, and artifact hash as evidence. |
| Adopt/Adapt/Reject license and security rationale is justified for each frozen reference capability. | `DIF-11` | SPDX-like syntax and file presence do not decide compatibility, clean-room obligations, or security acceptance. | Review all current Adopt/Adapt records and a risk-based sample of Reject records; confirm source location, license evidence, rationale, target owner/location, tests, risk, and review revision; unresolved items must remain blocked or stale. |

These reviews validate source interpretation only. Structural coverage, cross-references, proof/freshness gates, counts, and generated-view consistency remain fully automated.

---

## Validation Sign-Off

- [x] Every planned validation partition has an automated command or an explicit Wave 0 dependency
- [x] Sampling continuity has no three consecutive implementation tasks without an automated quick gate
- [x] Wave 0 covers every currently missing validator, generator, fixture, and focused runner
- [x] Commands are non-interactive and contain no watch-mode flags
- [ ] First implementation run demonstrates quick feedback latency `<30s`
- [x] `nyquist_compliant: true` is set in frontmatter

**Approval:** strategy approved for planning on 2026-07-15; implementation evidence pending
