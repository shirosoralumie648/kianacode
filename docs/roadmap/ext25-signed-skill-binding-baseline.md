# EXT-25 Signed Skill Server Binding Baseline

EXT-25 upgrades the extension scope carried from the signed package registry into a server-bound
execution reference:

- ExtensionExecutionScope now binds the verified package hash, publisher, registry generation,
  selected role, effect, required capabilities, capability-diff digest, network policy digest,
  secret policy digest and a bounded issue/expiry window.
- ExtensionRegistry::skill_context creates the scope from the committed registry snapshot, not
  from prompt text or model metadata. PromptBundle remains context transport only.
- Broker admission treats the scope as an untrusted hint and rechecks the live registry generation,
  enabled package hash, role, manifest policy digests, compatibility and expiry immediately before
  dispatch. Upgrade, revoke, role drift and stale generations fail closed.
- The fixed model-tool mapper does not copy _extension_scopes from model arguments; if a prepared
  request carries a server-provided scope list, the domain schema and Broker admission still
  validate and recheck it.

The CI-only fixtures are kiana-domain/tests/ext25_signed_skill_scope.rs and
kiana-core/tests/ext25_signed_skill_scope_guard.rs. Local tests are intentionally not run; GitHub
Actions is the validation surface. This step does not claim durable scope storage, real provider
effects, cross-process recovery or physical/live proof.
