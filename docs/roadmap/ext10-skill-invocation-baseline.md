# EXT-10 Skill Invocation Baseline

This source slice keeps legacy `/skills` discovery while separating three adapter actions:

- `catalog` returns metadata-only Skill descriptors;
- `invoke` accepts a JSON object or string argv, activates a user-invocable Skill, validates the
  session/snapshot/argument boundary, and returns a typed invocation envelope;
- `resource_read` creates a bounded activation, reads only package-relative bytes, and returns a
  typed resource envelope without executing scripts.

The adapter explicitly marks `allowed_tools` as non-authorizing and returns
`does_not_execute=true`; actual model/capability work remains on the existing daemon/control-plane
spine. Local tests are intentionally not run; GitHub Actions is the validation surface.
