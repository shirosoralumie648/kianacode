# UI-11 CLI JSON/TTY/exit code presenter baseline

> Snapshot date: 2026-09-24, completion slice 2026-10-17. The UI-11 presenter contract and
> deny-first fixtures are wired into GitHub Actions. Local tests, builds and checks are
> intentionally not run; CI results are not awaited.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`UI-11`](ui-entrypoints.md#step-ui-11) |
| source snapshot | `566b7db4` (first slice) plus the 2026-10-17 completion slice on branch `step/ui-11-20261017-1`, based on `d570b789` |
| feature_status | `implemented` (JSON/TTY/quiet projection, stable exit table, adapter writer contract, bounded pagination and Web/CLI classification parity) |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | server-owned `CliOutput` DTO → `kiana-client::present_cli_output` → adapter-owned stdout/stderr writes (`write_cli_presentation`) |

The presenter consumes the normalized UI-10 `CliOutput` only. JSON success is one bounded
`kiana.cli-output.v1` DTO on stdout; non-success JSON is the same structured DTO on stderr. TTY
output is a bounded, locale-aware disposable view and never becomes a fact source. Quiet mode emits
no stdout. Warnings remain in the DTO and are routed to stderr by the presenter. Authority stays in
the server path: the presenter never opens a file, spawns a process or pager, installs a signal
handler, or re-runs a command to fetch "the rest" of a page.

## Completion slice (2026-10-17, CI-only)

The first slice rendered the three modes and fenced a few inputs. The completion slice closes the
card's remaining exit conditions with five presentation-only contracts in
`kiana-client/src/cli_presenter.rs`:

| Contract | Behaviour |
|---|---|
| `CLI_EXIT_CODE_TABLE` | one documented row per stable shell class (0, 1, 2, 3, 4, 5, 6, 7, 8, 130, 141) with wire name and meaning; the fixture asserts row-by-row that `CliExitCode::ALL`, `wire_name()`, `meaning()`, `parse_name()` and `CapabilityErrorCode::policy().cli_exit` still agree, so docs, help text and shells cannot drift from the code that produces the result |
| `CliExitCode::from_capability_error` | public now, but numerically identical to the private table it replaces; unknown `cli_exit` numbers still fall back to `ExecutionFailed` instead of success |
| `CliSignal` + `CliStreamFault` + `write_cli_presentation` + `exit_code_after_write` | signal names (`SIGINT`/`INT`/…, `SIGPIPE`/`EPIPE`/…) classify to stable codes without installing a handler; the writer contract stops at the first operating-system fault, never retries, never flushes or closes a caller-owned stream, and maps `EPIPE`→141 / interrupt→130 while an ordinary io fault keeps the response class (a denied command stays exit 3) |
| `CliPagerPlan` | bounded, line-aligned page split of the already bounded TTY body: pages concatenate back to the body byte-for-byte, a single line larger than the page budget keeps its own page instead of being cut mid-line, the footer budget is reserved before bounding the page text so the footer is always present, and `page_bytes` below `CLI_MIN_PAGE_BYTES`, a page budget that leaves no footer room, or more than `CLI_MAX_PAGE_COUNT` pages fail closed with `cli_pager_page_bytes_invalid` / `cli_pager_too_many_pages`. `page_text` returns `None` for page 0 or past the end, so an adapter cannot invent a page |
| `CliLocale` + `CliPresentation::stdout_is_pipe_safe` | locale values (`zh_CN.UTF-8`, `zh-Hans`, `C`, empty, garbled) only pick labels and fall back to English instead of gating a command; pipe safety proves stdout is either empty or exactly one JSON document without ANSI, so a log line cannot hide inside a JSON pipeline |

`CliPresentation` gained the `pager` field and `CliPresenterOptions` gained `page_bytes`
(default `CLI_DEFAULT_PAGE_BYTES`, `CLI_DEFAULT_TTY_BYTES` unchanged). Pagination is a presentation
contract, not a terminal control protocol: no pager process, no key read-back, no re-run.

## Failure-first fixture matrix

| Fixture / guard | Assertion |
|---|---|
| lifecycle exit table | `Completed` is 0; policy denied is 3; approval required is 4; cancelled is 130; `ResultUnknown` is 8 and never success; `completed` + a structured error is still non-zero |
| signal and pipe handling | SIGINT maps to 130 and broken pipe maps to 141 without invoking a process or pager; the second stream is never written after the first fault; an ordinary io fault never rewrites a denial |
| JSON stream contract | JSON contains only the versioned output DTO, one line with the declared key order, rejects ANSI diagnostics, keeps errors on stderr and preserves the structured error field verbatim |
| TTY/no-TTY and pagination | TTY mode without a TTY is rejected; pagination is accepted only for a TTY and never for JSON/quiet mode; pages reconstruct the bounded body and stay inside the page and body budgets |
| bounded human output | locale labels are presentation-only; warnings/errors are separated and TTY artifact output is truncated at a caller-supplied bound |
| redaction and warning bounds | secret-shaped, ANSI or overlong warnings/errors fail closed before rendering |
| pager budgets | page budgets below the minimum, budgets that leave no footer room and bodies that would need more than the page-count bound are refused |
| entrypoint boundary guard | presenter has no DaemonHost, ControlPlane, Broker, Harness, EventStore, EventLog, filesystem, network, process spawn, signal handler or model loop; the frozen `kiana-entrypoints/src/cli.rs` keeps no second presenter or exit table |

## Exit-code table

| Code | Meaning |
|---:|---|
| 0 | accepted/in-flight/completed presentation with no structured error |
| 1 | execution failure or unknown error class (fail-closed fallback) |
| 2 | invalid request or unsupported schema |
| 3 | policy/permission denial |
| 4 | approval required |
| 5 | conflict/not found class |
| 6 | capacity or budget limit |
| 7 | unavailable/timeout class |
| 8 | result is unknown and must be reconciled |
| 130 | cancelled or SIGINT |
| 141 | broken pipe |

## Acceptance

* `kiana-client/tests/ui11_cli_presenter.rs` — 10 fixtures: `the_documented_exit_table_matches_the_code_that_produces_it`,
  `lifecycle_scenarios_use_the_declared_exit_classes`,
  `signals_and_stream_faults_map_to_stable_codes_without_installing_handlers`,
  `locale_classification_is_presentation_only`,
  `exit_classes_follow_the_protocol_error_table_and_fail_closed`,
  `golden_outputs_match_the_declared_cases`, `denied_inputs_fail_closed_before_any_rendering`,
  `pager_plan_rejects_budgets_outside_the_contract`,
  `writes_stop_at_the_first_fault_and_never_hide_the_response_class`,
  `cli_and_web_agree_on_the_same_server_response_without_touching_authority`.
* Declarative fixture: [`kiana-client/tests/fixtures/ui11-cli-presenter.json`](../../kiana-client/tests/fixtures/ui11-cli-presenter.json)
  (11 exit-code rows, 11 lifecycle scenarios, 2 signals, 3 stream faults, 7 locales, 12 golden
  cases, a 12-row deny matrix, 11 Web/CLI parity rows, required/forbidden markers and limitations).
* `kiana-entrypoints/tests/ui11_cli_presenter_guard.rs` — 2 guards (pure projection + fixture
  required/forbidden binding, and fixture/fixture-name/lane/shard/USER.md/baseline wiring).
* Manual lane: [`.github/workflows/ui11-cli-presenter.yml`](../../.github/workflows/ui11-cli-presenter.yml)
  (`workflow_dispatch`-only, `permissions: contents: read`, `cargo fetch --locked`,
  `cargo fmt --all --check`, the two test targets and one `--no-run` compile step). The lane was
  deleted by `08552ada` when per-slice workflows were consolidated, while the 2026-09-24 evidence
  block still referenced it, so it is rebuilt here.
* Automatic lanes: the unified `ci.yml` whole-crate shards `kiana-client` and `kiana-entrypoints`
  discover both targets (`scripts/ci/test-shards.json` is unchanged).
* User-facing documentation: `USER.md` records the `--json`/`--quiet` contract, the same stable
  exit table and the pipe/pagination bounds.

## Evidence block

```text
source_snapshot / worktree_status / command_argv / cwd·environment /
fixture·cassette / exit_code / status change / proof-level change /
limitations / reviewer
```

GitHub Actions runs `cargo fetch --locked`, `cargo fmt --all --check`, the client presenter
fixtures, the entrypoint boundary/wiring guard and a `--no-run` compile of both crates. Local
tests, builds, checks, clippy and smoke commands are not run; CI results are not awaited.

## Limitations

* Local compilation and execution never happened for this slice: the presenter, fixtures and lane
  are correct only as far as static review and the CI run that follows prove.
* An exit code reports the server-provided classification only. It does not prove a receipt is
  correct, a session exists, an approval is valid or a terminal state is durable; those remain
  EventLog/ControlPlane facts, and an exit code never authorizes a retry.
* Pagination is a bounded page split of one already bounded TTY body plus a deterministic footer.
  It is not a pager: no user key read-back, no terminal control sequences, no re-run, and no
  proof about a real terminal's rows/columns.
* The JSON golden pins the wire key order that serde emits for the DTO; a field reorder is a real
  drift signal and fails the fixture rather than being tolerated.
* Parity covers terminal/policy responses: `stable_error_from_response` reports non-terminal
  statuses as failures, so UI-11 asserts the shared classification only where both surfaces
  describe a completed action outcome, and records that boundary instead of treating it as parity.
* The frozen `kiana-entrypoints/src/cli.rs` still owns the real entrypoint path and is not migrated
  to this presenter; the guard only forbids a second presenter or exit table from appearing there.
* `bash scripts/ci/validate-workflows.sh` needs ruby, which is not installed in the local session,
  so the lane was validated with a YAML parse instead; the CI workflow-structure job still runs the
  full check. Real PTY/pipe byte evidence, `--json`/`--quiet` behaviour on a live daemon and
  durable/live/physical proof remain open (UI-12/UI-31/UI-37 and later slices).
