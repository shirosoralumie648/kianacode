//! CAP-30 failure-first fixtures: one test per rejected-first item on the card.
//!
//! Each test names the exact rejection it proves. These are decision fixtures over supplied
//! facts, not runtime tests: nothing here loads an extension, verifies a signature, starts a
//! broker or expands a schema into a real model prompt.

use kiana_domain::{
    check_manifest_effect, search_tool_catalog, shadows_builtin_descriptor, tool_schemas,
    ExtensionEffect, ToolCatalogSnapshot, ToolDiscoveryCandidate, ToolDiscoveryDenial,
    ToolDiscoveryHealth, ToolDiscoveryOrigin, ToolDiscoveryPlan, ToolDiscoveryReport,
    ToolDiscoveryStatus, ToolDiscoveryTrust, TOOL_CATALOG_VERSION,
};
use serde_json::json;
use std::collections::BTreeSet;

fn schema() -> serde_json::Value {
    json!({"type": "object", "properties": {"path": {"type": "string"}}})
}

fn roles(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn catalog() -> ToolCatalogSnapshot {
    ToolCatalogSnapshot::current()
}

fn plan(role: &str, ceiling: ExtensionEffect) -> ToolDiscoveryPlan {
    ToolDiscoveryPlan::new(role, "project-1", ceiling, &catalog(), 16, 8_192).expect("plan")
}

fn read_only_candidate(name: &str) -> ToolDiscoveryCandidate {
    ToolDiscoveryCandidate::new(
        name,
        ToolDiscoveryOrigin::Extension,
        "ext-report",
        ToolDiscoveryTrust::Verified,
        ToolDiscoveryHealth::Healthy,
        ExtensionEffect::ReadOnly,
        "query",
        format!("ext.{name}.read"),
        false,
        roles(&["builder"]),
        BTreeSet::from([name.to_owned()]),
        schema(),
    )
    .expect("candidate")
}

#[test]
fn tool_search_never_returns_invisible_or_untrusted_descriptors() {
    // Untrusted source: the whole descriptor is absent, not filtered field-by-field.
    let mut untrusted = read_only_candidate("report.render");
    untrusted.trust = ToolDiscoveryTrust::Unverified;
    let report =
        ToolDiscoveryReport::evaluate(&plan("builder", ExtensionEffect::ReadOnly), &[untrusted])
            .expect("report");
    assert!(!report.admitted());
    assert_eq!(report.status, ToolDiscoveryStatus::Empty);
    assert_eq!(
        report.denials.get("report.render"),
        Some(&ToolDiscoveryDenial::SourceUntrusted)
    );
    assert!(report.admitted_tools.is_empty());

    // Unbound role: the descriptor is verified but the server never bound it for this role.
    let unbound = read_only_candidate("report.render");
    let other_role_plan = plan("reviewer", ExtensionEffect::ReadOnly);
    let report = ToolDiscoveryReport::evaluate(&other_role_plan, &[unbound]).expect("report");
    assert_eq!(
        report.denials.get("report.render"),
        Some(&ToolDiscoveryDenial::RoleNotBound)
    );

    // Degraded and stale sources are absences of proof, so they are denied before ranking.
    for (health, expected) in [
        (
            ToolDiscoveryHealth::Degraded,
            ToolDiscoveryDenial::SourceUnhealthy,
        ),
        (
            ToolDiscoveryHealth::Stale,
            ToolDiscoveryDenial::SourceUnhealthy,
        ),
    ] {
        let mut degraded = read_only_candidate("report.render");
        degraded.health = health;
        let report =
            ToolDiscoveryReport::evaluate(&plan("builder", ExtensionEffect::ReadOnly), &[degraded])
                .expect("report");
        assert_eq!(report.denials.get("report.render"), Some(&expected));
    }

    // A catalog registered against a different version is not searchable against this one.
    let mut stale_catalog = read_only_candidate("report.render");
    stale_catalog.catalog_version = kiana_domain::SchemaVersion::new(9, 9);
    let report = ToolDiscoveryReport::evaluate(
        &plan("builder", ExtensionEffect::ReadOnly),
        &[stale_catalog],
    )
    .expect("report");
    assert_eq!(
        report.denials.get("report.render"),
        Some(&ToolDiscoveryDenial::CatalogVersionStale)
    );

    // The existing search executor keeps its own guarantee: results are bounded, versioned and
    // explicitly do not grant execution.
    let allowed = tool_schemas()
        .into_iter()
        .filter_map(|schema| schema["name"].as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let response = search_tool_catalog(
        "memory",
        &allowed,
        4,
        128 * 1024,
        &kiana_domain::ToolSearchOptions {
            catalog_version: TOOL_CATALOG_VERSION,
            max_context_tokens: 8_192,
            require_healthy_catalog: true,
            replay_safe_only: false,
        },
    )
    .expect("search");
    assert!(response.does_not_grant_execution);
    assert!(response
        .tools
        .iter()
        .all(|tool| allowed.contains(&tool["name"].as_str().unwrap_or_default().to_owned())));
}

#[test]
fn extension_cannot_self_grant_or_replace_builtin_binding() {
    // A descriptor claiming an existing built-in name is a binding conflict.
    let shadow = read_only_candidate("shell");
    let report = ToolDiscoveryReport::evaluate(
        &plan("builder", ExtensionEffect::ReadOnly),
        &[shadow.clone()],
    )
    .expect("report");
    assert_eq!(
        report.denials.get("shell"),
        Some(&ToolDiscoveryDenial::BuiltinBindingConflict)
    );
    assert!(!report.admitted());

    // A different name but the same operation is still a takeover of the binding.
    let renamed = read_only_candidate("shell.fast");
    let report =
        ToolDiscoveryReport::evaluate(&plan("builder", ExtensionEffect::ReadOnly), &[renamed])
            .expect("report");
    assert_eq!(
        report.denials.get("shell.fast"),
        Some(&ToolDiscoveryDenial::BuiltinBindingConflict)
    );
    assert!(shadows_builtin_descriptor(&shadow));

    // A descriptor cannot grant a role it was not bound for; the plan's role is the only one
    // consulted, and the candidate's own binding set is the only input to that check.
    let unbound = read_only_candidate("report.render");
    assert!(unbound.bound_roles.contains("builder"));
    assert!(!unbound.bound_roles.contains("sponsor"));
    let sponsor_plan = plan("sponsor", ExtensionEffect::ReadOnly);
    let report = ToolDiscoveryReport::evaluate(&sponsor_plan, &[unbound]).expect("report");
    assert_eq!(
        report.denials.get("report.render"),
        Some(&ToolDiscoveryDenial::RoleNotBound)
    );

    // A duplicate operation across two candidates makes the visible binding ambiguous.
    let first = read_only_candidate("report.render");
    let mut second = read_only_candidate("report.render.copy");
    second.operation = first.operation.clone();
    let report = ToolDiscoveryReport::evaluate(
        &plan("builder", ExtensionEffect::ReadOnly),
        &[first, second],
    )
    .expect("report");
    assert_eq!(report.admitted_tools, vec!["report.render".to_owned()]);
    assert_eq!(
        report.denials.get("report.render.copy"),
        Some(&ToolDiscoveryDenial::DuplicateToolName)
    );

    // A signed manifest whose declared effect is weaker than the scope it requires is refused
    // before it can ever produce a descriptor.
    let manifest = extension_manifest(ExtensionEffect::ReadOnly, &["filesystem.transaction"]);
    assert_eq!(
        check_manifest_effect(&manifest).unwrap_err(),
        "tool_discovery_manifest_read_only_write_scope"
    );
    let honest = extension_manifest(ExtensionEffect::ReadOnly, &["query.read"]);
    assert!(check_manifest_effect(&honest).is_ok());
}

#[test]
fn read_only_extension_write_is_denied_at_executor() {
    // The descriptor claims a write capability under a read-only contract. The descriptor is the
    // untrusted side of this boundary, so its own claim is the evidence.
    let mut writer = read_only_candidate("report.render");
    writer.capability = "filesystem".to_owned();
    let report =
        ToolDiscoveryReport::evaluate(&plan("builder", ExtensionEffect::ReadOnly), &[writer])
            .expect("report");
    assert_eq!(
        report.denials.get("report.render"),
        Some(&ToolDiscoveryDenial::ReadOnlyWriteDenied)
    );
    assert!(!report.admitted());

    // The same denial applies to every effect-bearing capability, and to prefixed spellings
    // that would otherwise slip past a bare equality check.
    for capability in [
        "filesystem",
        "process",
        "secret",
        "computer",
        "network",
        "filesystem_transaction",
        "secret_store",
    ] {
        let mut candidate = read_only_candidate("report.render");
        candidate.capability = capability.to_owned();
        let report = ToolDiscoveryReport::evaluate(
            &plan("builder", ExtensionEffect::ReadOnly),
            &[candidate],
        )
        .expect("report");
        assert_eq!(
            report.denials.get("report.render"),
            Some(&ToolDiscoveryDenial::ReadOnlyWriteDenied),
            "capability {capability} was not denied"
        );
    }

    // A read-only plan is also a ceiling on the contract itself: a read-write extension cannot
    // be admitted by a read-only request even if the tool claims a harmless capability.
    let mut read_write = read_only_candidate("report.render");
    read_write.effect = ExtensionEffect::ReadWrite;
    read_write.capability = "query".to_owned();
    let report =
        ToolDiscoveryReport::evaluate(&plan("builder", ExtensionEffect::ReadOnly), &[read_write])
            .expect("report");
    assert_eq!(
        report.denials.get("report.render"),
        Some(&ToolDiscoveryDenial::EffectInsufficient)
    );

    // An honest read-only extension is admitted, and admission still grants nothing.
    let report = ToolDiscoveryReport::evaluate(
        &plan("builder", ExtensionEffect::ReadOnly),
        &[read_only_candidate("report.render")],
    )
    .expect("report");
    assert!(report.admitted());
    assert_eq!(report.admitted_tools, vec!["report.render".to_owned()]);
    assert!(report.does_not_grant_execution);
    assert!(report.estimated_context_tokens > 0);
}

#[test]
fn decision_order_is_fixed_and_a_tampered_report_is_rejected() {
    // A candidate that is both a binding conflict and untrusted is reported as the conflict,
    // because replacing a built-in binding is the more serious claim of the two.
    let mut both = read_only_candidate("shell");
    both.trust = ToolDiscoveryTrust::Denied;
    let report =
        ToolDiscoveryReport::evaluate(&plan("builder", ExtensionEffect::ReadOnly), &[both.clone()])
            .expect("report");
    assert_eq!(
        report.denials.get("shell"),
        Some(&ToolDiscoveryDenial::BuiltinBindingConflict)
    );

    // Dropping trust alone lets the structural rule surface instead.
    let mut structural_only = both;
    structural_only.tool_name = "shell.fast".to_owned();
    let report = ToolDiscoveryReport::evaluate(
        &plan("builder", ExtensionEffect::ReadOnly),
        &[structural_only],
    )
    .expect("report");
    assert_eq!(
        report.denials.get("shell.fast"),
        Some(&ToolDiscoveryDenial::BuiltinBindingConflict)
    );

    // An admitted report recomputed against a different candidate set is refused, so a caller
    // cannot present one pass's answer as another's.
    let admitted = read_only_candidate("report.render");
    let good = ToolDiscoveryReport::evaluate(
        &plan("builder", ExtensionEffect::ReadOnly),
        &[admitted.clone()],
    )
    .expect("report");
    let mut tampered = good.clone();
    tampered.admitted_tools.push("shell".to_owned());
    assert_eq!(
        tampered
            .validate_against(&plan("builder", ExtensionEffect::ReadOnly), &[admitted])
            .unwrap_err(),
        "tool_discovery_report_binding_invalid"
    );

    let mut flipped = good.clone();
    flipped.status = ToolDiscoveryStatus::Empty;
    assert!(flipped
        .validate_against(
            &plan("builder", ExtensionEffect::ReadOnly),
            &[read_only_candidate("report.render")]
        )
        .is_err());

    // The context budget is a bound, not a hint: an admitted set that cannot fit is refused.
    let candidates = vec![
        read_only_candidate("report.render"),
        read_only_candidate("report.export"),
    ];
    let tight = ToolDiscoveryPlan::new(
        "builder",
        "project-1",
        ExtensionEffect::ReadOnly,
        &catalog(),
        16,
        1,
    )
    .expect("plan");
    let report = ToolDiscoveryReport::evaluate(&tight, &candidates).expect("report");
    assert!(!report.admitted());
    assert!(report
        .denials
        .values()
        .all(|denial| *denial == ToolDiscoveryDenial::PlanInvalid));
}

fn extension_manifest(
    effect: ExtensionEffect,
    required_capabilities: &[&str],
) -> kiana_domain::ExtensionManifest {
    serde_json::from_value(json!({
        "schema": "kiana.extension-manifest.v1",
        "extension_id": "ext-report",
        "version": "1.0.0",
        "publisher": "kiana-fixture",
        "license": "Apache-2.0",
        "content_hash": "a".repeat(64),
        "signature": {
            "algorithm": "ed25519",
            "key_id": "fixture",
            "value": "b".repeat(128)
        },
        "extension_type": "capability",
        "effect": effect,
        "provided_capabilities": ["ext.report.render.read"],
        "required_capabilities": required_capabilities,
        "supported_roles": ["builder"],
        "data_classes": [],
        "network_policy": {"mode": "deny"},
        "secret_refs": [],
        "requires": {
            "kiana_version": "0.1.0",
            "protocol_version": "kiana.protocol.v1"
        }
    }))
    .expect("manifest")
}
