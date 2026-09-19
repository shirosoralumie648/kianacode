# SC-21 DataClass / Purpose / DataBoundary Baseline

## Scope

SC-21 records the strict domain data-governance contracts already used by the security/persistence
surfaces: `DataClass`, purpose and retention metadata, `ProcessingGrant`/`DataPolicy` with revoke
propagation and data epoch, and `DataBoundary` with project/class/external allowance, authority
epoch and digest. Unknown/invalid class-purpose-retention, source/hash, revoked-parent, boundary
ordering and digest/epoch drift fail closed at the value layer.

The contracts do not themselves grant access, perform redaction, delete bytes or authorize a
Broker effect; ControlPlane/policy/storage adapters must consume the server-owned values.

## Evidence and limits

- `kiana-domain/tests/sc21_data_boundary_guard.rs` protects the strict contract inventory and
  domain-only/no-execution dependency boundary.
- `.github/workflows/sc21-data-boundary.yml` runs the source guard and workspace compilation in
  GitHub Actions; local tests are intentionally not executed.
- Existing OA-20/P2-K7-01 domain fixtures remain the behavior matrix for policy register/revoke,
  retention expiry, derived-store state and epoch propagation.

This slice is `feature_status=implemented`, `proof_level=source`: domain contracts and CI guard are
present, while full cross-entrypoint enforcement, durable retention/delete and live compliance are
not claimed.
