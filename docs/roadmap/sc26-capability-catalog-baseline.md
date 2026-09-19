# SC-26 ExtensionManifest and CapabilityCatalog baseline

## Delivered source slice

- `kiana-domain::extension_manifest_digest` binds the complete validated manifest, including
  signature metadata, effect, version, requirements and capability declarations.
- `CapabilityCatalog` records requested versus effective capabilities for each manifest. Effective
  capabilities are the intersection of host availability, parent/Grant scope and exact approval;
  `provided_capabilities` or skill `allowed-tools` are not authorization inputs.
- ReadOnly manifests cannot request write-capable operations. Missing intersection members,
  duplicate extension identities, digest drift and widened effective sets fail closed.
- The catalog is a value-only snapshot; it does not load packages, execute tools or call Broker.

## Boundary and proof ceiling

The source/ProjectTrust and extension lifecycle adapters still supply filesystem/signature and
activation facts. This step does not claim package installation, signed-key trust, sandboxing,
runtime revocation, durable catalog storage or external effect safety; those belong to SC-25/27+
and the existing ControlPlane/Broker path.
