# EQ-48 evaluation report artifact baseline

## Scope

EQ-48 defines a provider-independent report artifact contract over already committed evaluation
facts. `QualityReport` validates a recomputed status and summary, binds every case evidence
reference to a strict evidence manifest, and requires a redacted reproduction command. The
read-only renderer emits JSON, JUnit XML, a human summary, the evidence manifest and a shell-safe
reproduction command. It does not run evaluation, open fixtures, write files or grant authority.

The shared domain redactor runs before the report boundary. Secret markers, provider raw errors,
absolute paths, path traversal and unredacted command arguments are rejected. Environment names
may be listed in a reproduction command, but environment values are never accepted or emitted.

## CI-only evidence

`.github/workflows/eq48-reports.yml` runs formatting, domain artifact fixtures, the Core read-only
boundary guard and affected test-target compilation in GitHub Actions. Local Cargo tests, builds,
checks, clippy and smoke commands were intentionally not run, and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `report_is_machine_readable_and_redacted` | all five artifacts render and contain no raw path or secret marker |
| `report_rejects_forged_summary_paths_secret_and_missing_evidence` | forged counts, absolute path, secret value and unbound evidence fail closed |
| `report_redactor_removes_path_and_secret_values` | shared redaction plus path marker removes sensitive values before construction |
| `quality_report_uses_read_only_control_plane_adapter` | report adapter has no provider, Broker, Runner, filesystem or publish path |

## Limitations and handoff

- This is a source-level renderer contract; it does not prove a real evaluation result, durable
  artifact archive, JUnit consumer compatibility, live provider behavior or physical outcome.
- No report is persisted and no `kiana eval` command writes these artifacts; EQ-49 adds explicit
  CI scripts and EQ-50/EQ-51 add workflow lanes and archival evidence.
- CI results are intentionally not awaited; proof level remains `source`.
