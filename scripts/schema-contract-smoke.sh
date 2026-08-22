#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

# Production JSON Schema corpus under docs/schemas, docs/eval/fixtures,
# and docs/proof-templates was deleted. This smoke is kept as a no-op so
# Makefile and packaging callers remain valid.
echo "schema contract smoke skipped: production schema corpus deleted"
