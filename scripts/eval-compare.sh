#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'
umask 077

# EQ-49 compare lane. This is a strict ControlPlane route adapter and does not
# read report files, compute a diff, or create a second evaluation loop.
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd -- "$ROOT"

die() {
  echo "eval-compare: $*" >&2
  exit 2
}

usage() {
  cat >&2 <<'USAGE'
usage: scripts/eval-compare.sh --reference REF --candidate REF

Compare two opaque evaluation references through `kiana eval compare`.
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
  if [[ -n "${run_dir:-}" && "$run_dir" == /tmp/kiana-eval-compare.* && -d "$run_dir" ]]; then
    env -i PATH=/usr/local/bin:/usr/bin:/bin rm -rf -- "$run_dir"
  fi
}
trap cleanup EXIT HUP INT TERM

run_dir="$(env -i PATH=/usr/local/bin:/usr/bin:/bin TMPDIR=/tmp mktemp -d -t kiana-eval-compare.XXXXXX)"
[[ "$run_dir" == /tmp/kiana-eval-compare.* ]] || die "temporary directory is outside the allowed root"
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

reference_ref=""
candidate_ref=""
seen_reference=0
seen_candidate=0

while (($# > 0)); do
  case "$1" in
    --reference)
      ((seen_reference == 0)) || die "duplicate --reference"
      (($# >= 2)) || die "--reference requires a value"
      [[ "$2" != -* ]] || die "--reference requires a value"
      reference_ref="$2"
      seen_reference=1
      shift 2
      ;;
    --reference=*)
      ((seen_reference == 0)) || die "duplicate --reference"
      reference_ref="${1#*=}"
      seen_reference=1
      shift
      ;;
    --candidate)
      ((seen_candidate == 0)) || die "duplicate --candidate"
      (($# >= 2)) || die "--candidate requires a value"
      [[ "$2" != -* ]] || die "--candidate requires a value"
      candidate_ref="$2"
      seen_candidate=1
      shift 2
      ;;
    --candidate=*)
      ((seen_candidate == 0)) || die "duplicate --candidate"
      candidate_ref="${1#*=}"
      seen_candidate=1
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

[[ -n "$reference_ref" ]] || { usage; die "--reference is required"; }
[[ -n "$candidate_ref" ]] || { usage; die "--candidate is required"; }
reject_secret_or_path "$reference_ref"
reject_secret_or_path "$candidate_ref"

binary="$(resolve_binary)"
args=(eval compare --reference "$reference_ref" --candidate "$candidate_ref" --json)

if run_control_plane "$binary" "${args[@]}"; then
  :
else
  status=$?
  echo "eval-compare.sh: ControlPlane eval.compare failed (exit=$status)" >&2
  exit "$status"
fi
