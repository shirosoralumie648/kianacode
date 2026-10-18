# PD-23 restore verification baseline

> Completion slice, 2026-10-18. Previous state (2026-09-27) was a source-only fact plus a
> read-only daemon check: `external_effects_reconciled` was a caller-supplied boolean, artifact
> refs only had to be non-empty/unique (never compared with `SnapshotManifest.files`), the daemon
> verifier returned `Ok` for any structurally valid fact regardless of `Verified`, and nothing
> connected a restored fact to DEP-23 activation. This slice closes those gaps and runs the four
> focused targets locally.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`PD-23`](persistence-data-layer.md#step-pd-23) (queue row 578) |
| source snapshot | branch `step/pd23-20261018-1`, baseline `a80330f5` |
| feature_status | `implemented` for the bounded local restore gate (manifest/artifact binding, receipt-derived effect evidence, `Verified` + activated readiness, host command intake) |
| proof_level | `local_behavior`; no durable/live/physical promotion |
| canonical path | sealed `SnapshotManifest` → `RestoreVerificationFact` → Domain contract → Core evidence/readiness → `DaemonHost` gate → DEP-23 `ActivationState` |

The production entry points are `DaemonHost::restore_gate`, `DaemonHost::restore_gate_after_restart`
and `DaemonHost::admit_restored_command`. They compose, in this fixed order:

```text
kiana_domain::bind_restore_artifact_refs  (manifest files <-> artifact refs)
kiana_core::verify_restore_effect_claims  (re-derive each claim from its receipt)
kiana_core::require_restore_ready         (Verified fact + ActivationState::ready())
kiana_core::admit_restored_command        (root/instance/fence/epoch intake)
```

## What changed in this slice

### 1. Artifact refs bound to manifest file hashes

`artifact_refs` entries now take the form `artifact:<relative_path>#<content_hash>` via
`kiana_domain::restore_artifact_ref`. `bind_restore_artifact_refs` refuses an invalid shape
(`restore_artifact_ref_invalid`), a path the manifest does not contain
(`restore_artifact_ref_unknown`), a hash that does not equal the sealed
`SnapshotFileSeal.content_hash` (`restore_artifact_ref_hash_mismatch`), and a manifest file with
no ref (`restore_artifact_ref_missing`). `verify_restore` calls it on every check, so a fact can
no longer pass by listing arbitrary non-empty strings.

### 2. Effect reconciliation replaces the `reconciled` boolean

The `external_effects_reconciled: bool` field is gone. `RestoreVerificationFact` now carries
`external_effect_claims: Vec<RestoreExternalEffectClaim>` (bounded at 64, unique by claim digest).
Each claim stores the observation digest/state, the resolution, the receipt digest, the approval
ref, the authority epoch and a self-digest. `validate()` refuses `Unknown` observations
(`restore_effect_claim_unknown`), contradictory `(observation, resolution)` pairs
(`restore_effect_claim_resolution_state_mismatch`), and unsealed/out-of-range values
(`restore_effect_claim_invalid`); a `NeedsReconciliation` fact cannot carry claims.

Core derives a claim only through `restore_effect_claim_from_receipt`, which re-validates the
receipt against its request, validates the full observation, requires an admitted receipt and a
matching observation digest/authority epoch, and refuses `Unknown` before the claim exists.
`verify_restore_effect_claims` then requires every claim in the fact to have exactly one supplied
`(request, receipt, observation)` triple and to be byte-identical to the re-derived claim
(`RestoreEffectClaimError::{Invalid, Unknown, MissingReceipt, ReceiptMismatch, Duplicate}`).
A caller can no longer assert reconciliation; it must present the receipt the control plane
already issued.

### 3. `Verified` is a precondition, not a label

`RestoreVerificationFact::require_verified` / `require_verified_restore_verification` and
`kiana_core::require_restore_ready` refuse `Blocked` and `NeedsReconciliation` with
`restore_verification_not_verified`, then refuse a non-ready `ActivationState` with
`restore_activation_not_ready` (bad state shape: `restore_activation_state_invalid`). The daemon
gate cannot be satisfied by matching command fields alone.

### 4. Activation wired into `DaemonHost`

`DaemonHost::restore_gate_after_restart` re-runs the same structural/evidence checks and then
requires `ActivationState::ready()`, so a restart that reads a persisted `prepared` state refuses
with `restore_activation_not_ready` rather than silently admitting commands.
`DaemonHost::admit_restored_command` performs the gate first and only then delegates to
`kiana_core::admit_command_after_activation`; the superseded root is refused with
`restore_activation_superseded_root_write`. The daemon does **not** mint leases or fences:
DEP-23 `restore_activation` remains the only authority, and a source guard asserts the daemon
verifier contains no `OperationLeaseCas`, `OperationLease::` or `FenceTokenId::new`.

### 5. Projector / ACL read-model limitation (explicit)

No projector or ACL read-model check is performed by this slice. `source_cursor`,
`projection_cursor`, `projection_digest` and `pending_approval_count` are caller observations
validated for internal consistency (cursor equality, digest shape, digest self-consistency); the
module never reads a projector or ACL store. Likewise the daemon verifier copies no files,
acquires no lock, fences no process, revives no trigger and does not cut the root over. A
projector/ACL comparison remains open and is not claimed as part of this card's evidence.

## Failure-first fixture matrix

Deny-first: every refusal test precedes the corresponding success fixture.

| Target | Fixture | Assertion |
|---|---|---|
| domain (7) | `artifact_refs_must_cover_the_manifest_files_with_matching_hashes` | missing/unknown/mismatched/hash-drift refs refused with `restore_artifact_ref_*` before the valid binding |
| domain | `unknown_effect_cannot_be_sealed_as_reconciled_evidence` | `Unknown` claim refused (`restore_effect_claim_unknown`) even after `recompute_digest` |
| domain | `duplicate_effect_claims_and_unsealed_claims_fail_closed` | duplicate digest, bad shape, contradiction |
| domain | `needs_reconciliation_cannot_carry_external_effect_claims` | claims are not allowed to masquerade as pending work |
| domain | `old_instance_fence_and_shape_tampering_fail_closed` | tampered digest/shape/version |
| domain | `blocked_and_reconciliation_facts_are_not_ready_for_activation` | readiness refused for non-`Verified` |
| domain | `verified_restore_requires_new_epochs_fences_and_quiescent_references` | success fixture last |
| core guard (5) | `restore_verification_keeps_fences_epochs_and_pause_visible` | source markers + no effect execution + daemon second-authority negative assertions |
| core guard | `readiness_gate_requires_a_verified_fact_and_an_activated_state` | `restore_verification_not_verified`, `restore_activation_not_ready` |
| core guard | `effect_claims_are_rederived_from_receipts_and_unknown_is_never_reconciled` | receipt mismatch and forged `Unknown` refused |
| core guard | `a_claim_without_its_receipt_is_refused` | `MissingReceipt` |
| core guard | `command_intake_is_refused_until_the_root_is_activated` | prepared/needs-reconciliation/superseded-root refusals before the activated success |
| core DEP-23 (20) | existing `dep23_restore_activation` fixtures | activation state machine unchanged and still passing |
| daemon (9) | `missing_unknown_and_mismatched_artifact_refs_are_refused` | manifest binding matrix |
| daemon | `blocked_and_needs_reconciliation_facts_never_pass_the_ready_gate` | readiness refusal at host gate |
| daemon | `unknown_effect_cannot_be_claimed_as_reconciled_evidence` | Unknown refused at claim, fact and gate levels |
| daemon | `evidence_that_does_not_match_its_claim_or_receipt_is_refused` | missing receipt, foreign observation, forged approval ref |
| daemon | `a_prepared_activation_never_admits_commands_after_restart` | restart defaults to `prepared` refusal |
| daemon | `external_receipt_is_never_accepted_without_a_provider_observation` | no observation -> no claim |
| daemon | `host_command_intake_never_admits_commands_before_activation` | prepared / superseded root refused, activated root admitted |
| daemon | `an_empty_effect_claim_list_is_only_valid_when_nothing_is_unknown` | empty claims valid only as vacuous truth |
| daemon | `verified_fact_with_bound_evidence_and_activated_state_passes_the_host_gate` | success fixture last |
| daemon guard (1) | `daemon_restore_verifier_only_checks_manifest_and_replacement_fence` | no filesystem/lock/activation/EventLog effects |

## Evidence

All four required targets were run locally, offline, single-threaded, with
`CARGO_TARGET_DIR=D:/code/kiana-steps/target-pd23`:

| Command | Result |
|---|---|
| `cargo test -p kiana-domain --test restore_verification --offline -- --test-threads=1` | exit 0, 7 passed |
| `cargo test -p kiana-core --test restore_verification_guard --offline -- --test-threads=1` | exit 0, 5 passed |
| `cargo test -p kiana-core --test dep23_restore_activation --offline -- --test-threads=1` | exit 0, 20 passed |
| `cargo test -p kiana-daemon --test pd23_restore_verifier --offline -- --test-threads=1` | exit 0, 9 passed |
| `cargo test -p kiana-daemon --test pd23_restore_verifier_guard --offline -- --test-threads=1` | exit 0, 1 passed |
| `cargo check -p kiana-domain -p kiana-core -p kiana-daemon --tests --offline` | exit 0, 0 errors |

CI: `.github/workflows/pd23-restore.yml` is `workflow_dispatch` only, `permissions: contents: read`,
and runs `cargo fetch --locked`, `cargo fmt --all --check`, five `--no-run` compiles and the five
targets above with `--test-threads=1`. The run was not triggered and its result is not awaited.

## Limitations

- No projector or ACL read-model verification (see item 5); cursor/digest values are caller
  observations.
- No real backup restore, fsync/hash scan over the restored bytes, multi-process lock handoff,
  trigger revival or active-root cutover; the gate is metadata and evidence only.
- No durable replay of effect claims: they live in the fact handed to the gate, not in an event log
  projection.
- `admit_restored_command` returns `Result<(), String>` to preserve Core's refusal codes; callers
  must not treat a refused command as retryable without re-reading activation state.
- Local evidence is `--offline --test-threads=1` on Windows; default parallel runs, workspace
  tests, clippy and smoke scripts were not run.
