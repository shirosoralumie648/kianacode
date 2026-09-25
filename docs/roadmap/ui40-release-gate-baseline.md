# UI-40 release gate and evidence bundle baseline (source/CI boundary)

UI-40 now has a GitHub-only release gate for UI-32 deny, UI-33 recovery, UI-34 resource, UI-38
conformance and UI-39 opt-in evidence,
source snapshots, CURRENT_STATUS blocks, module-map links, diff checks, release-build/asset checks
and explicit limitations. It consumes the current UI-38 conformance and UI-39 opt-in boundaries
without converting their source rows into live or release success.

The evidence block records `source_snapshot`, `fixture`, `command_argv`, `exit_code`, `proof_level`
and `limitations` for every case; missing or drifted fields fail closed.

Evidence command arguments use a marker boundary scan for API keys, client/access/refresh tokens,
authorization, bearer, password, private key and secret forms, including hyphenated CLI flags.

The protocol now provides `UiEvidenceCase` and `UiEvidenceBundle`. Each case binds a shared source
snapshot, command argv without raw secrets, fixture/environment digests, exit classification,
feature status, proof level, receipt/artifact references, limitations and reviewer; the bundle
rejects source drift and duplicate cases. Implemented cannot be source-only, a passed live case
requires live/physical proof, and skipped/failed/unknown cases retain limitations.

`scripts/verify-ui40-release-evidence.sh` fails closed unless `GITHUB_ACTIONS=true`, checks the required evidence
references and rejects blanket live/physical completion claims. The workflow runs deny/recovery before
parity/resource/conformance, UI-39 protocol/client guards, the UI-40 evidence fixtures, and the existing
UI-36 release-build/asset gate. It does not run local tests or publish a release in this task. Desktop/
runtime performance, external provider/connector receipts, live/physical effects and unsupported
combinations remain not_supported or partial. The evidence bundle must retain command argv, fixture
hashes, exit codes, proof level and limitations.
The workflow path filter also includes current CM-36 `kiana-domain/src/memory_workbench.rs`, so a
fresh remote gate covers the repository-wide fmt dependency; that result is pending/unobserved.
