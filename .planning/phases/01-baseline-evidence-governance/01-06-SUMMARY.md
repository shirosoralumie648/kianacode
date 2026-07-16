---
phase: 01-baseline-evidence-governance
plan: "06"
subsystem: governance-verification
tags: [python, rust-supervisor, negative-corpus, ancestry, drift, security, tdd]

requires:
  - phase: 01-baseline-evidence-governance
    provides: Production governance validator, positive supervised slices, and official-source coverage corpus from Plans 01-04 and 01-05
provides:
  - Exact post-implementation ancestry, semantic, drift, security, and alias adversarial corpus
  - Correct production diagnostic contract for freshness, history, drift, containment, alias, secret, and summary failures
  - Rust-supervised semantic-negative aggregate gate covering 34 negative cases and four protected valid fixtures
affects: [01-07-freeze-refresh, 01-10-legacy-authority, 01-11-generated-views, 01-12-schema-gate]

tech-stack:
  added: []
  patterns:
    - Standard-library unittest RED-GREEN contract correction before post-implementation corpus generation
    - Small repository-relative mutation wrappers preserve protected valid fixture bytes
    - Drift observation and fingerprint comparison remain separate production operations
    - Rust supervisor explicitly allowlists every public semantic slice

key-files:
  created:
    - scripts/tests/test_capability_governance.py
    - scripts/run-capability-governance-corpus.py
    - scripts/fixtures/capability-governance/invalid/integrity/expected-errors.json
    - scripts/fixtures/capability-governance/invalid/integrity/ancestry-mutations.json
    - scripts/fixtures/capability-governance/invalid/integrity/semantic-mutations.json
    - scripts/fixtures/capability-governance/invalid/integrity/security-mutations.json
    - scripts/fixtures/capability-governance/invalid/integrity/alias-contract.json
    - scripts/fixtures/capability-governance/invalid/drift/repository-drift.json
    - scripts/fixtures/capability-governance/invalid/drift/official-source-drift.json
    - scripts/fixtures/capability-governance/invalid/drift/target-revision-drift.json
  modified:
    - scripts/capability_governance.py
    - scripts/validate-capability-governance.py
    - scripts/capability-governance-smoke.sh
    - kiana-capability-governance-supervisor/src/lib.rs
    - kiana-capability-governance-supervisor/tests/supervisor_linux.rs

key-decisions:
  - "Plan 01-06 exact diagnostic names are production API, not fixture aliases; production was corrected before the post-implementation corpus was authored."
  - "Symlink and oversized reads preserve the authoritative read/usage exit 2 contract; corpus cases declare expected_exit while semantic and drift failures remain exit 1."
  - "Repository-relative mutation bases resolve from the repository root through the same containment authority used by production bindings."
  - "The semantic-negative harness calls the real CLI or production drift comparator and never branches production behavior on fixture path or case ID."

patterns-established:
  - "Post-implementation adversarial verification: freeze API -> RED correction tests -> GREEN production -> author corpus -> execute through Rust authority."
  - "Protected fixture derivatives are declarative mutation wrappers; the canonical positive files remain byte-identical."

requirements-completed: [COD-01, DIF-11]

coverage:
  - id: D1
    description: "Fourteen ancestry and semantic invariants have independent exact-code corpus cases, including expiry and newer failed retest with exact stale subjects."
    requirement: COD-01
    verification:
      - kind: integration
        ref: "scripts/run-capability-governance-corpus.py#34 exact negative cases"
        status: pass
      - kind: unit
        ref: "scripts/tests/test_capability_governance.py#GovernanceCorrectionContractTests"
        status: pass
    human_judgment: false
  - id: D2
    description: "Repository HEAD/tree/license/content, official source, target revision, and unavailable-source drift each emit their exact non-current diagnostic."
    requirement: DIF-11
    verification:
      - kind: unit
        ref: "scripts/tests/test_capability_governance.py#GovernanceDriftAndUsageContractTests"
        status: pass
      - kind: integration
        ref: "scripts/run-capability-governance-corpus.py#production drift scenarios"
        status: pass
    human_judgment: false
  - id: D3
    description: "Path, symlink, size, markup, sensitive value, and four alias-contract variants fail with bounded redacted output while hostile valid text remains accepted."
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "bash scripts/capability-governance-smoke.sh semantic-negative"
        status: pass
    human_judgment: false
  - id: D4
    description: "The semantic-negative aggregate executes only through the Rust supervisor with exact receipt identity, offline tracing, deadline, cleanup, and protected-input preservation."
    requirement: DIF-11
    verification:
      - kind: integration
        ref: "cargo test -p kiana-capability-governance-supervisor --locked --offline --no-fail-fast -- --test-threads=1"
        status: pass
      - kind: integration
        ref: "semantic-negative elapsed_seconds=15.969 offline=true"
        status: pass
    human_judgment: false

