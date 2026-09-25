# UI-41 UI/Entrypoints handoff baseline (source handoff boundary)

The UI handoff records the current evidence ceiling rather than a blanket completion claim. The
current source snapshot is `f04d886f` plus this UI-41 handoff slice; the GitHub UI-40 gate and its
`UiEvidenceBundle` are the evidence input, not a replacement for reviewer judgement:

- implemented/source-gated: versioned protocol/handshake/instance shapes, common DaemonHost
  routing, CLI/Workbench/Web/Desktop parity inputs, UI-38 conformance, UI-39 opt-in source
  boundary and UI-40 evidence gate;
- partial: UI recovery/performance/accessibility, Desktop package behavior, ACP/IDE live opt-in,
  cross-entry runtime receipts and provider/live handoff;
- deferred/not_supported: unprovided live ACP/IDE host, external credentials, physical effects,
  durable cross-process reconnect and scale/benchmark evidence.

The canonical next actions are the open UI cards, CAP/ER/H/provider dependencies and the
target-specific live handoff process. A reviewer must use the exact source snapshot, CI run and
CURRENT_STATUS evidence blocks; no transcript or model statement upgrades proof level.

The handoff now points to the typed `UiEvidenceCase`/`UiEvidenceBundle` contract from UI-40.
Each classification remains separate from proof level and keeps reviewer, source snapshot,
receipt/artifact references, exit code, limitations and next actions visible. The bundle is a
source/fixture handoff aid, not a runtime, durable, live or physical acceptance record.

## Status matrix and next actions

| scope | feature_status | proof_level | evidence | next_action |
|---|---|---|---|---|
| UI-26–38 entry contracts, parity, deny/recovery and conformance | implemented | source | UI-26–38 baselines and focused CI fixtures | observe GitHub results; add runtime/durable evidence only with matching receipts |
| UI-39 ACP/IDE opt-in | implemented | source | `LiveAcpOptIn`, `LiveAcpSession`, local preflight and CI fixtures | approved external host/session, permission timeout, reconnect and receipt cassette |
| UI-40 release/evidence gate | implemented | source | deny→recovery→parity/resource/conformance workflow and `UiEvidenceBundle` | preserve CI run IDs, exit codes and reviewer decision; do not promote source-only cases |
| UI runtime, cross-process durable recovery, desktop/browser/PTY performance | partial | source | limitations in CURRENT_STATUS and UI-40 bundle | run scoped CI/UAT fixtures with real environment and bounded metrics |
| external provider/connector/live/physical effect | not_supported | source | no authorized target or external receipt | obtain explicit operator approval, isolated target, receipt, reconcile and cleanup evidence |

Required handoff fields are `source_snapshot`, `feature_status`, `proof_level`, `reviewer`,
`receipt_digest`, `limitations` and `next_action`; missing or blanket-completion language is a
handoff failure.
