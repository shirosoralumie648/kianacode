# DEP-41 operator runbook and release handoff

This runbook is a controlled decision aid, not an execution bypass. Every mutating operation
still enters the `DaemonHost → ControlPlane → policy/gate/approval → capability broker` spine.
The operator must preserve the distinction between `feature_status` and `proof_level`; source or
CI evidence does not become durable, live or physical merely because a gate is green.

## 1. First response and read-only preflight

Capture the exact source snapshot and worktree before diagnosing a release, upgrade, backup,
restore, migration or rollout:

```text
git rev-parse HEAD
git status --short
git diff --check
```

Use the read-only status/health/doctor/preflight surfaces first. Record the redacted command argv,
cwd/environment, source snapshot, fixture or cassette and exit code. A missing cursor, stale
authority/data epoch, migration drift, unverified backup, active writer, capacity failure or
`result_unknown` is a block, not a healthy/empty result.

Local work in this repository uses formatting and workspace static compilation only. Test and
smoke execution belongs to GitHub Actions for this project policy; a local static check is not a
runtime receipt.

## 2. Decision tree

```text
request
  -> read-only status / health / preflight
  -> trust + identity + scope + authority/data epoch
  -> backup / migration / lease / capacity gates
  -> approval and idempotency recheck
  -> commit intent/fact through ControlPlane
  -> execute through the existing broker/adapter
  -> verify independent receipt or mark result_unknown
  -> reconcile before any retry, promote or cleanup
```

For release and upgrade, require the release manifest, Cargo.lock/source/target identity,
signature, SBOM, license and secret-scan evidence. For backup and restore, require a verified
manifest, source cursor/generation/epoch match, quarantine verification and an explicit activation;
never overwrite the active root. For migration, require the ordered registry, preflight, backup,
lease/fence, bounded steps and post-migration rebuild. For rollback, choose binary/data/effect
reconciliation explicitly and retain the old root until the retention gate permits deletion.

Promotion or retirement additionally requires target readiness/liveness, health-window evidence,
zero old writers/runs, old-revision fencing and a reviewer/approval decision. A failed canary,
expired progress deadline or uncertain stop stays paused/quarantined.

## 3. Live and physical boundary

The default path is fake/source-bound and offline. A live provider, external connector, OTLP
backend, target OS, Desktop package installation or physical side effect requires explicit
opt-in, isolated environment, non-secret credential reference, operator approval, independent
receipt, reconcile evidence, retention and cleanup. `scripts/oa28-live-handoff-preflight.sh` is a
preflight boundary, not proof that a live handoff occurred. Never use mock output, transcript,
model self-report or a CI artifact as a live/physical receipt.

## 4. Evidence block

Every status change must carry all fields below; omit no limitation:

```text
source_snapshot:
worktree_status:
command_argv:
cwd·environment:
fixture·cassette:
exit_code:
status change:
proof-level change:
limitations:
reviewer:
```

The capability/proof matrix in [`dep41-capability-proof-matrix.md`](dep41-capability-proof-matrix.md)
is a handoff index, not a second fact source. `CURRENT_STATUS.md` and precise source/CI evidence
remain authoritative for what is actually proved.

The current typed evidence contract index is: `ProviderLiveConnectionEvidence`,
`ContextMemoryGoldenPathEvidence`, `UiEvidenceBundle`, `CompanyLiveCloseoutEvidence`,
`ContainerLifecycleEvidence`, `OrchestratedRolloutEvidence`, `RolloutLifecycleEvidence`,
`SupplyChainReleaseEvidence` and `ReleaseUatEvidence`. These contracts bind claims to digests and
receipts, but their existence or CI fixture execution never upgrades a slice beyond the proof
level recorded in `CURRENT_STATUS.md`.

## 5. Stop conditions and handoff

Stop and request a decision when a fix would bypass ControlPlane, widen a capability intersection,
introduce a second execution loop, weaken a frozen boundary, or require credentials/external
spend. Otherwise record the blocker and the next safe action. Close a step only when the code,
CI gate, evidence block and limitations agree; do not infer project completion from this runbook.
