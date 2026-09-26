# DEP-41 capability and proof matrix

This matrix is a compact handoff index. It deliberately records partial/target/deferred/not_supported
states and proof ceilings; it does not override source code, exact CI receipts or `CURRENT_STATUS.md`.

| Slice | feature_status | proof_level | Current evidence | Hard limit / next gate |
|---|---|---|---|---|
| CAP-33 container / gVisor | partial | source | digest-pinned EnvironmentPort, labels, no-host fallback guard | no real runtime, gVisor receipt, durable inventory or restart recovery |
| DEP-27..31 migration contracts | partial | source | registry, preflight, bounded primitives, runner and rebuild guards | no durable EventLog runner, applied rows, projector rebuild or recovery receipt |
| DEP-32 rollback gate | partial | source | binary/data/effect decision and retained-root checks | no restore, writer fencing, external reconciliation or rollback effect |
| DEP-33..35 release/local rollout | partial | source | revision pin/drain, release preflight, local phase machine | no supervisor, backup/replace/promote effects or durable lease |
| DEP-36 container lifecycle | partial | source | root identity, env allowlist, SIGTERM, startup/readiness/liveness contract, `ContainerLifecycleEvidence` | no container harness receipt, traffic drain or cross-process cleanup |
| DEP-37 orchestrated rollout | partial | source | canary/blue-green/rainbow routes, target backend labels and `OrchestratedRolloutEvidence` | Kubernetes/generic orchestrator remains target; no live routing/fence |
| DEP-38 rollout lifecycle | partial | source | pause/resume/promote/rollback, health, retention, retirement and `RolloutLifecycleEvidence` | no durable rollout state, traffic effect, cleanup or live verification |
| DEP-39 supply chain | partial | source | artifact/checksum/signature/SBOM/license/secret/compliance gate and `SupplyChainReleaseEvidence` | no external signer, production artifact upload or installed Desktop package |
| DEP-40 cross-entrypoint UAT | partial | source | four-entrypoint scenario matrix, parity/spine fixtures and `ReleaseUatEvidence` | no durable cross-process E2E, real provider/account or physical/live UAT |
| H36 / CM-39 / UI-41 / CO-48 | partial | source | integration, memory, UI and CompanyOS handoff guards | fake/source evidence cannot prove live provider, business delivery or physical outcome |
| DEP-41 handoff gate | partial | source | this runbook, matrix, evidence validator and CI workflow | documentation cannot promote any dependent slice or close the roadmap |

| Typed evidence contract index | partial | source | Provider/Context/Memory/UI/CompanyOS and DEP-36..40 contracts are named above | contract presence and CI fixtures are not runtime, durable, live or physical receipts |

## Proof vocabulary

`source` means source/guard/static evidence only. `local_behavior` requires a reproducible local
behavior receipt; `durable` requires persisted facts and restart/replay proof; `live` requires an
independent external receipt; `physical` requires target-environment effect and cleanup proof.
No row may silently upgrade between these levels. `implemented` in a source document means only
that the source slice exists when the linked `CURRENT_STATUS` block says so; it never means
durable/live/physical.

## Required handoff fields

Each row must link or name `source_snapshot`, `worktree_status`, `command_argv`, `cwd·environment`,
`fixture·cassette`, `exit_code`, `status change`, `proof-level change`, `limitations` and
`reviewer`. The reviewer must be a concrete owner/reviewer statement, not a model self-report.

The GitHub-only validator parses every matrix row, requiring five non-empty columns, an allowed
`feature_status`/`proof_level` pair and a concrete evidence/next-gate cell; marker presence alone
cannot close the handoff.
