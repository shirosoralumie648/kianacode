#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'ui39_live_acp_opt_in_failed:%s\n' "$1" >&2
  exit 1
}

if [[ "${KIANA_UI39_OPT_IN:-}" != "1" ]]; then
  printf 'ui39_live_acp_status=not_supported reason=explicit_opt_in_required\n'
  exit 2
fi

for name in \
  KIANA_UI39_PROTOCOL \
  KIANA_UI39_HOST_ID \
  KIANA_UI39_HOST_VERSION \
  KIANA_UI39_ENVIRONMENT_DIGEST \
  KIANA_UI39_WORKSPACE_DIGEST \
  KIANA_UI39_APPROVAL_REF \
  KIANA_UI39_TRANSPORT \
  KIANA_UI39_REDACTED; do
  [[ -n "${!name:-}" ]] || fail "${name}_missing"
done

case "${KIANA_UI39_PROTOCOL}" in
  acp.v1|acp.v2) ;;
  *) fail "protocol_not_supported" ;;
esac

case "${KIANA_UI39_TRANSPORT}" in
  local_stdio|local_unix_socket) ;;
  *) fail "transport_not_local" ;;
esac

[[ "${KIANA_UI39_REDACTED}" == "1" ]] || fail "redaction_required"
[[ "${KIANA_UI39_APPROVAL_REF}" == approval:* ]] || fail "approval_ref_invalid"
[[ "${KIANA_UI39_ENVIRONMENT_DIGEST}" =~ ^sha256:[[:xdigit:]]{64}$ ]] || fail "environment_digest_invalid"
[[ "${KIANA_UI39_WORKSPACE_DIGEST}" =~ ^sha256:[[:xdigit:]]{64}$ ]] || fail "workspace_digest_invalid"

for name in KIANA_UI39_HOST_ID KIANA_UI39_HOST_VERSION KIANA_UI39_APPROVAL_REF; do
  value="${!name}"
  [[ "${value}" != *$'\n'* && "${value}" != *$'\r'* ]] || fail "${name}_newline"
  case "${value,,}" in
    *bearer\ *|*token*|*secret*|*api_key*|*api-key*) fail "${name}_secret_marker" ;;
  esac
done

[[ "${KIANA_UI39_EXTERNAL_HOST:-}" != "1" ]] || fail "external_host_execution_not_supported"

printf 'ui39_live_acp_status=opted_in_source_boundary protocol=%s transport=%s\n' \
  "${KIANA_UI39_PROTOCOL}" "${KIANA_UI39_TRANSPORT}"
