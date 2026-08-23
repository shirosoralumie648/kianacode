# v1.0.2 Context — REL-03 P0 closeout

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`
Requirements: REL-03
Chosen slice: audit-close the already-green P0 coding pack + a thin NOTICE. Not more tools, not live provider, not packaged/signed release, not Daily/Research, not v1.x.

## Classify

Phase, not spike. User-visible completion is: `docs/coding-pack-matrix.md` P0-LOOP…P0-PROV stay 已绿 with owner/test/evidence pointers; a closeout gate fails if any P0 row loses 已绿; `NOTICE` points at MIT OR Apache-2.0 and `deny.toml` without pretending to be an SBOM. After that, v1.0 may be checked with the same honesty ceiling as v1.0.1.

This is not a tool dump. This is not `scripts/release-smoke.sh`. This is not generating a crate-by-crate license tree. This is not claiming live Anthropic/OpenAI/Ollama. This is not Daily/Research. This is not enterprise.

## Locked discuss decisions

Do not reopen v0.2–v0.6, v1.0.1 install lifecycle, TUI park, or TeamCreate/SendMessage.

1. **REL-03 is an audit closeout, not recode.** P0-LOOP…P0-PROV are already green from v0.4–v0.6. The gate is `scripts/v10-p0-closeout-smoke.sh`: every `| P0-` row in the matrix must contain `已绿`. HTTP MCP, SkillTool, post/stop/session hooks, and live providers stay in Not claimed.
2. **NOTICE is dual-license plus deny.toml, not SBOM.** Write `NOTICE` pointing to `LICENSE-MIT`, `LICENSE-APACHE`, and `deny.toml`. Do not run `scripts/generate-sbom.sh` / `scripts/generate-license-summary.sh` as this phase's gate.
3. **REL-04 stays draft.** Daily/Research have no same-core golden path. Do not copy the runner.
4. **Checking v1.0 is allowed after this slice, with the existing ceiling.** v1.0 means a local personal company: write path, departments, receipts, temp install, USER.md, P0 coding pack. It does not mean `~/.local/bin` production, signed tarball, TUI pixel-parity, live provider, or enterprise.
5. **Do not open v1.x.** No tenant/RBAC/cloud.

Demo:

```text
docs/coding-pack-matrix.md P0 rows all contain 已绿
NOTICE names MIT OR Apache-2.0 and deny.toml
scripts/v10-p0-closeout-smoke.sh exits 0
USER.md and v10-personal-lifecycle-smoke.sh still exist
ROADMAP v1.0 may be checked; REL-04 remains draft
```

## Requirements this phase

REL-03. REL-01/REL-02 still true. REL-04 stays draft. PATH/TRUST/SESS/EVD/ROLE/ORCH/SYMP/WB/REV/CODE-01..04/DEPT-02/MEM-01..04/LONG-02 still true as regression.

## Frozen

- expanding `kiana-tools` / SkillTool / P1-READ
- live provider / unsupported_streaming
- HTTP/SSE MCP as complete
- `scripts/release-smoke.sh` / `package-lifecycle-smoke.sh` / SBOM / signing
- `~/.local/bin` production install
- Daily/Research as complete
- JointSymposium / staffing every COMPANY.md role / Librarian
- TeamCreate / SendMessage
- new CLI flags / formatting `cli.rs`
- worktrees / SDK / IDE / Desktop / Web
- migrating TUI
- opening v1.x
