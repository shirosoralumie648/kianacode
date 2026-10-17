# CP-23 Company transactions baseline

## Scope

Company mutations use the existing `ControlPlane` transaction boundary.  The
server derives `CompanyProof` and any `HumanDecision` from the authenticated
`RequestContext` and current Company revision, validates the decision binding,
transitions `CompanyState`, and commits the Company event through the protected
EventStore path.  A `CompanyCommandReceipt` is created only after that commit.

The decision binding covers actor, actor kind, role, session, command event
name and digest, expected revision, authority scope digest, and expiry.  A
caller cannot supply a proof, receipt, or model conclusion in the strict
`CompanyCommandRequest` payload.  `TransitionBatch` includes both the Company
aggregate and the authority read-set, so stale authority or Company revisions
cannot silently authorize a mutation.

## GitHub evidence

`cp23_company_policy.rs` exercises the accepted server decision and rejects
actor, role, session, command digest, revision, scope, expiry, missing-decision,
and unexpected-decision cases.  `cp23_company_transaction_guard.rs` checks the
source ordering and protected commit path, the request DTO boundary, and the
absence of a Company-specific Broker/model execution loop.

`.github/workflows/cp23-company-transactions.yml` runs these fixtures and
workspace test-target compilation on GitHub Actions.  Local runtime tests and
build commands are intentionally not part of this step.

## Limits

This is source plus remote-fixture wiring evidence.  It does not claim a
cross-process durable Company transaction, physical power-loss recovery, real
external business effect, live provider effect, or physical proof.  GitHub CI
results are not observed by the implementation branch; later closeout work must
promote proof only from recorded CI evidence.

## 2026-10-17 completion slice (CI-only)

The 2026-09-24 slice bound the server-created `HumanDecision` to the transaction
boundary.  This slice closes the rest of the card: every Company mutation class
is registered in one permission table, the two operator surfaces named by the
card (`memory.review` promote, `data.governance` revoke) join the same table,
and the three named acceptance fixtures now run against a real
`DaemonHost -> ControlPlane -> EventStore` path.

| Boundary | What changed |
|---|---|
| `kiana-domain/src/company_policy.rs` | New `CompanyMutationClass` (19 classes: assignment, packet claim/change, review, acceptance, delivery, closeout, objective, project, budget, change, run, risk, incident, artifact, outcome, handoff + `memory_promote`/`data_revoke`), `CompanyMutationPermission` with `allowed_roles`/`actor_kinds`/`operator_only`/`assignment_required`/`evidence_required`/`decision_purposes`/`preconditions`/`subject_denied_code` and a self-verifying digest, `company_mutation_permissions()`, `operator_mutation_permission(operation, action)`, `CompanyCommand::{mutation_class, mutation_permission}` and `CompanyMutationPermission::{authorize_policy, authorize_operator_context, authorize_operator_request}` |
| `command_policy_roles` | `ReclaimPacketClaim` now registers `["builder", "closer"]`.  Before this the class fell back to an empty role set, so `authorize_context` denied every subject and the documented expired-lease scan could never commit |
| `kiana-core/src/company.rs` | `handle_company_command` revalidates the class entry against the per-command policy before the Company state is read (`company_mutation_permission_drift`), so a drift cannot change any related object |
| `kiana-core/src/memory_proposals.rs` | `prepare_memory_review` consults the `memory.review/accept_proposal` entry and denies a packet cell or missing actor with `memory_review_operator_required` before reading the proposal |
| `kiana-core/src/data_governance.rs` | `prepare_data_revocation` consults the `data.governance/{delete,revoke,expire}` entry and denies a packet cell or missing actor with `governance_operator_required` before writing the invalidation fact |
| Domain fixtures | `kiana-domain/tests/cp23_company_policy.rs` 4 -> 9 fixtures: table coverage/validation, per-command drift, builder self-accept/self-promotion denial, reclaim subject registration, operator-surface subject denial |
| Core guard | `kiana-core/tests/cp23_company_transaction_guard.rs` 1 -> 3 guards: table markers + preconditions + ordering (permission gate before state read) + no second execution loop; named fixtures bound to the manual lane |
| Daemon fixtures | New `kiana-daemon/tests/cp23_company_transactions.rs` with the three card-named acceptance fixtures: `cp_builder_cannot_self_accept_or_promote_authority`, `cp_company_command_conflict_changes_no_related_object`, `cp_close_cannot_hide_unresolved_effects` |

The daemon fixtures drive the real command path: a Builder is denied project
approval, acceptance, budget, closeout and assignment promotion with
`company_role_denied` while no fact or revision changes; a stale revision and a
reused idempotency key conflict without touching any related object; and
`CloseProject` is refused for a missing acceptance, a missing delivery, a
prepared delivery and a delivered-but-unconfirmed handoff, committing exactly one
`company.ProjectClosed` fact once the chain is resolved.

`.github/workflows/cp23-company-transactions.yml` runs `cargo fetch --locked`,
`cargo fmt --all --check`, the domain fixture target, the core guard target and
the daemon target by fixture name (`--exact --test-threads=1`), plus `--no-run`
compilation of all three targets.  The lane stays `workflow_dispatch`-only with
`permissions: contents: read`; the unified `ci.yml` shards already claim
`kiana-domain-s2/4` (`cp23_company_policy`), `kiana-core-s2/6`
(`cp23_company_transaction_guard`) and the whole-crate `kiana-daemon` shard
(`scripts/ci/test-shards.json` unchanged).

No `Cargo.toml`/`Cargo.lock`/`package.json` change, no new dependency, no second
execution loop, and the five model-visible tools
(`shell`/`apply_patch`/`mcp`/`memory.search`/`memory.write`) are unchanged.

### Limits of this slice

- Local tests, build, check, clippy and smoke were not run (only `cargo fmt --all`);
  the CI receipt is not awaited and proof stays at `source`.
- Classes `risk` and `incident` are registered but keep an empty subject set, so
  their owning cards still decide the subjects and preconditions.
- Durable cross-process Company transactions, physical power-loss recovery,
  external business effects and live/physical evidence remain open.


