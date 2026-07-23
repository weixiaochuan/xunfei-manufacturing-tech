#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

require_command docker
require_command curl
load_cloud_env

wait_for_service_health postgres healthy 20 3
wait_for_service_health account-server healthy 20 3

for internal_service in postgres casdoor account-server; do
  if [[ -n "$(cloud_compose port "${internal_service}" 5432 2>/dev/null || true)" ||
        -n "$(cloud_compose port "${internal_service}" 8000 2>/dev/null || true)" ||
        -n "$(cloud_compose port "${internal_service}" 3010 2>/dev/null || true)" ]]; then
    die "${internal_service} unexpectedly publishes an internal port."
  fi
done

check_https_endpoint() {
  local label="$1"
  local url="$2"
  local attempts="${3:-24}"

  for ((attempt = 1; attempt <= attempts; attempt += 1)); do
    if curl --fail --silent --show-error --max-time 10 --output /dev/null "${url}"; then
      printf '%s reachable: %s\n' "${label}" "${url}"
      return 0
    fi
    sleep 5
  done

  die "${label} did not become reachable over trusted HTTPS: ${url}"
}

check_https_endpoint "Casdoor OIDC discovery" \
  "https://${AUTH_DOMAIN}/.well-known/openid-configuration"
check_https_endpoint "Account Server liveness" \
  "https://${API_DOMAIN}/health/live"
check_https_endpoint "Account Server readiness" \
  "https://${API_DOMAIN}/health/ready"

printf 'Cloud service health checks passed.\n'
