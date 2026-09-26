# CO-19 Company packet attempt and lease fencing baseline

## Scope

CO-19 separates the long-lived packet identity from a short-lived `PacketAttempt`. Each attempt
records worker cell/session, execution request, packet revision, lease expiry and monotonic epoch.
The append-only `PacketAttemptLedger` admits one active attempt per packet and keeps every prior
epoch for replay and audit.

An expired attempt is fenced before reuse. An attempt with a started or unknown effect becomes
`ResultUnknown` and cannot be reassigned until explicit reconciliation. A never-started or
confirmed-stopped attempt becomes `Fenced` and can be reclaimed with a new epoch. Late results,
heartbeats and stop records from an older attempt id/epoch/request are rejected. Repeating the same
worker/request claim is idempotent and cannot create a second run.

## Implemented source slice

- `PacketAttempt` carries typed status/effect/stop/terminal fields, execution request, epoch and
  lease digest. Its transition methods fail closed at expiry/terminal/fence boundaries.
- `PacketAttemptLedger` keeps ordered per-packet history, enforces one active claimant, fences
  expired attempts, rejects unknown-effect retry and rejects stale epoch tokens.
- `CompanyState` stores the serializable attempt ledger and exposes claim/start/stop/complete/fence
  helpers while preserving legacy `PacketClaim` and existing Company commands.
- GitHub-only fixtures cover duplicate claimant, idempotent retry, stale epoch, confirmed stop,
  unknown effect, ResultUnknown reassignment denial and serialized history.

## CI-only evidence

`.github/workflows/co19-company-attempt.yml` runs formatting, the domain attempt fixtures, the
Core source guard and domain/core/daemon test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The new ledger is a typed CompanyState contract and is not yet wired as the sole durable
  EventLog CAS command reducer; existing legacy Claim/Renew/Reclaim commands remain compatible.
- Real process stop confirmation, provider/external effect reconciliation, durable cross-process
  attempt projection and budget settlement remain open. CO-20 owns cell/resource admission.
