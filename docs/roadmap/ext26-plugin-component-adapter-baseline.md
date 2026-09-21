# EXT-26 Plugin Component Adapter Baseline

EXT-26 adds a daemon-built, fail-closed component registry. The registry is a deterministic
projection of a package that has already passed signature, content-hash, ProjectTrust and
dependency checks. It is not an execution engine and does not turn a manifest declaration into a
capability.

## Component matrix

| Component | Adapter contract in this slice | Default status boundary |
| --- | --- | --- |
| Skill | Context/resource metadata only; package entry and binding digests are retained | `available` only after verified enabled binding; declaration-only input is `denied` |
| Hook | Controlled hook adapter descriptor; dispatch remains behind `ExtensionAdmission` | `available` only after lifecycle and approval checks |
| MCP | Declarative stdio connector descriptor; broker owns the request | network/HTTP/host access is `denied` |
| Capability, Workflow, Memory, Provider, UI | No port, policy, receipt and recovery contract in EXT-26 | `not_supported` with an explicit unsupported capability |
| Native/script/host-file entry | No adapter | `denied` |

The descriptor binds package SHA-256, manifest/source/snapshot/binding digests, registry generation,
lifecycle revision, effect, network-policy digest and approval digest. `RequiresApproval` is a
real state for staged/read-write components; it is not silently converted to `Available`.

## Deny-first evidence

The GitHub-only workflow `ext26-extension-adapters.yml` runs the focused domain fixtures and the
daemon/broker source guard. The fixtures cover declaration-only descriptors, missing or executable
entries, malformed package bytes, unsupported component kinds and the binding recheck markers.
The guard ensures the adapter module has no direct process, socket or network primitive and that
the existing broker `ExtensionAdmission` remains the final boundary.

## Boundaries and limitations

This slice does not claim a live Hook process, an MCP transport, arbitrary network access,
durable restart recovery, or physical side-effect correctness. Hook and MCP runtime work must
continue through the existing ControlPlane/Broker contracts and receive their own receipt and
recovery evidence before being promoted beyond this metadata/binding slice.
