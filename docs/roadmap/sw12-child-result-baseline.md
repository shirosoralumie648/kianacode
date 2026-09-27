# SW-12 typed child result baseline

SW-12 adds a strict typed child result/failure fact. Succeeded, Failed and ResultUnknown carry
different required evidence; Unknown and failure never masquerade as success, and successful
results remain independently review bound. The Core facade validates facts only and does not merge,
publish, execute or append them.

`kiana-domain/tests/swarm_child_result.rs` and `kiana-core/tests/swarm_child_result_guard.rs` run
in `.github/workflows/sw12-child-result.yml`. Local Cargo tests/build/check/clippy/smoke commands
were not run. This slice is `feature_status=partial`, `proof_level=source`; durable delegation
facts, independent review execution, merge/release and live/physical outcomes remain open.
