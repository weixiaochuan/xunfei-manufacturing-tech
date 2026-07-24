#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=public-ip-test-common.sh
source "${SCRIPT_DIR}/public-ip-test-common.sh"

public_ip_test_require_command docker
public_ip_test_require_command curl
public_ip_test_load_environment

public_ip_test_wait_for_status postgres healthy 20 3
public_ip_test_wait_for_status casdoor running 20 3
public_ip_test_wait_for_status account-server healthy 20 3
public_ip_test_wait_for_status caddy running 20 3

public_ip_test_compose exec -T account-server node -e \
  "fetch('http://127.0.0.1:3010/health/ready').then((response) => { if (!response.ok) process.exit(1); }).catch(() => process.exit(1));"
printf 'Account Server internal readiness check passed.\n'

public_ip_test_retry_url \
  "Account Server liveness" \
  "http://82.157.119.201:8080/health/live"
public_ip_test_retry_url \
  "Account Server readiness" \
  "http://82.157.119.201:8080/health/ready"
public_ip_test_retry_url \
  "Casdoor OIDC discovery" \
  "http://82.157.119.201:8000/.well-known/openid-configuration"

discovery_file="$(mktemp)"
trap 'rm -f "${discovery_file}"' EXIT
curl --fail --silent --show-error --max-time 10 \
  --output "${discovery_file}" \
  "http://82.157.119.201:8000/.well-known/openid-configuration"
grep -Eq '"issuer"[[:space:]]*:[[:space:]]*"http://82\.157\.119\.201:8000"' \
  "${discovery_file}" ||
  public_ip_test_die "Casdoor discovery issuer does not match the temporary public origin."

printf 'Public IP TEST health and OIDC checks passed.\n'
