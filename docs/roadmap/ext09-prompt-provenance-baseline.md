# EXT-09 Prompt Provenance Baseline

This source slice binds Skill prompt material to a typed, non-authorizing provenance record:

- `PromptBundle.skill_provenance` records skill ID/version/content hash/trust/activation reason,
  budget usage and snapshot ID for each context section;
- `PromptBudgetUsage` captures byte/token budget, actual usage and whether a body was truncated or
  omitted; omission and truncation cannot be conflated;
- PromptBundle validation rejects provenance attached to Product sections or unknown sections;
- regular Skills record complete-or-omitted body disclosure, while verified extension packages
  record bounded UTF-8 truncation explicitly;
- provenance remains context metadata and does not enter the capability grant path.

Focused fixtures live in `kiana-domain/tests/ext09_prompt_provenance.rs`; the product-path source
guard is `kiana-core/tests/ext09_prompt_provenance_guard.rs`. Local tests are intentionally not
run; GitHub Actions is the validation surface for this step.
