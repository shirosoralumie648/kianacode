# UI-40 release gate and evidence bundle baseline (partial)

UI-40 now has a CI-only source gate for deny/recovery/parity/performance evidence, source snapshots,
CURRENT_STATUS blocks, module-map links, diff checks and explicit limitations. It consumes the
CAP-34/H36/UI-39 boundaries without converting their partial rows into release success.

The protocol now provides `UiEvidenceCase` and `UiEvidenceBundle`. Each case binds a shared source
snapshot, command argv without raw secrets, fixture/environment digests, exit classification,
feature status, proof level, receipt/artifact references, limitations and reviewer; the bundle
rejects source drift and duplicate cases. Implemented cannot be source-only, a passed live case
requires live/physical proof, and skipped/failed/unknown cases retain limitations.

The gate does not run local tests or publish a release in this task. UI-32/33 recovery, UI-39 live
ACP/IDE, Desktop/runtime performance, live/physical Provider and unsupported combinations remain
not_supported or partial. The evidence bundle must retain command argv, fixture hashes, exit
codes, proof level and limitations.
