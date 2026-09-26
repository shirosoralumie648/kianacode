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
