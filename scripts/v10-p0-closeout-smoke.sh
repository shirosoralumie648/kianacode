#!/usr/bin/env bash
# v1.0.2 REL-03 closeout: P0 matrix rows stay 已绿 + NOTICE exists.
# Does not recode tools, run release-smoke.sh, or claim live provider.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() {
  echo "v10-p0-closeout-smoke: $*" >&2
  exit 1
}

[[ -f "$ROOT/USER.md" ]] || die "USER.md missing"
[[ -f "$ROOT/scripts/v10-personal-lifecycle-smoke.sh" ]] || die "v10-personal-lifecycle-smoke.sh missing"
[[ -f "$ROOT/NOTICE" ]] || die "NOTICE missing"
[[ -f "$ROOT/LICENSE-MIT" ]] || die "LICENSE-MIT missing"
[[ -f "$ROOT/LICENSE-APACHE" ]] || die "LICENSE-APACHE missing"
[[ -f "$ROOT/deny.toml" ]] || die "deny.toml missing"

notice="$(cat "$ROOT/NOTICE")"
[[ "$notice" == *"MIT"* ]] || die "NOTICE missing MIT"
[[ "$notice" == *"Apache-2.0"* ]] || die "NOTICE missing Apache-2.0"
[[ "$notice" == *"deny.toml"* ]] || die "NOTICE missing deny.toml"
[[ "$notice" == *"not an SBOM"* ]] || die "NOTICE must say it is not an SBOM"

matrix="$ROOT/docs/coding-pack-matrix.md"
[[ -f "$matrix" ]] || die "coding-pack-matrix.md missing"

python_bin="$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)"
[[ -n "$python_bin" ]] || die "need python3 to parse P0 rows"

MATRIX="$matrix" "$python_bin" - <<'PY' || die "P0 closeout failed"
import os
import sys
from pathlib import Path

text = Path(os.environ["MATRIX"]).read_text(encoding="utf-8")
lines = text.splitlines()
in_p0 = False
rows = []
for line in lines:
    if line.startswith("## 1. P0"):
        in_p0 = True
        continue
    if in_p0 and line.startswith("## "):
        break
    if in_p0 and line.startswith("| P0-"):
        cols = [c.strip() for c in line.strip().strip("|").split("|")]
        rows.append(cols)

if not rows:
    print("no P0 rows found", file=sys.stderr)
    sys.exit(1)

required = {
    "P0-LOOP", "P0-WRITE", "P0-SHELL", "P0-TRUST", "P0-SANDBOX", "P0-FAIL",
    "P0-CONT", "P0-CANCEL", "P0-RCPT", "P0-ROLE", "P0-REV", "P0-ORCH",
    "P0-MCP", "P0-SKILL", "P0-HOOK", "P0-PROV",
}
seen = {row[0] for row in rows}
missing = sorted(required - seen)
extra_fail = []
if missing:
    print("missing P0 ids: " + ", ".join(missing), file=sys.stderr)
    extra_fail.append("missing")

for row in rows:
    status = row[-1] if row else ""
    if "已绿" not in status:
        print(f"{row[0]} is not 已绿: {status}", file=sys.stderr)
        extra_fail.append(row[0])

if extra_fail:
    sys.exit(1)

print("P0 rows: " + ", ".join(row[0] for row in rows))
PY

echo "v1.0.2 P0 closeout smoke passed (local_behavior)"
echo "not claimed: live provider, HTTP MCP, SkillTool, packaged tarball, SBOM/signing, Daily/Research, v1.x"
