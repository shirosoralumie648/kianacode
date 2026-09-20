# EXT-06 Progressive Disclosure Baseline

This source slice defines the Skill disclosure boundary:

- catalog and search return metadata only, including source, trust, status, content digest and
  package hash;
- `load_skill_body` returns the complete UTF-8 body or a structured `over_budget` error;
- `read_skill_resource` resolves only package-relative regular files and returns package hash,
  normalized relative path, content bytes and quota usage;
- body and resource quotas are checked independently by UTF-8 byte count and a bounded
  `bytes / 4` token estimate; no response claims completeness after truncation;
- the harness adapter exposes an explicit omission reason when automatic body disclosure exceeds
  its bounded budget.

The focused fixtures live in `kiana-skills/tests/ext06_progressive_disclosure.rs` and the product
path source guard lives in `kiana-core/tests/ext06_progressive_disclosure_guard.rs`. Local tests
are intentionally not run; GitHub Actions is the validation surface for this step.
