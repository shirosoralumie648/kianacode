use kiana_domain::{IdentityMigration, IDENTITY_MIGRATION_SCHEMA};
use serde_json::json;

#[test]
fn local_user_migration_is_an_explicit_non_authorizing_fact() {
    let migration = IdentityMigration::new(
        "migration-1",
        "local-user",
        "principal-v2",
        "protected local credential enrolled",
        10,
    )
    .unwrap();
    assert_eq!(migration.schema, IDENTITY_MIGRATION_SCHEMA);
    migration.validate().unwrap();
    let mut encoded = serde_json::to_value(&migration).unwrap();
    encoded["unexpected"] = json!(true);
    assert!(serde_json::from_value::<IdentityMigration>(encoded).is_err());
    assert!(!serde_json::to_string(&migration)
        .unwrap()
        .contains("credential_value"));
}
