use kiana_daemon::eval_runtime::EvalRuntimeSandbox;

#[test]
fn eval_target_is_isolated_from_operator_home_and_has_fixed_clock_seed() {
    let sandbox = EvalRuntimeSandbox::create("eq09-isolation", 10_000, 42, 7).unwrap();
    assert!(sandbox.workspace().starts_with(sandbox.root()));
    assert!(sandbox.kiana_home().starts_with(sandbox.root()));
    assert_eq!(sandbox.clock().wall_now_unix_ms, 10_000);
    assert_eq!(sandbox.clock().monotonic_now_ms, 42);
    assert_eq!(sandbox.random_seed(), 7);
    let environment = sandbox.environment();
    assert_eq!(
        environment["KIANA_HOME"],
        sandbox.kiana_home().display().to_string()
    );
    assert!(environment["HOME"].starts_with(sandbox.root().to_str().unwrap()));
    assert_eq!(
        sandbox.deterministic_bytes("fixture", 32).unwrap(),
        sandbox.deterministic_bytes("fixture", 32).unwrap()
    );
}

#[test]
fn eval_runtime_environment_is_scoped_and_restored() {
    let sandbox = EvalRuntimeSandbox::create("eq09-env", 20_000, 99, 11).unwrap();
    let before = std::env::var_os("KIANA_HOME");
    sandbox
        .with_process_environment(|| {
            assert_eq!(
                std::env::var_os("KIANA_HOME"),
                Some(sandbox.kiana_home().as_os_str().to_owned())
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(std::env::var_os("KIANA_HOME"), before);
}
