# Kiana Rust Toolchain Upgrade Policy

## Channel Choice

Kiana uses `channel = "stable"` in `rust-toolchain.toml` rather than pinning an exact version.

**Rationale:** Pinning an exact version (e.g. `1.96.0`) creates friction — every routine Rust release requires a deliberate bump commit. The stable channel keeps the toolchain current and secure. The tradeoff is that two CI runs a week apart may compile with different patch versions; this is acceptable because:

1. The exact resolved Rust version is recorded in `dist/build-inputs.json` at CI time, providing a reproducibility anchor.
2. Behavioral differences between adjacent stable releases are rare and surfaced by the existing CI test suite.
3. Supply-chain risk of a malicious Rust release is low and accepted at the current project risk level.

Note: the three reference projects in `reference/` pin exact versions (`1.92.0`, `1.95.0`, `1.96`). Kiana intentionally differs — the version discipline is enforced through `build-inputs.json`, not the toolchain file.

## MSRV Alignment

The workspace `rust-version` in `Cargo.toml` is currently `1.96`. The stable channel must remain at or above this MSRV. If `rustup update stable` would install a version below `rust-version`, do **not** bump `rust-toolchain.toml` — instead investigate and resolve the MSRV constraint first.

## Upgrade Steps

1. Confirm the new stable has been out for at least 2–3 weeks (allow time for ecosystem issues to surface).
2. Run `rustup update stable` locally.
3. Verify:
   ```bash
   cargo check --all-targets --workspace
   cargo clippy --all-targets --workspace -- -D warnings
   cargo test --workspace --locked --offline --no-fail-fast
   bash scripts/schema-contract-smoke.sh
   bash scripts/release-smoke.sh
   ```
4. If all checks pass, commit **only** `rust-toolchain.toml` (no other changes in the same commit).
5. In CI the upgrade is automatic — the stable channel advances on `rustup update`. Record the actual version via the `dist/build-inputs.json` step in `release-smoke.yml`.

## Components

`components = ["rustfmt", "clippy", "rust-src"]` — required for:
- `rustfmt`: code formatting gate (`cargo fmt --all --check`)
- `clippy`: lint gate (`cargo clippy`)
- `rust-src`: rust-analyzer IDE support

Do not remove components without updating the CI lint gates.

## CI Workflow Note

`.github/workflows/release-smoke.yml`, `.github/workflows/release.yml`, and `.github/workflows/release-tui.yml` use `dtolnay/rust-toolchain@master`. The `@master` variant reads `rust-toolchain.toml` automatically. Do **not** revert to `@stable` — it silently ignores the toolchain file.
