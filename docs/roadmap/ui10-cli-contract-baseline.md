# UI-10 · CLI command and output normalization baseline

> Snapshot date: 2026-09-24, completion slice 2026-10-17. UI-10 source contracts and fixtures are
> executed by GitHub Actions; local tests, builds and checks are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-10`](ui-entrypoints.md#step-ui-10) |
| source snapshot | `2c21ef57` (first slice) plus the 2026-10-17 completion slice on branch `step/ui-10-20261017-1` |
| feature_status | `implemented` (command/argument contract, error catalogue, session fence, legacy migration and help text) |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | CLI parser → `CliInvocation` → typed `KianaClient` facade → versioned protocol; authority remains server-owned |

The contract fixes the ten canonical commands (`run`, `status`, `events`, `approve`, `deny`,
`cancel`, `resume`, `receipt`, `export`, `session`) and keeps compatibility aliases at the parse
edge. Every invocation carries a workspace, optional session, request-bound command ID, output mode,
TTY/interactivity flags and bounded JSON arguments. Mutating commands reject implicit retries and
interactive requests without a TTY. Output validation rejects secret-shaped fields, secret-shaped
text values, oversized payloads and ANSI escapes in JSON mode; invocation arguments apply the same
recursive text fence before dispatch.

## Completion slice (2026-10-17, CI-only)

The first slice only parsed commands and fenced secrets. The completion slice closes the card's
remaining exit conditions with four pure tables on the same client boundary
(`kiana-client/src/cli_contract.rs`):

| Table | Contract |
|---|---|
| `CLI_COMMAND_SPECS` | one row per canonical command: CLI token, wire name, summary, usage, primary typed operation, read-only follow-up operation, facade, session/retry/idempotency policy, bounded arguments and supported output modes |
| `CliArgumentError` | 27 stable codes with one remediation each; `validate_typed`/`validate` and the output validator map to the same strings as before, so no existing code was renamed |
| `CliSessionRef` | `latest` sentinel plus explicit ids; `resolve_session(known_sessions)` refuses ids the server-derived snapshot does not contain (`cli_session_unknown`) and returns `None` for `latest` because only the server picks the session |
| `CLI_LEGACY_FLAGS` + `normalize_legacy_args` | one-way legacy migration: `--session-id`→`--session`, `-c/--continue/-r`→`--resume`, `-p`→`--prompt`, `--print` dropped; transport/recording flags (`--sdk-url`, `--input-format`, `--output-format`, `--debug-file`, `--record-only`, `--replay-user-messages`, `--include-partial-messages`, `--fork-session`) fail closed with `cli_legacy_flag_retired` |

Command arguments are bounded per name (`run.prompt`, `status.detail`, `events.after_sequence`,
`events.follow`, `approve.approval_id`/`note`, `deny.approval_id`/`reason`, `cancel.reason`,
`resume.prompt`, `receipt.detail`, `export.format`/`since_sequence`, `session.action`). Missing,
unknown, wrongly shaped and oversized arguments fail closed with `cli_argument_missing`,
`cli_argument_unknown`, `cli_argument_invalid` and `cli_argument_oversized` before any invocation
reaches the client facade. `quiet` is supported exactly by `run`, `status`, `cancel` and `resume`;
commands whose server-owned payload is the result (`events`, `approve`, `deny`, `receipt`, `export`,
`session`) refuse it with `cli_output_mode_unsupported` instead of hiding the decision or stream.
`cli_help_text()` renders the same tables deterministically (`kiana.cli-command-contract.v1`); the
fixture's `help_shape` block pins the shape, so help text cannot become a second contract.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| command aliases and wire names | aliases normalize to one canonical command and stable operation name |
| workspace/session fences | missing workspace, required session, control characters and overlong values fail closed |
| unknown session | explicit ids the server snapshot does not contain fail closed; `latest` stays server-resolved |
| interaction/retry policy | non-TTY interaction and implicit mutation retry are rejected before dispatch |
| per-command arguments | missing, unknown, wrongly typed, non-enum and oversized arguments are refused |
| output modes | `quiet` only for `run`/`status`/`cancel`/`resume`; JSON refuses ANSI; TTY requires a TTY |
| redaction and output bounds | argument/output secret fields and secret-shaped text values are rejected; JSON cannot carry ANSI |
| command contract fixture | `command_specs_match_the_declared_contract_fixture` compares every row both ways and pins facade/operation per command |
| legacy migration | `legacy_args_rewrite_or_fail_closed_per_the_migration_table` replays every declared argv row; retired flags and attached values fail closed |
| legacy bounds | `legacy_normalization_bounds_and_passthrough_are_enforced` caps argv and notices and keeps `--` literal |
| entrypoint boundary guard | contract has no DaemonHost, ControlPlane, Broker, Harness, EventStore, process spawn, filesystem or model loop; entrypoints keep no second command table |

## Acceptance

* `kiana-client/tests/ui10_cli_contract.rs` — 12 fixtures: the three original deny-first fixtures
  plus `command_specs_match_the_declared_contract_fixture`,
  `help_text_covers_every_command_and_output_mode`, `argument_errors_follow_the_stable_catalogue`,
  `per_command_arguments_fail_closed_before_dispatch`, `output_mode_support_is_enforced_per_command`,
  `session_resolution_refuses_unknown_ids_from_the_server_snapshot`,
  `legacy_args_rewrite_or_fail_closed_per_the_migration_table`,
  `legacy_normalization_bounds_and_passthrough_are_enforced`,
  `typed_validation_maps_to_the_same_codes_as_the_string_api`.
* Declarative fixture: [`kiana-client/tests/fixtures/ui10-cli-contract.json`](../../kiana-client/tests/fixtures/ui10-cli-contract.json)
  (10 command rows, 27 error codes, 14 legacy flags, 12 argv migration rows, budgets, help shape,
  required markers, denied list and limitations).
* `kiana-entrypoints/tests/ui10_cli_boundary_guard.rs` — 2 guards (client boundary + fixture/lane
  binding).
* Manual lane: [`.github/workflows/ui10-cli-contract.yml`](../../.github/workflows/ui10-cli-contract.yml)
  (`workflow_dispatch`-only, `permissions: contents: read`, `cargo fetch --locked`,
  `cargo fmt --all --check`, the two test targets and one `--no-run` compile step).
* Automatic lanes: the unified `ci.yml` whole-crate shards `kiana-client` and `kiana-entrypoints`
  discover both targets (`scripts/ci/test-shards.json` is unchanged).

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the client command fixtures,
the entrypoint source guard and `cargo check --workspace --tests --locked`. Local tests, builds,
checks, clippy and smoke commands are not run; CI results are not awaited.

## Limitations

This slice does not move or rewrite the frozen `kiana-entrypoints/src/cli.rs`; the legacy print/SDK
branches still exist there and are only classified by the migration table. The contract rejects
malformed *local* input and never grants authority: session existence, approval validity, cursor
freshness and terminal state remain server facts (`resolve_session` needs a server-derived snapshot).
`quiet` is an exit-code-only contract; byte-level rendering belongs to UI-11. No local compilation or
test run happened, so compilation and behavior rely on GitHub Actions, and the proof level stays
`source` (no `local_behavior`, `durable`, `live` or `physical` promotion).
