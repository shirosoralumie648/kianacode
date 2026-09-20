# EXT-12 Hook Discovery Baseline

This source slice adds a pure deterministic discovery layer:

- candidates are matched by normalized event and bounded glob/`re:` regex matcher;
- ordering is fixed by guard/observer phase, source priority, matcher specificity, declaration
  order and hook ID;
- snapshots retain matched IDs, guard/observer partitions, unmatched reasons, input digest and
  snapshot digest;
- discovery is read-only metadata work and never starts a process, invokes MCP, or changes policy.

Focused fixtures live in `kiana-skills/tests/ext12_hook_discovery.rs`; the source guard is
`kiana-core/tests/ext12_hook_discovery_guard.rs`. Local tests are intentionally not run;
GitHub Actions is the validation surface.
