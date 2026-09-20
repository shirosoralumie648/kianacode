# EXT-21 Extension Dependency and Binding Baseline

The extension boundary now has a typed, deterministic source contract for dependency resolution
and snapshot-scoped bindings:

- Skill, Hook, MCP, Capability, Memory, Provider and UI component nodes form a bounded directed
  graph with exact-version, kind, platform and scope checks;
- cycles, missing required nodes, version drift, platform mismatch and disjoint scopes fail closed
  before a resolution is produced;
- bindings carry destination, WorkPacket, role, data class, network policy, secret handles,
  budget, expiry, parent scope digest and snapshot identity;
- a parent scope digest change invalidates child bindings, and binding snapshots remain inert
  metadata that does not grant capabilities or bypass ControlPlane/Broker authorization.

The CI-only domain fixture is `kiana-domain/tests/ext21_extension_binding.rs`; the product-path
guard is `kiana-core/tests/ext21_extension_binding_guard.rs`. Local tests are intentionally not
run; GitHub Actions is the validation surface. Runtime registry integration, durable binding
storage, lifecycle mutation and live connector/provider effects remain EXT-22+ work.
