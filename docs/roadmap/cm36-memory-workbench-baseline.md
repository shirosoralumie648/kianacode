# CM-36 User memory workbench and bulk operations baseline

> Snapshot date: 2026-09-23. Focused fixtures run in GitHub Actions only; local tests are intentionally not run.

| Item | Record |
|---|---|
| roadmap card | [`CM-36`](context-memory.md#step-cm-36) |
| feature_status | `implemented` (domain list, mutation-plan/result and export contracts) |
| proof_level | `source`; GitHub workflow is added, and its checks are not awaited |
| canonical path | server ACL `ReviewList` → redacted list/relations → expected-revision mutation plan → existing mutation authority → explicit per-item result/receipt → scoped redacted export |
| authority | workbench is a projection and plan only; mutations still require existing ControlPlane/MemoryMutation authorization |

## Contract and behavior

`MemoryWorkbenchList` derives entries from per-record `MemoryAclDecision`, drops denied records,
redacts bounded previews, omits private previews, and restricts similar/conflict relations to
records visible in the same list. Approval, rejection, publish, expiry and delete are represented
with the existing `MemoryMutationOperation` vocabulary; plans bind actor, scope, data/policy epoch,
idempotency keys and exact expected revisions. Operator-only mutations reject agent authority.

Atomic plans require one scope and reject partial success. Explicitly split plans report each
mutation outcome, but each named group must itself be all-success or all-failure/unknown. Unknown
effects remain `unknown`, never become a success claim. Export manifests bind scope, purpose,
recipient, redaction profile, epoch and row digests; user-layer rows cannot carry previews.
Delete is only a governed logical mutation intent. This step does not implement or claim physical
erasure, adapter execution, user-interface integration, or receipt persistence.

## GitHub CI fixtures

| Fixture | Assertion |
|---|---|
| `private_memory_is_redacted_in_list_and_export` | private rows have no preview; secret text is redacted; forged leaks are rejected |
| `denied_records_and_out_of_list_relations_are_not_exposed` | ACL-denied entries and relations outside the visible list are omitted |
| `bulk_review_rejects_partial_atomic_and_split_groups_but_reports_split_failures` | atomic and same-group partial outcomes are rejected; independent split-group failures are explicit |
| `bulk_plan_requires_current_target_revisions_and_reports_unknown_results` | missing expected revisions and agent approval fail; unknown stays unknown |
| `export_digest_and_rows_are_bound_to_scope_and_recipient` | bounded exports have per-row/content/scope/recipient/redaction digests |
| `memory_workbench_stays_redacted_acl_gated_and_outside_execution_authority` | source guard checks ACL, operator authority, CAS, mutation verbs, export binding and no direct storage/execution path |

Workflow: `.github/workflows/cm36-memory-workbench.yml`. GitHub-only argv:

```bash
cargo fmt --all --check
cargo test -p kiana-domain --test cm36_memory_workbench --locked -- --test-threads=1
cargo test -p kiana-core --test cm36_memory_workbench_guard --locked -- --test-threads=1
```

## Proof ceiling and limitations

The CM-36 ceiling is `source` plus CI wiring. No local tests, build, check, clippy or smoke command
was run; GitHub CI was not awaited. The source proves typed redacted list/plan/result/export
contracts only. No daemon handler or UI consumes these DTOs yet, and no mutation execution,
approval UI, durable bulk transaction, artifact export, physical deletion or recovery behavior is
claimed. A future adapter must use existing ControlPlane/MemoryMutation/EventStore paths and
provide real receipts before increasing the proof level.
