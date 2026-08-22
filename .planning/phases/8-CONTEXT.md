# v0.3 Phase 4 Context — Eval + install

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`

## Classify

Phase, not spike. User-visible completion is: a fixture repo + cassette replay proves the v0.3 demo (anti-meeting symposium → WorkPacket → independent Builder write) and the v0.2 Builder write still works; `install.sh` installs a binary that `--version`s and whose `kiana run --help` names `--symposium` / `--packet`. TUI stays parked.

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 Phase 1–3, five departments, RAG, or TeamCreate/SendMessage.

1. **TUI stays parked (WB-03).** `kiana tui` remains legacy SDK/stream. It is not the product path and is not migrated this phase. Non-TTY `kiana tui` must fail closed and must not print `kiana-harness`.
2. **Eval is cassette `local_behavior`, not a live provider.** `scripts/harness-golden-smoke.sh` owns WB-01. It creates an isolated git fixture + `$KIANA_HOME`, runs:
   - `kiana trust .`
   - `kiana run --symposium --anti-meeting --sandbox workspace-write --json -- "create GOLDEN_PATH.txt containing hello"`
   - asserts `plan/DECISION.json` + `packet/TASK.json` (`kiana.decision-record.v1` + `kiana.work-packet.v1`, `builder_present=false`); anti-meeting does not write `GOLDEN_PATH.txt`
   - `kiana run --packet packet/TASK.json --sandbox workspace-write --json` with `KIANA_HARNESS_SCRIPT` apply_patch cassette
   - asserts `GOLDEN_PATH.txt` = `hello\n`, receipt `role_id=builder`, `work_packet_id`, files_changed
   - v0.2 regression on a second fixture: `kiana run --sandbox workspace-write --json -- "create GOLDEN_PATH.txt containing hello"`
   Fail-closed: missing files, `status≠completed`, Builder seated, live-provider claims. `scripts/provider-live-smoke.sh` is not a gate.
3. **Install / release smoke verifies demo commands.** Do not run full `scripts/release-smoke.sh` (it starts with `cargo fmt --all --check` and workspace tests; that is not this phase). WB-02 = `install.sh` copies a binary into a temp `INSTALL_DIR`, `--version` works, `kiana run --help` contains `--symposium` and `--packet`, and the success text is the v0.3 demo. Proof uses `KIANA_SKIP_PATH_SETUP=1` and does not write `~/.local/bin` as completion. `KIANA_BIN` / `KIANA_SKIP_BUILD=1` may reuse an existing debug/release binary; this phase does not require `cargo build --release`.
4. **Proof ceiling remains `local_behavior`.** Cassette + isolated home. Not live, not physical, not production-install-ready.

Demo:

```bash
bash scripts/harness-golden-smoke.sh
INSTALL_DIR=/tmp/kiana-phase4-bin KIANA_SKIP_PATH_SETUP=1 KIANA_SKIP_BUILD=1 \
  KIANA_BIN=target/debug/kiana bash install.sh
bash scripts/v03-workbench-smoke.sh
```

## Requirements this phase

WB-01, WB-02, WB-03. PATH/TRUST/ROLE/ORCH/SYMP still true as regression.

## Frozen

- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- `kiana-tools` new family
- migrating TUI onto DaemonHost
- `scripts/release-smoke.sh` / `cargo fmt --all` as this phase's gate
- `scripts/provider-live-smoke.sh` as completion
- v0.4 Reviewer≠author (opens after this phase is green)
