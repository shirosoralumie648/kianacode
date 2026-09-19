# CAP-31 macOS backend baseline (partial)

The shared EnvironmentPort and ProcessSupervisor contracts are present, but the current product
backend is Linux/bwrap (`SANDBOX_BACKEND`) and the repository has no macOS Seatbelt implementation
or target-machine behavior receipt. Linux cfg compilation is not macOS evidence.

The new GitHub macOS job only compiles the workspace and runs a source guard. It keeps the required
negative cases explicit: `macos_backend_denies_host_secrets_and_gui_escape`,
`macos_descendant_escape_or_stop_failure_is_visible`, and
`macos_missing_backend_has_no_host_fallback`. Until a real macOS backend/probe and target fixture
exist, CAP-31 remains `partial` with `behavior_verified=false`.
