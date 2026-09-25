#!/usr/bin/env bash
set -euo pipefail

if [[ "${GITHUB_ACTIONS:-}" != "true" ]]; then
  printf 'company_os_business_smoke:remote_ci_required\n' >&2
  exit 2
fi

cargo fmt --all --check
cargo test -p kiana-daemon --test company_lifecycle --locked -- --test-threads=1
cargo test -p kiana-daemon --test p3_i06_company_golden --locked -- --test-threads=1
cargo test -p kiana-core --test co47_company_lifecycle_guard --locked -- --test-threads=1
cargo test -p kiana-core --test p3_i06_company_golden --locked -- --test-threads=1
cargo check --workspace --tests --locked
