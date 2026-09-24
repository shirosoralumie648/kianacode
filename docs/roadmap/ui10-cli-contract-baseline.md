# UI-10 · CLI command and output normalization baseline

> Snapshot date: 2026-09-24. UI-10 source contracts and fixtures are executed by GitHub Actions;
> local tests, builds and checks are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-10`](ui-entrypoints.md#step-ui-10) |
| source snapshot | `2c21ef57` (latest `origin/master` before this slice) |
| feature_status | `implemented` (versioned command/argument/output contracts and client boundary guard) |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | CLI parser → `CliInvocation` → typed `KianaClient` facade → versioned protocol; authority remains server-owned |

The contract fixes the ten canonical commands (`run`, `status`, `events`, `approve`, `deny`,
`cancel`, `resume`, `receipt`, `export`, `session`) and keeps compatibility aliases at the parse
edge. Every invocation carries a workspace, optional session, request-bound command ID, output mode,
TTY/interactivity flags and bounded JSON arguments. Mutating commands reject implicit retries and
interactive requests without a TTY. Output validation rejects secret-shaped fields, oversized
payloads and ANSI escapes in JSON mode.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| command aliases and wire names | aliases normalize to one canonical command and stable operation name |
| workspace/session fences | missing workspace, required session, control characters and overlong values fail closed |
| interaction/retry policy | non-TTY interaction and implicit mutation retry are rejected before dispatch |
| redaction and output bounds | argument/output secret fields and oversized payloads are rejected; JSON cannot carry ANSI |
| entrypoint boundary guard | contract has no DaemonHost, ControlPlane, Broker, Harness, process spawn or model loop; CLI uses the client facade |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the client command fixtures,
the entrypoint source guard and `cargo check --workspace --tests --locked`. Local tests, builds,
checks, clippy and smoke commands are not run; CI results are not awaited.

Limitations: this slice does not move or rewrite the frozen `kiana-entrypoints/src/cli.rs`, does not
claim every legacy branch has migrated to the new contract, and does not prove protocol transport,
server authorization, durable session recovery, cross-surface presenter parity or live/physical
effects. Unknown sessions and command outcomes remain server responses; the client contract only
rejects malformed local input and never grants authority.
