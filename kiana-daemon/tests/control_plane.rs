use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_protocol::{
    ApprovalChallenge, ApprovalDecision, ApprovalId, ExecutionStatus, RequestEnvelope,
    RequestMetadata, ResponseEnvelope,
};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

struct InProcessTransport {
    host: Arc<DaemonHost>,
}

#[async_trait]
impl ClientTransport for InProcessTransport {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        Ok(self.host.handle(request).await)
    }
}

#[derive(Clone, Copy)]
struct FixedProjectTrustAuthority {
    trusted: bool,
}

impl ProjectTrustAuthority for FixedProjectTrustAuthority {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(self.trusted)
    }
}

fn local_host_with_trust(trusted: bool) -> DaemonHost {
    DaemonHost::local_with_project_authority(Arc::new(FixedProjectTrustAuthority { trusted }))
        .unwrap()
}

fn trusted_local_host() -> DaemonHost {
    local_host_with_trust(true)
}

fn untrusted_local_host() -> DaemonHost {
    local_host_with_trust(false)
}

fn trusted_metadata() -> RequestMetadata {
    let mut metadata = RequestMetadata::local("session-1", "/repo");
    metadata.project_trusted = true;
    metadata
}

#[tokio::test]
async fn in_process_client_reaches_core_through_daemon() {
    let host = Arc::new(trusted_local_host());
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .command(trusted_metadata(), "system.architecture", Value::Null)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed);
    assert_eq!(response.output["control_plane"], "kiana-core");
    assert_eq!(response.output["composition_root"], "kiana-daemon");
    assert_eq!(response.output["harness"], "kiana-harness");
    assert_eq!(response.output["capability_mode"], "brokered");
}

#[tokio::test]
async fn unknown_and_untrusted_commands_are_blocked() {
    let host = Arc::new(trusted_local_host());
    let client = KianaClient::new(InProcessTransport { host });
    let unknown = client
        .command(trusted_metadata(), "unknown.command", Value::Null)
        .await
        .unwrap();
    assert_eq!(unknown.status, ExecutionStatus::Blocked);
    assert_eq!(unknown.error.as_deref(), Some("command_unregistered"));

    let untrusted_host = Arc::new(untrusted_local_host());
    let untrusted_client = KianaClient::new(InProcessTransport {
        host: untrusted_host,
    });
    let untrusted = untrusted_client
        .command(
            RequestMetadata::local("session-2", "/repo"),
            "system.architecture",
            Value::Null,
        )
        .await
        .unwrap();
    assert_eq!(untrusted.status, ExecutionStatus::Blocked);
    assert_eq!(untrusted.error.as_deref(), Some("project_untrusted"));

    let untrusted_query = untrusted_client
        .command(
            RequestMetadata::local("session-3", "/repo"),
            "context.query.v1",
            json!({
                "operation": "repo_map",
                "output": "json",
                "options": {},
            }),
        )
        .await
        .unwrap();
    assert_eq!(untrusted_query.status, ExecutionStatus::Denied);
    assert_eq!(untrusted_query.error.as_deref(), Some("project_untrusted"));
}

#[tokio::test]
async fn malformed_envelope_is_rejected_before_core() {
    let host = trusted_local_host();
    let valid = RequestEnvelope::command(trusted_metadata(), "system.architecture", Value::Null);
    let mut invalid = valid.clone();
    invalid.schema = "kiana.protocol.v0".to_owned();
    let rejected = host.handle(invalid).await;
    assert_eq!(rejected.status, ExecutionStatus::Blocked);
    assert_eq!(
        rejected.error.as_deref(),
        Some("protocol_schema_unsupported")
    );

    let accepted = host.handle(valid).await;
    assert_eq!(accepted.status, ExecutionStatus::Completed);
}

#[tokio::test]
async fn approval_decision_without_a_daemon_challenge_is_blocked() {
    let host = trusted_local_host();
    let mut metadata = trusted_metadata();
    metadata.request_id = kiana_protocol::RequestId::new();
    let response = host
        .handle(RequestEnvelope::approval_decision_with_proof(
            metadata,
            ApprovalId::new(),
            ApprovalDecision::Approve,
            Some("sha256:missing".to_owned()),
            Some("missing".to_owned()),
        ))
        .await;
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert!(response
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("approval_not_found"));
}

