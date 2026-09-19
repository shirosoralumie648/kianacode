//! EQ-25 curated/deep scenario catalog with visible skips and stable dedupe.

use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const CATALOG_SCHEMA: &str = "kiana.quality-scenario-catalog.v1";
pub const MAX_CATALOG_ENTRIES: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogTier {
    Curated,
    Deep,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogStatus {
    Ready,
    NoGolden,
    Skipped,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogScenario {
    pub scenario_id: String,
    pub tier: CatalogTier,
    pub stable_order: String,
    pub dedupe_key: String,
    #[serde(default)]
    pub golden_trace_digest: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl CatalogScenario {
    fn validate(&self) -> Result<(), CatalogError> {
        if self.scenario_id.trim().is_empty()
            || self.stable_order.trim().is_empty()
            || self.dedupe_key.trim().is_empty()
            || self.scenario_id.len() > 256
            || self.stable_order.len() > 256
            || self.dedupe_key.len() > 512
            || self.scenario_id.contains(['\0', '\n', '\r'])
            || self.stable_order.contains(['\0', '\n', '\r'])
            || self.dedupe_key.contains(['\0', '\n', '\r'])
            || self.tags.len() > 32
            || self
                .tags
                .iter()
                .any(|tag| tag.trim().is_empty() || tag.len() > 64 || tag.contains(['\0', '\n']))
        {
            return Err(CatalogError::ScenarioInvalid);
        }
        if self
            .golden_trace_digest
            .as_deref()
            .is_some_and(|digest| !is_digest(digest))
        {
            return Err(CatalogError::GoldenDigestInvalid);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogOptions {
    pub include_deep: bool,
    pub max_entries: usize,
}

impl Default for CatalogOptions {
    fn default() -> Self {
        Self {
            include_deep: false,
            max_entries: MAX_CATALOG_ENTRIES,
        }
    }
}

impl CatalogOptions {
    fn validate(&self) -> Result<(), CatalogError> {
        if self.max_entries == 0 || self.max_entries > MAX_CATALOG_ENTRIES {
            return Err(CatalogError::OptionsInvalid);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub scenario_id: String,
    pub tier: CatalogTier,
    pub stable_order: String,
    pub dedupe_key: String,
    pub status: CatalogStatus,
    pub skip_reason: Option<String>,
    pub golden_trace_digest: Option<String>,
    pub duplicate_count: usize,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioCatalog {
    pub schema: String,
    pub include_deep: bool,
    pub entries: Vec<CatalogEntry>,
    pub catalog_digest: String,
}

impl ScenarioCatalog {
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.schema != CATALOG_SCHEMA
            || self.entries.is_empty()
            || self.entries.len() > MAX_CATALOG_ENTRIES
            || self.entries.windows(2).any(|pair| {
                (pair[0].stable_order.as_str(), pair[0].scenario_id.as_str())
                    > (pair[1].stable_order.as_str(), pair[1].scenario_id.as_str())
            })
        {
            return Err(CatalogError::CatalogInvalid);
        }
        let mut dedupe_keys = BTreeSet::new();
        for entry in &self.entries {
            if !dedupe_keys.insert(entry.dedupe_key.clone())
                || entry.duplicate_count == 0
                || (entry.status == CatalogStatus::Ready && entry.golden_trace_digest.is_none())
                || (entry.status == CatalogStatus::NoGolden
                    && entry.skip_reason.as_deref() != Some("no_golden_trace"))
                || (entry.status == CatalogStatus::Skipped && entry.skip_reason.is_none())
            {
                return Err(CatalogError::CatalogInvalid);
            }
        }
        if !is_digest(&self.catalog_digest) {
            return Err(CatalogError::CatalogDigestInvalid);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CatalogError {
    #[error("quality_catalog_scenario_invalid")]
    ScenarioInvalid,
    #[error("quality_catalog_golden_digest_invalid")]
    GoldenDigestInvalid,
    #[error("quality_catalog_options_invalid")]
    OptionsInvalid,
    #[error("quality_catalog_duplicate_key_invalid")]
    DuplicateKeyInvalid,
    #[error("quality_catalog_empty")]
    CatalogEmpty,
    #[error("quality_catalog_catalog_invalid")]
    CatalogInvalid,
    #[error("quality_catalog_digest_invalid")]
    CatalogDigestInvalid,
}

fn is_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn build_catalog(
    mut scenarios: Vec<CatalogScenario>,
    options: CatalogOptions,
) -> Result<ScenarioCatalog, CatalogError> {
    options.validate()?;
    if scenarios.is_empty() {
        return Err(CatalogError::CatalogEmpty);
    }
    if scenarios.len() > MAX_CATALOG_ENTRIES {
        return Err(CatalogError::OptionsInvalid);
    }
    for scenario in &scenarios {
        scenario.validate()?;
    }
    scenarios.sort_by(|left, right| {
        (
            left.stable_order.as_str(),
            left.scenario_id.as_str(),
            left.tier,
            left.dedupe_key.as_str(),
        )
            .cmp(&(
                right.stable_order.as_str(),
                right.scenario_id.as_str(),
                right.tier,
                right.dedupe_key.as_str(),
            ))
    });

    let mut grouped = BTreeMap::<String, (CatalogScenario, usize)>::new();
    for scenario in scenarios {
        grouped
            .entry(scenario.dedupe_key.clone())
            .and_modify(|(_, count)| *count += 1)
            .or_insert((scenario, 1));
    }
    let mut selected = grouped.into_values().collect::<Vec<_>>();
    selected.sort_by(|(left, _), (right, _)| {
        (
            left.stable_order.as_str(),
            left.scenario_id.as_str(),
            left.tier,
        )
            .cmp(&(
                right.stable_order.as_str(),
                right.scenario_id.as_str(),
                right.tier,
            ))
    });

    let mut entries = Vec::new();
    for (selected, duplicate_count) in selected {
        let dedupe_key = selected.dedupe_key.clone();
        let (status, skip_reason) = if selected.tier == CatalogTier::Deep && !options.include_deep {
            (
                CatalogStatus::Skipped,
                Some("deep_opt_in_required".to_owned()),
            )
        } else if selected.golden_trace_digest.is_none() {
            (CatalogStatus::NoGolden, Some("no_golden_trace".to_owned()))
        } else {
            (CatalogStatus::Ready, None)
        };
        entries.push(CatalogEntry {
            scenario_id: selected.scenario_id.clone(),
            tier: selected.tier,
            stable_order: selected.stable_order.clone(),
            dedupe_key,
            status,
            skip_reason,
            golden_trace_digest: selected.golden_trace_digest.clone(),
            duplicate_count,
            tags: selected.tags.clone(),
        });
    }
    if entries.len() > options.max_entries {
        entries.truncate(options.max_entries);
    }
    let catalog_digest = json_digest(&json!({
        "schema": CATALOG_SCHEMA,
        "include_deep": options.include_deep,
        "entries": entries.clone(),
    }));
    let catalog = ScenarioCatalog {
        schema: CATALOG_SCHEMA.to_owned(),
        include_deep: options.include_deep,
        entries,
        catalog_digest,
    };
    catalog.validate()?;
    Ok(catalog)
}
