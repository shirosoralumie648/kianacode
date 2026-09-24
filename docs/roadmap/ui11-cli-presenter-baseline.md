# UI-11 CLI JSON/TTY/exit code presenter baseline

> Snapshot date: 2026-09-24. The UI-11 presenter contract and deny-first fixtures are wired into
> GitHub Actions. Local tests, builds and checks are intentionally not run.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-11`](ui-entrypoints.md#step-ui-11) |
| source snapshot | `566b7db4` plus this UI-11 source slice |
| feature_status | `implemented` (pure JSON/TTY/quiet presenter, stable exit classes, signal and stream contracts) |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | server-owned `CliOutput` DTO → `kiana-client::present_cli_output` → adapter-owned stdout/stderr writes |

The presenter consumes the normalized UI-10 `CliOutput` only. JSON success is one bounded
`kiana.cli-output.v1` DTO on stdout; non-success JSON is the same structured DTO on stderr. TTY
output is a bounded, locale-aware disposable view and never becomes a fact source. Quiet mode emits
no stdout. Warnings remain in the DTO and are routed to stderr by the presenter.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| lifecycle exit table | `Completed` is 0; policy denied is 3; approval required is 4; cancelled is 130; `ResultUnknown` is 8 and never success |
| signal and pipe handling | SIGINT maps to 130 and broken pipe maps to 141 without invoking a process or pager |
| JSON stream contract | JSON contains only the versioned output DTO, rejects ANSI diagnostics, keeps errors on stderr and preserves the structured error field |
| TTY/no-TTY and pagination | TTY mode without a TTY is rejected; pagination is accepted only for a TTY and never for JSON/quiet mode |
| bounded human output | locale labels are presentation-only; warnings/errors are separated and TTY artifact output is truncated at a caller-supplied bound |
| redaction and warning bounds | secret-shaped, ANSI or overlong warnings fail closed before rendering |
| entrypoint boundary guard | presenter has no DaemonHost, ControlPlane, Broker, Harness, filesystem, network, process spawn or model loop |

## Exit-code table

| Code | Meaning |
|---:|---|
| 0 | accepted/completed presentation with no structured error |
| 1 | execution failure or unknown error class |
| 2 | invalid request or unsupported schema |
| 3 | policy/permission denial |
| 4 | approval required |
| 5 | conflict/not found class |
| 6 | capacity or budget limit |
| 7 | unavailable/timeout class |
| 8 | result is unknown and must be reconciled |
| 130 | cancelled or SIGINT |
| 141 | broken pipe |

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the client presenter
fixtures, the entrypoint source guard and `cargo check --workspace --tests --locked`. Local tests,
builds, checks, clippy and smoke commands are not run; CI results are not awaited.

Limitations: this slice is a pure output projection and does not migrate every legacy branch in the
frozen `kiana-entrypoints/src/cli.rs`, own OS signal/write handling, spawn a pager, or prove
cross-process protocol transport, server authorization, durable session recovery, Web parity,
provider/live timing or physical effects. Receipt correctness remains EventLog/ControlPlane-owned;
an exit code only reports the server-provided response classification.
