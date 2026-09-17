# Evaluation fixture manifest

This directory is the checked-in, deterministic fixture root for the EQ-08 contract. Every case
must be declared in `manifest.json` with an allow-listed fixture schema, relative path and byte
limit. CI uses `kiana_commands::eval_fixtures::load_fixture_manifest`; it never reads undeclared
files or the operator's home. Runtime tests execute in GitHub Actions only.
