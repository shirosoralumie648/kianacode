# Testing Patterns

**Analysis Date:** 2026-07-13

## Test Framework

**Runner:**
- Rust built-in test runner through Cargo.
- Async tests use `#[tokio::test]`; environment-mutating async tests often specify `#[tokio::test(flavor = "current_thread")]`, as in `kiana-commands/tests/workflow_integrity_command.rs`.
- Config: no standalone test config file detected; test behavior is driven by crate `Cargo.toml` files and shell smoke scripts.

**Assertion Library:**
- Standard Rust assertions: `assert_eq!`, `assert!`, `matches!`, and `unwrap_err`.
- JSON contract assertions use `serde_json::Value` and `serde_json::json`, as in `kiana-types/tests/runtime_event_schema.rs` and `kiana-commands/tests/validate_command.rs`.

**Run Commands:**
~~~bash
cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1  # Release-style full workspace gate
cargo test -p kiana-commands --test validate_command --locked --offline        # Focused command integration test
cargo test -p kiana-tasks --test workflow_runtime --locked --offline           # Focused library integration test
cargo test -p kiana-entrypoints --test cli_eval --locked --offline             # Real binary CLI integration test
cargo fmt --all --check                                                        # Required formatting gate
bash scripts/schema-contract-smoke.sh                                          # JSON schema/examples contract gate
bash scripts/release-smoke.sh                                                  # Full release smoke gate
bash scripts/package-lifecycle-smoke.sh                                        # Built archive lifecycle gate
~~~

## Test File Organization

**Location:**
- Integration tests live in each crate's `tests/` directory: `kiana-commands/tests/`, `kiana-entrypoints/tests/`, `kiana-tasks/tests/`, `kiana-services/tests/`, `kiana-types/tests/`, and `kiana-bridge/tests/`.
- Unit tests live inside source modules with `#[cfg(test)]`, especially for focused helpers in `kiana-color-diff/src/lib.rs`, `kiana-chrome-mcp/src/native_install.rs`, `kiana-commands/src/tasks.rs`, and `kiana-types/src/trust.rs`.
- Smoke tests live in `scripts/` and validate assembled product, schema, package, release, and handoff behavior.

**Naming:**
- Name integration test files by command or subsystem: `validate_command.rs`, `evidence_command.rs`, `workflow_runtime.rs`, `provider_standard.rs`, `runtime_event_schema.rs`.
- Name test functions as behavior sentences in `snake_case`: `validate_records_real_pass_checks_as_evidence_and_verification_packet`, `initialize_workflow_run_creates_artifacts_state_and_eventlog`, `fake_provider_standard_contract_maps_tool_calls`.

