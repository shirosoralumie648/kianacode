# DEP-39 supply-chain and compliance baseline (partial)

DEP-39 adds a read-only `SupplyChainGateReport` that binds binary/archive and DesktopPackage
artifacts to source-tree and Cargo.lock digests, checksum/signature proofs, SBOM and package
manifest digests, license status, secret-scan status, signing/compliance policy digests and a
reviewer. Unknown licenses, dependency findings, secret-scan failure, missing Desktop evidence or
any incomplete artifact evidence block `publish_allowed`.

GitHub Actions runs the domain gate fixture, source guard, release/compliance script syntax,
offline compliance/SBOM/license fixtures and the existing signature-verification smoke. The
package and Desktop scripts are wired as boundaries, but no external signer, release upload,
live package installation or production desktop binary was used. CI fixture output is not a live release;
DEP-39 remains partial until real signed artifacts and policy receipts are reviewed.
