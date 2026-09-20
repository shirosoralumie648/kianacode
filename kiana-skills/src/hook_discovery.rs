use crate::manifest::{NormalizedHookDescriptor, NormalizedHookEvent};
use kiana_domain::json_digest;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cmp::Ordering;

pub const HOOK_DISCOVERY_SCHEMA: &str = "kiana.hook-discovery.v1";

#[derive(Clone, Debug)]
pub struct HookDiscoveryCandidate {
    pub descriptor: NormalizedHookDescriptor,
    pub source_priority: u16,
    pub declaration_order: u32,
}

#[derive(Clone, Debug)]
pub struct HookDiscoveryRequest {
    pub event: NormalizedHookEvent,
    pub subject: String,
    pub input: Value,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookMatchPhase {
    Guard,
    Observer,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookMatchRecord {
    pub hook_id: String,
    pub matched: bool,
    pub reason: String,
    pub phase: HookMatchPhase,
    pub source_digest: String,
    pub source_priority: u16,
    pub declaration_order: u32,
    pub specificity: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookDiscoverySnapshot {
    pub schema: String,
    pub event: NormalizedHookEvent,
    pub input_digest: String,
    pub ordered: Vec<HookMatchRecord>,
    pub matched_ids: Vec<String>,
    pub guard_ids: Vec<String>,
    pub observer_ids: Vec<String>,
    pub snapshot_digest: String,
}

pub fn discover_hooks(
    candidates: &[HookDiscoveryCandidate],
    request: &HookDiscoveryRequest,
) -> Result<HookDiscoverySnapshot, String> {
    if request.subject.len() > 4_096 || request.subject.contains('\0') {
        return Err("hook_discovery_subject_invalid".to_owned());
    }
    if candidates.len() > 256 {
        return Err("hook_discovery_limit_exceeded".to_owned());
    }
    let input_digest = json_digest(&request.input);
    let mut records = candidates
        .iter()
        .map(|candidate| match_candidate(candidate, request))
        .collect::<Result<Vec<_>, _>>()?;
    records.sort_by(compare_records);
    let matched_ids = records
        .iter()
        .filter(|record| record.matched)
        .map(|record| record.hook_id.clone())
        .collect::<Vec<_>>();
    let guard_ids = records
        .iter()
        .filter(|record| record.matched && record.phase == HookMatchPhase::Guard)
        .map(|record| record.hook_id.clone())
        .collect::<Vec<_>>();
    let observer_ids = records
        .iter()
        .filter(|record| record.matched && record.phase == HookMatchPhase::Observer)
        .map(|record| record.hook_id.clone())
        .collect::<Vec<_>>();
    let mut snapshot = HookDiscoverySnapshot {
        schema: HOOK_DISCOVERY_SCHEMA.to_owned(),
        event: request.event,
        input_digest,
        ordered: records,
        matched_ids,
        guard_ids,
        observer_ids,
        snapshot_digest: String::new(),
    };
    snapshot.snapshot_digest = snapshot.digest();
    Ok(snapshot)
}

impl HookDiscoverySnapshot {
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "event": self.event,
            "input_digest": self.input_digest,
            "ordered": self.ordered,
            "matched_ids": self.matched_ids,
            "guard_ids": self.guard_ids,
            "observer_ids": self.observer_ids,
        }))
    }
}

fn match_candidate(
    candidate: &HookDiscoveryCandidate,
    request: &HookDiscoveryRequest,
) -> Result<HookMatchRecord, String> {
    let descriptor = &candidate.descriptor;
    let phase = match descriptor.phase {
        kiana_domain::HookPhase::Guard => HookMatchPhase::Guard,
        kiana_domain::HookPhase::Observer => HookMatchPhase::Observer,
    };
    let specificity = descriptor
        .matcher
        .chars()
        .filter(|character| !matches!(character, '*' | '?' | '[' | ']'))
        .count();
    if descriptor.event != request.event {
        return Ok(HookMatchRecord {
            hook_id: descriptor.id.clone(),
            matched: false,
            reason: "event_mismatch".to_owned(),
            phase,
            source_digest: descriptor.source_digest.clone(),
            source_priority: candidate.source_priority,
            declaration_order: candidate.declaration_order,
            specificity,
        });
    }
    let (matched, reason) = match compile_matcher(&descriptor.matcher) {
        Ok(matcher) => (
            matcher.is_match(&request.subject),
            "matcher_evaluated".to_owned(),
        ),
        Err(_) => (false, "matcher_invalid".to_owned()),
    };
    Ok(HookMatchRecord {
        hook_id: descriptor.id.clone(),
        matched,
        reason,
        phase,
        source_digest: descriptor.source_digest.clone(),
        source_priority: candidate.source_priority,
        declaration_order: candidate.declaration_order,
        specificity,
    })
}

fn compile_matcher(matcher: &str) -> Result<Regex, regex::Error> {
    if matcher.len() > 1_024 {
        return Regex::new("(?!)");
    }
    if let Some(pattern) = matcher.strip_prefix("re:") {
        return Regex::new(pattern);
    }
    let mut pattern = String::from("^");
    let mut chars = matcher.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '*' => pattern.push_str(".*"),
            '?' => pattern.push('.'),
            _ => pattern.push_str(&regex::escape(&character.to_string())),
        }
    }
    pattern.push('$');
    Regex::new(&pattern)
}

fn compare_records(left: &HookMatchRecord, right: &HookMatchRecord) -> Ordering {
    left.phase
        .cmp(&right.phase)
        .then_with(|| left.source_priority.cmp(&right.source_priority))
        .then_with(|| right.specificity.cmp(&left.specificity))
        .then_with(|| left.declaration_order.cmp(&right.declaration_order))
        .then_with(|| left.hook_id.cmp(&right.hook_id))
}
