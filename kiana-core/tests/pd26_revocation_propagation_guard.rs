//! PD-26 source guard: propagation is ordered, receipt-bound, epoch-fenced and never an effect.

#[test]
fn pd26_propagation_covers_every_derived_layer_in_a_fixed_order() {
    let core = include_str!("../src/revocation_propagation.rs");
    for marker in [
        "RevocationLayer",
        "RevocationKind",
        "RevocationLayerState",
        "RevocationLayerObservation",
        "RevocationPropagationRequest",
        "RevocationPropagationReport",
        "RevocationPropagationStatus",
        "revocation_derived_layer_still_serves_data",
        "revocation_layer_state_unknown",
        "revocation_layer_tombstone_not_applied",
        "revocation_layer_epoch_stale",
        "revocation_layer_order_violation",
        "revocation_propagation_incomplete",
        "revocation_observation_receipt_required",
        "revocation_observation_unreachable_with_receipt",
        "revocation_observation_pending_with_receipt",
        "revocation_request_observation_out_of_order",
        "revocation_report_completion_with_pending",
        "revocation_report_binding_invalid",
        "revocation_stale_data_epoch_write",
        "revocation_write_epoch_ahead",
        "revocation_tombstone_revival_denied",
        "revocation_recovered_epoch_superseded",
        "revocation_receipt_regression",
        "revocation_receipt_conflict",
        "revocation_receipt_layer_mismatch",
    ] {
        assert!(core.contains(marker), "PD-26 marker missing: {marker}");
    }

    // The card names the layer order. Assert it literally against the `RevocationLayer::ALL`
    // block, and not against the whole file: `as_str` repeats the same wire names, so a file-wide
    // search would pass even if the list were reordered.
    let all_at = core
        .find("pub const ALL: [Self; 6] = [")
        .expect("PD-26 must declare the full derived-layer set");
    let all_end = core[all_at..]
        .find("];")
        .map(|end| all_at + end)
        .expect("PD-26 must close the derived-layer list");
    let all = &core[all_at..all_end];
    let expected = [
        ("Facts", "\"facts\""),
        ("Artifact", "\"artifact\""),
        ("Memory", "\"memory\""),
        ("Index", "\"index\""),
        ("Cache", "\"cache\""),
        ("Checkpoint", "\"checkpoint\""),
    ];
    for (variant, wire) in expected {
        assert!(
            all.contains(&format!("Self::{variant},")),
            "PD-26 propagation order must declare {variant} in RevocationLayer::ALL"
        );
        assert!(
            core.contains(wire),
            "PD-26 must name the layer on the wire as {wire}"
        );
    }
    // And the variants appear in the order the card lists them.
    let mut cursor = 0;
    for (variant, _) in expected {
        let at = all[cursor..]
            .find(&format!("Self::{variant},"))
            .unwrap_or_else(|| panic!("PD-26 propagation order must reach {variant} in order"));
        cursor += at;
    }

    // The decision order is fixed, so the reported reason is deterministic. A layer that still
    // serves the payload is checked before every other rule, and the rules run in the documented
    // sequence: serving, unknown, pending, epoch, order, incomplete.
    let rules = [
        "revocation_derived_layer_still_serves_data",
        "revocation_layer_state_unknown",
        "revocation_layer_tombstone_not_applied",
        "revocation_layer_epoch_stale",
        "revocation_layer_order_violation",
        "revocation_propagation_incomplete",
    ];
    let mut at = 0;
    for rule in rules {
        let found = core[at..]
            .find(rule)
            .unwrap_or_else(|| panic!("PD-26 decision order must reach {rule}"));
        at += found + rule.len();
    }
}

#[test]
fn pd26_acknowledgement_is_only_a_contiguous_prefix() {
    let core = include_str!("../src/revocation_propagation.rs");
    // Credit is the leading run, not a set: an out-of-order acknowledgement must not shorten the
    // gap it left behind, so the loop breaks at the first unacknowledged layer.
    assert!(
        core.contains("fn acknowledged_prefix(request: &RevocationPropagationRequest)"),
        "PD-26 must credit acknowledgement as a contiguous prefix"
    );
    assert!(
        core.contains("done.rank() > layer.rank()"),
        "PD-26 must detect a layer that acknowledged past an unreached one"
    );
    // Completion is only reachable with an empty pending set and no reason.
    assert!(
        core.contains("self.status == RevocationPropagationStatus::Complete\n            && (!self.pending_layers.is_empty() || !self.reason.is_empty())"),
        "PD-26 must reject a completion that still names pending layers or a reason"
    );
}

#[test]
fn pd26_propagation_is_decided_not_performed() {
    let core = include_str!("../src/revocation_propagation.rs");
    // This module decides; it must not erase bytes, mutate a store or call an adapter.
    for forbidden in [
        "std::fs",
        "std::process",
        "File::",
        "tokio::",
        "EventStorePort",
        "ArtifactStorePort",
        "MemoryRecord",
        "remove_file",
        "remove_dir",
        "delete_object",
        "write_all",
        "sync_all",
        "copy(",
    ] {
        assert!(
            !core.contains(forbidden),
            "PD-26 propagation crossed the effect boundary: {forbidden}"
        );
    }
    // An object reference and a digest are what travel; payload bytes never do.
    assert!(
        core.contains("pub object_ref: String"),
        "PD-26 must carry the object reference"
    );
    assert!(
        !core.contains("pub payload: String"),
        "PD-26 must never carry a payload body"
    );
    // The tombstone itself is appended through the existing EventLog path, not from here.
    assert!(
        !core.contains("fn append"),
        "PD-26 must not append anything itself"
    );
}

#[test]
fn pd26_derived_writes_and_recovered_reads_are_epoch_fenced() {
    let core = include_str!("../src/revocation_propagation.rs");
    assert!(
        core.contains("pub fn admit_derived_write(write_data_epoch: u64, current_data_epoch: u64)"),
        "PD-26 must refuse a derived write issued under a superseded data epoch"
    );
    assert!(
        core.contains(
            "pub fn admit_derived_read_after_recovery(\n    observation: &RevocationLayerObservation,"
        ),
        "PD-26 must fence a read from a rebuilt or restored derived store"
    );
    // Recovery restores bytes, not governance: a layer restored at an older epoch is refused even
    // when it reports no leak, because the restore itself did not carry the tombstone.
    assert!(
        core.contains("observation.data_epoch < current_data_epoch"),
        "PD-26 must refuse a layer restored below the current data epoch"
    );
}
