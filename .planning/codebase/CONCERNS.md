# Concerns And Technical Debt

**Analysis Date:** 2026-07-13

## Executive Summary

Kiana is a large Rust workspace with active productization, workflow-runtime, trust, release, EDA, evaluation, and bounded-swarm work in flight. The primary codebase risks are not missing files; they are concentration of behavior in very large modules, a dirty worktree with many uncommitted feature surfaces, commercial-release checks that still distinguish local proof from external proof, and evolving contracts across CLI, app-server, MCP, scripts, and JSON schemas.

Use this document as a planning guardrail: preserve existing semantics, add focused tests before changing shared routing or trust surfaces, and prefer extracting stable submodules over adding more branches to existing mega-files.

## Highest-Risk Areas

### Large Entry-Point And Routing Modules

- `kiana-entrypoints/src/cli.rs` is the largest file in the workspace at roughly 25k lines; avoid adding new command behavior here without extracting command-specific handlers or tests.
- `kiana-entrypoints/src/runner.rs` centralizes prompt/session execution and is above 11k lines; changes can affect CLI, streaming, permissions, trust context, and tool orchestration at once.
- `kiana-commands/src/tasks.rs` is above 10k lines and contains task/workflow command behavior; new task lifecycle features should move into `kiana-tasks/src/` or narrower command modules.
- `kiana-commands/src/plugin.rs`, `kiana-tools/src/agent.rs`, `kiana-entrypoints/src/tui.rs`, and `kiana-tools/src/team_create.rs` are each large enough to hide cross-cutting regressions; add regression tests around the exact surface touched.

## Active Worktree Risk

The repository has a large dirty worktree spanning source, docs, schemas, and scripts. Treat the current tree as an active integration branch, not a clean baseline.

Representative modified or new areas:

- Modified command/runtime surfaces: `kiana-commands/src/lib.rs`, `kiana-commands/src/release.rs`, `kiana-entrypoints/src/cli.rs`, `kiana-entrypoints/src/runner.rs`, `kiana-tools/src/tool_execution.rs`, `kiana-types/src/trust.rs`.
- New command modules: `kiana-commands/src/audit.rs`, `kiana-commands/src/eda.rs`, `kiana-commands/src/eval.rs`, `kiana-commands/src/evidence.rs`, `kiana-commands/src/project.rs`, `kiana-commands/src/report.rs`, `kiana-commands/src/validate.rs`.
- New task/workflow modules: `kiana-tasks/src/evidence.rs`, `kiana-tasks/src/integrity.rs`, `kiana-tasks/src/project_board.rs`, `kiana-tasks/src/swarm.rs`, `kiana-tasks/src/workflow.rs`.
- New schema and design surfaces under `docs/schemas/`, `docs/superpowers/plans/`, `docs/superpowers/specs/`, and `docs/kiana_project_os/`.

Before claiming readiness, run verification against the live dirty tree and avoid relying on older roadmap language alone.

## Release And Commercialization Blockers

- `scripts/commercial-release-blockers-report.sh` explicitly computes `external_blocking` and `local_blocking`; do not bypass this script with README-level claims.
- `docs/commercial-release-readiness.md` distinguishes local workflow proof from actual release proof, clean tracked-tree evidence, signatures, channels, hosted services, and customer acceptance.
- `scripts/release-smoke.sh`, `scripts/package-release.sh`, `scripts/package-lifecycle-smoke.sh`, and `scripts/schema-contract-smoke.sh` are release gates; update all affected scripts and schemas together when output contracts change.
- New workflow/commercial schemas such as `docs/schemas/kiana-release-workflow-proof.v1.schema.json`, `docs/schemas/kiana-workflow-transition.v1.schema.json`, and `docs/schemas/kiana-workflow-completion.v1.schema.json` must stay synchronized with Rust emitters and smoke validation.

## Trust And Permission Surfaces

- `kiana-types/src/trust.rs` is a high-sensitivity module because project trust gates project hooks, skills, agents, MCP config, plugins, and mutating tools.
- `kiana-tools/src/permissions.rs`, `kiana-tools/src/bash_sandbox.rs`, `kiana-tools/src/bash_tool.rs`, and `kiana-tools/src/tool_execution.rs` combine policy decisions with execution; always include negative tests for bypass attempts.
- `kiana-query/src/stop_hooks.rs`, `kiana-skills/src/loader.rs`, and `kiana-skills/src/plugins.rs` depend on effective trust decisions; changes in trust persistence must be tested through these consumers, not only through `kiana-types`.
- Project-local files under `.kiana/` exist in the working tree; never allow project-local state to authorize its own trust or policy unless a test proves the intended fail-closed boundary.

