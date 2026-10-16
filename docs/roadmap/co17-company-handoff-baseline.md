# CO-17 cross-department handoff and ACK baseline

## Scope

CO-17 adds a versioned accountability handoff contract for CO-13 department packets and CO-16
plan outputs. `CompanyHandoff` freezes the exact packet revision, packet digest, input references,
write scope, result basis and budget reference. Both the owner and recipient are bound to a
project-scoped assignment snapshot with an assignment version and expiry.

The recipient must present the same packet, material and budget evidence before an ACK can be
accepted. Only an accepted ACK changes `current_owner_assignment_id`; rejection and timeout leave
the original owner responsible and create an explicit escalation object. Pending projections expose
the recipient, terminal projections expose the owner and escalation, and a replayed offer is
idempotent by its immutable offer digest. The contract carries no runtime grant or lease.

## Implemented source slice

- `HandoffPacketRevision` validates the DepartmentPacket, exact revision digest, input refs,
  write scope, result contract and budget reference.
- `HandoffAssignment` validates role/department, project scope, assignment version, session and
  active window. `CompanyHandoff::offer` rejects mismatched recipient assignment, same-party
  transfer, inactive assignments and expiry beyond either assignment.
- `CompanyHandoffLedger` applies recipient ACK/reject and explicit expiry. Wrong recipients,
  stale material, expired assignments, terminal repeats and runtime-lease injection fail closed;
  rejection/expiry return accountability to the owner with a stable escalation object.
- `CompanyState` carries the serializable accountability ledger and projection helpers. The legacy
  `PacketHandoff` and `AcknowledgeHandoff` path remains compatible while this stricter contract is
  introduced.

## CI-only evidence

`.github/workflows/co17-company-handoff.yml` runs formatting, the domain handoff fixtures, the
Core source guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The new ledger is a typed CompanyState projection with explicit state methods; protocol DTO
  routing and EventLog CAS materialization for these new fields remain follow-up integration.
- Assignment snapshots are validated at the handoff boundary but are not yet resolved from the
  external identity/authority adapter. Packet artifact reads, budget settlement, cross-process
  delivery and live/physical business effects remain unproven.
- Chat and StatusReport are not used by this ledger and cannot mutate accountability; no runtime
  lease is acquired by ACK.

## Current-head same-principal fence (2026-10-08)

`CompanyHandoff::offer` now rejects an owner and recipient that share the same `principal_id`,
even when assignment IDs and session IDs differ. This closes the same-party transfer gap across
assignment/session rotation while preserving the existing assignment and session checks.
`Expired` is also a valid no-ACK terminal state when it carries its required timeout escalation;
`Rejected` and `Acknowledged` still require a matching receiver ACK.

```text
source_snapshot: current dirty worktree; kiana-domain/src/company_handoff.rs; kiana-domain/tests/co17_company_handoff.rs; kiana-core/tests/co17_company_handoff_guard.rs
worktree_status: CO-17 row 516 remains 🔄 / feature_status=partial / proof_level=source; same-principal handoff is denied before digest construction; no runtime lease or second execution path
command_argv: RED `cargo test -p kiana-domain --test co17_company_handoff handoff_rejects_same_principal_even_when_assignment_and_session_differ --locked --offline -- --exact --nocapture --test-threads=1`; timeout RED from `reject_or_timeout_returns_to_owner_with_escalation_and_resend_is_idempotent`; exact GREEN same selector; full domain target; Core guard
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, 32 MiB test stack, stripped/non-incremental test profile; principal RED 347912 KiB / 50% CPU; principal exact GREEN 83564 KiB / 23% CPU; full domain GREEN 83788 KiB / 14% CPU; Core guard 4049500 KiB / 89% CPU; all runs reported 0 swaps
fixture·cassette: principal RED same principal with distinct assignment/session returned Ok; timeout RED returned `handoff_terminal_ack_missing` for a valid Expired/no-ACK state; GREEN domain 4/4 and Core guard 1/1, with stable `handoff_parties_must_be_distinct` and `handoff_expired_escalation_missing` fences
exit_code: RED 101 expected for principal and timeout selectors; exact principal GREEN 0; domain/Core GREEN 0
status_change: selected CO-17 same-principal and Expired/no-ACK boundaries move source → local_behavior; whole CO-17 remains partial/source
proof-level_change: handoff party and timeout-state validation source → local_behavior; no durable/live/physical promotion
limitations: EventLog CAS, external identity resolution, cross-process delivery and live/physical effects remain open
reviewer: root integration agent with CO-17 handoff audit; focused same-principal reviewer
```
