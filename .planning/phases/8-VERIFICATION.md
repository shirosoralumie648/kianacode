# v0.3 Phase 4 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass

## Demo contract

```bash
bash scripts/harness-golden-smoke.sh
INSTALL_DIR=/tmp/kiana-phase4-bin KIANA_SKIP_PATH_SETUP=1 KIANA_SKIP_BUILD=1 \
  KIANA_BIN=target/debug/kiana bash install.sh
bash scripts/v03-workbench-smoke.sh
```

Cassette path uses isolated git fixture + `$KIANA_HOME`. Anti-meeting writes `plan/DECISION.json` + `packet/TASK.json` without a model. Packet spawn and v0.2 Builder direct-write replay `scripts/fixtures/harness-golden-apply-patch.json`. TUI stays parked. Install proof is a temp `INSTALL_DIR`, not `~/.local/bin`.

## Evidence

| Criterion | Result |
|---|---|
| Fixture + cassette: symposium artifacts then file + receipt | `scripts/harness-golden-smoke.sh` — `plan/DECISION.json` `kiana.decision-record.v1`, `packet/TASK.json` `kiana.work-packet.v1`, `builder_present=false`, `skipped_meeting=true`; packet spawn writes `GOLDEN_PATH.txt` = `hello\n`; receipt `role_id=builder` `department_id=executing` `work_packet_id` matches packet id; `files_changed` includes `GOLDEN_PATH.txt` |
| Anti-meeting does not write src | smoke asserts `GOLDEN_PATH.txt` absent until packet spawn |
| v0.2 Builder direct-write still green | same script, second fixture, cassette `kiana run --sandbox workspace-write --json` |
| `install.sh` verifies demo commands | temp `INSTALL_DIR`; `--version`; `kiana run --help` contains `--symposium` and `--packet`; success text is v0.3 demo; `KIANA_BIN` / `KIANA_SKIP_BUILD=1` reuses existing binary |
| TUI remains parked | `kiana --help` contains `Parked in v0.2` and `not DaemonHost`; `kiana tui </dev/null` fails and does not print `kiana-harness` |
| Fail-closed on live claims / missing files | smoke unsets provider keys; dies on missing artifacts, `status≠completed`, Builder seated, `live provider` text |

## Commands run

```
bash -n scripts/harness-golden-smoke.sh
bash -n scripts/v03-workbench-smoke.sh
bash -n install.sh
bash scripts/harness-golden-smoke.sh
bash scripts/v03-workbench-smoke.sh
```

All listed commands passed. Binary: `target/debug/kiana`. Did not `cargo fmt --all`, did not run `scripts/release-smoke.sh`, did not run `scripts/provider-live-smoke.sh`, did not install to `~/.local/bin`.

## Not claimed

- live provider / physical readiness
- production install to `~/.local/bin`
- upgrade / rollback of a packaged release (v1.0 REL-01)
- TUI on DaemonHost
- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- v0.4 Reviewer≠author
