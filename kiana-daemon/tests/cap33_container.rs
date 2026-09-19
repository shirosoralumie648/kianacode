use kiana_daemon::container_environment::{
    ContainerEnvironmentAdapter, ContainerLaunchConfig, DEFAULT_CONTAINER_NETWORK,
};

const IMAGE: &str =
    "ghcr.io/kiana/ci-fixture@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn config() -> ContainerLaunchConfig {
    ContainerLaunchConfig::new(IMAGE, std::env::current_dir().expect("workspace"))
}

#[test]
fn create_plan_is_non_root_networkless_and_labelled() {
    let config = config();
    let args = ContainerEnvironmentAdapter::create_args(
        &config,
        "owner-1",
        "scope-1",
        "plan-1",
        "kiana-plan-1",
    )
    .expect("valid container plan");
    assert!(args
        .windows(2)
        .any(|pair| { pair[0] == "--network" && pair[1] == DEFAULT_CONTAINER_NETWORK }));
    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "--cap-drop" && pair[1] == "ALL"));
    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "--user" && pair[1] == "65532:65532"));
    assert!(args.iter().any(|arg| arg == "--read-only"));
    assert!(args.iter().any(|arg| arg == "io.kiana.owner=owner-1"));
    assert!(!args.iter().any(|arg| arg.contains("DOCKER_HOST")));
    assert!(!args.iter().any(|arg| arg.contains("SSH_AUTH_SOCK")));
}

#[test]
fn unpinned_images_and_host_control_paths_are_denied() {
    let mut config = config();
    config.image = "alpine:latest".to_owned();
    assert!(config.validate().is_err());

    config.image = IMAGE.to_owned();
    config.workspace = std::path::PathBuf::from("/run");
    assert!(config.validate().is_err());
}
