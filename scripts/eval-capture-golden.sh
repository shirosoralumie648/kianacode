#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'
umask 077

# EQ-49 golden capture lane. This only routes an opaque source reference through
# ControlPlane; it never opens a fixture, overwrites a golden, or starts a runner.
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd -- "$ROOT"

die() {
  echo "eval-capture-golden: $*" >&2
  exit 2
}

usage() {
  cat >&2 <<'USAGE'
usage: scripts/eval-capture-golden.sh (--source REF | --run-id ID | --fixture REF) [--name NAME]

Capture a new GoldenTrace through the existing `kiana eval capture` route.
USAGE
}

reject_secret_or_path() {
  local value="$1"
  [[ -n "$value" ]] || die "reference must not be empty"
  [[ "${#value}" -le 256 ]] || die "reference is too long"
  [[ "$value" != *$'\0'* && "$value" != *$'\n'* && "$value" != *$'\r'* ]] || die "reference contains a control character"
  [[ "$value" != /* && "$value" != ~* && "$value" != *".."* && "$value" != *\\* ]] || die "reference path escapes the project scope"
  if [[ "$value" =~ (sk-[A-Za-z0-9_-]{8,}|AKIA[0-9A-Z]{16}|Bearer[[:space:]]|BEGIN[[:space:]]|raw-secret|api[_-]?key|password) ]]; then
    die "secret-like reference is rejected"
  fi
}

run_dir=""
cleanup() {
  if [[ -n "${run_dir:-}" && "$run_dir" == /tmp/kiana-eval-capture.* && -d "$run_dir" ]]; then
    env -i PATH=/usr/local/bin:/usr/bin:/bin rm -rf -- "$run_dir"
  fi
}
trap cleanup EXIT HUP INT TERM

run_dir="$(env -i PATH=/usr/local/bin:/usr/bin:/bin TMPDIR=/tmp mktemp -d -t kiana-eval-capture.XXXXXX)"
[[ "$run_dir" == /tmp/kiana-eval-capture.* ]] || die "temporary directory is outside the allowed root"
env -i PATH=/usr/local/bin:/usr/bin:/bin mkdir -m 700 -p "$run_dir/home" "$run_dir/kiana-home" "$run_dir/tmp"

resolve_binary() {
  local candidate="${KIANA_BIN:-}"
  if [[ -n "$candidate" ]]; then
    [[ "$candidate" != *$'\n'* && "$candidate" != *$'\r'* && "$candidate" != *".."* ]] || die "KIANA_BIN path is invalid"
    [[ "$candidate" == /* ]] || candidate="$ROOT/$candidate"
    [[ "$candidate" == "$ROOT"/* && -x "$candidate" ]] || die "KIANA_BIN must be an executable inside the repository"
    printf '%s\n' "$candidate"
    return 0
  fi
  for candidate in "$ROOT/target/debug/kiana" "$ROOT/target/release/kiana"; do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  die "kiana binary is missing; set KIANA_BIN to a repository executable"
}

run_control_plane() {
  local binary="$1"
  shift
  local -a clean_env=(
    env -i
    "PATH=/usr/local/bin:/usr/bin:/bin"
    "HOME=$run_dir/home"
    "KIANA_HOME=$run_dir/kiana-home"
    "TMPDIR=$run_dir/tmp"
    "LC_ALL=C"
    "LANG=C"
    "RUST_BACKTRACE=0"
  )
  "${clean_env[@]}" "$binary" "$@"
}

source_kind=""
source_ref=""
trace_name=""
seen_name=0

while (($# > 0)); do
  case "$1" in
    --source|--run-id|--fixture)
      [[ -z "$source_kind" ]] || die "exactly one source reference is required"
      (($# >= 2)) || die "$1 requires a value"
      [[ "$2" != -* ]] || die "$1 requires a value"
      source_kind="${1#--}"
      source_ref="$2"
      shift 2
      ;;
    --source=*|--run-id=*|--fixture=*)
      [[ -z "$source_kind" ]] || die "exactly one source reference is required"
      source_kind="${1%%=*}"
      source_kind="${source_kind#--}"
      source_ref="${1#*=}"
      shift
      ;;
    --name)
      ((seen_name == 0)) || die "duplicate --name"
      (($# >= 2)) || die "--name requires a value"
      [[ "$2" != -* ]] || die "--name requires a value"
      trace_name="$2"
      seen_name=1
      shift 2
      ;;
    --name=*)
      ((seen_name == 0)) || die "duplicate --name"
      trace_name="${1#*=}"
      seen_name=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      usage
      die "unknown or positional argument '$1'"
      ;;
  esac
done

[[ -n "$source_kind" && -n "$source_ref" ]] || { usage; die "one source reference is required"; }
((seen_name == 0 || -n "$trace_name")) || die "--name requires a non-empty value"
reject_secret_or_path "$source_ref"
[[ -z "$trace_name" ]] || reject_secret_or_path "$trace_name"

binary="$(resolve_binary)"
args=(eval capture "--${source_kind}" "$source_ref" --json)
[[ -n "$trace_name" ]] && args+=(--name "$trace_name")

if run_control_plane "$binary" "${args[@]}"; then
  :
else
  status=$?
  echo "eval-capture-golden.sh: ControlPlane eval.capture failed (exit=$status)" >&2
  exit "$status"
fi
