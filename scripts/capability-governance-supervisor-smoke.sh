#!/usr/bin/bash -p
set -euo pipefail

if ((BASH_VERSINFO[0] < 5)); then
  echo "capability governance supervisor smoke requires Bash 5 or newer" >&2
  exit 1
fi

ROOT="$(cd -- "${BASH_SOURCE[0]%/*}/.." && pwd -P)"
RUNNER="$ROOT/scripts/capability-governance-smoke.sh"
SUPERVISOR="$ROOT/target/debug/kiana-capability-governance-supervisor"
CARGO_BIN="$(command -v cargo 2>/dev/null || true)"
RUSTC_BIN="$(command -v rustc 2>/dev/null || true)"
PYTHON_BIN="$(python3 -c 'import sys; print(sys.executable)' 2>/dev/null || true)"
BUILD_PATH="$PATH"
GATE_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
export PATH="/usr/bin:/bin"
export LC_ALL="C"

fail() {
  echo "supervisor_smoke_failed: $*" >&2
  exit 1
}

[[ -x "$RUNNER" ]] || fail "public runner is not executable"
[[ -x "$SUPERVISOR" ]] || fail "prebuilt supervisor is unavailable"
[[ "$CARGO_BIN" == /* && -x "$CARGO_BIN" ]] || fail "Cargo is unavailable"
[[ "$RUSTC_BIN" == /* && -x "$RUSTC_BIN" ]] || fail "rustc is unavailable"
[[ "$PYTHON_BIN" == /* && -x "$PYTHON_BIN" ]] || fail "build Python is unavailable"

tmp_root="$(mktemp -d /var/tmp/kiana-supervisor-smoke.XXXXXX)"
cleanup() {
  local status=$?
  trap - EXIT INT TERM
  rm -rf "$tmp_root"
  return "$status"
}
trap cleanup EXIT INT TERM

assert_status() {
  local expected="$1"
  shift
  set +e
  "$@" >"$tmp_root/status.out" 2>"$tmp_root/status.err"
  local actual=$?
  set -e
  [[ "$actual" == "$expected" ]] || {
    sed -n '1,40p' "$tmp_root/status.out" >&2
    sed -n '1,40p' "$tmp_root/status.err" >&2
    fail "expected status $expected, got $actual: $*"
  }
  if grep -q 'offline=true' "$tmp_root/status.out" "$tmp_root/status.err"; then
    fail "non-success path published a final marker: $*"
  fi
}

assert_status 2 "$RUNNER"
assert_status 2 "$RUNNER" unknown
assert_status 2 "$SUPERVISOR"
assert_status 2 "$SUPERVISOR" unknown

mkdir -p "$tmp_root/missing/scripts"
cp "$RUNNER" "$tmp_root/missing/scripts/capability-governance-smoke.sh"
chmod 0755 "$tmp_root/missing/scripts/capability-governance-smoke.sh"
assert_status 1 "$tmp_root/missing/scripts/capability-governance-smoke.sh" schemas
grep -Fq 'supervisor_unavailable: build kiana-capability-governance-supervisor with --locked --offline' \
  "$tmp_root/status.err" || fail "missing-helper diagnostic changed"

assert_status 1 env -i \
  PATH=/usr/bin:/bin \
  KIANA_GOVERNANCE_PYTHON=/usr/bin/python3 \
  "$RUNNER" --internal-worker schemas
assert_status 1 "$SUPERVISOR" __worker-launcher \
  --runner /tmp/forged --slice schemas

mkdir -p "$tmp_root/shims"
for command in bash bwrap strace python python3 sh; do
  printf '%s\n' '#!/usr/bin/bash' 'echo forged offline=true' 'exit 0' \
    >"$tmp_root/shims/$command"
  chmod 0755 "$tmp_root/shims/$command"
done
printf '%s\n' 'echo forged offline=true' 'exit 0' >"$tmp_root/bash-env"

env \
  PATH="$tmp_root/shims" \
  BASH_ENV="$tmp_root/bash-env" \
  ENV="$tmp_root/bash-env" \
  PYTHONPATH="$tmp_root/shims" \
  PYTHONHOME="$tmp_root/shims" \
  TMPDIR="$tmp_root/shims" \
  "$RUNNER" fixture-shapes \
  >"$tmp_root/shim.out" 2>"$tmp_root/shim.err"
[[ ! -s "$tmp_root/shim.err" ]] || fail "environment shim produced public stderr"
[[ "$(grep -c 'offline=true' "$tmp_root/shim.out")" == 1 ]] ||
  fail "environment shim changed final marker authority"
! grep -Fq 'forged' "$tmp_root/shim.out" || fail "environment shim executed"

mkdir -p "$tmp_root/caller-cwd"
(
  cd "$tmp_root/caller-cwd"
  "$SUPERVISOR" fixture-shapes
) >"$tmp_root/caller-cwd.out" 2>"$tmp_root/caller-cwd.err"
[[ ! -s "$tmp_root/caller-cwd.err" ]] ||
  fail "caller cwd changed supervisor stderr: $(tr '\n' ' ' <"$tmp_root/caller-cwd.err")"
[[ "$(grep -c 'offline=true' "$tmp_root/caller-cwd.out")" == 1 ]] ||
  fail "caller cwd selected a different production runner"

"$RUSTC_BIN" --edition=2021 --test \
  "$ROOT/kiana-capability-governance-supervisor/build.rs" \
  -o "$tmp_root/build-tests"
PATH="$BUILD_PATH" KIANA_TEST_PYTHON="$PYTHON_BIN" \
  "$tmp_root/build-tests" >"$tmp_root/build-tests.out"
grep -Fq '4 passed' "$tmp_root/build-tests.out" || fail "build prerequisite tests failed"

"$RUSTC_BIN" --edition=2021 \
  "$ROOT/kiana-capability-governance-supervisor/build.rs" \
  -o "$tmp_root/build-script"
mkdir -p "$tmp_root/no-python"
CARGO_CFG_TARGET_OS=windows PATH="$tmp_root/no-python" \
  "$tmp_root/build-script" >"$tmp_root/non-linux-build.out" \
  2>"$tmp_root/non-linux-build.err" || fail "non-Linux stub build prerequisites did not skip"
[[ ! -s "$tmp_root/non-linux-build.err" ]] || fail "non-Linux stub build emitted prerequisite errors"
[[ "$(wc -l <"$tmp_root/non-linux-build.out")" == 1 ]] &&
  grep -Fxq 'cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS' \
    "$tmp_root/non-linux-build.out" || fail "non-Linux stub build executed a runtime prerequisite"

for slice in schemas fixture-shapes; do
  "$RUNNER" "$slice" >"$tmp_root/$slice.out" 2>"$tmp_root/$slice.err"
  [[ ! -s "$tmp_root/$slice.err" ]] || fail "$slice produced public stderr"
  [[ "$(grep -c 'offline=true' "$tmp_root/$slice.out")" == 1 ]] ||
    fail "$slice did not publish exactly one final marker"
  if grep -Eq 'socket\(|INJECTED|complete:[a-f0-9]{64}|\[pid [0-9]+\]|/tmp/kiana-capability-governance-' \
    "$tmp_root/$slice.out"; then
    fail "$slice disclosed trace, token, pid, or runtime path"
  fi
done

env -u CARGO_BUILD_TARGET PATH="$BUILD_PATH" \
  "$CARGO_BIN" test --target-dir "$GATE_TARGET_DIR" \
  -p kiana-capability-governance-supervisor \
  --locked --offline --test supervisor_linux --no-fail-fast -- \
  --test-threads=1 >"$tmp_root/linux-tests.out" 2>"$tmp_root/linux-tests.err" || {
  sed -n '1,80p' "$tmp_root/linux-tests.err" >&2
  fail "Linux integration gate failed"
}
! grep -q 'SKIP prerequisite:' "$tmp_root/linux-tests.out" "$tmp_root/linux-tests.err" ||
  fail "Linux integration gate skipped a prerequisite"

protected_hashes="$tmp_root/protected-hashes"
if git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  cat >"$protected_hashes" <<'HASHES'
9be4f3f2c2af1dde3da4081f0b072f41c5219d41db0d264d2e77d798da0e216b  scripts/schema-contract-smoke.sh
4c51d722c2049e249b174fe9a09e611629084b611703a9cbbce68153c0c90002  scripts/validate-json-schema.py
df019195eb3affca89eb4053c68ce130f1e780cc61c7d9f40c42bec4b0e19a3b  scripts/fixtures/capability-governance/valid/full-38-repositories.json
65b678f5ee4412729e39cbfd09a036df750299df5f804b31ad14da7333cd6f3c  scripts/fixtures/capability-governance/valid/hostile-rendering.json
6a83baaa24edbd199deca17d610665006aeb585edda8f74b328cb5fdd437e79d  scripts/fixtures/capability-governance/valid/minimal-graph.json
5ca4f2c6313e6d583e6cd31f68958a547ade091d768fbbaa19993d4ef7827201  scripts/fixtures/capability-governance/valid/offline-source-identity.json
HASHES
else
  cat >"$protected_hashes" <<'HASHES'
9be4f3f2c2af1dde3da4081f0b072f41c5219d41db0d264d2e77d798da0e216b  scripts/schema-contract-smoke.sh
c5ef0b131508c6d7beb99e123039f4c0d29f4e7f82f8f8625dcff62c27bf4470  scripts/validate-json-schema.py
df019195eb3affca89eb4053c68ce130f1e780cc61c7d9f40c42bec4b0e19a3b  scripts/fixtures/capability-governance/valid/full-38-repositories.json
65b678f5ee4412729e39cbfd09a036df750299df5f804b31ad14da7333cd6f3c  scripts/fixtures/capability-governance/valid/hostile-rendering.json
6a83baaa24edbd199deca17d610665006aeb585edda8f74b328cb5fdd437e79d  scripts/fixtures/capability-governance/valid/minimal-graph.json
5ca4f2c6313e6d583e6cd31f68958a547ade091d768fbbaa19993d4ef7827201  scripts/fixtures/capability-governance/valid/offline-source-identity.json
HASHES
fi
(cd "$ROOT" && sha256sum -c "$protected_hashes" >/dev/null) ||
  fail "protected input bytes changed"

if find /tmp -maxdepth 1 -type d -name 'kiana-capability-governance-[0-9a-f]*' \
  -print -quit | grep -q .; then
  fail "runtime root residue remains"
fi

echo "OK: capability governance Rust supervisor gate passed"
