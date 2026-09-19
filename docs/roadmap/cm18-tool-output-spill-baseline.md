# CM-18 tool-output spill baseline

> Snapshot date: 2026-09-20. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-18`](context-memory.md#step-cm-18) |
| feature_status | `implemented` (domain spill/page contract over the existing H15 read path) |
| proof_level | `source`; local checks are formatting/diff hygiene only, CI fixtures are remote and not awaited |
| canonical path | bounded preview + typed `ExecutionOutputRef` → `ToolOutputSpill` identity → verified bounded `ToolOutputPage` |
| authority | spill/page references are evidence-only; ControlPlane/Broker still authorize `execution.output.read` |

## Contract and behavior

`ToolOutputSpill` records a bounded preview, complete-content digest/size, binary/text mode and
page size while binding the existing output reference to `run_id`, `turn_id`, capability ID,
source-scope digest and expiry. It never stores complete output bytes or grants read authority.

`ToolOutputSpill::page` checks the caller identity, source scope, complete content digest and
UTF-8/binary page boundary before returning a bounded `ToolOutputPage`. Stable cursors bind output,
turn, capability, source scope, content digest and offset; page and next-page cursors are digest
checked, so a foreign reference, tampered content or cursor cannot be reused.

## CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `huge_tool_output_is_bounded_before_buffering` | a large complete result exposes only a bounded preview plus truncation metadata |
| `foreign_run_reference_is_denied` | run and source-scope mismatches fail before page construction |
| `verified_page_retrieval_preserves_digest` | bounded pages preserve the complete digest and reject content tampering |
| `tool_output_spill_keeps_bounded_preview_and_scope_page_contract` | Core source guard keeps this as a domain contract, not a second execution path |

## Proof ceiling and handoff

The CM-18 ceiling is `source` plus remote CI wiring. H15 remains the daemon implementation of the
authorized output-read adapter; this step does not claim cross-process ArtifactStore durability,
retention/deletion propagation, MCP-wide spill migration, or live/physical effect proof.
