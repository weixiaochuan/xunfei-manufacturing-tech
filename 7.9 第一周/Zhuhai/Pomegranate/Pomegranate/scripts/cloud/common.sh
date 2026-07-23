#!/usr/bin/env bash

set -Eeuo pipefail

CLOUD_SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd -- "${CLOUD_SCRIPT_DIR}/../.." && pwd)"
CLOUD_ENV_FILE="${POMEGRANATE_ENV_FILE:-${PROJECT_ROOT}/.env.cloud}"
CLOUD_COMPOSE_FILE="${PROJECT_ROOT}/compose.cloud.yml"

die() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "Required command not found: $1"
}

require_env_file() {
  [[ -f "${CLOUD_ENV_FILE}" ]] ||
    die "Missing ${CLOUD_ENV_FILE}. Copy .env.cloud.example to .env.cloud and replace every placeholder."
}

load_cloud_env() {
  require_env_file
  set -a
  # The deployment operator owns this file. It must contain shell-compatible KEY=VALUE entries.
  # shellcheck disable=SC1090
  source "${CLOUD_ENV_FILE}"
  set +a
}

cloud_compose() {
  docker compose --env-file "${CLOUD_ENV_FILE}" -f "${CLOUD_COMPOSE_FILE}" "$@"
}

container_health() {
  local service_name="$1"
  local container_id
  container_id="$(cloud_compose ps -q "${service_name}")"
  [[ -n "${container_id}" ]] || return 1
  docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "${container_id}"
}

wait_for_service_health() {
  local service_name="$1"
  local expected_status="$2"
  local attempts="${3:-40}"
  local delay_seconds="${4:-3}"
  local status=""

  for ((attempt = 1; attempt <= attempts; attempt += 1)); do
    status="$(container_health "${service_name}" 2>/dev/null || true)"
    if [[ "${status}" == "${expected_status}" ]]; then
      printf '%s status: %s\n' "${service_name}" "${status}"
      return 0
    fi
    sleep "${delay_seconds}"
  done

  cloud_compose ps >&2 || true
  die "${service_name} did not reach ${expected_status}; last status was ${status:-missing}."
}
