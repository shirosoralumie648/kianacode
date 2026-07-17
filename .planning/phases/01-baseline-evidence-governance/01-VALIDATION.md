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
| **Framework** | Rust supervisor (`std + libc`) + Python 3.13 standard library semantic worker + existing read-only JSON Schema helper |
| **Config file** | Rust authority from Plan 03; production semantic CLI from Plan 04; protected `scripts/validate-json-schema.py` is consumed read-only |
| **Quick run command** | After the owning implementation action, run its exact `<automated>` command and the narrowest implemented Rust-supervised slice |
| **Full suite command** | `bash scripts/capability-governance-supervisor-smoke.sh`, then `bash scripts/capability-governance-smoke.sh`; run `bash scripts/schema-contract-smoke.sh` only after Plan 12 wires the focused gate into the broad repository contract |
| **Estimated runtime** | Final no-argument focused gate target `<30s`; first successful production verification must record measured duration |

---

## Sampling Rate

- **After every task commit:** Run that task's exact post-implementation `<automated>` command; once its slice exists, also run the corresponding Rust-supervised slice.
- **After every plan wave:** Run every focused slice implemented through that wave. Do not call a future slice before its producing plan completes.
- **Before `$gsd-verify-work`:** Run `bash scripts/capability-governance-smoke.sh`, repeat the `production` slice, run `bash scripts/schema-contract-smoke.sh`, then run `git diff --check -- docs/agent-program/kiana-completion/governance docs/schemas scripts/capability_governance.py scripts/validate-capability-governance.py scripts/freeze-capability-governance.py scripts/generate-capability-governance.py scripts/capability-governance-smoke.sh scripts/schema-contract-smoke.sh docs/reference_audit/CAPABILITY-GOVERNANCE.generated.txt`.
- **Max feedback latency:** 30 seconds for the quick gate

### Wave Sampling

| Wave | Plans | Required wave gate |
|------|-------|--------------------|
| 1 | `01`, `02` | Parse all eight schemas and run their exact field/closed-object assertions. |
| 2 | `03`, `04` | Rust supervisor focused/adversarial/archive gates, then production semantic CLI syntax and positive `public-baseline`/`reference-governance` verification. |
| 3 | `05`, `06` | Generate post-implementation coverage/integrity/drift/security corpora, execute every exact oracle, then run Rust-supervised `semantic-negative`. |
| 4 | `07` | `drift-refresh` plus the unchanged semantic slices. |
| 5 | `08` | `public-baseline`, `drift-refresh`, and source-artifact/history validation. |
| 6 | `09` | `reference-governance` plus registry/decision predecessor and 38-identity validation. |
| 7 | `10` | Evidence history validation and the newly implemented `legacy-authority` slice. |
| 8 | `11` | `generated-views`, no-argument full gate, then `production`; all slices pass offline in `<30s`. |
| 9 | `12` | Prove the HEAD-derived alternate-index patch contains only one broad-gate invocation, validate the new-HEAD parent/tree entry, update only the target normal-index cache entry while preserving unrelated staged entries and every live dirty byte, then rerun focused and broad gates. |

---

## Threat Coverage

