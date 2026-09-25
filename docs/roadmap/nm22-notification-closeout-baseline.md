# NM-22 notification evidence and closeout baseline

> Snapshot date: 2026-09-26. This is a GitHub-only release/evidence gate. It is not run locally and
> does not wait for its CI result in this worktree.

The closeout gate checks the notification source baseline, NM-04..NM-21 baseline files, every
NM-00..NM-21 `CURRENT_STATUS` block, the NM-22 roadmap transition and explicit limitations. Remote
CI then runs Rust format, domain/core/entrypoint test targets, the desktop notification contract and
workspace test-target compilation.

The gate preserves the split: notification contracts and source adapters may be
`feature_status=implemented` while their `proof_level` remains `source`. It rejects missing
limitations and selected durable/live/physical overclaims. It does not claim durable read state,
cross-process delivery, provider/external ACK, live timing, OS physical delivery, or business
outcome correctness.

`feature_status=implemented`; `proof_level=source`.

Known limits: GitHub CI execution is configured but not awaited by this roadmap run; the gate is a
structural/source boundary, not a production release, durability, live, physical or UAT sign-off.
