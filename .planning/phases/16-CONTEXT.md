# v0.5 later Context — User-visible compact + resume-after-compact

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`
Requirements: LONG-02
Chosen slice: compact/resume, not SYMP-04 department symposiums

## Classify

Phase, not spike. User-visible completion is: when harness history exceeds
the compact trigger, a `run.compacted` fact is recorded and the run receipt
exposes `compact.applied=true` plus token counts and `summary_present`.
Same-host `continue` after that compact still completes and can write
`GOLDEN_PATH.txt`. Under-budget runs report `compact.applied=false`.

v0.6's open condition is "v0.5 resume 真能用". This slice is that resume
proof on the owned harness. It is not a new CLI flag. It is not
cross-process transcript restore. It is not model-written summaries.
It is not department symposiums, JointSymposium, or Librarian.

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 symposium/packet, v0.4
review/MCP/skills/provider, v0.5.1 five departments, v0.5.2 six-layer
memory, TUI park, or TeamCreate/SendMessage.

1. **Compact is a receipt fact, not a silent rewrite.** Existing
   `kiana-runner` compact already replaces over-budget history with retained
   user turns plus a Codex-style summary marker. This slice emits
   `RunnerEvent::Compacted` and copies it to the receipt as
   `kiana.compact.v1`. `kiana-query` token budget is not the product engine.
2. **Resume is same-host continue after compact.** P0-CONT already covers
   same-host continue. LONG-02 adds: continue still works *after* compact
   fired, including a later `apply_patch` write. Cross-process new host
   stays fail-closed (`session_not_found`). Do not persist the live
   transcript as a new product.
3. **Pause stays cancel.** No new pause command. `kiana run --cancel` /
   existing cancel path remains. Do not format `cli.rs`.
4. **Stub summary is honest.** Keep `(no summary available)` plus the
   Codex summary prefix. Do not call a second model to write the summary
   this slice. Token-after may still include the long prefix; shrinking is
   proven with a large discarded history, not a 20-token toy prompt.
5. **Product proof is DaemonHost.** Same-host tests, not a CLI compile.

Demo (same-host):

```text
trusted Builder + compact budget + over-budget prompt
  → receipt.compact.applied=true
  → tokens_before / tokens_after present
  → summary_present=true
  → tokens_after < tokens_before when discarded history is large
under-budget prompt
  → receipt.compact.applied=false
  → count=0
continue after compact + workspace-write
  → GOLDEN_PATH.txt appears
  → continue receipt still shows compact.applied=true
default Builder can still write GOLDEN_PATH.txt without compact
```

## Requirements this phase

LONG-02 (compact + resume-after-compact; pause remains cancel).
PATH/TRUST/SESS/EVD/ROLE/ORCH/SYMP/WB/REV/CODE-01..04/DEPT-02/MEM-01..04
still true. PATH-03 unchanged.

## Frozen

- JointSymposium / staffing every COMPANY.md role / Librarian
- SYMP-04 department symposiums (the other v0.5 later candidate)
- vector DB / kiana-query as compact engine / letta landing page
- model-written compact summaries
- cross-process live-session restore
- new CLI flags / formatting `cli.rs`
- expanding `kiana-tools`
- live provider / unsupported_streaming
- HTTP/SSE MCP
- migrating TUI
