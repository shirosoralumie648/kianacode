use kiana_domain::{
    json_digest, ExternalResourceKind, ExternalResourceRequest, ExternalResourceSnapshot,
};
use serde_json::json;

fn digest(value: &str) -> String {
    json_digest(&json!({"value": value}))
}

fn request() -> ExternalResourceRequest {
    ExternalResourceRequest::new(
        ExternalResourceKind::Connector,
        "connector:docs/1",
        digest("locator"),
        digest("content"),
        digest("grant"),
        digest("quota"),
        digest("scope"),
        digest("scope"),
        vec!["project:code".to_owned()],
        64 * 1024,
        8,
    )
    .unwrap()
}

#[test]
fn external_resource_cannot_widen_scope() {
    let mut request = request();
    request.requested_scope_digest = digest("wider-scope");
    assert_eq!(
        request.validate().unwrap_err(),
        "external_resource_scope_widening_denied"
    );
    let mut write = request();
    write.allow_memory_write = true;
    assert_eq!(
        write.validate().unwrap_err(),
        "external_resource_scope_widening_denied"
    );
}

#[test]
fn connector_source_is_revocable() {
    let request = request();
    let mut snapshot =
        ExternalResourceSnapshot::new(&request, "connector-revision:1", 100, Some(200), 3).unwrap();
    assert!(snapshot.readable_at(150, 3));
    snapshot.revoke(160).unwrap();
    assert!(!snapshot.readable_at(150, 3));
    assert!(!snapshot.readable_at(250, 3));
    snapshot.validate().unwrap();
}
