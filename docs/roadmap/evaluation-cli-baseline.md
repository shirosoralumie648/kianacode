# EQ-47 evaluation CLI routing baseline

## Scope

EQ-47 adds a strict, provider-independent command contract for `kiana eval run`, `capture`,
`compare`, `explain`, and `list`. Each migrated form is parsed into the versioned
`kiana.eval-cli.v1` argument object and routed as a generic command through the existing
`DaemonHost -> ControlPlane` dispatch seam. The parser does not open paths, call a provider, invoke
the Broker, run a model, or create a second execution loop.

The historical `run --suite <path> [--baseline <path>] [--json] [--fail-on-failure]` form remains an
explicit local compatibility adapter so existing fixture/report JSON stays unchanged. It is not a
new quality authority and does not grant, promote, mutate a route, or alter a receipt.

## Versioned mapping

| CLI action | ControlPlane command | Required/accepted references |
|---|---|---|
| `run` | `eval.run` | optional suite/dataset/experiment/case/target/baseline IDs |
| `capture` | `eval.capture` | exactly one source (`--source`, `--run-id`, or `--fixture`), optional name |
| `compare` | `eval.compare` | `--reference` and `--candidate` |
| `explain` | `eval.explain` | one case/finding/result/target reference |
| `list` | `eval.list` | optional bounded kind filter |

Every request carries `schema=kiana.eval-cli.v1`, `version=1`, an explicit action, and text/json
output mode. Unknown subcommands/options, duplicate options, missing values, path escape markers,
invalid list kinds, incomplete compare/capture/explain requests, and control characters fail closed.

## CI evidence and limits

- `kiana-commands/tests/eq47_cli.rs` checks route mapping, strict argument rejection, legacy route
  preservation, and the no-local-second-loop boundary.
- `kiana-entrypoints/tests/eq47_cli_guard.rs` checks the versioned map, quality event registry, and
  absence of Provider/Broker/Runner ownership in the CLI parser.
- `.github/workflows/eq47-cli.yml` runs these fixtures, the existing quality wire fixture, format,
  and workspace test-target compilation in GitHub Actions. Local Cargo tests/build/check/clippy and
  smoke commands are intentionally not run.

This slice is `feature_status=partial`, `proof_level=source`. The generic eval command names are
registered for the wire/event contract, but no EvalStore execution, target isolation, report/JUnit
rendering, durable artifact, provider quality result, promotion, live route, or physical evidence
is claimed. The legacy path parser remains a compatibility surface until later EQ steps migrate its
fixture I/O behind ControlPlane-owned quality ports.