**Structure:**
~~~text
kiana-commands/tests/{command}_command.rs      # Command trait tests using CommandContext
kiana-entrypoints/tests/cli_*.rs               # Real binary tests using env!("CARGO_BIN_EXE_kiana")
kiana-tasks/tests/*.rs                         # Library workflow/evidence/swarm contract tests
kiana-services/tests/provider_standard.rs      # Provider fake/streaming contract tests
kiana-types/tests/runtime_event_schema.rs      # Serialization/schema contract tests
scripts/*-smoke.sh                             # Release and contract smoke gates
~~~

## Test Structure

**Suite Organization:**
~~~rust
fn temp_root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-validate-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn context(args: impl Into<String>, root: &Path) -> CommandContext {
    CommandContext {
        args: args.into(),
        app_state: HashMap::from([("cwd".to_string(), json!(root.to_string_lossy().to_string()))]),
    }
}

#[tokio::test]
async fn validate_requires_an_existing_workflow() {
    let root = temp_root("missing-workflow");
    std::fs::create_dir_all(&root).unwrap();

    let error = ValidateCommand
        .execute(context("--json --profile project_p0", &root))
        .await
        .unwrap_err()
        .to_string();

    assert!(error.contains("workflow_not_found"), "unexpected error: {error}");
    let _ = std::fs::remove_dir_all(root);
}
~~~

**Patterns:**
- Build temp project roots with `std::env::temp_dir()`, process ID, timestamp nanos, and sometimes an atomic counter.
- Construct `CommandContext` directly with `app_state["cwd"]` for command tests.
- Assert both machine-readable JSON fields and human/error strings because both are user-visible contracts.
- Remove temp roots at test end; when a test mutates process environment, install a guard that restores previous environment values in `Drop`.

## Mocking

**Framework:** No external mocking framework is detected.

**Patterns:**
~~~rust
let provider = FakeProvider::new(
    FAKE_MODEL_ID.to_string(),
    vec![FakeProviderStep::ToolCall {
        id: Some("toolu_read".to_string()),
        name: "Read".to_string(),
        input: json!({"file_path": "src/lib.rs"}),
        text: Some("Need the file".to_string()),
    }],
);

let response = provider
    .create_message(request(FAKE_MODEL_ID, Some(vec![json!({"name": "Read"})])))
    .await
    .unwrap();
assert_eq!(response.stop_reason.as_deref(), Some("tool_use"));
~~~

**What to Mock:**
- Use deterministic in-process fakes for provider/model behavior, as in `kiana-services/tests/provider_standard.rs`.
- Use temporary scripts for command-side external gates, for example a generated `scripts/release-smoke.sh` in `kiana-commands/tests/validate_command.rs`.
- Use temp JSON files and hand-built workflow/event/packet fixtures for workflow, evidence, schema, and release-contract tests.

**What NOT to Mock:**
- Do not mock the command trait boundary when the behavior is a Kiana command; call `Command::execute` with a real `CommandContext`.
- Do not mock the packaged CLI boundary in `kiana-entrypoints/tests/`; use `Command::new(env!("CARGO_BIN_EXE_kiana"))`.
- Do not mock schema validation in smoke scripts; validate real `docs/schemas/*.json` and proof examples through `scripts/validate-json-schema.py`.

## Fixtures and Factories

**Test Data:**
~~~rust
fn workflow(root: &Path) -> kiana_tasks::WorkflowRun {
    initialize_workflow_run(
        root,
        WorkflowInit {
            request: "Validate temporary project".to_string(),
            input_kind: WorkflowInputKind::Qa,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )
    .unwrap()
}
~~~

**Location:**
- Inline fixture builders live at the top of each integration test file, for example `temp_root`, `context`, `workflow`, `write_task`, and `write_packet` in `kiana-commands/tests/evidence_command.rs`.
- Packaged product fixtures live under `docs/`, especially `docs/eval/fixtures/basic-runtime-suite.json` and `docs/proof-templates/*.json`.
- Schema fixtures and temporary generated JSON live inside `scripts/schema-contract-smoke.sh` and are validated against `docs/schemas/*.json`.

## Coverage

**Requirements:** No numeric coverage threshold or coverage tool config is detected.

**View Coverage:**
~~~bash
cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1  # Current correctness proxy
~~~

## Test Types

**Unit Tests:**
- Scope pure helpers, serialization, platform planners, and local invariants inside `#[cfg(test)]` modules, for example `kiana-types/src/trust.rs`, `kiana-chrome-mcp/src/native_install.rs`, and `kiana-commands/src/swarm_process_identity.rs`.

**Integration Tests:**
- Scope command contracts in `kiana-commands/tests/`, real binary behavior in `kiana-entrypoints/tests/`, workflow/evidence/swarm persistence in `kiana-tasks/tests/`, provider contracts in `kiana-services/tests/`, and runtime-event serialization in `kiana-types/tests/`.

**E2E Tests:**
- Use shell smoke scripts rather than a separate E2E framework. `scripts/release-smoke.sh` runs formatting, full workspace tests, focused parity/notebook tests, product shell smoke, and release binary build unless `KIANA_RELEASE_SMOKE_SKIP_BUILD_GATES=1`.
- Use `scripts/package-lifecycle-smoke.sh` after `scripts/package-release.sh` to verify archive target, checksum files, packaged binary format, required package docs, schemas, and proof templates.

## Common Patterns

**Async Testing:**
~~~rust
#[tokio::test(flavor = "current_thread")]
async fn integrity_init_is_idempotent_and_never_exposes_secret() {
    let _lock = env_lock();
    let root = temp_root("init");
    let home = root.join("home");
    std::fs::create_dir_all(&root).unwrap();
    let _env = IntegrityEnv::install(&home);

    let first = run("workflow integrity init --json", &root).await.unwrap();
    let first_json: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(first_json["schema"], "kiana.workflow-integrity-key-status.v1");
}
~~~

**Error Testing:**
~~~rust
let error = EvidenceCommand
    .execute(context("show --workflow missing evt-1", &root))
    .await
    .unwrap_err()
    .to_string();

assert!(error.contains("workflow"), "unexpected error: {error}");
~~~

**Environment Safety:**
- Serialize tests that mutate global environment with `OnceLock<Mutex<()>>`, as in `kiana-commands/tests/workflow_integrity_command.rs` and `kiana-commands/src/lib.rs`.
- Restore environment variables in `Drop` guards such as `IntegrityEnv` and trust test environments.
- Run broad workspace tests with `--test-threads=1` when release behavior depends on global env, filesystem state, or serialized artifacts.

**Schema and Contract Smoke:**
- Keep new JSON contracts accompanied by `docs/schemas/*.json` and sample/fixture validation in `scripts/schema-contract-smoke.sh`.
- Add focused CLI or command tests for every new schema-emitting surface before relying on release smoke.

---

*Testing analysis: 2026-07-13*
