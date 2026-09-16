use kiana_domain::{RoleSpec, ROLE_ANALYST, ROLE_BUILDER, ROLE_QA};
use kiana_provider::{ProviderConfig, ProviderGateway};
use serde_json::Value;

fn route<'a>(catalog: &'a Value, profile: &str) -> &'a Value {
    catalog["connections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|connection| connection["route"]["profile"] == profile)
        .map(|connection| &connection["route"])
        .unwrap()
}

#[test]
fn planning_and_execution_roles_can_use_different_models() {
    let previous = std::env::var_os("KIANA_MODEL_PROFILES_JSON");
    let previous_streaming = std::env::var_os("KIANA_STREAMING");
    let previous_concurrency = std::env::var_os("KIANA_MODEL_MAX_CONCURRENCY");
    std::env::set_var("KIANA_STREAMING", "off");
    std::env::set_var("KIANA_MODEL_MAX_CONCURRENCY", "2");
    std::env::set_var(
        "KIANA_MODEL_PROFILES_JSON",
        r#"{
            "planning":{"provider":"ollama","model":"planning-model","base_url":"http://127.0.0.1:22115"},
            "executing":{"provider":"ollama","model":"execution-model","base_url":"http://127.0.0.1:22116"},
            "quality":{"provider":"ollama","model":"quality-model","base_url":"http://127.0.0.1:22117"}
        }"#,
    );
    let gateway = ProviderGateway::from_env(ProviderConfig {
        provider: Some("ollama".to_owned()),
        model: Some("default-model".to_owned()),
        base_url: Some("http://127.0.0.1:22114".to_owned()),
        api_key: None,
    })
    .unwrap();
    let catalog = gateway.catalog();
    assert_eq!(
        RoleSpec::lookup(ROLE_ANALYST).unwrap().model_profile,
        "analysis"
    );
    assert_eq!(
        RoleSpec::lookup(ROLE_BUILDER).unwrap().model_profile,
        "executing"
    );
    assert_eq!(RoleSpec::lookup(ROLE_QA).unwrap().model_profile, "quality");
    assert_eq!(route(&catalog, "planning")["model_id"], "planning-model");
    assert_eq!(route(&catalog, "executing")["model_id"], "execution-model");
    assert_eq!(route(&catalog, "quality")["model_id"], "quality-model");
    assert_ne!(
        route(&catalog, "planning")["connection_id"],
        route(&catalog, "executing")["connection_id"]
    );
    assert_ne!(
        route(&catalog, "executing")["connection_id"],
        route(&catalog, "quality")["connection_id"]
    );

    match previous {
        Some(value) => std::env::set_var("KIANA_MODEL_PROFILES_JSON", value),
        None => std::env::remove_var("KIANA_MODEL_PROFILES_JSON"),
    }
    match previous_streaming {
        Some(value) => std::env::set_var("KIANA_STREAMING", value),
        None => std::env::remove_var("KIANA_STREAMING"),
    }
    match previous_concurrency {
        Some(value) => std::env::set_var("KIANA_MODEL_MAX_CONCURRENCY", value),
        None => std::env::remove_var("KIANA_MODEL_MAX_CONCURRENCY"),
    }
}
