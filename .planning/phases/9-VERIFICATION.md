# v0.4 Phase 1 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass

## Demo contract

```bash
kiana trust .
kiana run --sandbox workspace-write --json -- "create GOLDEN_PATH.txt containing hello"
# capture session_id
kiana run --review "$SESSION" --json
# gate/REVIEW.json; role_id=reviewer; department_id=monitoring; session_id != author
```

Same-host proof is in-process DaemonHost: Builder cassette write, then review. Review does not call the model (`seen` unchanged). Reviewer tools are empty and sandbox is read-only.

CLI still uses a fresh `DaemonHost` per process. Review finds the author through durable events (`session_id` / `role_id` on `run.start` plus `run.receipt`), not by copying the Builder transcript.

## Evidence

| Criterion | Result |
|---|---|
| Catalog adds `monitoring/reviewer`; tools empty; sandbox read-only | domain `v0_3_catalog_has_planning_and_executing_roles` + `RoleSpec::reviewer()` |
| Reviewer session ≠ author session | core `review_author_run_uses_a_fresh_reviewer_session`; daemon `review_after_builder_uses_new_session_without_model`; CLI `review_after_builder_writes_gate_packet` |
| Same session as author fail-closed | core/daemon `review_author_session_denied`; packet validate `review_author_session_denied` |
| Author must be Builder | domain `review_packet_requires_builder_author`; core `review_builder_chair_fails_closed` / CLI `--role builder --review` → `review_role_must_be_reviewer` |
| Missing author receipt fail-closed | core/daemon/CLI `review_author_not_found` |
| Prompt / missing id fail-closed | CLI `review_prompt_conflict`, `review_author_required` |
| Writes `gate/REVIEW.json` `kiana.review-packet.v1` | core/daemon/CLI file + result schema `kiana.review-result.v1` |
| No model turn for the gate | daemon CapturingModel `seen` unchanged across review |
| Reviewer cannot `apply_patch` src | CLI `planning_reviewer_cannot_apply_patch_source` → `role_tool_denied` / `role_sandbox_read_only` |
| Disk receipts still isolate runs | daemon `disk_receipts_survive_restart_and_do_not_overwrite_the_first_run` |

## Commands run

```
cargo fmt -p kiana-domain -p kiana-protocol -p kiana-client -p kiana-core -p kiana-daemon
cargo test -p kiana-domain --locked --lib -- --test-threads=1
cargo test -p kiana-protocol --locked --lib -- --test-threads=1
cargo test -p kiana-client --locked -- --test-threads=1
cargo test -p kiana-core --test control_plane --locked -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked -- --test-threads=1
cargo test -p kiana-entrypoints --test cli_run --locked -- --test-threads=1
```

All listed targets passed (10 / 8 / 4 / 25 / 22 / 30). Did not `cargo fmt --all`, did not format `kiana-entrypoints/src/cli.rs`, did not run `scripts/release-smoke.sh` or live provider.

## Not claimed

- coding pack public-behavior matrix (`docs/coding-pack-matrix.md`, Phase 2)
- MCP client / skills-on-harness / provider live matrix
- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- migrating TUI
- using `kiana-tasks` ReviewPacket as the product path
- live provider / physical readiness