| Threat Ref | Threat | Automated secure behavior |
|------------|--------|---------------------------|
| `T-01` | Repository, source, or evidence path traversal | Reject absolute paths, `..`, unsupported URI schemes, and resolved paths outside allowed roots with a stable non-zero diagnostic. |
| `T-02` | Markdown/HTML injection through source metadata or rationale | Escape table/link/control characters and reject raw unsafe HTML in generated views. |
| `T-03` | Stale or mismatched evidence reported as current completion | Require matching subject, source/target revision, freshness, proof threshold, and current evidence before counting completion. |
| `T-04` | License ambiguity silently treated as compatible | Keep unknown/conflicting license state non-current and block Adopt/Adapt completion until explicit compatibility evidence exists. |
| `T-05` | Generated view or summary diverges from canonical data | Recompute summaries and compare generated output byte-for-byte; any drift exits non-zero. |
| `T-06` | Dirty broad-gate user hunks are staged, rewritten, or hidden by the Phase 1 integration | Require byte-identical anchors, remove-one-line preservation proofs, a HEAD-derived alternate index scoped to one path/one addition, exact new-HEAD parent/mode/blob/path validation, a target-only normal-index cacheinfo update, unchanged unrelated staged entries, and a still-dirty unstaged worktree after commit. |

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| `01-01-01` | 01 | 1 | `COD-01`, `DIF-11` | `T-01-01` | Official-source/public schemas close every record and require exclusive capability mapping or reviewed exclusion with immutable ancestry. | schema contract | `python3 -m json.tool docs/schemas/kiana-official-source-artifact.v1.schema.json && python3 -m json.tool docs/schemas/kiana-public-parity-baseline.v1.schema.json` | No - Plan 01 creates both schemas | pending |
| `01-01-02` | 01 | 1 | `COD-01`, `DIF-11` | `T-01-02`, `T-01-03` | Registry/decision schemas require exact HEAD/tree/license/target fingerprints and the single closed `aliases`/`domains` contract. | schema contract | `python3 -m json.tool docs/schemas/kiana-reference-repository-registry.v1.schema.json && python3 -m json.tool docs/schemas/kiana-capability-decisions.v1.schema.json` | No - Plan 01 creates both schemas | pending |
| `01-02-01` | 02 | 1 | `COD-01`, `DIF-11` | `T-02-01` | Evidence successors cannot omit predecessor identity/hash; genesis requires an explicit reason and record chains are immutable. | schema contract | `python3 -m json.tool docs/schemas/kiana-capability-evidence-index.v1.schema.json` | No - Plan 02 creates the schema | pending |
| `01-02-02` | 02 | 1 | `COD-01`, `DIF-11` | `T-02-02`, `T-02-03` | Diff, legacy classification, and current selector are closed metadata contracts and cannot store a second completion state. | schema contract | `python3 -m json.tool docs/schemas/kiana-capability-governance-diff.v1.schema.json && python3 -m json.tool docs/schemas/kiana-legacy-authority-classification.v1.schema.json && python3 -m json.tool docs/schemas/kiana-capability-governance-bundle.v1.schema.json` | No - Plan 02 creates all three schemas | pending |
| `01-03-01` | 03 | 2 | `COD-01`, `DIF-11` | `T-03-01`, `T-03-02` | Rust owns trusted paths, Python binding, sandbox, deadline, FDs, trace, receipt, bounded output, cleanup, and the only positive success conjunction. | production implementation | `cargo check -p kiana-capability-governance-supervisor --locked --offline` | No - Plan 03 creates the supervisor crate and hidden-worker cutover | pending |
| `01-03-02` | 03 | 2 | `COD-01`, `DIF-11` | `T-03-02`, `T-03-03`, `T-03-04` | Focused and adversarial cases run only after implementation and reject every forged, malformed, missing, unknown, timeout, leak, and cleanup-incomplete state. | post-implementation runtime verification | `cargo test -p kiana-capability-governance-supervisor --locked --offline --no-fail-fast && bash scripts/capability-governance-supervisor-smoke.sh` | No - Plan 03 creates tests and the independent gate after production code | pending |
| `01-03-03` | 03 | 2 | `COD-01`, `DIF-11` | `T-03-05` | Scoped and delivered-HEAD archives build/test locked and offline, exclude `reference/`, preserve protected bytes, and leave no child/runtime-root leak before summary restoration. | post-implementation archive/review gate | `cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1 && cargo build --workspace --locked --offline && git diff --check` | No - Plan 03 records archive/protection evidence after implementation | pending |
| `01-04-01` | 04 | 2 | `COD-01`, `DIF-11` | `T-04-01`, `T-04-02`, `T-04-03`, `T-04-04` | One production module implements source coverage, identities, decisions, ancestry, proof/freshness, drift, containment, redaction, and exact CLI statuses before any negative corpus exists. | production implementation | `python3 -m py_compile scripts/capability_governance.py scripts/validate-capability-governance.py` | No - Plan 04 creates the production module and CLI | pending |
| `01-04-02` | 04 | 2 | `COD-01`, `DIF-11` | `T-04-01`, `T-04-02` | Protected valid bundles exercise the completed production CLI and Rust-supervised positive slices without modifying fixture bytes. | post-implementation positive verification | `python3 scripts/validate-capability-governance.py validate --fixture-bundle scripts/fixtures/capability-governance/valid/minimal-graph.json --json && bash scripts/capability-governance-smoke.sh public-baseline && bash scripts/capability-governance-smoke.sh reference-governance` | No - Plan 04 adds positive slices after implementation | pending |
| `01-05-01` | 05 | 3 | `COD-01` | `T-05-01`, `T-05-02`, `T-05-03`, `T-05-04` | Missing capability, unmapped entry, duplicate mapping, and unjustified exclusion fixtures are generated only after Plan 04 production completion. | post-implementation adversarial fixture | `for f in scripts/fixtures/capability-governance/invalid/coverage/*.json; do python3 -m json.tool "$f" >/dev/null; done` | No - Plan 05 creates the coverage corpus after implementation | pending |
| `01-05-02` | 05 | 3 | `COD-01` | `T-05-01`, `T-05-02`, `T-05-03` | Every coverage case executes against production with its declared exact code/subject, then the protected valid base still returns 0. | post-implementation adversarial execution | `python3 scripts/validate-capability-governance.py validate --fixture-bundle scripts/fixtures/capability-governance/valid/offline-source-identity.json --json` | No - Plan 05 executes its generated manifest after implementation | pending |
| `01-06-01` | 06 | 3 | `COD-01`, `DIF-11` | `T-06-01`, `T-06-04` | Ancestry rewrite, proof/freshness, decision, reference, and summary cases are derived after production; expiry and newer failed retest remain independent. | post-implementation integrity fixture | `for f in scripts/fixtures/capability-governance/invalid/integrity/{expected-errors,ancestry-mutations,semantic-mutations}.json; do python3 -m json.tool "$f" >/dev/null; done` | No - Plan 06 creates the integrity corpus after implementation | pending |
| `01-06-02` | 06 | 3 | `COD-01`, `DIF-11` | `T-06-02`, `T-06-04` | Repository HEAD/tree/license/content, official source, target revision, and unavailable-source cases independently require non-current output. | post-implementation drift fixture | `for f in scripts/fixtures/capability-governance/invalid/drift/*.json; do python3 -m json.tool "$f" >/dev/null; done` | No - Plan 06 creates drift cases after implementation | pending |
| `01-06-03` | 06 | 3 | `COD-01`, `DIF-11` | `T-06-03`, `T-06-04` | Path/symlink/size/markup/secret and alias-shape cases have exact bounded redacted expectations while valid hostile text stays accepted. | post-implementation security fixture | `python3 -m json.tool scripts/fixtures/capability-governance/invalid/integrity/security-mutations.json && python3 -m json.tool scripts/fixtures/capability-governance/invalid/integrity/alias-contract.json` | No - Plan 06 creates security cases after implementation | pending |
| `01-06-04` | 06 | 3 | `COD-01`, `DIF-11` | `T-06-01`, `T-06-02`, `T-06-03`, `T-06-04` | Every negative manifest case executes against production, exact diagnostics match, valid inputs still pass, and `semantic-negative` runs only through Rust authority. | post-implementation adversarial integration | `bash scripts/capability-governance-smoke.sh semantic-negative` | No - Plan 06 adds the aggregate slice after all production and fixture work | pending |
| `01-07-01` | 07 | 4 | `COD-01`, `DIF-11` | `T-07-01`, `T-07-03` | Freeze is offline/atomic; actual `check-drift` proves no-drift current and seven independent drift/unavailable cases non-current with exact codes. | CLI integration | `set -e; tmp="$(mktemp -d)"; for c in no_drift repository_head_drift repository_tree_drift license_hash_drift content_tree_drift official_source_hash_drift target_revision_drift source_unavailable; do set +e; python3 scripts/freeze-capability-governance.py check-drift --fixture-bundle scripts/fixtures/capability-governance/valid/refresh-request.json --case "$c" --output "$tmp/$c.json"; rc=$?; set -e; if test "$c" = no_drift; then test "$rc" -eq 0; else test "$rc" -eq 1; fi; done` | No - Plan 07 creates CLI/request fixture | pending |
| `01-07-02` | 07 | 4 | `COD-01`, `DIF-11` | `T-07-02`, `T-07-03` | Refresh preserves predecessors, appends stale/supersession events, validates before atomic publish, and never overwrites. | lifecycle integration | `bash scripts/capability-governance-smoke.sh drift-refresh` | No - Plan 07 creates result fixture/refresh command | pending |
| `01-08-01` | 08 | 5 | `COD-01` | `T-08-01`, `T-08-04` | Production controlled source artifact is public-only, archive/hash bound, uniquely indexed, and verifiable offline. | production contract | `python3 scripts/validate-json-schema.py docs/schemas/kiana-official-source-artifact.v1.schema.json docs/agent-program/kiana-completion/governance/source-artifacts/claude-code-public-2026-07-15.json` | No - Plan 08 creates the artifact | pending |
| `01-08-02` | 08 | 5 | `COD-01` | `T-08-02`, `T-08-03` | Genesis/current public revisions cover every official entry and required child without inflating proof beyond current evidence. | production history | `python3 scripts/validate-capability-governance.py validate-history --head docs/agent-program/kiana-completion/governance/public-baselines/cc-public-2026-07-15.json --json` | No - Plan 08 creates both revisions | pending |
| `01-09-01` | 09 | 6 | `DIF-11` | `T-09-01`, `T-09-02`, `T-09-03` | Reviewed registry/decision chains preserve 38 unique identities, exact inventory coverage, fingerprints, license, owner/risk/test/evidence, reasons, and governance/product separation. | production history | `bash scripts/capability-governance-smoke.sh reference-governance` | No - Plan 09 creates registry/decision chains | pending |
| `01-10-01` | 10 | 7 | `COD-01`, `DIF-11` | `T-10-01` | Production evidence successor has mandatory ancestry and current subject/source/target/environment binding; genuine expired success, newer failed retest, stale, and supersession history remains append-only and only actually observed events are recorded. | production history | `python3 scripts/validate-capability-governance.py validate-history --head docs/agent-program/kiana-completion/governance/evidence/revisions/evidence-2026-07-15.json --json` | No - Plan 10 creates evidence chain | pending |
| `01-10-02` | 10 | 7 | `COD-01`, `DIF-11` | `T-10-02`, `T-10-03`, `T-10-04` | Every live audit/status path is hash-classified with rationale/replacement; current selects explicit heads and one checked-in RFC3339 `evaluation_time` without completion fields; the runner owns `legacy-authority` at this wave. | legacy/manifest integration | `bash scripts/capability-governance-smoke.sh legacy-authority` | No - Plan 10 creates classification/current/compat manifests and slice | pending |
| `01-11-01` | 11 | 8 | `COD-01`, `DIF-11` | `T-11-01`, `T-11-03` | Default render rejects repository paths before writes; two-root allowlisted compat render is deterministic and `--check` preserves bytes/mtime. | deterministic integration | `tmp="$(mktemp -d)" && python3 scripts/generate-capability-governance.py render --manifest docs/agent-program/kiana-completion/governance/current.json --output-root "$tmp/default" && python3 scripts/generate-capability-governance.py render-compat --manifest docs/agent-program/kiana-completion/governance/current.json --output-root "$tmp/compat" --repository-output-root "$tmp/repository" --compat-manifest docs/agent-program/kiana-completion/governance/compat-outputs.json --check` | No - Plan 11 creates generator/shared render functions | pending |
| `01-11-02` | 11 | 8 | `COD-01`, `DIF-11` | `T-11-02` | Both family `diff` commands are independently repeatable, schema-valid, exact from/to ID/hash bound, and each matching `--check` exits 1 without rewriting its own one-byte-tampered output. | generated artifact | `set -e; tmp="$(mktemp -d)"; check_diff(){ family="$1"; from="$2"; to="$3"; out="$4"; python3 scripts/generate-capability-governance.py diff --family "$family" --from "$from" --to "$to" --output "$out"; cp "$out" "$out.first"; python3 scripts/generate-capability-governance.py diff --family "$family" --from "$from" --to "$to" --output "$out"; cmp "$out.first" "$out"; python3 -c 'import pathlib,sys; p=pathlib.Path(sys.argv[1]); p.write_bytes(p.read_bytes()+b" ")' "$out"; cp "$out" "$out.tampered"; set +e; python3 scripts/generate-capability-governance.py diff --family "$family" --from "$from" --to "$to" --output "$out" --check; rc=$?; set -e; test "$rc" -eq 1; cmp "$out.tampered" "$out"; }; check_diff public-baseline docs/agent-program/kiana-completion/governance/public-baselines/cc-public-2026-07-15-genesis.json docs/agent-program/kiana-completion/governance/public-baselines/cc-public-2026-07-15.json "$tmp/public.json"; check_diff repository-registry docs/agent-program/kiana-completion/governance/repository-registry/references-2026-07-15-genesis.json docs/agent-program/kiana-completion/governance/repository-registry/references-2026-07-15.json "$tmp/repository.json"` | No - Plan 11 creates views/diffs | pending |
| `01-11-03` | 11 | 8 | `COD-01`, `DIF-11` | `T-11-03`, `T-11-04` | Generated directory warning uses allowlisted compat output; final offline gate checks all heads, ancestry, drift, legacy hashes, 38 identities, source coverage, views, and diffs. | phase gate | `bash scripts/capability-governance-smoke.sh && bash scripts/capability-governance-smoke.sh production` | No - Plan 11 completes runner and warning | pending |
| `01-12-01` | 12 | 9 | `COD-01`, `DIF-11` | `T-12-01`, `T-12-02`, `T-12-03` | A HEAD-derived alternate-index commit adds exactly one focused-runner invocation and no deletion; before a target-only normal-index cacheinfo update, the new HEAD has the expected parent and one exact blob entry with the original mode and verified phase-only blob; unrelated staged entries and live bytes remain unchanged, the target staged diff is empty, the complete original live dirty bytes are recoverable by removing only that line, and focused plus broad gates pass. | dirty-worktree integration | `path=scripts/schema-contract-smoke.sh && test "$(git diff-tree --no-commit-id --name-only -r HEAD)" = "$path" && patch="$(git show --format= --unified=0 HEAD -- "$path")" && python3 -c 'import sys; lines=sys.argv[1].splitlines(); added=[x for x in lines if x.startswith("+") and not x.startswith("+++")]; removed=[x for x in lines if x.startswith("-") and not x.startswith("---")]; assert added==["+bash scripts/capability-governance-smoke.sh"],added; assert not removed,removed' "$patch" && git diff --cached --quiet -- "$path" && if git diff --quiet -- "$path"; then exit 1; fi && test "$(awk '$0=="bash scripts/capability-governance-smoke.sh"{n++} END{print n+0}' "$path")" -eq 1 && bash scripts/capability-governance-smoke.sh && bash scripts/schema-contract-smoke.sh` | Existing path is dirty; Plan 12 adds one protected invocation | pending |

