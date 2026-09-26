# EQ-43 quality promote/rollback admission baseline

## Scope

EQ-43 routes `quality.promote` and `quality.rollback` through `ControlPlane::handle_command` and
adds a second authorization admission. The server rechecks trusted project/actor/role, current
authority epoch and scope digest, reads an `approval:` decision, requires an independent approved
decision whose request hash binds the exact operation/candidate/gate/scope/epoch, then appends the
quality command fact. Stale epoch, scope drift, missing/expired/reused/non-independent approval
and digest mismatch are rejected.

This slice records an authorized quality mutation admission. It does not change a live provider
route or grant; that remains a later explicitly authorized transition. No entrypoint or evaluator
creates a second authorization path.

## Evidence and limits

- `kiana-core/tests/eq43_quality_mutation_guard.rs` checks the shared command route, approval
  decision read, scope/epoch/request-hash fences, event fields and no direct Broker/Runner path.
- `.github/workflows/eq43-quality-mutation.yml` runs the source guard, formatting and workspace
  test-target compilation in GitHub Actions; local tests are intentionally not executed.

This slice is `feature_status=partial`, `proof_level=source`: the admission fact path is wired,
while durable candidate/gate stores, actual route mutation, second effect receipt and live/physical
promotion or rollback remain open.
