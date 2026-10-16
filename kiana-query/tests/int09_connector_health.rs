//! INT-09 连接器健康事件投影的行为 fixture。
//!
//! 测试在内存中构造 binding 与 health `RuntimeEvent`，检查最新事实、revision drift 和畸形/孤立事实
//! 的失败路径；不连接真实 connector adapter、凭证或 EventStore，也不证明 live health。

use kiana_domain::*;
use kiana_query::project_connector_health;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// 构造带指定配置 revision 的本地演示 binding。
fn binding(revision: u64) -> ConnectorBindingSnapshot {
    ConnectorBindingSnapshot {
        definition: ConnectorDefinition {
            schema: "kiana.connector-definition.v1".to_owned(),
            connector_id: "connector-demo".to_owned(),
            version: "v1".to_owned(),
            provider_id: "provider-demo".to_owned(),
            adapter: "local_fixture".to_owned(),
            operations: BTreeMap::from([(
                "lookup".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::ReadOnly,
                    required_scope: "read".to_owned(),
                    data_classes: BTreeSet::new(),
                },
            )]),
            rate_limit_per_minute: 10,
            idempotency_required: true,
            reconciliation_required: true,
            data_processing: "local_only".to_owned(),
        },
        binding: AccountBinding {
            schema: "kiana.account-binding.v1".to_owned(),
            binding_id: "binding-demo".to_owned(),
            connector_id: "connector-demo".to_owned(),
            account_id: "account-demo".to_owned(),
            read_scopes: BTreeSet::from(["read".to_owned()]),
            write_scopes: BTreeSet::new(),
            fixture_path: "fixtures/connector.json".to_owned(),
            fixture_sha256: format!("sha256:{}", "a".repeat(64)),
            credential_ref: None,
        },
        project_root: "/repo".to_owned(),
        revision,
        status: "active".to_owned(),
    }
}

/// 构造 sequence 与 stream version 一致的合成事件，便于投影 fixture 控制事件顺序。
fn event(sequence: u64, kind: &str, data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), sequence, kind, data)
        .unwrap()
        .with_stream_metadata(CONNECTOR_STREAM, "/repo", sequence)
}

/// 构造携带指定 binding revision 和检查时间的健康事实。
fn health(revision: u64, checked_at_unix_ms: u64) -> ConnectorHealthFact {
    ConnectorHealthFact::new(
        "connector-demo",
        "binding-demo",
        ConnectorHealthStatus::ConnectivityOnly,
        "read_only",
        checked_at_unix_ms,
        None,
        None,
        revision,
        None,
        CONNECTOR_FIXTURE_SOURCE,
        vec!["local_fixture_only".to_owned()],
    )
    .unwrap()
}

#[test]
/// 更新 binding revision 后旧 health fact 会标 stale；追加新 revision 的事实后恢复匹配。
fn projection_keeps_latest_fact_and_marks_revision_drift_stale() {
    let events = vec![
        event(1, "connector.binding", json!({"state": binding(1)})),
        event(
            2,
            CONNECTOR_HEALTH_EVENT_KIND,
            json!({"health": health(1, 1_700_000_000_000)}),
        ),
        event(3, "connector.binding", json!({"state": binding(2)})),
    ];
    let projection = project_connector_health(&events).expect("projection");
    assert_eq!(projection.source_cursor, 3);
    assert_eq!(projection.entries.len(), 1);
    assert!(projection.entries[0].stale);
    assert!(projection.entries[0]
        .limitations
        .contains(&"binding_revision_changed".to_owned()));

    let mut current = events;
    current.push(event(
        4,
        CONNECTOR_HEALTH_EVENT_KIND,
        json!({"health": health(2, 1_700_000_000_001)}),
    ));
    let projection = project_connector_health(&current).expect("latest projection");
    assert_eq!(projection.entries[0].source_cursor, 4);
    assert!(!projection.entries[0].stale);
    assert_eq!(projection.entries[0].health.binding_revision, 2);
}

#[test]
/// 无对应 binding 的健康事实和字段不完整的健康 payload 都使整份投影失败。
fn projection_fails_closed_for_foreign_or_malformed_health_facts() {
    let foreign = ConnectorHealthFact::new(
        "connector-demo",
        "binding-foreign",
        ConnectorHealthStatus::ConnectivityOnly,
        "read_only",
        1_700_000_000_000,
        None,
        None,
        1,
        None,
        CONNECTOR_FIXTURE_SOURCE,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        project_connector_health(&[
            event(1, "connector.binding", json!({"state": binding(1)})),
            event(2, CONNECTOR_HEALTH_EVENT_KIND, json!({"health": foreign}),),
        ])
        .unwrap_err(),
        "connector_health_binding_missing"
    );
    assert_eq!(
        project_connector_health(&[event(
            1,
            CONNECTOR_HEALTH_EVENT_KIND,
            json!({"health": {"schema": CONNECTOR_HEALTH_FACT_SCHEMA}}),
        )])
        .unwrap_err(),
        "connector_health_fact_invalid"
    );
}