The map contains all 26 tasks from the checked 12-plan graph. Plan/task renumbering requires an immediate map update before execution.

---

## Wave 0 Requirements

- [ ] Plans `01` and `02`: eight versioned closed schemas for official source, four canonical families, diff, legacy classification, and head selection.
- [ ] Plan `03`: Rust supervisor implementation, hidden-worker cutover, post-implementation focused/adversarial gates, two fresh archives, and protected-byte proof; the four valid fixtures remain read-only.
- [ ] Plan `04`: complete production `scripts/capability_governance.py` plus thin CLI and positive verification before any negative corpus is authored.
- [ ] Plans `05` and `06`: post-implementation coverage, ancestry, semantic, drift, security, and alias corpora plus exact production execution evidence.
- [ ] Plan `07`: explicit offline freeze/check-drift/refresh CLI and lifecycle fixtures.
- [ ] Plans `08` and `09`: non-genesis production source/public and reference/decision heads.
- [ ] Plan `10`: evidence/legacy successors, path/hash-only manifests, and progressive `legacy-authority` slice.
- [ ] Plan `11`: deterministic generator, generated views/diffs, compat warning, and final `generated-views` and `production` slices.
- [ ] Plan `12`: one exact focused-runner invocation added through the HEAD-derived alternate-index protocol; the new-HEAD parent/tree entry is verified before a target-only normal-index cacheinfo update, unrelated staged entries and original live dirty bytes are preserved, and focused/broad gates rerun.
- [ ] First successful focused run records elapsed time and proves the `<30s` budget without network, package install, or a full Cargo workspace; the broad schema smoke remains a separate final integration gate.

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
- [x] Production supervisor and semantic implementations precede all newly authored focused/adversarial verification assets
- [x] Sampling continuity has no three consecutive implementation tasks without an automated quick gate
- [x] Wave 0 covers every currently missing validator, generator, fixture, and focused runner
- [x] Commands are non-interactive and contain no watch-mode flags
- [ ] First implementation run demonstrates quick feedback latency `<30s`
- [x] `nyquist_compliant: true` is set in frontmatter

**Approval:** strategy approved for planning on 2026-07-15; implementation evidence pending
