#!/usr/bin/env bash

set -Eeuo pipefail

PUBLIC_IP_TEST_SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PUBLIC_IP_TEST_PROJECT_ROOT="$(cd -- "${PUBLIC_IP_TEST_SCRIPT_DIR}/../.." && pwd)"
PUBLIC_IP_TEST_CLOUD_ENV="${POMEGRANATE_CLOUD_ENV_FILE:-${PUBLIC_IP_TEST_PROJECT_ROOT}/.env.cloud}"
PUBLIC_IP_TEST_ENV="${POMEGRANATE_PUBLIC_IP_TEST_ENV_FILE:-${PUBLIC_IP_TEST_PROJECT_ROOT}/.env.public-ip-test}"
PUBLIC_IP_TEST_CLOUD_COMPOSE="${PUBLIC_IP_TEST_PROJECT_ROOT}/compose.cloud.yml"
PUBLIC_IP_TEST_OVERRIDE_COMPOSE="${PUBLIC_IP_TEST_PROJECT_ROOT}/compose.public-ip-test.yml"

public_ip_test_die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

public_ip_test_require_command() {
  command -v "$1" >/dev/null 2>&1 ||
    public_ip_test_die "Required command not found: $1"
}

public_ip_test_require_files() {
  [[ -f "${PUBLIC_IP_TEST_CLOUD_ENV}" ]] ||
    public_ip_test_die "Missing private base environment: ${PUBLIC_IP_TEST_CLOUD_ENV}"
  [[ -f "${PUBLIC_IP_TEST_ENV}" ]] ||
    public_ip_test_die "Missing temporary environment: ${PUBLIC_IP_TEST_ENV}"
  [[ -f "${PUBLIC_IP_TEST_CLOUD_COMPOSE}" ]] ||
    public_ip_test_die "Missing ${PUBLIC_IP_TEST_CLOUD_COMPOSE}"
  [[ -f "${PUBLIC_IP_TEST_OVERRIDE_COMPOSE}" ]] ||
    public_ip_test_die "Missing ${PUBLIC_IP_TEST_OVERRIDE_COMPOSE}"
}

public_ip_test_load_environment() {
  public_ip_test_require_files
  set -a
  # The private cloud file supplies existing secrets; the temporary file may override only public settings.
  # shellcheck disable=SC1090
  source "${PUBLIC_IP_TEST_CLOUD_ENV}"
  # shellcheck disable=SC1090
  source "${PUBLIC_IP_TEST_ENV}"
  set +a
}

public_ip_test_compose() {
  docker compose \
    --env-file "${PUBLIC_IP_TEST_CLOUD_ENV}" \
    --env-file "${PUBLIC_IP_TEST_ENV}" \
    -f "${PUBLIC_IP_TEST_CLOUD_COMPOSE}" \
    -f "${PUBLIC_IP_TEST_OVERRIDE_COMPOSE}" \
    "$@"
}

public_ip_test_container_status() {
  local service_name="$1"
  local container_id
  container_id="$(public_ip_test_compose ps -q "${service_name}")"
  [[ -n "${container_id}" ]] || return 1
  docker inspect \
    --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' \
    "${container_id}"
}

public_ip_test_wait_for_status() {
  local service_name="$1"
  local expected_status="$2"
  local attempts="${3:-40}"
  local delay_seconds="${4:-3}"
  local status=""

  for ((attempt = 1; attempt <= attempts; attempt += 1)); do
    status="$(public_ip_test_container_status "${service_name}" 2>/dev/null || true)"
    if [[ "${status}" == "${expected_status}" ]]; then
      printf '%s status: %s\n' "${service_name}" "${status}"
      return 0
    fi
    sleep "${delay_seconds}"
  done

  public_ip_test_compose ps >&2 || true
  public_ip_test_die \
    "${service_name} did not reach ${expected_status}; last status was ${status:-missing}."
}

public_ip_test_retry_url() {
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

  public_ip_test_die "${label} did not become reachable: ${url}"
}
