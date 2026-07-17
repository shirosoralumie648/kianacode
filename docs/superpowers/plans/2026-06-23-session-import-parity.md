# Session Import Parity Implementation Plan

> **Execution rule:** This project does not use TDD. Implement the approved import contract first, then run the listed focused CLI, integration, and release verification. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Phase 1 session import parity so imported sessions restore both legacy JSON and the JSONL runtime event tree.

**Architecture:** Implement `kiana session import <path>` in the existing `kiana-commands/src/session.rs` command path. The command reads an exported JSON session, validates its `session_id` and `messages`, writes `<session_id>.json`, and rebuilds `<session_id>/events.jsonl` through the same session persistence helpers used by reply/fork.

**Tech Stack:** Rust 2021, serde_json, existing Kiana command registry, CLI integration tests.

---

## File Structure

- Modify `kiana-entrypoints/tests/cli_session.rs`: add focused integration verification for `kiana session import`.
- Modify `kiana-commands/src/session.rs`: add import argument parsing, validation, storage, usage, and command tests.
- Modify `kiana-entrypoints/src/cli.rs`: route `session import` through the local session command and document it in usage.
- Modify `docs/reference-migration-roadmap.md` and `docs/reference-feature-matrix.md`: update Phase 1 status after verification.

## Task 1: Import Command Parity

**Files:**
- Modify: `kiana-entrypoints/tests/cli_session.rs`
- Modify: `kiana-commands/src/session.rs`
- Modify: `kiana-entrypoints/src/cli.rs`
- Modify: `docs/reference-migration-roadmap.md`
- Modify: `docs/reference-feature-matrix.md`

- [ ] **Step 1: Implement the minimal import command**

In `kiana-commands/src/session.rs`, add an `import` match branch, parse `<path>` plus optional `--force`, read JSON, validate `session_id` and `messages`, reject existing sessions unless `--force` is set, remove any stale event tree on forced replacement, then call `write_session`.

- [ ] **Step 2: Route and document the command**

In `kiana-entrypoints/src/cli.rs`, include `import` in `session_main` routing and `session_usage`. Update `kiana-commands/src/session.rs` usage text with the same command.

- [ ] **Step 3: Add the focused CLI verification case**

Add a test named `session_import_restores_legacy_json_and_runtime_events` that writes an exported session JSON file, runs:

```bash
kiana session import /tmp/exported-session.json
```

and asserts that `<sessions_dir>/imported-session.json` and `<sessions_dir>/imported-session/events.jsonl` both exist with two message events linked by `parent_turn_id`.

- [ ] **Step 4: Run focused verification**

Run:

```bash
cargo test -p kiana-entrypoints --test cli_session session_import_restores_legacy_json_and_runtime_events
cargo test -p kiana-commands session::tests::
cargo test -p kiana-entrypoints --test cli_session
```

Expected: all pass.

- [ ] **Step 5: Verify release gate**

Run:

```bash
cargo fmt --all --check
bash scripts/release-smoke.sh
```

Expected: release smoke passes.