duration: 2h 44m
completed: 2026-07-16
status: complete
---

# Phase 1 Plan 6: Semantic Governance Adversarial Corpus Summary

**Thirty-four exact adversarial cases now prove capability-governance history, freshness, drift, security, and alias behavior through the Rust offline authority while four protected valid bundles remain accepted and byte-identical.**

## Performance

- **Duration:** 2h 44m
- **Started:** 2026-07-16T11:41:36Z
- **Completed:** 2026-07-16T14:25:34Z
- **Tasks:** 4
- **Files modified:** 37

## Accomplishments

- Corrected the production exact-code contract with two recorded RED-GREEN cycles and 24 focused standard-library tests.
- Added 14 ancestry/semantic cases, seven drift cases, five security cases, four alias cases, and retained the four prior coverage cases in one 34-case aggregate oracle.
- Added a bounded corpus runner that exercises the real CLI and production drift comparator, revalidates all four protected valid fixtures, checks redaction, and verifies 12 protected input hashes.
- Registered `semantic-negative` as a first-class Rust supervisor slice; the final supervised run completed in 15.969 seconds with `offline=true`.

## Task Commits

1. **Implementation correction plan and TDD diagnostic contract** - `dac4634`, `bb1ea44`, `db46d04`, `14a37f4`, `8be1432`, `8c4ef4e`, `e7803ee`, `4e4823e`, `f07aa9f`
2. **Task 1: History and semantic cases** - `779b441`
3. **Task 2: Governance drift cases** - `6a4ea73`
4. **Task 3: Security and alias cases** - `f316e00`
5. **Task 4: Rust-supervised aggregate gate** - `b53d016`, `387fbb6`

**Plan metadata:** committed with this SUMMARY and the GSD tracking updates.

## Files Created/Modified

- `scripts/capability_governance.py` - Exact diagnostic, freshness, history, containment, drift-comparison, and cached structural validation authority.
- `scripts/validate-capability-governance.py` - Thin fixed-root CLI with typed usage diagnostics.
- `scripts/tests/test_capability_governance.py` - 24 production behavior tests with observed RED failures before correction.
- `scripts/run-capability-governance-corpus.py` - Bounded aggregate exact oracle over CLI, drift, temporary security, valid, redaction, and protected-hash checks.
- `scripts/fixtures/capability-governance/invalid/integrity/` - Post-implementation manifest, mutation sets, and focused wrapper cases.
- `scripts/fixtures/capability-governance/invalid/drift/` - Declarative repository, official-source, target, and unavailable-source observations.
- `scripts/capability-governance-smoke.sh` - Hidden and public `semantic-negative` worker path.
- `kiana-capability-governance-supervisor/src/lib.rs` - Explicit `SemanticNegative` slice and receipt identity.
- `kiana-capability-governance-supervisor/tests/supervisor_linux.rs` - Five-slice public execution matrix.

## Decisions Made

- Kept `0=valid/current`, `1=invalid/stale`, and `2=read-or-usage failure`; Plan 06 manifests carry `expected_exit` instead of weakening symlink/oversize behavior to exit 1.
- Defined `summary_mismatch` narrowly as registry `expected_count` versus actual repository count, never as product-completion proof.
- Chose small mutation wrappers over full bundle copies so every case changes one invariant and protected base bytes stay canonical.
- Kept live repository observation in `repository_fingerprint`/`detect_drift` and extracted `compare_repository_fingerprint` for exact comparator testing without duplicating semantic rules in the corpus harness.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added an explicit production-contract correction before corpus generation**
- **Found during:** Plan 06 preflight
- **Issue:** Plan 01-04 production emitted older aggregate aliases while the approved post-implementation Plan 01-06 froze newer exact diagnostics.
- **Fix:** Wrote an independent correction artifact, observed RED failures, corrected production, and re-proved all positive inputs before authoring negative assets.
- **Files modified:** `scripts/capability_governance.py`, `scripts/validate-capability-governance.py`, `scripts/tests/test_capability_governance.py`
- **Verification:** 24 focused tests pass; every RED test failed for the old alias or missing classification before implementation.
- **Committed in:** `dac4634` through `f07aa9f`

