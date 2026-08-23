# v1.0.1 Verification — Personal install lifecycle + USER.md

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass
Requirement: REL-01 (temp INSTALL_DIR install/upgrade/rollback/recover/uninstall) + REL-02 (thin USER.md)
Chosen slice: personal installer lifecycle and a real manual, not packaged tarball / SBOM / REL-03 recode

## Demo contract

```text
temp INSTALL_DIR
  install → --version works
  upgrade (reinstall same binary) → checksum unchanged
  rollback (restore backup) → checksum matches first install
  recover (delete then reinstall) → --version works
  uninstall → binary gone; uninstall again is idempotent
USER.md names the real commands (run / symposium / packet / review)
```

Install proof uses `install.sh` with `KIANA_BIN` / `KIANA_SKIP_BUILD=1` /
`KIANA_SKIP_PATH_SETUP=1`. Uninstall is `install.sh --uninstall` (idempotent).
Binary: `target/debug/kiana`. Temp dir is not `~/.local/bin`.

## Evidence

| Criterion | Result |
|---|---|
| USER.md names real commands | smoke asserts trust / run / symposium / packet / review / install / uninstall, plus TUI park, HTTP MCP fail-closed, live-provider honesty |
| install → --version | temp `INSTALL_DIR`; `install.sh` copies `target/debug/kiana`; `--version` works |
| upgrade checksum unchanged | reinstall same binary; sha256 matches first install |
| rollback restores first install | backup copy restored; sha256 matches first install; `--version` works |
| recover delete-then-reinstall | binary removed then `install.sh` again; `--version` works |
| uninstall idempotent | `install.sh --uninstall` removes binary; second uninstall still absent |
| No CLI format / no kiana-tools | Did not format `cli.rs`; did not expand `kiana-tools`; did not rebuild CLI |

## Commands run

```
bash -n install.sh
bash -n scripts/v10-personal-lifecycle-smoke.sh
bash scripts/v10-personal-lifecycle-smoke.sh
```

All listed commands passed (EXIT:0). Binary: `target/debug/kiana`. Did not
`cargo fmt --all`, did not format `kiana-entrypoints/src/cli.rs`, did not
compile the CLI crate, did not run `scripts/release-smoke.sh`,
`scripts/package-lifecycle-smoke.sh`, or live provider.

## Not claimed

- `~/.local/bin` production install
- packaged tarball / enterprise offline manifest
- SBOM / signing / NOTICE supply-chain as this slice
- REL-03 recode (P0 already green from v0.4; closeout later)
- REL-04 Daily/Research
- live provider / `unsupported_streaming`
- HTTP / SSE / WS MCP
- migrating TUI
- worktrees / SDK / IDE / Desktop / Web
- JointSymposium / staffing every COMPANY.md role / Librarian
- TeamCreate / SendMessage
- new CLI flags / formatting `cli.rs`
- expanding `kiana-tools`
- whole v1.0 product
