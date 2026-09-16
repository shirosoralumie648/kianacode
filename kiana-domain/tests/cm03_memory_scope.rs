use kiana_domain::{
    AuthenticatedPrincipalRef, MemoryCollection, MemoryScope, ProjectIdentity, Purpose,
};

fn project() -> ProjectIdentity {
    ProjectIdentity::new(
        "/tmp/cm03-project",
        "/tmp/cm03-project",
        Some(1),
        Some(2),
        "trust",
    )
    .unwrap()
}

fn scope(collections: &[&str], allow_write: bool) -> MemoryScope {
    MemoryScope::new(
        AuthenticatedPrincipalRef::local(),
        project(),
        "session-cm03",
        collections
            .iter()
            .map(|collection| MemoryCollection::parse(collection).unwrap())
            .collect(),
        Purpose {
            id: "context.read".to_owned(),
            description: "CM-03 fixture".to_owned(),
        },
        allow_write,
    )
    .unwrap()
}

#[test]
fn read_scope_is_intersection_of_all_grants() {
    let parent = scope(&["project", "department:executing"], true);
    let child = scope(&["project:code"], true);
    let effective = parent.intersect(&child).unwrap();
    assert!(effective.allows_collection(&MemoryCollection::parse("project:code").unwrap()));
    assert!(!effective.allows_collection(&MemoryCollection::parse("department:executing").unwrap()));
    assert!(effective.scope_digest != parent.scope_digest);
}

#[test]
fn write_scope_cannot_be_widened_by_context_text() {
    let read_only = scope(&["project:code"], false);
    let write_requested = scope(&["project:code"], true);
    let effective = read_only.intersect(&write_requested).unwrap();
    assert!(!effective.allow_write);
    assert!(effective.allows_collection(&MemoryCollection::parse("project:code").unwrap()));

    let other_session = MemoryScope::new(
        AuthenticatedPrincipalRef::local(),
        project(),
        "other-session",
        vec![MemoryCollection::parse("project").unwrap()],
        Purpose {
            id: "context.read".to_owned(),
            description: "CM-03 fixture".to_owned(),
        },
        true,
    )
    .unwrap();
    assert!(read_only.intersect(&other_session).is_err());
}
