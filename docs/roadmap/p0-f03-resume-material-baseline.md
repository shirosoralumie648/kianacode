# P0-F-03 续跑材料落盘与 RunSnapshot 基线

> 快照日期：2026-09-18。本页回填 RunSnapshot、pending invocation 重建和显式 Resume；本地不运行测试，验收夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`P0-F-03`](../roadmap.md#step-p0-f-03) |
| feature_status | `implemented`（domain/core/runner/daemon approval source；CI-only source guard） |
| proof_level | `source`；本地只做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | EventLog RunSnapshot and invocation projection are the recovery facts; ControlPlane re-authorizes before restore/dispatch |
| this step does | approval wait persists redacted invocation/run snapshot material with runner-state digest, authority/data revisions and optional cell state; a fresh ControlPlane rebuilds pending invocation from the ledger, requires explicit Resume, rechecks scope/policy/gate/approval, claims stream CAS and restores the same Runner |
| this step does not | 不在 daemon 启动时自动恢复，不按 PID 复连，不把 snapshot/artifact 存在等同于外部 effect 成功；跨进程 projector/Power-loss reconciliation remains later PD/H24 work |

## 1. Contract

The snapshot is a resumable material, not a second source of truth. It is written only at an
approved pause boundary and includes a digest of the serialized Runner checkpoint plus authority,
data-epoch, role and scope identity. `resume_run` is an explicit request: it reconstructs the
pending approval from EventLog, validates the exact request and current approval material, claims
the observed run stream version, restores Runner state and leaves approval continuation to the
existing `resume_approved_invocation` → `drive_run` path.

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `fresh_process_resume_reconstructs_pending_approval` | a fresh core can rebuild pending invocation material from EventLog/projection rather than an in-memory map |
| `restart_pending_approval_waits_for_explicit_resume` | no startup auto-resume; missing snapshot/material fails closed and explicit Resume performs restore/CAS |
| `restored_pending_approval_rechecks_policy_gate_and_approval` | restored request is re-prepared and re-authorized against current scope/authority/data/approval before dispatch |

## 3. Proof ceiling and handoff

P0-F-03 proof ceiling is `source`: redacted RunSnapshot, checkpoint digest, pending reconstruction,
explicit resume, stream CAS and pre-dispatch revalidation are explicit. Crash-safe durable
cross-process Runner hydration, power-loss reconciliation, process/approval projector and live/
physical effect proof remain H24/H25/PD/ER/INT work.
