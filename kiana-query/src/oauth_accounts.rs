//! Redaction-safe OAuth account query projection.
//!
//! Queries expose lifecycle metadata and digests only. They never return SecretRef keys or raw
//! access/refresh material and do not mint authority for connector execution.

use kiana_domain::{OAuthAccountProjection, OAuthAccountRecord, ProviderAccountId};
use std::collections::BTreeMap;

pub const OAUTH_ACCOUNT_QUERY_SCHEMA: &str = "kiana.oauth-account-query.v1";

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthAccountQueryPage {
    pub schema: String,
    pub accounts: Vec<OAuthAccountProjection>,
    pub source_cursor: u64,
    pub projection_version: String,
    pub stale: bool,
    pub limitations: Vec<String>,
}

pub fn project_oauth_accounts(
    records: impl IntoIterator<Item = OAuthAccountRecord>,
    now_unix_ms: u64,
    source_cursor: u64,
) -> Result<OAuthAccountQueryPage, String> {
    if source_cursor == 0 {
        return Err("oauth_account_source_cursor_invalid".to_owned());
    }
    let mut by_id = BTreeMap::<ProviderAccountId, OAuthAccountProjection>::new();
    for record in records {
        let projection = OAuthAccountProjection::from_record(&record, now_unix_ms)?;
        if let Some(previous) = by_id.insert(projection.account_id, projection.clone()) {
            if previous.account_digest != projection.account_digest {
                return Err("oauth_account_projection_conflict".to_owned());
            }
        }
    }
    Ok(OAuthAccountQueryPage {
        schema: OAUTH_ACCOUNT_QUERY_SCHEMA.to_owned(),
        accounts: by_id.into_values().collect(),
        source_cursor,
        projection_version: OAUTH_ACCOUNT_QUERY_SCHEMA.to_owned(),
        stale: false,
        limitations: vec![
            "metadata_only_no_provider_request".to_owned(),
            "source_cursor_is_adapter_supplied".to_owned(),
        ],
    })
}
