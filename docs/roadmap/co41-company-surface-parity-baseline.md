# CO-41 Company Web/Desktop surface parity baseline

## Scope

CO-41 adds a shared surface parity contract for CLI, Workbench, Web and Desktop. Each frame carries
the same Company read-model digest, source/projection cursor, revision, authority epoch, terminal
status and next action. A stale action is rejected before it can reach the command path.

## Implemented source slice

- `CompanySurfaceFrame` binds every surface to one `CompanyReadModelSnapshot`; four surfaces are
  required and duplicate/missing surface frames fail closed.
- `StaleSurfaceAction` rejects old digest/revision/authority epoch, while fresh actions remain
  inputs to the existing DaemonHost/ControlPlane path.
- Web/Workbench/Protocol source guards prove shared snapshot/governance and no second model loop or
  business state store; Desktop remains a presentation surface.

## CI-only evidence

`.github/workflows/co41-company-surface-parity.yml` runs formatting, four-surface/stale-action
fixtures, the Web/Workbench/Protocol source guard and entrypoint target compilation on GitHub
Actions. Local Cargo tests, builds, checks, clippy and smoke scripts were not run and CI is not
awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The parity contract does not implement Web/Desktop rendering or durable event hydration; it only
  constrains the shared DTO/action boundary.
- Cross-process reconnect, browser/desktop identity and full UI E2E remain later UI/NM/CO steps.
