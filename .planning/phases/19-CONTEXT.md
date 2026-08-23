# v1.0.1 Context — Personal install lifecycle + USER.md

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`
Requirements: REL-01, REL-02 (thin)
Chosen slice: temp `INSTALL_DIR` install/upgrade/rollback/recover/uninstall + a real user manual. Not packaged tarball, not SBOM/signing, not REL-03 recode, not `~/.local/bin` production.

## Classify

Phase, not spike. User-visible completion is: a temp `INSTALL_DIR` can install a working `kiana` binary, reinstall the same binary without changing its checksum, restore a backup (rollback), delete-and-reinstall (recover), and uninstall twice (idempotent). `USER.md` documents the commands that actually run today.

This is not `scripts/release-smoke.sh`. This is not `scripts/package-lifecycle-smoke.sh` (dist tarball + enterprise offline manifest). This is not a new CLI flag. This is not REL-03 tool work. This is not Daily/Research. This is not SDK/IDE/worktree.

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 planning symposium/packet, v0.4 review/MCP/skills/provider, v0.5 departments/RAG/compact/department symposiums, v0.6 parallel Builders, TUI park, or TeamCreate/SendMessage.

1. **REL-01 is a temp-dir lifecycle, not a packaged release.** Reuse `install.sh` (`KIANA_BIN` / `KIANA_SKIP_BUILD=1` / `KIANA_SKIP_PATH_SETUP=1` / `INSTALL_DIR`) and add `--uninstall` there so the personal installer is one script. Copy the upgrade/rollback/uninstall assertions from `scripts/package-lifecycle-smoke.sh`; do not run that file. Prefer the existing `target/debug/kiana`. Do not use `target/release/kiana` (stale 1.6M from 8/11). Do not write `~/.local/bin` as completion.
2. **REL-02 is a short real manual.** `USER.md` lists trust / run / symposium / packet / review plus the install lifecycle. Honest limits: parallel Builders have no new CLI; TUI stays parked; HTTP MCP is `mcp_transport_unsupported`; live provider is not completion.
3. **New gate is thin.** `scripts/v10-personal-lifecycle-smoke.sh` is the phase gate. It must not call `release-smoke.sh`, `package-lifecycle-smoke.sh`, `cargo fmt --all`, or a CLI rebuild.
4. **REL-03 is not this slice.** P0-LOOP…P0-PROV are already green from v0.4. Do not add tools. Do not claim the whole v1.0 product in this commit.
5. **Proof ceiling remains `local_behavior`.** Cassette + temp install. Not live, not physical, not production-install-ready, not signed artifacts.

Demo:

```text
temp INSTALL_DIR
  install → --version works
  upgrade (reinstall same binary) → checksum unchanged
  rollback (restore backup) → checksum matches first install
  recover (delete then reinstall) → --version works
  uninstall → binary gone; uninstall again is idempotent
USER.md names the real commands (run / symposium / packet / review)
```

## Requirements this phase

REL-01, REL-02 (thin). PATH/TRUST/SESS/EVD/ROLE/ORCH/SYMP/WB/REV/CODE-01..04/DEPT-02/MEM-01..04/LONG-02 still true as regression. REL-03 / REL-04 stay later.

## Frozen

- `scripts/release-smoke.sh` / `cargo fmt --all` as this phase's gate
- `scripts/package-lifecycle-smoke.sh` / dist tarball / enterprise offline manifest
- SBOM / signing / `~/.local/bin` production install
- REL-03 recode / expanding `kiana-tools` / P1-READ
- REL-04 Daily/Research as complete
- JointSymposium / staffing every COMPANY.md role / Librarian
- TeamCreate / SendMessage
- new CLI flags / formatting `cli.rs`
- worktrees / SDK / IDE / Desktop / Web
- live provider / unsupported_streaming
- HTTP/SSE MCP
- migrating TUI
- NOTICE/SBOM supply-chain as this slice
