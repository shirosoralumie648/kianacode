//! CAP-15 source guard for one typed patch plan shared by preview and commit.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-15 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn patch_preview_and_commit_share_parser_overlay_preconditions_and_digest() {
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let harness = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let baseline = include_str!("../../docs/roadmap/capability.md");
    require(
        patch,
        &[
            "preview_codex_patch",
            "apply_codex_patch",
            "parse_patch",
            "plan_hunks",
            "capture_preconditions",
            "verify_preconditions",
            "patch_preview",
            "patch_digest",
            "affected_paths",
            "before_hashes",
            "hunk_count",
            "MAX_PATCH_BYTES",
            "MAX_PATCH_HUNKS",
            "read_only",
        ],
        "shared patch plan",
    );
    require(
        harness,
        &[
            "apply_patch.preview",
            "ApplyPatchPreviewHandler",
            "prepare_patch_request",
            "preview_codex_patch",
            "AdapterCommitState::NotStarted",
        ],
        "preview capability",
    );
    require(
        baseline,
        &[
            "patch_preview_and_commit_use_identical_target_set",
            "malformed_late_hunk_makes_no_earlier_change",
            "move_source_and_target_both_require_authority",
        ],
        "CAP-15 card",
    );
    assert!(!patch.contains("parse_patch(patch)?;\n    let _lock"));
}

#[test]
fn patch_plan_never_turns_preview_into_a_write_path() {
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let preview_start = patch
        .find("pub(crate) fn preview_codex_patch")
        .expect("preview function");
    let preview_end = patch[preview_start..]
        .find("/// Publish a script")
        .map(|offset| preview_start + offset)
        .unwrap_or(patch.len());
    let preview = &patch[preview_start..preview_end];
    assert!(preview.contains("read_only"));
    assert!(!preview.contains("ProjectPatchLock::acquire"));
    assert!(!preview.contains("commit_planned"));
}
