#!/usr/bin/env bash

set -Eeuo pipefail

CASDOOR_TEST_SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CASDOOR_TEST_PROJECT_ROOT="$(cd -- "${CASDOOR_TEST_SCRIPT_DIR}/../.." && pwd)"
CASDOOR_TEST_ENV_FILE="${POMEGRANATE_CASDOOR_TEST_ENV_FILE:-${CASDOOR_TEST_PROJECT_ROOT}/.env.casdoor-test}"
CASDOOR_TEST_COMPOSE_FILE="${CASDOOR_TEST_PROJECT_ROOT}/compose.casdoor-test.yml"

casdoor_test_die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

casdoor_test_require_command() {
  command -v "$1" >/dev/null 2>&1 ||
    casdoor_test_die "Required command not found: $1"
}

casdoor_test_require_files() {
  [[ -f "${CASDOOR_TEST_ENV_FILE}" ]] ||
    casdoor_test_die \
      "Missing ${CASDOOR_TEST_ENV_FILE}. Copy .env.casdoor-test.example to .env.casdoor-test and replace every placeholder."
  [[ -f "${CASDOOR_TEST_COMPOSE_FILE}" ]] ||
    casdoor_test_die "Missing ${CASDOOR_TEST_COMPOSE_FILE}."
}

casdoor_test_load_environment() {
  casdoor_test_require_files
  set -a
  # The deployment operator owns this shell-compatible, test-only environment file.
  # shellcheck disable=SC1090
  source "${CASDOOR_TEST_ENV_FILE}"
  set +a
}

casdoor_test_compose() {
  docker compose \
    --env-file "${CASDOOR_TEST_ENV_FILE}" \
    -f "${CASDOOR_TEST_COMPOSE_FILE}" \
    "$@"
}

casdoor_test_container_status() {
  local service_name="$1"
  local container_id
  container_id="$(casdoor_test_compose ps -q "${service_name}")"
  [[ -n "${container_id}" ]] || return 1
  docker inspect \
    --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' \
    "${container_id}"
}

casdoor_test_wait_for_status() {
  local service_name="$1"
  local expected_status="$2"
  local attempts="${3:-40}"
  local delay_seconds="${4:-3}"
  local status=""

  for ((attempt = 1; attempt <= attempts; attempt += 1)); do
    status="$(casdoor_test_container_status "${service_name}" 2>/dev/null || true)"
    if [[ "${status}" == "${expected_status}" ]]; then
      printf '%s status: %s\n' "${service_name}" "${status}"
      return 0
    fi
    sleep "${delay_seconds}"
  done

  casdoor_test_compose ps >&2 || true
  casdoor_test_die \
    "${service_name} did not reach ${expected_status}; last status was ${status:-missing}."
}
