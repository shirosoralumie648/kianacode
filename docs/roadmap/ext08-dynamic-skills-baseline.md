# EXT-08 Dynamic Skills Baseline

This source slice makes conditional and parameterized Skill activation explicit:

- `PathGlobAst` validates and sorts bounded patterns, retains a digest, and records matched paths
  in deterministic order;
- `DynamicSkillStore` isolates conditional/active Skills by `session_id` and
  `snapshot_generation`; the legacy process adapter is an explicit compatibility wrapper;
- path-trigger activation emits a typed receipt with reason, trigger paths, trigger digest and
  activation record; revoke removes the Skill from the visible dynamic set;
- invocation preparation accepts bounded structured argv only, rejects undeclared named arguments,
  NUL/oversized values and undeclared positional input, and never builds a shell command.

Focused fixtures live in `kiana-skills/tests/ext08_dynamic_scope.rs`; the product-path source guard
is `kiana-core/tests/ext08_dynamic_skill_guard.rs`. Local tests are intentionally not run;
GitHub Actions is the validation surface for this step.