#[tokio::test]
async fn proofless_wire_approval_is_rejected_before_store_lookup() {
    let host = trusted_local_host();
    let response = host
        .handle(RequestEnvelope::approval_decision(
            trusted_metadata(),
            ApprovalId::new(),
            ApprovalDecision::Approve,
        ))
        .await;
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(response.error.as_deref(), Some("approval_proof_required"));
}

#[tokio::test]
async fn repo_map_reaches_query_handler_and_uses_metadata_project_root() {
    let root = fixture_root("repo-map");
    let forged = fixture_root("forged-root");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(&forged).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub struct RoutedWidget;\nfn routed_render() {}\n",
    )
    .unwrap();
    fs::write(forged.join("secret.txt"), "must not be scanned\n").unwrap();

    let host = Arc::new(trusted_local_host());
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = RequestMetadata::local("session-query", root.to_string_lossy());
    metadata.project_trusted = true;
    let response = client
        .command(
            metadata,
            "context.query.v1",
            json!({
                "operation": "repo_map",
                "output": "json",
                "options": { "max_tokens": 1000 },
                "project_root": forged,
            }),
        )
        .await
        .unwrap();

    assert_eq!(response.status, ExecutionStatus::Completed);
    let result = &response.output["command_result"];
    assert_eq!(result["output_type"], "text");
    let map: Value = serde_json::from_str(result["value"].as_str().unwrap()).unwrap();
    assert_eq!(map["token_budget"], 1000);
    assert_eq!(map["files"][0]["path"], "src/lib.rs");
    assert!(map["files"][0]["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol == "struct RoutedWidget"));
    assert!(!result["value"].as_str().unwrap().contains("secret.txt"));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(forged);
}

#[tokio::test]
async fn malformed_repo_map_intent_is_blocked_before_the_handler() {
    let host = Arc::new(trusted_local_host());
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .command(
            trusted_metadata(),
            "context.query.v1",
            json!({
                "operation": "repo_map",
                "output": "json",
                "options": { "max_tokens": 0 },
            }),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(response.error.as_deref(), Some("command_arguments_invalid"));
}

#[tokio::test]
async fn read_only_context_queries_preserve_json_and_text_contracts() {
    let root = fixture_root("read-only-context");
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("docs/design.md"),
        "The release API is implemented in src/lib.rs.\n",
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub fn checkout_flow() {}\n// checkout workflow\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/lib_test.rs"),
        "use kiana::checkout_flow;\n",
    )
    .unwrap();

    let graph = context_query(
        &root,
        json!({
            "operation": "artifact_graph",
            "output": "json",
            "options": {},
        }),
    )
    .await;
    let graph = command_json(&graph);
    assert_eq!(
        graph["schema"],
        "kiana.context-artifact-dependency-graph.v1"
    );
    assert_eq!(graph["nodes"].as_array().unwrap().len(), 3);
    assert!(graph["edges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edge| edge["relation"] == "path_reference"));

    let readiness = context_query(
        &root,
        json!({
            "operation": "artifact_readiness",
            "output": "json",
            "options": {},
        }),
    )
    .await;
    let readiness = command_json(&readiness);
    assert_eq!(readiness["schema"], "kiana.context-artifact-readiness.v1");
    assert_eq!(readiness["status"], "incomplete");
    assert_eq!(readiness["missing_roles"], json!(["prd", "tasks"]));

    let search = context_query(
        &root,
        json!({
            "operation": "search",
            "output": "json",
            "options": { "query": "checkout", "limit": 1 },
        }),
    )
    .await;
    let search = command_json(&search);
    assert_eq!(search["schema"], "kiana.context-search.v1");
    assert_eq!(search["limit"], 1);
    assert_eq!(search["hits"][0]["path"], "src/lib.rs");

    let vectors = context_query(
        &root,
        json!({
            "operation": "vector_search",
            "output": "json",
            "options": { "query": "checkout flow", "limit": 1 },
        }),
    )
    .await;
    let vectors = command_json(&vectors);
    assert_eq!(vectors["schema"], "kiana.context-vector-search.v1");
    assert_eq!(vectors["dimensions"], 64);
    assert_eq!(vectors["hits"][0]["path"], "src/lib.rs");

    let pack = context_query(
        &root,
        json!({
            "operation": "pack",
            "output": "text",
            "options": {
                "query": "checkout",
                "limit": 1,
                "max_snippet_lines": 1
            },
        }),
    )
    .await;
    let pack = command_text(&pack);
    assert!(pack.starts_with("Context pack\nroot: "));
    assert!(pack.contains("artifact_graph: schema=kiana.context-artifact-graph.v1 nodes=1 edges=1"));
    assert!(pack.contains("relation=matched terms=checkout"));

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn context_materialization_reads_and_approved_writes_use_daemon_handlers() {
    let root = fixture_root("materialization");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();
    fs::create_dir_all(root.join("source")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn materialize() {}\n").unwrap();
    fs::write(root.join("tests/lib_test.rs"), "use kiana::materialize;\n").unwrap();
    fs::write(root.join("source/research.md"), "materialization source\n").unwrap();

    let host = Arc::new(trusted_local_host());
    let client = KianaClient::new(InProcessTransport { host });

    for (operation, schema) in [
        ("index", "kiana.context-index.v1"),
        ("artifacts", "kiana.context-artifacts.v1"),
        ("artifact_store", "kiana.context-artifact-store.v1"),
    ] {
        let response = client
            .command(
                trusted_metadata_for(&root, "materialization-read"),
                "context.query.v1",
                json!({
                    "operation": operation,
                    "output": "json",
                    "options": {},
                }),
            )
            .await
            .unwrap();
        assert_eq!(response.status, ExecutionStatus::Completed);
        assert_eq!(command_json(&response)["schema"], schema);
    }
    assert!(!root.join(".kiana").exists());

    for (operation, cache, schema) in [
        (
            "index_cache_write",
            ".kiana/context-index.json",
            "kiana.context-index.v1",
        ),
        (
            "artifacts_cache_write",
            ".kiana/context-artifacts.json",
            "kiana.context-artifacts.v1",
        ),
        (
            "artifact_store_cache_write",
            ".kiana/context-artifact-store.json",
            "kiana.context-artifact-store.v1",
        ),
    ] {
        let response = client
            .command(
                trusted_metadata_for(&root, "materialization-write"),
                "context.query.v1",
                json!({
                    "operation": operation,
                    "output": "json",
                    "options": { "cache": cache },
                }),
            )
            .await
            .unwrap();
        assert_eq!(response.status, ExecutionStatus::AwaitingApproval);
        assert!(!root.join(cache).exists());
        let challenge: ApprovalChallenge =
            serde_json::from_value(response.output["approval"].clone()).unwrap();
        let approved = client
            .approval_decision_with_proof(
                trusted_metadata_for(&root, "materialization-write"),
                challenge.approval_id,
                ApprovalDecision::Approve,
                Some(challenge.request_hash.clone()),
                Some(challenge.nonce.clone()),
            )
            .await
            .unwrap();
        assert_eq!(approved.status, ExecutionStatus::Completed);
        assert_eq!(command_json(&approved)["schema"], schema);
        assert!(root.join(cache).is_file());
    }

    let response = client
        .command(
            trusted_metadata_for(&root, "materialization-ingest"),
            "context.query.v1",
            json!({
                "operation": "artifact_ingest_write",
                "output": "json",
                "options": { "source": "source", "store": ".kiana/ingest" },
            }),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::AwaitingApproval);
    assert!(!root.join(".kiana/ingest/manifest.json").exists());
    let challenge: ApprovalChallenge =
        serde_json::from_value(response.output["approval"].clone()).unwrap();
    let approved = client
        .approval_decision_with_proof(
            trusted_metadata_for(&root, "materialization-ingest"),
            challenge.approval_id,
            ApprovalDecision::Approve,
            Some(challenge.request_hash.clone()),
            Some(challenge.nonce.clone()),
        )
        .await
        .unwrap();
    assert_eq!(approved.status, ExecutionStatus::Completed);
    assert_eq!(
        command_json(&approved)["schema"],
        "kiana.context-artifact-ingest.v1"
    );
    assert!(root.join(".kiana/ingest/manifest.json").is_file());

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn materialization_writes_fail_closed_for_context_and_path_escapes() {
    let root = fixture_root("materialization-boundaries");
    let outside = fixture_root("materialization-outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(root.join("README.md"), "boundary\n").unwrap();

    let host = Arc::new(trusted_local_host());
    let client = KianaClient::new(InProcessTransport { host });

    let parent = client
        .command(
            trusted_metadata_for(&root, "boundary"),
            "context.query.v1",
            json!({
                "operation": "index_cache_write",
                "output": "json",
                "options": { "cache": "../outside.json" },
            }),
        )
        .await
        .unwrap();
    assert_eq!(parent.status, ExecutionStatus::Blocked);
    assert_eq!(parent.error.as_deref(), Some("command_arguments_invalid"));
    assert!(!outside.join("outside.json").exists());

    let source_escape = client
        .command(
            trusted_metadata_for(&root, "boundary"),
            "context.query.v1",
            json!({
                "operation": "artifact_ingest_write",
                "output": "json",
                "options": { "source": "../materialization-outside" },
            }),
        )
        .await
        .unwrap();
    assert_eq!(source_escape.status, ExecutionStatus::Blocked);
    assert_eq!(
        source_escape.error.as_deref(),
        Some("command_arguments_invalid")
    );

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, root.join("cache-link")).unwrap();
        let symlink_pending = client
            .command(
                trusted_metadata_for(&root, "boundary"),
                "context.query.v1",
                json!({
                    "operation": "index_cache_write",
                    "output": "json",
                    "options": { "cache": "cache-link/index.json" },
                }),
            )
            .await
            .unwrap();
        assert_eq!(symlink_pending.status, ExecutionStatus::AwaitingApproval);
        let challenge: ApprovalChallenge =
            serde_json::from_value(symlink_pending.output["approval"].clone()).unwrap();
        let symlink_escape = client
            .approval_decision_with_proof(
                trusted_metadata_for(&root, "boundary"),
                challenge.approval_id,
                ApprovalDecision::Approve,
                Some(challenge.request_hash.clone()),
                Some(challenge.nonce.clone()),
            )
            .await
            .unwrap();
        assert_eq!(symlink_escape.status, ExecutionStatus::Failed);
        assert!(symlink_escape
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("outside_project"));
        assert!(!outside.join("index.json").exists());
    }

    let pending = client
        .command(
            trusted_metadata_for(&root, "boundary"),
            "context.query.v1",
            json!({
                "operation": "index_cache_write",
                "output": "json",
                "options": { "cache": "safe.json" },
            }),
        )
        .await
        .unwrap();
    let challenge: ApprovalChallenge =
        serde_json::from_value(pending.output["approval"].clone()).unwrap();
    let mut wrong_actor = trusted_metadata_for(&root, "boundary");
    wrong_actor.actor_id = Some("other-actor".to_owned());
    let wrong = client
        .approval_decision_with_proof(
            wrong_actor,
            challenge.approval_id,
            ApprovalDecision::Approve,
            Some(challenge.request_hash.clone()),
            Some(challenge.nonce.clone()),
        )
        .await
        .unwrap();
    assert_eq!(wrong.status, ExecutionStatus::Blocked);
    assert!(wrong
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("approval_context_mismatch"));
    assert!(!root.join("safe.json").exists());

    let approved = client
        .approval_decision_with_proof(
            trusted_metadata_for(&root, "boundary"),
            challenge.approval_id,
            ApprovalDecision::Approve,
            Some(challenge.request_hash.clone()),
            Some(challenge.nonce.clone()),
        )
        .await
        .unwrap();
    assert_eq!(approved.status, ExecutionStatus::Completed);
    assert!(root.join("safe.json").is_file());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}

#[tokio::test]
async fn context_query_roots_are_confined_to_metadata_project_root() {
    let root = fixture_root("root-confinement");
    let outside = fixture_root("root-confinement-outside");
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(root.join("nested/inside.md"), "inside checkout\n").unwrap();
    fs::write(outside.join("secret.md"), "outside checkout\n").unwrap();

    let inside = context_query(
        &root,
        json!({
            "operation": "search",
            "output": "json",
            "options": { "query": "checkout", "root": "nested" },
            "project_root": outside,
        }),
    )
    .await;
    assert_eq!(inside.status, ExecutionStatus::Completed);
    let inside = command_json(&inside);
    assert_eq!(inside["hits"][0]["path"], "inside.md");
    assert!(!inside.to_string().contains("secret.md"));

    let absolute = context_query(
        &root,
        json!({
            "operation": "search",
            "output": "json",
            "options": { "query": "checkout", "root": outside },
        }),
    )
    .await;
    assert_eq!(absolute.status, ExecutionStatus::Blocked);
    assert_eq!(absolute.error.as_deref(), Some("command_arguments_invalid"));

    let parent = context_query(
        &root,
        json!({
            "operation": "search",
            "output": "json",
            "options": { "query": "checkout", "root": "../" },
        }),
    )
    .await;
    assert_eq!(parent.status, ExecutionStatus::Blocked);
    assert_eq!(parent.error.as_deref(), Some("command_arguments_invalid"));

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
        let symlink = context_query(
            &root,
            json!({
                "operation": "search",
                "output": "json",
                "options": { "query": "checkout", "root": "escape" },
            }),
        )
        .await;
        assert_eq!(symlink.status, ExecutionStatus::Failed);
        assert!(symlink
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("context_root_outside_project"));
    }

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}

#[tokio::test]
async fn context_query_operation_and_numeric_bounds_fail_closed() {
    let root = fixture_root("query-bounds");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("README.md"), "checkout\n").unwrap();

    let unknown = context_query(
        &root,
        json!({
            "operation": "execute",
            "output": "json",
            "options": {},
        }),
    )
    .await;
    assert_eq!(unknown.status, ExecutionStatus::Blocked);
    assert_eq!(unknown.error.as_deref(), Some("command_unregistered"));

    let oversized_limit = context_query(
        &root,
        json!({
            "operation": "search",
            "output": "json",
            "options": { "query": "checkout", "limit": 1001 },
        }),
    )
    .await;
    assert_eq!(oversized_limit.status, ExecutionStatus::Blocked);
    assert_eq!(
        oversized_limit.error.as_deref(),
        Some("command_arguments_invalid")
    );

    let oversized_bytes = context_query(
        &root,
        json!({
            "operation": "artifact_graph",
            "output": "json",
            "options": { "max_bytes_per_file": 16 * 1024 * 1024 + 1 },
        }),
    )
    .await;
    assert_eq!(oversized_bytes.status, ExecutionStatus::Blocked);
    assert_eq!(
        oversized_bytes.error.as_deref(),
        Some("command_arguments_invalid")
    );

    let _ = fs::remove_dir_all(root);
}

async fn context_query(root: &Path, arguments: Value) -> ResponseEnvelope {
    let host = Arc::new(trusted_local_host());
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = RequestMetadata::local("session-context", root.to_string_lossy());
    metadata.project_trusted = true;
    client
        .command(metadata, "context.query.v1", arguments)
        .await
        .unwrap()
}

fn trusted_metadata_for(root: &Path, session: &str) -> RequestMetadata {
    let mut metadata = RequestMetadata::local(session, root.to_string_lossy());
    metadata.project_trusted = true;
    metadata
}

fn command_text(response: &ResponseEnvelope) -> &str {
    assert_eq!(response.status, ExecutionStatus::Completed);
    response.output["command_result"]["value"].as_str().unwrap()
}

fn command_json(response: &ResponseEnvelope) -> Value {
    serde_json::from_str(command_text(response)).unwrap()
}

fn fixture_root(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-daemon-{label}-{}-{nanos}",
        std::process::id()
    ))
}
