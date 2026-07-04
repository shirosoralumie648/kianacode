# Commercial Proof Templates

These templates show the JSON shape expected by the full commercial release
gates. They are intentionally non-accepted examples. Do not copy them into the
default full-preflight paths until the corresponding evidence has actually been
reviewed and accepted.

Default full-preflight input paths for `VERSION=0.1.0` are:

- `docs/source-control/0.1.0.json`
- `docs/product-acceptance/0.1.0.json`
- `docs/entitlements/0.1.0.json`
- `docs/release-ops/0.1.0.json`
- `docs/platform-security/0.1.0-linux.json`
- `docs/platform-security/0.1.0-macos.json`
- `docs/platform-security/0.1.0-windows.json`

The same files may be supplied from outside the repository with:

- `KIANA_SOURCE_CONTROL_PROOF_FILE`
- `KIANA_PRODUCT_ACCEPTANCE_FILE`
- `KIANA_ENTITLEMENT_PROOF_FILE`
- `KIANA_RELEASE_OPS_FILE`
- `KIANA_PLATFORM_SECURITY_PROOF_FILE`
- `KIANA_PLATFORM_SECURITY_PROOF_DIR`

Run the lightweight blocker report before requesting release approval:

```bash
bash scripts/commercial-release-blockers-report.sh
```

Run the strict full gates only after real evidence is available:

```bash
bash scripts/release-preflight.sh
bash scripts/stage-commercial-release-proofs.sh
bash scripts/verify-commercial-release-artifacts.sh
```

`scripts/stage-commercial-release-proofs.sh` does not create accepted evidence.
It only copies already accepted/live proof files into `DIST_DIR/proofs`, writes
`PROOF-MANIFEST.json`, and emits `HANDOFF.md` for the release owner.
