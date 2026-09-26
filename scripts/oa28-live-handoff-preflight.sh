#!/usr/bin/env bash
# OA-28 deliberately never enables a live/physical target by default.
# This preflight validates operator intent and non-secret references only; a separate, approved
# environment must perform the target-specific receipt/reconcile/cleanup runbook.
set -Eeuo pipefail

if [[ "${KIANA_LIVE_HANDOFF_OPT_IN:-0}" != "1" ]]; then
  echo "oa28-live-handoff-preflight: live_opt_in_required" >&2
  exit 2
fi

target="${KIANA_LIVE_HANDOFF_TARGET:-}"
target_id="${KIANA_LIVE_HANDOFF_TARGET_ID:-}"
environment="${KIANA_LIVE_HANDOFF_ENVIRONMENT:-}"
credential_ref="${KIANA_LIVE_HANDOFF_CREDENTIAL_REF:-}"
approval_ref="${KIANA_LIVE_HANDOFF_APPROVAL_REF:-}"
receipt_ref="${KIANA_LIVE_HANDOFF_RECEIPT_REF:-}"
cleanup_plan="${KIANA_LIVE_HANDOFF_CLEANUP_PLAN:-}"

case "$target" in
  provider|connector|otlp_backend|operating_system) ;;
  *) echo "oa28-live-handoff-preflight: target_invalid" >&2; exit 1 ;;
esac
for value in "$target_id" "$environment" "$credential_ref" "$approval_ref" "$cleanup_plan"; do
  [[ -n "${value//[[:space:]]/}" ]] || {
    echo "oa28-live-handoff-preflight: required_reference_missing" >&2
    exit 1
  }
done
[[ "$credential_ref" == secret-ref:* ]] || {
  echo "oa28-live-handoff-preflight: raw_credential_ref_rejected" >&2
  exit 1
}
[[ "$approval_ref" == approval:* ]] || {
  echo "oa28-live-handoff-preflight: approval_ref_invalid" >&2
  exit 1
}
[[ "$approval_ref" != *[[:space:]]* ]] || {
  echo "oa28-live-handoff-preflight: approval_ref_whitespace" >&2
  exit 1
}
[[ "$approval_ref" != *$'\n'* && "$approval_ref" != *$'\r'* ]] || {
  echo "oa28-live-handoff-preflight: approval_ref_newline" >&2
  exit 1
}
if [[ -z "$receipt_ref" ]]; then
  echo "oa28-live-handoff-preflight: provider_receipt_required_before_verification" >&2
  exit 1
fi
if [[ "$credential_ref" == *$'\n'* || "$credential_ref" == *$'\r'* || "${credential_ref,,}" == *"bearer "* ]]; then
  echo "oa28-live-handoff-preflight: credential_secret_sentinel" >&2
  exit 1
fi

printf '%s\n' "oa28-live-handoff-preflight: intent validated for target=$target environment=$environment id=$target_id"
printf '%s\n' "oa28-live-handoff-preflight: external effects remain disabled until target-specific receipt, reconcile, retention and cleanup evidence is recorded"
