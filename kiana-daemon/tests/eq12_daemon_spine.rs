use kiana_daemon::eval_runtime::{EvalRuntimeSandbox, EvalTarget};

#[test]
fn eval_uses_the_same_daemonhost_spine() {
    let runtime = include_str!("../src/eval_runtime.rs");
    let daemon = include_str!("../src/lib.rs");
    assert!(runtime.contains("pub struct EvalTarget"));
    assert!(runtime.contains("host: DaemonHost"));
    assert!(runtime.contains("DaemonHost::new(core)"));
    assert!(runtime.contains("self.host.handle(request).await"));
    assert!(daemon.contains("pub struct DaemonHost"));

    fn accepts_host_constructor(
        _constructor: fn(kiana_daemon::DaemonHost, EvalRuntimeSandbox) -> EvalTarget,
    ) {
    }
    accepts_host_constructor(EvalTarget::new);
}
