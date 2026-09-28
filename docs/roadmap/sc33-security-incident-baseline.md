# SC-33 security Incident / Vulnerability / Reconcile workflow baseline

> Snapshot date: 2026-09-28. This slice owns the *decision* that a named security condition was
> seen, that a named human owns it, that it has a deadline, and that every response step cites
> committed evidence in a fixed order. Local Cargo test/build/check/clippy/smoke commands are
> intentionally not run by the integration owner; GitHub Actions owns fixtures and the workspace
> gate.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SC-33`](security-compliance.md#step-sc-33) |
| code landing | `kiana-core/src/security_incident.rs`, registered by `kiana-core/src/lib.rs` |
| fixtures | `kiana-core/tests/sc33_security_incident.rs` (30 tests, deny-first, the two success paths last) |
| feature_status | `partial` for the source-level admission, ordering, evidence and closure contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | supplied facts -> `SecurityIncident::evaluate` -> `SecurityIncidentReport` -> `validate_against` re-derivation |

An incident here is not a ticket. It is the record that an *unknown external result*, a *suspected
secret leak* or a *supply-chain drift* was observed, that a named person owns it, and that every
transition cites a fact that is already committed. The three classes are closed on purpose: a
catch-all "something else went wrong" bucket would be a way to open an incident that is not
accountable to a severity ladder, a deadline and a reviewer, so the unknown direction is represented
explicitly by `SecurityIncidentClass::UnknownOutcome`.

## The ordering is the whole point

```text
Open --contain--> Contained --fence--> Fenced --reconcile--> Reconciled --close--> Closed (terminal)
```

Each step answers a different question. `contain` stops the bleeding, `fence` proves the old
capability can no longer act, `reconcile` resolves what the outside world actually did, and `close`
is the only step that may retire the incident. Two consequences are enforced in source rather than
documented:

* **No step may be skipped.** Closing an incident that was never reconciled would silently convert an
  unknown into a success, which is the exact failure the security constitution forbids
  (`security_incident_reconcile_required`, `security_incident_action_out_of_order`).
* **No step may be walked back.** `Closed` is checked before the ordering rules so that "reopen a
  closed incident" reports the real obstacle -- irreversibility -- rather than a generic bad step
  (`security_incident_state_irreversible`).

There is no `Skip`, `Force` or `Waive` action, and there is no `Replayed` status. Those are precisely
the escape hatches that let an unknown become a success, so they are not representable; duplicate
delivery is already caught by the ordering rule plus evidence single-use.

## Admission: what makes an incident accountable

| Field | Rule | Reason code |
|---|---|---|
| `owner` | non-empty, then length/secret-bounded | `security_incident_owner_required` vs `security_incident_owner` |
| `deadline_unix_ms` | non-zero and strictly after `opened_at_unix_ms` | `security_incident_deadline_required`, `security_incident_deadline_expired` |
| `evidence` | at least one, at most `MAX_SECURITY_INCIDENT_EVIDENCE` (32) | `security_incident_evidence_required`, `..._evidence_exhausted` |
| `class = SupplyChainDrift` | must carry a `SecurityVulnerability` | `security_incident_vulnerability_required` |
| severity | may not sit below the vulnerability's floor | `security_incident_severity_below_vulnerability_floor` |
| withdrawn advisory | may not back a `Low` drift | `security_incident_severity_below_vulnerability_floor` |

Emptiness is checked *before* the generic text guard on purpose. Otherwise `safe_text` would collapse
"nobody owns this" and "this owner string is malformed" into one code, and an operator could not tell
a missing owner from a corrupt one. The fixtures keep that distinction as two separate tests.

Evidence is a reference plus a digest plus a source digest, never a payload. That is what stops a
`SecretLeak` record from becoming the second copy of the secret, and it is why every evidence field
is length-bounded and run through the domain `redact_text` / `scan_secret_sentinels` helpers before
it can be sealed into a digest.

## Evidence is single-use

`SecurityIncidentState::consumed_evidence` records what earlier actions already spent. Two rules
follow, and they are the reason a single timeout cannot end up justifying a contain, a reconcile and
a close:

* an action may only cite evidence the incident itself carries -- a digest lifted from another
  incident is refused (`security_incident_evidence_not_in_incident`);
* an action may not re-cite anything already consumed (`security_incident_evidence_replay`), and
  citing nothing at all is refused as proving nothing (`security_incident_evidence_required`).

## Closure is a signed claim, not a status flip

`SecurityIncidentClosure` requires a root cause, an impact scope, the containment actions actually
taken, a residual risk statement and a **reviewer who is not the owner**
(`security_incident_closure_reviewer_conflict`). A closure is only accepted on a `Close` action and is
forbidden on the other three (`security_incident_closure_not_allowed`), so a containment step cannot
smuggle the closure fields in early. A closure with no containment actions is not a closure
(`security_incident_closure_containment_required`). Closing never deletes the underlying facts.

## The report cannot publish itself

`SecurityIncident::evaluate` returns a sealed `SecurityIncidentReport`. `validate_against` re-derives
the same decision from the same three inputs and refuses any report whose status, state, reason,
remediation or bound digests do not match (`security_incident_report_binding_invalid`). Two further
invariants make a rejection honest rather than decorative: a rejection always carries a reason and an
admission never does (`security_incident_report_reason_state_mismatch`,
`security_incident_report_remediation_with_outcome`), and an admitted report must actually advance
the phase (`security_incident_report_state_not_advanced`). A `report_digest` that does not cover its
own contents is refused with `security_incident_report_digest_mismatch`, so the seal cannot be
recomputed around an edit.

## Why this module has no side effects

The module appends no event, stores no state, calls no adapter and no port, and opens no file. It
cannot open an incident in a running system, fence a real lease, or talk to an external reconciler.
It decides whether a *proposed* action is admissible given an incident record and a supplied state.
This is the same read-only contract shape SC-32 uses, and it was chosen over wiring a second
execution path on purpose: `ControlPlane::handle_command` remains the only place a command becomes an
effect, and a caller that wants one must route through it and let that path record the fact. The
`Closed`-is-terminal rule, the evidence single-use rule and the reviewer separation are therefore
decisions this slice can make, while the *recording* of those decisions is still open work.

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` gate, which auto-discovers
`kiana-core/tests/*.rs`. No separate workflow was added, per the CI consolidation rule that a new
roadmap step must never multiply automatic CI fan-out.

This is a source-only decision over supplied records, states and timestamps. **It does not** append
an `incident.*` event, persist an incident, schedule a deadline, page anybody, fence a real
capability, verify a vulnerability against an advisory feed, or bind the closure to an approval. The
`derive` order is fixed and deny-first, and the 30 fixtures assert the refusal reasons rather than
any runtime effect. Two further gaps are named rather than papered over: the workflow is not yet
reachable from a command (no branch in `ControlPlane::handle_command`, which needs
`kiana-core/src/commands.rs`), and the vulnerability record is asserted by the caller rather than
resolved from a real advisory source, so a withdrawn-advisory check is only as good as the
`SecurityVulnerability` the caller supplies.
