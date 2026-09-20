# EXT-19 Plugin Manifest v2 Baseline

This source slice adds an explicit Plugin manifest v2 adapter:

- stable `plugin_id`, publisher, version, license, source and server-derived namespace;
- bounded unique component IDs with typed entrypoint kinds;
- required/optional dependency maps, configuration/state schemas and relative migration refs;
- original manifest bytes remain represented by `source_digest`, while legacy v1 parsing stays
  available through the existing adapter.

Focused fixtures live in `kiana-skills/tests/ext19_plugin_manifest_v2.rs`; the source guard is
`kiana-core/tests/ext19_plugin_manifest_guard.rs`. Local tests are intentionally not run;
GitHub Actions is the validation surface.
