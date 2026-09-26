# CO-21 Company task scope and fresh Run baseline

## Scope

CO-21 adds an explicit typed boundary between Company and standalone tasks. `CompanyTaskScope`
binds mode, project/packet/assignment, role/department, path scope, frozen input references and
authorized retrieval references. Company scopes reject private transcript/planning history and
require a packet-bound work scope; standalone scopes cannot carry Company identifiers.

`FreshTaskRun` binds a new Run to a fresh session, parent session lineage, packet, attempt and
scope digest. `for_department_packet` validates CO-13's input/result/write contract before a role
task is admitted. The existing Core `spawn_from_packet` path now records the explicit Company
`work_packet_id` binding for Company StartRun and invokes the same scope validator; lifecycle
fresh-session and Company capability guards remain the execution authority.

## Implemented source slice

- `CompanyTaskScope::company_builder` rejects forged packet/path/role/department binding and
  private planning context; `standalone` rejects Company binding fields.
- `for_department_packet` enforces DepartmentPacket validation, target role and Plan/Result
  prerequisites, and rejects same-session handoff. `FreshTaskRun` preserves input refs and scope
  digest across serialization.
- Company `StartRun` explicitly sets `work_packet_id` before the existing `spawn_from_packet`
  path, so direct runs cannot silently inherit Company scope. The existing ControlPlane/Runner
  Harness path remains the sole execution route.

## CI-only evidence

`.github/workflows/co21-company-task-scope.yml` runs formatting, the domain scope fixtures, the
Core source guard and domain/core/daemon/runner test-target compilation on GitHub Actions. Local
Cargo tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- Scope validation is source-wired at the packet spawn boundary; durable attempt/assignment
  materialization and full DepartmentPacket protocol routing remain open.
- Existing Company `author_session_id` and local CellRegistry are compatibility boundaries while
  CO-22/CO-23 add durable process management and wakeup/intent consumption. No live provider,
  external effect or physical business outcome is claimed.
