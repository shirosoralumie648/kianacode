//! Read-only connector quota projection and replay view.
//!
//! This projector rebuilds quota state from connector quota facts only. It never claims a worker,
//! releases a reservation, resolves credentials or turns a projection into authorization.

use kiana_domain::{
    ConnectorQuotaReservation, ConnectorQuotaSettlement, RuntimeEvent,
    CONNECTOR_QUOTA_EVENT_CLAIMED, CONNECTOR_QUOTA_EVENT_RELEASED, CONNECTOR_QUOTA_EVENT_RESERVED,
    CONNECTOR_QUOTA_EVENT_SETTLED, CONNECTOR_QUOTA_RESERVATION_SCHEMA,
    CONNECTOR_QUOTA_SETTLEMENT_SCHEMA,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONNECTOR_QUOTA_PROJECTION_SCHEMA: &str = "kiana.connector-quota-projection.v1";
pub const CONNECTOR_QUOTA_PROJECTION_VERSION: &str = "connector-quota.v1";
const MAX_QUOTA_ENTRIES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorQuotaProjectionEntry {
    pub reservation: ConnectorQuotaReservation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settlement: Option<ConnectorQuotaSettlement>,
    pub source_cursor: u64,
    pub projection_version: String,
    pub replayable: bool,
    pub proof_level: String,
}

impl ConnectorQuotaProjectionEntry {
    pub fn validate(&self) -> Result<(), String> {
        self.reservation.validate()?;
        if let Some(settlement) = &self.settlement {
            settlement.validate_against(&self.reservation)?;
        }
        if self.source_cursor == 0
            || self.projection_version != CONNECTOR_QUOTA_PROJECTION_VERSION
            || !self.replayable
            || self.proof_level != "source"
        {
            return Err("connector_quota_projection_entry_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorQuotaProjection {
    pub schema: String,
    pub source_cursor: u64,
    pub projection_version: String,
    pub entries: Vec<ConnectorQuotaProjectionEntry>,
    pub limitations: Vec<String>,
}

impl ConnectorQuotaProjection {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_QUOTA_PROJECTION_SCHEMA
            || self.source_cursor == 0
            || self.projection_version != CONNECTOR_QUOTA_PROJECTION_VERSION
            || self.entries.len() > MAX_QUOTA_ENTRIES
            || self.limitations.iter().any(|value| {
                value.trim().is_empty() || value.len() > 256 || value.contains(['\0', '\r', '\n'])
            })
        {
            return Err("connector_quota_projection_invalid".to_owned());
        }
        let mut ids = std::collections::BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !ids.insert(entry.reservation.reservation_id) {
                return Err("connector_quota_projection_duplicate_reservation".to_owned());
            }
        }
        Ok(())
    }
}

/// Rebuild latest reservation/settlement state. Unknown and released facts remain visible rather
/// than being silently treated as available capacity; malformed facts stop the projection.
pub fn project_connector_quota(
    events: &[RuntimeEvent],
) -> Result<ConnectorQuotaProjection, String> {
    let mut reservations: BTreeMap<
        kiana_domain::QuotaReservationId,
        (u64, ConnectorQuotaReservation),
    > = BTreeMap::new();
    let mut settlements: BTreeMap<kiana_domain::QuotaReservationId, ConnectorQuotaSettlement> =
        BTreeMap::new();
    let mut source_cursor = 0;
    for event in events {
        let cursor = event.stream_version.unwrap_or_default();
        source_cursor = source_cursor.max(cursor);
        match event.kind.as_str() {
            CONNECTOR_QUOTA_EVENT_RESERVED
            | CONNECTOR_QUOTA_EVENT_CLAIMED
            | CONNECTOR_QUOTA_EVENT_SETTLED
            | CONNECTOR_QUOTA_EVENT_RELEASED => {
                let reservation: ConnectorQuotaReservation =
                    serde_json::from_value(event.data["reservation"].clone())
                        .map_err(|_| "connector_quota_reservation_invalid".to_owned())?;
                if reservation.schema != CONNECTOR_QUOTA_RESERVATION_SCHEMA {
                    return Err("connector_quota_reservation_schema_invalid".to_owned());
                }
                reservation.validate()?;
                let id = reservation.reservation_id;
                if event.kind == CONNECTOR_QUOTA_EVENT_SETTLED {
                    let settlement: ConnectorQuotaSettlement =
                        serde_json::from_value(event.data["settlement"].clone())
                            .map_err(|_| "connector_quota_settlement_invalid".to_owned())?;
                    if settlement.schema != CONNECTOR_QUOTA_SETTLEMENT_SCHEMA {
                        return Err("connector_quota_settlement_schema_invalid".to_owned());
                    }
                    settlement.validate_against(&reservation)?;
                    settlements.insert(id, settlement);
                }
                reservations.insert(id, (cursor, reservation));
            }
            _ => {}
        }
    }
    if source_cursor == 0 {
        source_cursor = 1;
    }
    let mut entries = reservations
        .into_iter()
        .map(
            |(id, (cursor, reservation))| ConnectorQuotaProjectionEntry {
                settlement: settlements.remove(&id),
                reservation,
                source_cursor: cursor,
                projection_version: CONNECTOR_QUOTA_PROJECTION_VERSION.to_owned(),
                replayable: true,
                proof_level: "source".to_owned(),
            },
        )
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.reservation.reservation_id);
    let limitations = if entries.is_empty() {
        vec!["connector_quota_not_observed".to_owned()]
    } else {
        vec!["projection_is_not_authorization".to_owned()]
    };
    let projection = ConnectorQuotaProjection {
        schema: CONNECTOR_QUOTA_PROJECTION_SCHEMA.to_owned(),
        source_cursor,
        projection_version: CONNECTOR_QUOTA_PROJECTION_VERSION.to_owned(),
        entries,
        limitations,
    };
    projection.validate()?;
    Ok(projection)
}
