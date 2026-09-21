use kiana_query::{QueryDataBoundary, QueryDataDisposition};

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

#[test]
fn allowed_boundary_covers_memory_index_cache_and_explicit_export() {
    let boundary = QueryDataBoundary::derive("/repo", digest('a'), 4, false, true, true).unwrap();
    assert_eq!(boundary.memory, QueryDataDisposition::Allowed);
    assert_eq!(boundary.index, QueryDataDisposition::Allowed);
    assert_eq!(boundary.cache, QueryDataDisposition::Allowed);
    assert_eq!(boundary.export, QueryDataDisposition::Allowed);
}

#[test]
fn revoked_or_retention_blocked_boundary_denies_all_derived_views() {
    for (revoked, retention) in [(true, true), (false, false)] {
        let boundary =
            QueryDataBoundary::derive("/repo", digest('a'), 4, revoked, retention, true).unwrap();
        assert_eq!(boundary.memory, QueryDataDisposition::Denied);
        assert_eq!(boundary.index, QueryDataDisposition::Denied);
        assert_eq!(boundary.cache, QueryDataDisposition::Denied);
        assert_eq!(boundary.export, QueryDataDisposition::Denied);
        boundary.validate().unwrap();
    }
}
