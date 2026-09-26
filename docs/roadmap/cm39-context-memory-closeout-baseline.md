# CM-39 context/memory documentation and handoff baseline (partial)

CM-39 adds a CI-only documentation/source guard for the CM-38 handoff. It checks that the
context-memory roadmap, module map and CURRENT_STATUS evidence name the canonical memory broker,
retrieval/journal sources, fake/live proof ceiling, and open provider, cross-process, physical and
scale limitations. The guard is read-only and does not promote any feature.

The handoff now names `ContextMemoryGoldenPathEvidence` and
`ContextMemoryStageDigests` as the source contract for CM-38. Its scope/redaction, fake cassette,
live opt-in, stage completeness and limitation rules are linked from the module map and are
checked as source parity; the manifest is not a durable EventStore receipt and does not promote
live or physical proof.

The closeout remains partial until CM-33–37 and the durable/live evidence exist. This baseline is
the honest handoff: no undocumented provider, no cross-process recovery claim, no physical effect
claim, and no “skip means pass” interpretation. The CI guard also extracts the CM-38
`CURRENT_STATUS.md` block and requires all ten evidence fields plus the explicit source proof
ceiling, rather than accepting marker presence elsewhere in the ledger.
The workflow path filter also includes current CM-36 `kiana-domain/src/memory_workbench.rs`, so a
fresh remote format/source-guard run covers the repository-wide fmt dependency; that result is
pending/unobserved.
