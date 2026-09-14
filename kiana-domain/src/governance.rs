use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    Public,
    Internal,
    Confidential,
    Restricted,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Purpose {
    pub id: String,
    pub description: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Retention {
    pub expires_at_ms: Option<u64>,
    pub retain_audit_metadata: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessingGrant {
    pub id: String,
    pub source_path: String,
    pub content_hash: String,
    pub class: DataClass,
    pub purpose: Purpose,
    pub retention: Retention,
    pub parent_ids: Vec<String>,
    pub created_by: String,
    pub revoked: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DataPolicy {
    pub revision: u64,
    pub grants: BTreeMap<String, ProcessingGrant>,
    pub revoked_sources: BTreeSet<String>,
}
impl DataPolicy {
    pub fn register(&mut self, grant: ProcessingGrant) -> Result<(), String> {
        if grant.id.trim().is_empty()
            || grant.purpose.id.trim().is_empty()
            || grant.purpose.description.trim().is_empty()
            || crate::normalize_role_path(&grant.source_path).is_none()
            || grant.revoked
            || self.grants.contains_key(&grant.id)
            || self.revoked_sources.contains(&grant.source_path)
            || grant
                .parent_ids
                .iter()
                .any(|id| self.grants.get(id).is_none_or(|parent| parent.revoked))
        {
            return Err("processing_grant_invalid".to_owned());
        }
        self.grants.insert(grant.id.clone(), grant);
        self.revision += 1;
        Ok(())
    }
    pub fn revoke(&mut self, id: &str) -> Result<Vec<String>, String> {
        if !self.grants.contains_key(id) {
            return Err("processing_grant_not_found".to_owned());
        }
        let mut affected = BTreeSet::from([id.to_owned()]);
        loop {
            let before = affected.len();
            for grant in self.grants.values() {
                if grant.parent_ids.iter().any(|id| affected.contains(id)) {
                    affected.insert(grant.id.clone());
                }
            }
            if before == affected.len() {
                break;
            }
        }
        for id in &affected {
            let grant = self.grants.get_mut(id).expect("derived existing grant");
            grant.revoked = true;
            self.revoked_sources.insert(grant.source_path.clone());
        }
        self.revision += 1;
        Ok(affected.into_iter().collect())
    }
}
