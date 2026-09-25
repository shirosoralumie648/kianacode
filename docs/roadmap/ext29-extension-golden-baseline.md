# EXT-29 · Extension fake golden baseline

This source slice defines an offline golden trace for Skills, Plugins and Hooks. It records the
server-owned decisions needed to audit a deny or allow path without making the fixture an execution
authority.

## Source contract

- `ExtensionGoldenMatrix` requires explicit coverage for untrusted project, name collision, path
  escape, budget exceeded, Hook block/ask/update/timeout/cancel, signed registry mutation,
  dependency cycle, MCP deny and one allowed capability case.
- Each trace binds input, extension snapshot, policy, gate, Hook outcome, final input/revalidation
  when Hook updates, capability request/Broker result/Receipt or registry Receipt, package digest
  where relevant, and identical CLI/Workbench/Web/Desktop/MCP snapshot digests.
- Denied and unknown traces have zero effect count and no capability/Broker/Receipt evidence.
  Allowed capability traces require the full request→Broker→Receipt chain; signed package traces
  require a package and registry Receipt but cannot smuggle a capability request.
- The trace is metadata-only. Existing `ExtensionSnapshot`, Hook lifecycle, extension command,
  visibility and ControlPlane/Broker contracts remain the authorities for real loading, approval,
  mutation and execution.

## CI-only fixtures

`.github/workflows/ext29-extension-golden.yml` runs target formatting, the domain golden matrix,
the Core source guard and workspace test-target compilation. The path filter includes current
CM-36 `kiana-domain/src/memory_workbench.rs` so a fresh remote run covers the repository-wide
format dependency. Local tests, builds, checks, clippy and smoke commands are deliberately not
run, and GitHub CI is not awaited.

## Evidence ceiling and limitations

The slice is `feature_status=partial` with `proof_level=source` plus CI wiring. It does not prove
real ProjectTrust loading, package signature verification, Hook process outcomes, Broker effect
execution, durable registry/restart recovery, connector behavior, external network isolation or
physical/live evidence. A fake golden trace with an allow row is not a product success receipt.
