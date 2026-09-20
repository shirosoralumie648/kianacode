# EXT-24 Extension Secret, State and Migration Baseline

EXT-24 adds a typed, deny-first boundary for extension configuration and persistence:

- ExtensionSecretBinding carries only a SecretRef, destination, scope digest and expiry.
  validate_at rechecks extension, config key, destination and time immediately before an effect;
  no raw secret value can enter the contract.
- ExtensionConfigurationSnapshot separates bounded non-secret configuration from opaque secret
  bindings and rejects raw secret-like fields or values.
- ExtensionStateScope binds publisher, extension, project scope, mutable state namespace and
  read-only package cache namespace. The daemon keeps the actual state root separate from the
  immutable package cache and rejects either root inside the controlled project.
- ExtensionStateMigrationPlan requires old/new schema and state hashes, a verified-backup
  identifier, bounded bytes and a one-step generation CAS. ExtensionStateMigrationReceipt
  distinguishes committed, retained-old and unknown outcomes; unknown never becomes a success.
- Existing legacy stateless migration declarations remain readable. Controlled state migration
  declarations are validated and then rejected from automatic install/upgrade activation until an
  explicit migration action owns the effect boundary.

The CI-only fixtures are kiana-domain/tests/ext24_secret_state.rs and
kiana-core/tests/ext24_secret_state_guard.rs. Local tests are intentionally not run; GitHub
Actions is the validation surface. Durable state-store adapters, actual bounded migration
execution, backup persistence, crash recovery and external/live effects remain later work.
