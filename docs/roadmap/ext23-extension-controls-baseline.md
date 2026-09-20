# EXT-23 Extension Control Changes Baseline

The extension control boundary now has typed, fail-closed contracts for upgrade, disable,
revoke, rollback and uninstall:

- upgrade/revoke/uninstall change sets require new-request pause plus invalidation of stale
  approval, prompt and binding snapshots;
- rollback candidates require a verified signature, policy allowance, an unrevoked package and
  an older generation; revoked or unverified targets cannot be resurrected;
- uninstall carries a cleanup receipt that records configuration/state/cache disposition while
  retaining receipt references for auditability;
- existing daemon checks for version/content conflicts, revoked packages, rollback snapshots and
  append-only lifecycle CAS remain the runtime authority, with no new execution path.

The CI-only domain fixture is `kiana-domain/tests/ext23_extension_controls.rs`; the product-path
guard is `kiana-core/tests/ext23_extension_controls_guard.rs`. Local tests are intentionally not
run; GitHub Actions is the validation surface. Full runtime invalidation propagation and durable
multi-process state recovery remain later lifecycle/recovery work.
