# CO-33 versioned DeliveryManifest and local package baseline

## Scope

CO-33 adds an immutable manifest for a currently accepted project baseline and a local package
descriptor that must match it exactly. Every artifact carries its project, version, path, content
hash/size and producer run references. The channel is explicitly `local_package`; the contract does
not imply external sending or recipient receipt.

## Implemented source slice

- `DeliveryManifest` requires `AcceptanceStatus::Accepted`, a nonzero baseline, exact artifact
  references, acceptance digest, recipient, residual obligations and a deterministic local
  destination.
- Manifest and package paths reject absolute paths, `..`, dot/empty segments, backslashes and
  symlink entries; artifact and package sets are complete, unique and size bounded.
- `LocalDeliveryPackage` binds the manifest ID/digest and checks every package entry's path, hash,
  size and artifact reference; `DeliveryManifestLedger` is idempotent and rejects changed or
  foreign successors.
- `CompanyState` exposes the typed manifest/package ledger while filesystem packaging remains in
  the existing Broker/adapter boundary.

## CI-only evidence

`.github/workflows/co33-delivery-manifest.yml` runs formatting, DeliveryManifest and local package
fixtures, the Core Broker-boundary guard and target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- This is a source contract and ledger; it does not read/write a content store or create an actual
  directory/archive, and it does not prove delivery or recipient receipt.
- Existing Company closeout and delivery commands remain compatibility/effect authorities. CO-34
  owns approval, dispatch, effect observation and recipient confirmation.
