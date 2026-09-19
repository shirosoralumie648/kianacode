use kiana_domain::{RetrievalItem, RetrievalRequest};
use kiana_ports::RetrievalPort;
use kiana_query::{rank_unified, UnifiedRetrievalPort};

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

fn items() -> Vec<RetrievalItem> {
    vec![
        RetrievalItem {
            id: "a".to_owned(),
            text: "checkout deployment".to_owned(),
            source_digest: digest('a'),
            permission_scope_digest: digest('s'),
            allowed: true,
            created_at_ms: 1,
            dense_vector: None,
        },
        RetrievalItem {
            id: "b".to_owned(),
            text: "checkout deployment".to_owned(),
            source_digest: digest('b'),
            permission_scope_digest: digest('s'),
            allowed: true,
            created_at_ms: 2,
            dense_vector: None,
        },
    ]
}

#[test]
fn cli_and_memory_tool_have_identical_rankings() {
    let request = RetrievalRequest::new("checkout", digest('s'), None).unwrap();
    let direct = rank_unified(&request, &items(), 2).unwrap();
    let port = UnifiedRetrievalPort::default()
        .retrieve(&request, &items(), 2)
        .unwrap();
    assert_eq!(direct, port);
    assert_eq!(direct.hits[0].id, "b");
}
