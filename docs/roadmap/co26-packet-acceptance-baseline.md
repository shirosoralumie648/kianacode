# CO-26 packet-level acceptance baseline

## Scope

CO-26 adds a packet acceptance contract that consumes a frozen packet revision, attempt reference,
author session set, independent review and immutable evidence-ready fact. Acceptance cannot be
created from Run Completed or Handoff ACK alone. Review target/evidence digests and packet/project
identity must match exactly.

Accept/Reject/Waive decisions are explicit. Accept requires every reviewed criterion to avoid
Fail/InsufficientEvidence and a named acceptor assignment/session; Waive requires Sponsor and an
explicit waiver/reason. Declared dependents are stored on the accepted request so later readiness
can unlock only those edges. The ledger is idempotent by acceptance digest.

## Implemented source slice

- `PacketAcceptanceRequest` binds CO-24 `CompanyEvidenceReady`, CO-25 `IndependentReview`, packet
  version/attempt, author sessions, acceptor identity and declared dependents.
- Deny-first validation rejects missing review/evidence, review/evidence mismatch, incomplete
  criteria, NotApplicable without waiver, wrong acceptor, ACK-only claims and duplicate drift.
- Core adapter records the pure ledger; existing Company RequestAcceptance/DecideAcceptance remains
  the command/event authority for compatibility.
- GitHub-only fixtures cover rejection, successful dependent projection, waiver fence and replay.

## CI-only evidence

`.github/workflows/co26-packet-acceptance.yml` runs formatting, the domain acceptance fixtures, the
Core source guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- Durable acceptance projector, full EventLog CAS integration and actual dependent ready-query
  consumption remain open. Semantic review validity is not inferred from this type alone.
- Waiver/acceptance does not deliver external outputs or establish live/physical business outcome;
  later CO-27+ steps own milestone/project closure and dependency scheduling.
