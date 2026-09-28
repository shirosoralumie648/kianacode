//! SC-35 source guard: an evidence manifest is only worth something if it cannot be edited after
//! the fact, cannot claim a proof level above what it is about, and cannot cite a file that moved.
//!
//! The absence check is load-bearing. The module claims to be a read-only contract over supplied
//! values; the only thing that keeps that claim honest is that the file contains nothing that
//! could read a fixture, run a command, write a manifest or append an event.

#[test]
fn sc35_manifest_pins_the_five_fields_the_card_names_as_missing() {
    let source = include_str!("../src/evidence_manifest.rs");

    for marker in [
        "EvidenceManifest",
        "EvidenceFeatureStatus",
        "EvidenceProofLevel",
        "CommandInvocation",
        "EnvironmentFact",
        "FixtureRef",
        "FixtureKind",
        "FixtureRegistry",
        "EVIDENCE_MANIFEST_SCHEMA",
        "FIXTURE_REGISTRY_SCHEMA",
    ] {
        assert!(source.contains(marker), "SC-35 module lost {marker}");
    }

    // Each of the five goes missing in its own way, so each has its own refusal. Collapsing them
    // would make "no command" and "no exit code" indistinguishable to whoever has to fix it.
    for marker in [
        "evidence_manifest_command_required",
        "evidence_manifest_exit_code_required",
        "evidence_manifest_environment_required",
        "evidence_manifest_source_snapshot_invalid",
        "evidence_manifest_fixture_required",
        "evidence_manifest_limitations_required",
        "evidence_manifest_reviewer_required",
        "evidence_manifest_reviewer_self",
        "evidence_manifest_status_change_required",
    ] {
        assert!(source.contains(marker), "SC-35 module lost {marker}");
    }
}

#[test]
fn sc35_manifest_pins_the_hand_edit_defenses() {
    let source = include_str!("../src/evidence_manifest.rs");

    for marker in [
        // The seal. Without this the record is a document; with it, it is a record.
        "fn digest(&self) -> String",
        "manifest_digest",
        "evidence_manifest_digest_mismatch",
        "evidence_manifest_registry_digest_mismatch",
        // Binding a citation to a registry entry is what notices that a cassette went stale.
        "fn validate_against(&self, registry: &FixtureRegistry)",
        "evidence_manifest_fixture_digest_mismatch",
        "evidence_manifest_fixture_unknown",
        "evidence_manifest_fixture_kind_conflict",
        // The chain, so a sequence of claims is a sequence and not a pile.
        "fn verify_chain(&self, previous: Option<&EvidenceManifest>)",
        "previous_manifest_digest",
        "evidence_manifest_chain_broken",
        "evidence_manifest_chain_orphan",
        "evidence_manifest_chain_identity",
    ] {
        assert!(source.contains(marker), "SC-35 module lost {marker}");
    }
}

#[test]
fn sc35_manifest_refuses_a_claim_stronger_than_its_subject() {
    let source = include_str!("../src/evidence_manifest.rs");

    // The proof ladder is ordered, and the refusal compares across the two fields rather than
    // validating either in isolation.
    for marker in [
        "evidence_manifest_proof_above_feature",
        "EvidenceFeatureStatus::Deferred",
        "EvidenceProofLevel::Durable",
        "PartialOrd",
    ] {
        assert!(source.contains(marker), "SC-35 module lost {marker}");
    }

    // The five rungs, spelled out. A ladder with a missing rung cannot be walked.
    for marker in [
        "Source",
        "LocalBehavior",
        "Durable",
        "Live",
        "Physical",
        "local_behavior",
    ] {
        assert!(source.contains(marker), "SC-35 module lost the {marker} rung");
    }
}

#[test]
fn sc35_manifest_stays_a_read_only_contract() {
    let source = include_str!("../src/evidence_manifest.rs");

    for forbidden in [
        "std::fs",
        "File::",
        "Command::",
        "std::process",
        "TcpStream",
        "reqwest",
        "tokio",
        "spawn",
        "thread::sleep",
        "SystemTime",
        "Instant::now",
        "EventStore",
        "append_event",
    ] {
        assert!(
            !source.contains(forbidden),
            "SC-35 module gained a side-effect token: {forbidden}"
        );
    }
}