**2. [Rule 1 - Bug] Made repository-relative mutation bases internally consistent**
- **Found during:** Task 1 exact-oracle execution
- **Issue:** Loader validation required a repository-relative path but resolved it relative to the wrapper directory, making protected sibling bases unreachable.
- **Fix:** Resolve mutation bases from the repository root through the existing containment resolver.
- **Files modified:** `scripts/capability_governance.py`, `scripts/tests/test_capability_governance.py`
- **Verification:** Repository-relative base and symlink-escape CLI tests pass; 14 Task 1 cases execute exactly.
- **Committed in:** `4e4823e`, `f07aa9f`

**3. [Rule 3 - Blocking] Added the Rust supervisor authority registration**
- **Found during:** Task 4
- **Issue:** The shell worker could not become a public receipt authority until Rust explicitly allowed `semantic-negative`.
- **Fix:** Added a RED parser assertion, `SemanticNegative` enum mapping, and five-slice Linux public execution coverage.
- **Files modified:** `kiana-capability-governance-supervisor/src/lib.rs`, `kiana-capability-governance-supervisor/tests/supervisor_linux.rs`
- **Verification:** Supervisor passes 24 unit and 10 Linux integration tests.
- **Committed in:** `b53d016`, `387fbb6`

**4. [Rule 1 - Bug] Kept aggregate execution below the fixed 30-second deadline**
- **Found during:** Task 4 supervised execution
- **Issue:** Repeated repository-root discovery, structural helper/schema loading, and temporary Git observation amplified under syscall tracing and exceeded the deadline.
- **Fix:** Reused explicit trusted roots, cached immutable schema/helper loads by path/mtime/size, and separated live fingerprint observation from the production comparator so declared drift observations do not create temporary Git repositories.
- **Files modified:** `scripts/capability_governance.py`, `scripts/validate-capability-governance.py`, `scripts/run-capability-governance-corpus.py`
- **Verification:** Final supervised aggregate completed in 15.969 seconds; all 34 cases and four valid fixtures still run.
- **Committed in:** `387fbb6`

---

**Total deviations:** 4 auto-fixed (2 blocking prerequisites, 2 implementation bugs). **Impact:** All fixes preserve the approved production/corpus/supervisor boundaries; no requirement, commercial target, protected input, or dirty user file was removed or weakened.

## Issues Encountered

- The first two deadline hypotheses improved structural efficiency but did not clear the supervised deadline. A syscall profile then isolated temporary Git/subprocess work; the third bounded fix succeeded, and no fourth fix was attempted.
- Existing user changes in `scripts/validate-json-schema.py` and planning files remained outside every task commit; protected-input proof used the Plan 06 commit range so those pre-existing dirty bytes were neither claimed nor reverted.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 01-07 can consume exact drift codes and the production fingerprint comparator for freeze/check-drift/refresh lifecycle work.
- Plan 01-11 can reuse `semantic-negative` as the aggregate adversarial gate alongside generated views.
- No unresolved Plan 06 semantic, ancestry, drift, security, alias, supervisor, or protected-input blocker remains.

## Self-Check: PASSED

- All declared key files exist and all Plan 06 task commits are present.
- 24 Python contract tests, four valid bundles, three supervised semantic slices, Rust formatting, Python/Bash syntax, and the full supervisor crate pass.
- `semantic-negative` reports 34 negative cases, four valid fixtures, 12 protected inputs, 15.969 seconds, and `offline=true`.
- The Plan 06 commit range changes no protected fixture, governance schema, or `scripts/validate-json-schema.py` byte.

---
*Phase: 01-baseline-evidence-governance*
*Completed: 2026-07-16*