## Workflow Runtime And Evidence Integrity

- `kiana-tasks/src/workflow.rs`, `kiana-tasks/src/integrity.rs`, and `kiana-tasks/src/evidence.rs` are central to DAG progression, authenticated event logs, verification packets, and evidence binding.
- `kiana-commands/tests/workflow_transition_command.rs` intentionally includes placeholder-evidence rejection cases; keep this class of test whenever changing completion, advance, or validation logic.
- Evidence paths must remain bounded and regular-file-only. Regressions here can invalidate release proof and recovery integrity claims.
- Prefer appending authenticated events through existing writer-lease paths instead of introducing ad hoc JSON writes.

## Integration And External Boundary Gaps

- Several commercial readiness checks are external by design: signatures, release channels, hosted service reachability, entitlement evidence, customer acceptance, and operational proof cannot be manufactured locally.
- `kiana-remote/`, `kiana-services/`, `kiana-chrome-mcp/`, and `kiana-computer-mcp/` touch external protocols and platform behavior; use focused smoke tests and fixture-based unit tests before live/manual validation.
- Network-facing tools should continue using shared policy validation from `kiana-tools/` and `kiana-services/`; do not add new direct HTTP clients without SSRF/private-target checks.

## Complexity Hotspots

Large Rust files by line count:

- `kiana-entrypoints/src/cli.rs` (~25k lines)
- `kiana-entrypoints/src/runner.rs` (~11.5k lines)
- `kiana-commands/src/tasks.rs` (~10.9k lines)
- `kiana-bridge/src/work.rs` (~7k lines)
- `kiana-commands/src/plugin.rs` (~5.8k lines)
- `kiana-tools/src/agent.rs` (~5.5k lines)
- `kiana-entrypoints/src/tui.rs` (~4.4k lines)
- `kiana-tools/src/team_create.rs` (~4.4k lines)
- `kiana-tools/src/lsp_tool.rs` (~3.7k lines)
- `kiana-tasks/src/workflow.rs` (~3.1k lines)

When adding functionality near these files, first search for an existing narrower module, then add tests near the affected crate before refactoring.

## Stub And Placeholder Signals

- The codebase includes explicit placeholder/stub checks in tests, especially workflow evidence validation; do not remove them as cosmetic cleanup.
- Search terms to re-run before release claims: `TODO`, `FIXME`, `HACK`, `stub`, `placeholder`, `not implemented`, `todo!`, `unimplemented!`, `panic!`, and unchecked production `unwrap()`.
- Many `unwrap()` calls appear in tests and fixture builders; production `unwrap()` in core execution paths should be audited file-by-file instead of counted globally.

## Recommended Fix Approach

### For New Features

- Add behavior behind a narrow module in the owning crate: commands in `kiana-commands/src/`, domain state in `kiana-tasks/src/`, policy/tool execution in `kiana-tools/src/`, shared contracts in `kiana-types/src/`.
- Add a focused test in the same crate before touching `kiana-entrypoints/src/cli.rs` or `kiana-entrypoints/src/runner.rs`.
- Update the matching schema under `docs/schemas/` and run `scripts/schema-contract-smoke.sh` whenever JSON output changes.

### For Refactors

- Extract one route or domain concept at a time from mega-files.
- Keep public CLI strings and JSON schema fields stable unless a migration is documented.
- Run focused crate tests first, then workspace/release smoke if the touched surface affects packaging or command routing.

### For Release Readiness

- Treat `scripts/commercial-release-blockers-report.sh --json` as the source of truth for blocker counts.
- Treat `scripts/release-smoke.sh`, `scripts/package-lifecycle-smoke.sh`, and `scripts/schema-contract-smoke.sh` as minimum proof, not optional polish.
- Do not claim commercial readiness while external proof categories remain unresolved.

## Watch List

- `kiana-entrypoints/src/cli.rs`: command dispatch, help output, runtime flag stripping, and JSON output compatibility.
- `kiana-entrypoints/src/runner.rs`: stream/non-stream execution, permissions, trust context, tool loop, and resident worker state.
- `kiana-commands/src/release.rs`: release proof and blocker report integration.
- `kiana-tasks/src/workflow.rs`: workflow DAG, EventLog, completion gates, verification packets.
- `kiana-tools/src/tool_execution.rs`: policy-enforced tool calls and auditability.
- `kiana-types/src/trust.rs`: fail-closed trust root, writer lease, pending marker, tombstone behavior.
- `scripts/commercial-release-blockers-report.sh`: local/external blocker classification.

---

*Concerns analysis: 2026-07-13*
