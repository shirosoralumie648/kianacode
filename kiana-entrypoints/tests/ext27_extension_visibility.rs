//! EXT-27 entry projection: CLI, Workbench, Web and Desktop read the same server-owned, redacted
//! visibility snapshot from the shared `DaemonHost` route.
//!
//! Deny paths come first. An unknown-trust project gets no projection at all and the refusal
//! carries no project material; a bounded search cannot reach a Skill body, locator or secret;
//! and a cached identity is refused once the material it was minted from changed. Only then are
//! the four surfaces compared for identity, generation and digest.
//!
//! `KIANA_HOME` is process-global and read on every trust lookup, so both fixtures serialize on a
//! file-local lock and restore the variable on drop. Local tests are not run in this session
//! (CI-only); the manual lane is `.github/workflows/ext27-extension-visibility.yml`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use kiana_daemon::{DaemonHost, StoredProjectTrustAuthority};
use kiana_entrypoints::extension_projection::{
    extension_visibility_on_host, visibility_action_intent, visibility_json,
};
use kiana_protocol::{
    EntryPointKind, ExecutionStatus, ExtensionVisibilityActionKind, ExtensionVisibilityRisk,
    ExtensionVisibilitySnapshot, ExtensionVisibilityTrust, EXTENSION_MANAGE_OPERATION,
};
use serde_json::{json, Value};

