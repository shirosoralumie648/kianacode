# Local Company command API

The implementation is available through `RequestEnvelope::company_command` and the existing
`DaemonHost::handle` command transport. This is a local business record and execution interface;
it does not introduce another model loop, a remote tenant service, or an external delivery adapter.
The source was compiled without running tests. Runtime and durability claims require separate
verification; this document does not promote the roadmap's proof level.

Send a `CommandRequest` named `company.command.v1` with this argument shape:

```json
{
  "schema": "kiana.company-command.v1",
  "expected_revision": 0,
  "idempotency_key": "my-stable-command-key",
  "command": {
    "type": "register_artifact",
    "artifact_id": "charter-v1",
    "relative_path": "charter/CHARTER.md"
  }
}
```

`company.snapshot.v1` returns the current state and revision. The caller must send the returned
revision with its next mutation. A retry must retain the original command, expected revision,
idempotency key, role and session. A changed payload or stale revision is rejected. Requests use
the authenticated local principal and trusted workspace resolved by DaemonHost, with the role's
canonical department. Mutations require a non-safe permission profile. The API is also exposed
by `RequestEnvelope::company_snapshot` and `RequestEnvelope::company_command`.

Records use stable caller-assigned business IDs. Artifact references have the form
`artifact:charter-v1`; event evidence has the form `event:<RuntimeEvent.event_id>`. Registering
an artifact reads a relative, existing text file through the same protected artifact reader used
by Review. Its exact content is recorded as an immutable snapshot (nonempty UTF-8, at most
64 KiB, secret-bearing content rejected). Approval, Review, Delivery and close recheck referenced
workspace content. Changed files require registration under a new artifact ID and a fresh
business decision; they do not silently replace an earlier approved snapshot.

A minimal single-project cycle uses these command types:

1. `register_artifact` for the charter, then Sponsor `propose_objective`, `decide_objective` with
   `approve: true`, `propose_project`, `start_chartering`, `approve_project`. The Project needs
   an active Objective, charter artifact, budget reference, success criteria, non-goals, risk
   summary and a persisted decision reference.
2. PM `create_milestone`, `approve_packet`, `plan_project`. Packet write paths and acceptance
   criteria are frozen. Dependencies must already exist in the same project.
3. Builder `start_run` with project ID, packet ID and an optional sandbox. The command durably
   reserves the packet, then calls the existing `spawn_from_packet` path. Grant, Cell, sandbox,
   policy and approval enforcement remain in that path. Duplicate dispatch is rejected.
4. Continue or decide runtime approvals through the existing runtime endpoints. Use
   `reconcile_run` with the packet ID to import the latest persisted runtime outcome. A missing
   continuation or unconfirmed result does not cause automatic re-execution.
5. Register the produced artifacts, then Builder/Closer `request_acceptance`. All project
   packets must have completed; the acceptance snapshot includes the project's criteria and
   every frozen milestone and packet baseline. Evidence is linked to the real author Run.
6. A separate Reviewer session calls `record_review`, supplying one Boolean result for every
   exact criterion string and references to the author evidence. Reviewer/Sponsor then calls
   `decide_acceptance` with `accept`, `reject` or `waive`. Acceptance requires every criterion to
   pass. Rejection requires reasons. Waiver requires Sponsor authority and a decision reference
   and remains an explicit exception, not a successful business outcome.
7. After rejection, PM `rework_packet` registers a fresh replacement packet with the same
   acceptance, dependencies and write scope. Its fresh Builder Run can be reviewed again;
   the previous execution and rejected acceptance remain in the ledger.
8. Closer `prepare_delivery`, `approve_delivery`, `deliver`; the receiving local principal
   uses `confirm_delivery` with a handoff evidence reference. These commands record the local
   observed handoff. They do not send a message, upload a file or call an external service.
   DeliveryUnknown requires an Incident and `reconcile_delivery`; it cannot be retried blindly.
9. A session separate from both Builder and Reviewer calls `close_project`. The event includes
   a `kiana.company-closing-receipt.v1` record with project, acceptance, delivery, review, Run,
   artifact and evidence references. Unresolved Incidents block closure.
10. `record_outcome` accepts a finite metric observation only inside the Objective's frozen
    measurement window, after that window ends, with evidence. It calculates realization
    against the original target. Sponsor `achieve_objective` remains a separate decision and
    requires a Realized Outcome. Execution/Delivery/ClosingReceipt alone cannot achieve it.

The API also records Initiative assessment, ChangeRequest decisions, Risk and Incident
lifecycles, project pause/resume, and project cancellation/failure/archive. `request_cancel_project`
invokes the existing runtime cancellation method for its active Runs and leaves the Project in
CancelRequested until `confirm_cancel_project`. Unconfirmed stop needs an Incident and enters
ResultUnknown. Failed and Cancelled projects can only be archived; they are not resurrected.

Once a workspace has a Company Project, writing Runs and capabilities require a registered,
active packet reservation. The existing start, spawn, continue and approval execution paths
recheck that boundary and restore the frozen packet scope. Ad-hoc read-only inspection remains
available. Business state is folded from a principal/workspace EventStore stream using CAS;
there is no parallel Company database or mutable projection used as authority.
