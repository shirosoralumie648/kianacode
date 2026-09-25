#!/usr/bin/env bash
set -euo pipefail

if [[ "${CI:-}" != "true" && "${GITHUB_ACTIONS:-}" != "true" ]]; then
  printf 'nm22_notification_closeout:remote_ci_required\n' >&2
  exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
required_files=(
  "docs/roadmap/notifications-baseline.md"
  "docs/roadmap/nm04-notification-projector-baseline.md"
  "docs/roadmap/nm05-notification-resolver-baseline.md"
  "docs/roadmap/nm06-notification-materializer-baseline.md"
  "docs/roadmap/nm07-notification-dedup-baseline.md"
  "docs/roadmap/nm08-notification-outbox-baseline.md"
  "docs/roadmap/nm09-notification-store-baseline.md"
  "docs/roadmap/nm10-notification-actions-baseline.md"
  "docs/roadmap/nm11-notification-priority-baseline.md"
  "docs/roadmap/nm12-notification-lifecycle-baseline.md"
  "docs/roadmap/nm13-notification-stream-baseline.md"
  "docs/roadmap/nm14-notification-cli-baseline.md"
  "docs/roadmap/nm15-web-notification-baseline.md"
  "docs/roadmap/nm16-desktop-notification-baseline.md"
  "docs/roadmap/nm17-notification-policy-baseline.md"
  "docs/roadmap/nm18-notification-recovery-baseline.md"
  "docs/roadmap/nm19-external-notification-baseline.md"
  "docs/roadmap/nm20-notification-faults-baseline.md"
  "docs/roadmap/nm21-notification-parity-baseline.md"
  "CURRENT_STATUS.md"
  "docs/roadmap.md"
  "docs/module-map.md"
)

for relative in "${required_files[@]}"; do
  [[ -f "${root}/${relative}" ]] || {
    printf 'nm22_notification_closeout:missing:%s\n' "${relative}" >&2
    exit 1
  }
done

for baseline in "${root}"/docs/roadmap/nm0{4,5,6,7,8,9}-notification-*-baseline.md \
  "${root}"/docs/roadmap/nm1{0,1,2,3}-notification-*-baseline.md \
  "${root}"/docs/roadmap/nm1{4,5,6,7,8,9}-*-baseline.md \
  "${root}"/docs/roadmap/nm20-notification-faults-baseline.md \
  "${root}"/docs/roadmap/nm21-notification-parity-baseline.md; do
  grep -Fq 'proof_level=source' "${baseline}" || {
    printf 'nm22_notification_closeout:proof_marker_missing:%s\n' "${baseline#"${root}/"}" >&2
    exit 1
  }
  grep -Fq 'Known limits' "${baseline}" || {
    printf 'nm22_notification_closeout:limitations_missing:%s\n' "${baseline#"${root}/"}" >&2
    exit 1
  }
done

for number in {00..21}; do
  grep -Eq "NM-${number}" "${root}/CURRENT_STATUS.md" || {
    printf 'nm22_notification_closeout:status_missing:NM-%s\n' "${number}" >&2
    exit 1
  }
done

grep -Eq '\| 700 \| W9 \| 专项 \| \[`NM-22`\].*\| 🔄 \|' "${root}/docs/roadmap.md" || {
  printf 'nm22_notification_closeout:roadmap_not_in_progress\n' >&2
  exit 1
}

for forbidden in 'durable verified' 'live verified' 'physical verified' 'all notifications delivered'; do
  if grep -Fq "${forbidden}" "${root}/docs/roadmap/nm21-notification-parity-baseline.md"; then
    printf 'nm22_notification_closeout:overclaim:%s\n' "${forbidden}" >&2
    exit 1
  fi
done

printf 'schema=kiana.notification-closeout.v1 proof_level=source status=remote_ci_gate\n'