/// The Skill the fixture project ships; its body is never part of the projection.
const VISIBLE_SKILL: &str = "ext27-visible-skill";
/// A second Skill that appears later, changing the projection material.
const LATER_SKILL: &str = "ext27-later-skill";
const HIDDEN_BODY_TOKEN: &str = "ext27-hidden-body-token-3f9a";
const HIDDEN_PATH: &str = "/opt/ext27/private/skills/secret-source.pem";
const HIDDEN_SECRET: &str = "ext27-hidden-secret-8811";

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    _lock: MutexGuard<'static, ()>,
    original_home: Option<OsString>,
    root: PathBuf,
    project: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-ext27-{label}-{}-{stamp}",
            std::process::id()
        ));
        let home = root.join("kiana-home");
        fs::create_dir_all(&home).expect("fixture kiana home");
        let project = root.join("project");
        fs::create_dir_all(project.join(".git")).expect("fixture project");
        let original_home = std::env::var_os("KIANA_HOME");
        std::env::set_var("KIANA_HOME", &home);
        Self {
            _lock: lock,
            original_home,
            root,
            project,
        }
    }

    fn trust(&self, trust: kiana_types::ProjectTrust) {
        kiana_types::write_project_trust(&self.project, trust).expect("fixture trust record");
    }

    fn options(&self) -> HashMap<String, Value> {
        let mut options = HashMap::new();
        options.insert(
            "cwd".to_owned(),
            Value::String(self.project.to_string_lossy().into_owned()),
        );
        options
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        match self.original_home.as_ref() {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture_host() -> Arc<DaemonHost> {
    Arc::new(
        DaemonHost::with_env_harness_and_project_authority(Arc::new(StoredProjectTrustAuthority))
            .expect("fixture daemon host"),
    )
}

/// Skill bodies carry body/locator/secret material a projection must never expose.
fn hidden_skill_body() -> String {
    format!(
        "---\ndescription: Ext27 visible summary api_key={HIDDEN_SECRET}\n---\n{HIDDEN_BODY_TOKEN}\n{HIDDEN_PATH}\n"
    )
}

fn write_skill(project: &Path, name: &str, body: &str) {
    let dir = project.join(".claude").join("skills").join(name);
    fs::create_dir_all(&dir).expect("fixture skill dir");
    fs::write(dir.join("SKILL.md"), body).expect("fixture skill body");
}

async fn list_on(
    host: &Arc<DaemonHost>,
    fixture: &Fixture,
    surface: EntryPointKind,
    session: &str,
) -> anyhow::Result<ExtensionVisibilitySnapshot> {
    extension_visibility_on_host(
        host.clone(),
        session.to_owned(),
        surface,
        "list",
        None,
        None,
        128,
        &fixture.options(),
    )
    .await
}

async fn search_on(
    host: &Arc<DaemonHost>,
    fixture: &Fixture,
    surface: EntryPointKind,
    session: &str,
    query: &str,
) -> anyhow::Result<ExtensionVisibilitySnapshot> {
    extension_visibility_on_host(
        host.clone(),
        session.to_owned(),
        surface,
        "search",
        Some(query.to_owned()),
        None,
        128,
        &fixture.options(),
    )
    .await
}

fn assert_no_hidden_material(text: &str, context: &str) {
    for marker in [
        HIDDEN_BODY_TOKEN,
        HIDDEN_PATH,
        HIDDEN_SECRET,
        "SKILL.md",
        ".claude",
    ] {
        assert!(
            !text.contains(marker),
            "{context} leaked project material: {marker}"
        );
    }
}

/// Deny-first: an unknown-trust project has no projection, and neither the raw protocol refusal
/// nor any adapter error may echo project material back to a surface.
#[tokio::test]
async fn unknown_trust_project_has_no_projection_and_the_refusal_leaks_nothing() {
    let fixture = Fixture::new("untrusted");
    write_skill(&fixture.project, VISIBLE_SKILL, &hidden_skill_body());
    let host = fixture_host();
    let options = fixture.options();

    // The exact wire route a surface uses, read raw: only the envelope proves which authority
    // refused the projection and that no output was minted. A policy Deny is returned as
    // `Denied` (not `Blocked`) with the stable reason in `error`.
    let (client, metadata) =
        kiana_entrypoints::harness_run::client_on_host(host.clone(), "ext27-untrusted", &options)
            .expect("fixture client");
    let response = client
        .command(
            metadata,
            EXTENSION_MANAGE_OPERATION,
            json!({"action": "list", "max_results": 128}),
        )
        .await
        .expect("transport");
    assert_eq!(response.status, ExecutionStatus::Denied);
    assert_eq!(response.error.as_deref(), Some("project_untrusted"));
    assert!(response.output.is_null());
    let raw = serde_json::to_string(&response).expect("raw refusal");
    assert_no_hidden_material(&raw, "the raw refusal");
    assert!(!raw.contains(VISIBLE_SKILL));

    // The shared adapter surfaces the same refusal; no surface renders a projection that was
    // never minted.
    for surface in [
        EntryPointKind::Cli,
        EntryPointKind::Workbench,
        EntryPointKind::Web,
        EntryPointKind::Desktop,
    ] {
        let error = list_on(&host, &fixture, surface, "ext27-untrusted-surface")
            .await
            .expect_err("an unknown-trust project has no projection");
        let text = error.to_string();
        assert_no_hidden_material(&text, "the adapter refusal");
        assert!(!text.contains(VISIBLE_SKILL));
    }
}

/// Deny-first: bounded searches stay inside the redacted descriptor, the four surfaces read the
/// identical projection, and a cached identity is refused once the material changed.
#[tokio::test]
async fn four_surfaces_read_one_redacted_snapshot_and_stale_caches_are_fenced() {
    let fixture = Fixture::new("parity");
    fixture.trust(kiana_types::ProjectTrust::Trusted);
    write_skill(&fixture.project, VISIBLE_SKILL, &hidden_skill_body());
    let host = fixture_host();

    // Bodies, locators and secrets are not searchable even for a trusted project.
    for query in [HIDDEN_BODY_TOKEN, HIDDEN_PATH, HIDDEN_SECRET] {
        let searched = search_on(&host, &fixture, EntryPointKind::Cli, "ext27-search", query)
            .await
            .expect("bounded search");
        assert!(
            searched.entries.is_empty(),
            "hidden material must not be searchable: {query}"
        );
        assert_no_hidden_material(
            &serde_json::to_string(&searched).expect("encoded search"),
            "the bounded search",
        );
    }

    // The search that must match finds one redacted descriptor, not the Skill body.
    let found = search_on(
        &host,
        &fixture,
        EntryPointKind::Workbench,
        "ext27-search-hit",
        VISIBLE_SKILL,
    )
    .await
    .expect("bounded search hit");
    assert_eq!(
        found
            .entries
            .iter()
            .filter(|entry| entry.extension_id == VISIBLE_SKILL)
            .count(),
        1
    );
    let skill = found
        .entries
        .iter()
        .find(|entry| entry.extension_id == VISIBLE_SKILL)
        .expect("visible skill entry");
    assert_eq!(skill.source.trust, ExtensionVisibilityTrust::Trusted);
    assert_eq!(skill.risk, ExtensionVisibilityRisk::ReadOnly);
    let encoded = serde_json::to_string(&found).expect("encoded search hit");
    assert!(
        encoded.contains("Ext27 visible summary"),
        "the redacted summary stays readable"
    );
    assert!(
        !encoded.contains(HIDDEN_SECRET),
        "a secret-shaped summary value must be redacted"
    );
    assert_no_hidden_material(&encoded, "the search hit");

    // The four product surfaces read the identical projection: identity, generation and digest
    // match, and the surface label never selects a different projection.
    let surfaces = [
        (EntryPointKind::Cli, "ext27-cli"),
        (EntryPointKind::Workbench, "ext27-workbench"),
        (EntryPointKind::Web, "ext27-web"),
        (EntryPointKind::Desktop, "ext27-desktop"),
    ];
    let mut snapshots = Vec::new();
    for (surface, session) in surfaces {
        let snapshot = list_on(&host, &fixture, surface, session)
            .await
            .expect("shared visibility projection");
        visibility_json(&snapshot).expect("projection is serializable metadata only");
        snapshots.push((surface, snapshot));
    }
    let reference = &snapshots[0].1;
    for (surface, snapshot) in &snapshots {
        assert_eq!(
            snapshot.snapshot_id, reference.snapshot_id,
            "{surface:?} must read the same snapshot id"
        );
        assert_eq!(
            snapshot.generation, reference.generation,
            "{surface:?} must read the same generation"
        );
        assert_eq!(
            snapshot.snapshot_digest, reference.snapshot_digest,
            "{surface:?} must read the same projection digest"
        );
        assert_eq!(
            snapshot.entries, reference.entries,
            "{surface:?} must read the same entries"
        );
    }

    // Material changes: a second Skill appears in the project. The cached identity must be
    // refused instead of letting a stale surface reuse it as an authority.
    write_skill(
        &fixture.project,
        LATER_SKILL,
        "---\ndescription: Ext27 later summary\n---\nLater skill body\n",
    );
    kiana_skills::invalidate_extension_snapshots("ext27 fixture material change");
    let later = list_on(&host, &fixture, EntryPointKind::Web, "ext27-web-later")
        .await
        .expect("projection after the material change");
    assert_ne!(
        reference.snapshot_id, later.snapshot_id,
        "a material change must change the projection identity"
    );
    assert!(
        later
            .inspect(LATER_SKILL)
            .expect("inspect the later skill")
            .is_some(),
        "the new material must be visible to every surface"
    );
    assert_eq!(
        later
            .require_generation(reference.snapshot_id, reference.generation)
            .expect_err("the cached identity is stale"),
        "extension_visibility_snapshot_stale"
    );
    assert_eq!(
        reference
            .require_generation(later.snapshot_id, later.generation)
            .expect_err("a newer projection is not a valid precondition"),
        "extension_visibility_snapshot_stale"
    );
    later
        .require_generation(later.snapshot_id, later.generation)
        .expect("the live pair passes its own fence");

    // Labels are intents bound to the projection they were minted from; an action the entry does
    // not offer is denied instead of being inferred, and an unlisted extension has no intent.
    let intent = visibility_action_intent(
        &later,
        VISIBLE_SKILL,
        ExtensionVisibilityActionKind::Inspect,
        "EXT-27 parity fixture",
    )
    .expect("read-only intent");
    assert_eq!(intent.snapshot_id, later.snapshot_id);
    assert_eq!(intent.snapshot_digest, later.snapshot_digest);
    assert_eq!(
        visibility_action_intent(
            &later,
            VISIBLE_SKILL,
            ExtensionVisibilityActionKind::Revoke,
            "EXT-27 parity fixture",
        )
        .expect_err("a Skill row does not offer revoke")
        .to_string(),
        "extension_visibility_action_denied"
    );
    assert_eq!(
        visibility_action_intent(
            &later,
            "ext27-absent-skill",
            ExtensionVisibilityActionKind::Inspect,
            "EXT-27 parity fixture",
        )
        .expect_err("an unlisted extension has no intent")
        .to_string(),
        "extension_visibility_not_found"
    );
}
