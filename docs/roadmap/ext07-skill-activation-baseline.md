# EXT-07 Skill Activation Baseline

This source slice binds Skill resource reads to an explicit activation record:

- `activate_skill` issues a deterministic activation ID bound to skill name, package hash,
  source, snapshot generation, reason and expiry;
- `SkillActivation::validate_for` rejects tampered package hashes, stale/expired activations,
  revoked status, skill identity drift and activation digest drift;
- `read_skill_resource` checks activation before package-root containment, symlink and quota
  checks, and returns bytes without executing scripts;
- the existing `SourceResolver` remains the only relative-resource containment gate.

Focused fixtures live in `kiana-skills/tests/ext06_progressive_disclosure.rs` and
`kiana-skills/tests/ext07_activation.rs`; the product-path source guard is
`kiana-core/tests/ext07_skill_activation_guard.rs`. Local tests are intentionally not run;
GitHub Actions is the validation surface for this step.
