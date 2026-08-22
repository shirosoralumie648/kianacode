# v0.2 Phase 4 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass (park)

## Demo contract

```bash
kiana --help | grep tui
# Parked in v0.2 (legacy SDK stream, not DaemonHost)
kiana tui </dev/null
# fail closed without a TTY; does not print harness: kiana-harness
```

v0.2 product path remains `kiana run` / print → `DaemonHost`. TUI is not deleted; it is not PATH evidence.

## Evidence

| Criterion | Result |
|---|---|
| Written park | README, DESIGN.md Phase 4 row, `tui.rs` module docs, `.planning/phases/4-CONTEXT.md` |
| Help names the park | CLI `--help` contains `Parked in v0.2` and `not DaemonHost` |
| Non-TTY TUI is not a harness run | `tui_without_tty_is_not_the_product_path` — no `kiana-harness` |
| No `kiana-tui` feature dump | crate untouched this phase |

## Commands run

```
cargo test -p kiana-entrypoints --test cli_run --locked -- tui_ --test-threads=1
```

2 passed (`tui_help_is_parked_off_the_v0_2_product_path`, `tui_without_tty_is_not_the_product_path`).

## Not claimed

- TUI on DaemonHost
- TUI cancel UX
- live terminal TUI session
- v0.3 departments / Symposium / RAG
