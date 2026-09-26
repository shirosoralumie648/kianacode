#!/usr/bin/env bash
set -Eeuo pipefail
IFS=$'\n\t'
umask 077

# EQ-49 deep lane. This is only a thin ControlPlane route adapter: it does not
# evaluate fixtures, start a runner, or load the operator's environment.
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd -- "$ROOT"

die() {
  echo "eval-deep: $*" >&2
  exit 2
}

usage() {
  cat >&2 <<'USAGE'
usage: scripts/eval-deep.sh [options]

Run the provider-independent deep suite through `kiana eval run`.
Options: --suite-id ID --dataset-id ID --experiment-id ID --case-id ID
         --target REF --baseline-id ID
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
  if [[ -n "${run_dir:-}" && "$run_dir" == /tmp/kiana-eval-deep.* && -d "$run_dir" ]]; then
    env -i PATH=/usr/local/bin:/usr/bin:/bin rm -rf -- "$run_dir"
  fi
}
trap cleanup EXIT HUP INT TERM

run_dir="$(env -i PATH=/usr/local/bin:/usr/bin:/bin TMPDIR=/tmp mktemp -d -t kiana-eval-deep.XXXXXX)"
[[ "$run_dir" == /tmp/kiana-eval-deep.* ]] || die "temporary directory is outside the allowed root"
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

suite_id="deep"
dataset_id=""
experiment_id=""
case_id=""
target_ref=""
baseline_id=""
seen_suite=0
seen_dataset=0
seen_experiment=0
seen_case=0
seen_target=0
seen_baseline=0

while (($# > 0)); do
  case "$1" in
    --suite-id)
      ((seen_suite == 0)) || die "duplicate --suite-id"
      (($# >= 2)) || die "--suite-id requires a value"
      [[ "$2" != -* ]] || die "--suite-id requires a value"
      suite_id="$2"
      seen_suite=1
      shift 2
      ;;
    --suite-id=*)
      ((seen_suite == 0)) || die "duplicate --suite-id"
      suite_id="${1#*=}"
      seen_suite=1
      shift
      ;;
    --dataset-id)
      ((seen_dataset == 0)) || die "duplicate --dataset-id"
      (($# >= 2)) || die "--dataset-id requires a value"
      [[ "$2" != -* ]] || die "--dataset-id requires a value"
      dataset_id="$2"
      seen_dataset=1
      shift 2
      ;;
    --dataset-id=*)
      ((seen_dataset == 0)) || die "duplicate --dataset-id"
      dataset_id="${1#*=}"
      seen_dataset=1
      shift
      ;;
    --experiment-id)
      ((seen_experiment == 0)) || die "duplicate --experiment-id"
      (($# >= 2)) || die "--experiment-id requires a value"
      [[ "$2" != -* ]] || die "--experiment-id requires a value"
      experiment_id="$2"
      seen_experiment=1
      shift 2
      ;;
    --experiment-id=*)
      ((seen_experiment == 0)) || die "duplicate --experiment-id"
      experiment_id="${1#*=}"
      seen_experiment=1
      shift
      ;;
    --case-id)
      ((seen_case == 0)) || die "duplicate --case-id"
      (($# >= 2)) || die "--case-id requires a value"
      [[ "$2" != -* ]] || die "--case-id requires a value"
      case_id="$2"
      seen_case=1
      shift 2
      ;;
    --case-id=*)
      ((seen_case == 0)) || die "duplicate --case-id"
      case_id="${1#*=}"
      seen_case=1
      shift
      ;;
    --target)
      ((seen_target == 0)) || die "duplicate --target"
      (($# >= 2)) || die "--target requires a value"
      [[ "$2" != -* ]] || die "--target requires a value"
      target_ref="$2"
      seen_target=1
      shift 2
      ;;
    --target=*)
      ((seen_target == 0)) || die "duplicate --target"
      target_ref="${1#*=}"
      seen_target=1
      shift
      ;;
    --baseline-id)
      ((seen_baseline == 0)) || die "duplicate --baseline-id"
      (($# >= 2)) || die "--baseline-id requires a value"
      [[ "$2" != -* ]] || die "--baseline-id requires a value"
      baseline_id="$2"
      seen_baseline=1
      shift 2
      ;;
    --baseline-id=*)
      ((seen_baseline == 0)) || die "duplicate --baseline-id"
      baseline_id="${1#*=}"
      seen_baseline=1
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

((seen_suite == 0 || -n "$suite_id")) || die "--suite-id requires a non-empty value"
((seen_dataset == 0 || -n "$dataset_id")) || die "--dataset-id requires a non-empty value"
((seen_experiment == 0 || -n "$experiment_id")) || die "--experiment-id requires a non-empty value"
((seen_case == 0 || -n "$case_id")) || die "--case-id requires a non-empty value"
((seen_target == 0 || -n "$target_ref")) || die "--target requires a non-empty value"
((seen_baseline == 0 || -n "$baseline_id")) || die "--baseline-id requires a non-empty value"
for reference in "$suite_id" "$dataset_id" "$experiment_id" "$case_id" "$target_ref" "$baseline_id"; do
  [[ -n "$reference" ]] && reject_secret_or_path "$reference"
done

binary="$(resolve_binary)"
args=(eval run --suite-id "$suite_id" --json --fail-on-failure)
[[ -n "$dataset_id" ]] && args+=(--dataset-id "$dataset_id")
[[ -n "$experiment_id" ]] && args+=(--experiment-id "$experiment_id")
[[ -n "$case_id" ]] && args+=(--case-id "$case_id")
[[ -n "$target_ref" ]] && args+=(--target "$target_ref")
[[ -n "$baseline_id" ]] && args+=(--baseline-id "$baseline_id")

if run_control_plane "$binary" "${args[@]}"; then
  :
else
  status=$?
  echo "eval-deep.sh: ControlPlane eval.run failed (exit=$status)" >&2
  exit "$status"
fi
