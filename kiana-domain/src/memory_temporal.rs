//! Deterministic memory history, freshness and conflict selection.

use crate::{
    json_digest, MemoryAclDecision, MemoryAclRequest, MemoryRecord, MemoryState, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MEMORY_TEMPORAL_SCHEMA: &str = "kiana.memory-temporal-selection.v1";
pub const MEMORY_TEMPORAL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTemporalStatus {
    Current,
    Historical,
    Conflict,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryTemporalRecord {
    pub record_id: String,
    pub revision: u64,
    pub created_at_ms: u64,
    pub status: MemoryTemporalStatus,
    pub conflict_key: Option<String>,
    pub supersedes: Option<String>,
    pub record_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryTemporalOmission {
    pub record_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryTemporalSelection {
    pub schema: String,
    pub version: SchemaVersion,
    pub query_digest: String,
    pub scope_digest: String,
    pub as_of_ms: u64,
    pub data_epoch: u64,
    pub records: Vec<MemoryTemporalRecord>,
    pub conflict_sets: Vec<Vec<String>>,
    pub omitted: Vec<MemoryTemporalOmission>,
    pub selection_digest: String,
}

impl MemoryTemporalSelection {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_TEMPORAL_SCHEMA
            || !self.version.is_compatible_with(&MEMORY_TEMPORAL_VERSION)
            || self.as_of_ms == 0
            || self.data_epoch == 0
            || self
                .records
                .windows(2)
                .any(|pair| pair[0].record_id >= pair[1].record_id)
            || self
                .omitted
                .windows(2)
                .any(|pair| pair[0].record_id >= pair[1].record_id)
            || self.conflict_sets.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("memory_temporal_header_invalid".to_owned());
        }
        digest(&self.query_digest, "memory_temporal_query_digest")?;
        digest(&self.scope_digest, "memory_temporal_scope_digest")?;
        let mut ids = BTreeSet::new();
        for record in &self.records {
            if record.record_id.trim().is_empty()
                || record.revision == 0
                || record.created_at_ms == 0
                || !ids.insert(record.record_id.clone())
            {
                return Err("memory_temporal_record_invalid".to_owned());
            }
            digest(&record.record_digest, "memory_temporal_record_digest")?;
            if let Some(key) = &record.conflict_key {
                if key.trim().is_empty() || key.len() > 512 {
                    return Err("memory_temporal_conflict_key_invalid".to_owned());
                }
            }
        }
        let mut omitted_ids = BTreeSet::new();
        for omission in &self.omitted {
            if omission.record_id.trim().is_empty()
                || omission.reason.trim().is_empty()
                || !omitted_ids.insert(omission.record_id.clone())
            {
                return Err("memory_temporal_omission_invalid".to_owned());
            }
        }
        for conflict in &self.conflict_sets {
            if conflict.len() < 2
                || conflict.windows(2).any(|pair| pair[0] >= pair[1])
                || conflict.iter().any(|id| !ids.contains(id))
            {
                return Err("memory_temporal_conflict_set_invalid".to_owned());
            }
        }
        digest(&self.selection_digest, "memory_temporal_selection_digest")?;
        if self.selection_digest != self.digest() {
            return Err("memory_temporal_selection_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "query_digest": self.query_digest,
            "scope_digest": self.scope_digest,
            "as_of_ms": self.as_of_ms,
            "data_epoch": self.data_epoch,
            "records": self.records,
            "conflict_sets": self.conflict_sets,
            "omitted": self.omitted,
        }))
    }
}

pub fn resolve_memory_history(
    records: &[MemoryRecord],
    request: &MemoryAclRequest,
    as_of_ms: u64,
) -> Result<MemoryTemporalSelection, String> {
    request.validate()?;
    if as_of_ms == 0 || as_of_ms > request.now_ms {
        return Err("memory_temporal_as_of_invalid".to_owned());
    }
    let as_of_request = MemoryAclRequest::new(
        request.scope.clone(),
        request.path,
        request.sensitivity_ceiling,
        as_of_ms,
        request.data_epoch,
    )?;
    let mut eligible = Vec::new();
    let mut omitted = Vec::new();
    for record in records {
        if record.created_at_ms > as_of_ms {
            omitted.push(MemoryTemporalOmission {
                record_id: record.id.clone(),
                reason: "created_after_as_of".to_owned(),
            });
            continue;
        }
        let decision = MemoryAclDecision::evaluate(&as_of_request, record)?;
        if !decision.allowed {
            omitted.push(MemoryTemporalOmission {
                record_id: record.id.clone(),
                reason: if decision.reason == "validity_denied" {
                    "not_valid_at_as_of".to_owned()
                } else {
                    format!("acl_denied:{}", decision.reason)
                },
            });
            continue;
        }
        eligible.push(record);
    }
    eligible.sort_by(|left, right| left.id.cmp(&right.id));
    let eligible_ids = eligible
        .iter()
        .map(|record| record.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut superseded_by = BTreeMap::new();
    for record in &eligible {
        if let Some(target) = &record.supersedes {
            if eligible_ids.contains(target.as_str()) {
                superseded_by
                    .entry(target.clone())
                    .or_insert_with(|| record.id.clone());
            }
        }
    }
    for (record_id, successor) in &superseded_by {
        omitted.push(MemoryTemporalOmission {
            record_id: record_id.clone(),
            reason: format!("superseded_by:{successor}"),
        });
    }
    let visible = eligible
        .into_iter()
        .filter(|record| !superseded_by.contains_key(&record.id))
        .collect::<Vec<_>>();
    let mut conflict_groups = BTreeMap::<String, Vec<String>>::new();
    for record in &visible {
        if !record.kind.trim().is_empty() {
            conflict_groups
                .entry(format!("{}:{}", record.collection, record.kind))
                .or_default()
                .push(record.id.clone());
        }
    }
    let mut conflict_sets = conflict_groups
        .into_values()
        .filter(|ids| ids.len() > 1)
        .map(|mut ids| {
            ids.sort();
            ids
        })
        .collect::<Vec<_>>();
    conflict_sets.sort();
    let conflict_ids = conflict_sets
        .iter()
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut selected = visible
        .into_iter()
        .map(|record| MemoryTemporalRecord {
            record_id: record.id.clone(),
            revision: record.revision,
            created_at_ms: record.created_at_ms,
            status: if conflict_ids.contains(&record.id) {
                MemoryTemporalStatus::Conflict
            } else if record.created_at_ms < as_of_ms {
                MemoryTemporalStatus::Historical
            } else {
                MemoryTemporalStatus::Current
            },
            conflict_key: (!record.kind.trim().is_empty())
                .then(|| format!("{}:{}", record.collection, record.kind)),
            supersedes: record.supersedes.clone(),
            record_digest: if record.content_hash.starts_with("sha256:") {
                record.content_hash.clone()
            } else {
                json_digest(record)
            },
        })
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| left.record_id.cmp(&right.record_id));
    omitted.sort_by(|left, right| left.record_id.cmp(&right.record_id));
    let mut selection = MemoryTemporalSelection {
        schema: MEMORY_TEMPORAL_SCHEMA.to_owned(),
        version: MEMORY_TEMPORAL_VERSION,
        query_digest: request.request_digest.clone(),
        scope_digest: request.scope.scope_digest.clone(),
        as_of_ms,
        data_epoch: request.data_epoch,
        records: selected,
        conflict_sets,
        omitted,
        selection_digest: String::new(),
    };
    selection.selection_digest = selection.digest();
    selection.validate()?;
    Ok(selection)
}
