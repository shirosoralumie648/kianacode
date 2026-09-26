# EQ-49 evaluation script wrappers baseline

## Scope

EQ-49 adds four strict shell wrappers around the versioned `kiana eval` CLI surface:

- `scripts/eval-curated.sh` routes `eval run` to the default curated suite;
- `scripts/eval-deep.sh` routes `eval run` to the opt-in deep suite;
- `scripts/eval-capture-golden.sh` routes one opaque source reference to `eval capture`;
- `scripts/eval-compare.sh` routes two opaque references to `eval compare`.

The wrappers only construct argv and call the existing `kiana` binary. They do not parse fixtures,
render reports, invoke a provider, start a model/runner loop, or write a GoldenTrace themselves.
The existing `DaemonHost -> ControlPlane` route remains the execution authority.

## Environment and input fence

Each child command is launched with `env -i` and this explicit allowlist only:

```text
PATH=/usr/local/bin:/usr/bin:/bin
HOME=<new 0700 temporary home>
KIANA_HOME=<new 0700 temporary Kiana home>
TMPDIR=<new 0700 temporary directory>
LC_ALL=C
LANG=C
RUST_BACKTRACE=0
```

The parent `KIANA_BIN` value is used only to select an executable under the repository and is not
passed to the child. Provider keys, proxy variables, MCP configuration and the operator's existing
`.kiana` are therefore unavailable. Temporary directories are removed on normal exit and signals.

All selector/source/reference values are bounded, reject absolute paths, `~`, traversal/backslash,
control characters and secret-like markers. Duplicate/missing/unknown options fail closed. The
wrapper passes argv as an array, never through shell evaluation. A non-zero `kiana eval` exit code is
propagated as a failure; no `|| true` or fallback evaluator is present.

## CI-only evidence

`.github/workflows/eq49-eval-scripts.yml` runs `scripts/tests/eq49-eval-scripts-static.sh`, which runs
`bash -n` and checks executable mode, strict mode, the environment allowlist, secret/path guards,
ControlPlane route names and the absence of legacy filesystem evaluation or a second evaluator.
The workflow does not build or execute an evaluation binary. Local Cargo tests/build/check/clippy and
smoke commands are intentionally not run, and CI results are not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `scripts_use_explicit_environment_allowlist` | all four wrappers use `env -i`, temporary HOME/KIANA_HOME/TMPDIR and a fixed locale/path |
| `script_arguments_fail_closed` | unknown, duplicate, missing, absolute, traversal, control and secret-like values are rejected |
| `eval_scripts_route_without_second_loop` | wrappers only call `eval.run`, `eval.capture` or `eval.compare` through the existing binary |
| `command_failure_is_not_hidden` | non-zero ControlPlane exit is surfaced and cleanup still runs |

## Limitations and handoff

This is a source-level wrapper and CI static-guard contract. It does not prove a real curated/deep
result, report/JUnit archive, GoldenTrace persistence, compare semantics, provider quality, durable
restart behavior, live route or physical outcome. EQ-50 owns PR/nightly/release lane wiring and
EQ-51 owns artifact archival and the final evidence block.
