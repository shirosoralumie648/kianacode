//! One deterministic scheduler view for packet admission, next-work and board projections.
use crate::{CellId, WorkPacket, WorkPacketStatus};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PACKET_LEASE_TTL_MS: u64 = 300_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PacketClaim {
    pub owner: CellId,
    pub owner_session_id: crate::SessionId,
    pub lease_expires_at: u64,
    pub heartbeat_at: u64,
}
impl PacketClaim {
    pub fn active(&self, now_ms: u64) -> bool {
        now_ms < self.lease_expires_at
    }
    pub fn renew(
        &mut self,
        owner: CellId,
        now_ms: u64,
        deadline: Option<u64>,
    ) -> Result<(), &'static str> {
        if self.owner != owner {
            return Err("packet_claim_owner_mismatch");
        }
        if !self.active(now_ms) || now_ms < self.heartbeat_at {
            return Err("packet_claim_expired");
        }
        self.heartbeat_at = now_ms;
        self.lease_expires_at = now_ms
            .saturating_add(PACKET_LEASE_TTL_MS)
            .min(deadline.unwrap_or(u64::MAX));
        if self.lease_expires_at <= now_ms {
            return Err("packet_deadline_expired");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PacketGraphError {
    pub code: String,
    pub packet_id: String,
    /// Normalized cycle starts at its lexicographically smallest member and repeats it at end.
    pub cycle: Vec<String>,
}
impl std::fmt::Display for PacketGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.code,
            self.packet_id,
            self.cycle.join("->")
        )
    }
}
impl std::error::Error for PacketGraphError {}

pub fn validate_dependency_dag(
    packets: &BTreeMap<String, WorkPacket>,
) -> Result<Vec<String>, PacketGraphError> {
    fn error(code: &str, id: &str) -> PacketGraphError {
        PacketGraphError {
            code: code.into(),
            packet_id: id.into(),
            cycle: Vec::new(),
        }
    }
    fn visit(
        id: &str,
        packets: &BTreeMap<String, WorkPacket>,
        done: &mut BTreeSet<String>,
        stack: &mut Vec<String>,
        order: &mut Vec<String>,
    ) -> Result<(), PacketGraphError> {
        if done.contains(id) {
            return Ok(());
        }
        if let Some(offset) = stack.iter().position(|v| v == id) {
            let mut cycle = stack[offset..].to_vec();
            let first = cycle
                .iter()
                .enumerate()
                .min_by_key(|(_, value)| *value)
                .map(|(i, _)| i)
                .unwrap_or(0);
            cycle.rotate_left(first);
            cycle.push(cycle[0].clone());
            return Err(PacketGraphError {
                code: "packet_dependency_cycle".into(),
                packet_id: cycle[0].clone(),
                cycle,
            });
        }
        stack.push(id.into());
        let deps: BTreeSet<_> = packets[id].dependencies.iter().collect();
        for dep in deps {
            visit(dep, packets, done, stack, order)?;
        }
        stack.pop();
        done.insert(id.into());
        order.push(id.into());
        Ok(())
    }
    if packets.len() > 4096 {
        return Err(error("packet_graph_too_large", ""));
    }
    for (id, packet) in packets {
        if id != &packet.id || id.trim().is_empty() {
            return Err(error("packet_graph_identity_invalid", id));
        }
        if packet.dependencies.len() != packet.dependencies.iter().collect::<BTreeSet<_>>().len() {
            return Err(error("packet_dependency_duplicate", id));
        }
        for dep in &packet.dependencies {
            if !packets.contains_key(dep) {
                return Err(error("packet_dependency_missing", dep));
            }
        }
    }
    let mut done = BTreeSet::new();
    let mut order = Vec::new();
    for id in packets.keys() {
        visit(id, packets, &mut done, &mut Vec::new(), &mut order)?;
    }
    Ok(order)
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PacketReadiness {
    pub ready: Vec<String>,
    pub blocked: BTreeMap<String, String>,
    pub expired_claims: Vec<String>,
}

pub fn ready_packets(
    packets: &BTreeMap<String, WorkPacket>,
    now_ms: u64,
) -> Result<PacketReadiness, PacketGraphError> {
    let order = validate_dependency_dag(packets)?;
    let mut result = PacketReadiness::default();
    for id in order {
        let packet = &packets[&id];
        if packet.claim.as_ref().is_some_and(|c| !c.active(now_ms)) {
            result.expired_claims.push(id.clone());
        }
        if matches!(
            packet.status,
            WorkPacketStatus::Succeeded | WorkPacketStatus::Reviewed | WorkPacketStatus::Closed
        ) {
            continue;
        }
        let reason = if matches!(
            packet.status,
            WorkPacketStatus::Failed | WorkPacketStatus::Cancelled
        ) {
            Some("packet_failed_or_cancelled".to_owned())
        } else if let Some(dep) = packet
            .dependencies
            .iter()
            .filter(|d| result.blocked.contains_key(*d))
            .min()
        {
            Some(format!("dependency_blocked:{dep}"))
        } else if let Some(dep) = packet
            .dependencies
            .iter()
            .filter(|d| {
                !matches!(
                    packets[*d].status,
                    WorkPacketStatus::Succeeded
                        | WorkPacketStatus::Reviewed
                        | WorkPacketStatus::Closed
                )
            })
            .min()
        {
            Some(format!("dependency_incomplete:{dep}"))
        } else if packet
            .deadline_unix_ms
            .is_some_and(|deadline| now_ms >= deadline)
        {
            Some("packet_deadline_expired".into())
        } else if packet.claim.as_ref().is_some_and(|c| c.active(now_ms)) {
            Some("packet_claimed".into())
        } else if !matches!(
            packet.status,
            WorkPacketStatus::Approved | WorkPacketStatus::Assigned
        ) {
            Some(format!("packet_status:{:?}", packet.status).to_lowercase())
        } else {
            None
        };
        if let Some(reason) = reason {
            result.blocked.insert(id, reason);
        } else {
            result.ready.push(id);
        }
    }
    result.ready.sort();
    result.expired_claims.sort();
    Ok(result)
}
