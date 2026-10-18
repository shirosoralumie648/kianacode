# CO-13 versioned department packet baseline

## Scope

CO-13 adds a separate versioned contract for analysis, assessment, Charter/Plan proposal,
implementation, verification, delivery preparation and knowledge capture work. The existing v1
`WorkPacket` remains the Builder execution adapter and keeps its `packet_role_must_be_builder`
semantics.

## Implemented source slice

- `DepartmentPacket` and `ResultContract` carry a versioned kind, responsible department/role,
  assignment reference, typed Intake/Initiative/Charter/Plan basis, bounded input refs, output
  schema and controlled write scope.
- Each input basis binds exactly one approved reference: the Plan basis freezes the plan node and
  revision through `plan_binding` (the approved Charter baseline travels inside that binding), the
  Charter basis names the approved Charter baseline version, and Intake/Initiative carry no
  approved reference at all. A basis that carries a reference it must not
  (`department_packet_plan_reference_mismatch`) or lacks the one it needs
  (`department_packet_charter_baseline_required`) is refused.
- Analysis/planning packets can be proposed without an approved Plan where their input basis
  permits it; implementation packets require a Plan reference and a non-empty write set. Paths are
  normalized and restricted to the kind's controlled roots (`allowed_write_path` accepts the root
  directory itself and everything below it; `.` means the whole workspace).
- The admission transaction `CompanyCommand::AdmitDepartmentPacket` (event `DepartmentPacketAdmitted`,
  `CompanyState::department_packets`) is one ControlPlane transaction.
  `CompanyState::department_stage_facts` derives the approval stage from the aggregate's own
  records (intake provenance, Initiative assessment lane, approved Charter baseline, frozen Plan
  revision; a terminal project admits nothing), `CompanyState::department_role_appointment` derives
  the target-role appointment from the admitted CO-05 assignment, and the shared
  `department_packet_admission_verdict` revalidates contract, stage identity, appointment
  identity/activity and the input-basis gate before the transition.
- Admission writes one digest-bound `DepartmentPacketAdmission` with `runtime_authority: false`;
  repeating the exact contract is idempotent, a different contract under the same packet id is
  refused (`department_packet_already_admitted`), and the evidence records the approved Charter
  baseline the admission was actually checked against (from the plan binding for a Plan basis).
  `DepartmentPacketAdmission::validate` refuses a basis/plan-digest/baseline combination that
  cannot occur.
- Explicit `runtime_grant` and `budget_lease` compatibility fields are rejected, so a business
  packet cannot smuggle runtime authority. Actual Grant/Lease derivation remains in ControlPlane.
- The admission grants nothing: no Milestone, WorkPacket, PacketClaim, Run, budget or lease is
  created, and the project revision does not move. The CO-17 handoff digest now covers the complete
  contract (`charter_baseline_version`, `plan_binding`).
- The old Builder packet is unchanged and is covered alongside the new contract by CI fixtures and
  a Core source guard.

## Acceptance fixtures

Deny-first fixtures all live in `kiana-domain/tests/co13_department_packets.rs` (card success
fixture last):

- `department_packet_cannot_smuggle_builder_write_scope_or_runtime_grants` (card)
- `department_packet_contract_refuses_role_department_and_input_basis_drift`
- `department_packet_admission_refuses_unapproved_stage_and_inactive_appointment`
- `department_packet_admission_refuses_drifted_plan_binding`
- `admitted_department_packet_returns_digest_bound_evidence_without_runtime_authority`
- `analysis_and_implementation_packets_validate_through_versioned_contracts` (card, success)

Real-spine fixtures in `kiana-daemon/tests/co13_department_packet_transactions.rs` drive
`DaemonHost -> ControlPlane -> EventStore`:

- `co13_department_packet_admission_is_one_control_plane_transaction` (card, deny-first)
- `co13_department_packets_are_idempotent_and_refuse_a_changed_contract` (card, success)

The Core source guard `kiana-core/tests/co13_department_packets_guard.rs` (2 tests) pins the
markers, the rejection-code matrix, the basis contract, the fixture names and deny-first order,
the ControlPlane gate running before the transition, the manual lane commands and the automatic
shard bindings.

## CI-only evidence

`.github/workflows/co13-department-packets.yml` runs locked fetch, formatting, the domain packet
fixtures, the daemon admission-transaction fixtures, the Core source guard and the compilation of
all three targets on GitHub Actions. Automatic lanes keep running `kiana-domain-s2/4`
(`co13_department_packets`), `kiana-core-s2/6` (`co13_department_packets_guard`) and the
whole-crate `kiana-daemon` shard (which discovers `co13_department_packet_transactions`) from the
unified `ci.yml`. Local Cargo tests, builds, checks, clippy and smoke scripts were not run and CI
is not awaited.

```text
feature_status: implemented
proof_level: source
```

## Limitations and handoff

- The admission is the Company-aggregate contract. It does not dispatch a role session, write a
  proposal artifact or start a Run; those remain derived by ControlPlane follow-up work
  (CO-16/CO-18/CO-21).
- ProjectTrust, broker capability intersection and durable packet persistence beyond the EventLog
  remain server-side follow-up work. No Grant/Lease or live effect is claimed.
- A Plan-basis admission binds the frozen Plan revision by project and node; the contract does not
  yet drive a scheduler, ready predicate or packet claim (CO-18/CO-19).
- No model, broker, source-code write, external effect, durable, live or physical outcome is
  claimed.
