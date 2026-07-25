#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

casdoor_test_require_command docker
casdoor_test_require_command curl
bash "${SCRIPT_DIR}/validate-casdoor-test.sh"
casdoor_test_load_environment

casdoor_test_wait_for_status postgres-test healthy 20 3
casdoor_test_wait_for_status casdoor-test running 20 3

postgres_id="$(casdoor_test_compose ps -q postgres-test)"
casdoor_id="$(casdoor_test_compose ps -q casdoor-test)"
[[ -n "${postgres_id}" && -n "${casdoor_id}" ]] ||
  casdoor_test_die "Casdoor TEST containers are not both running."

postgres_volume="$(
  docker inspect \
    --format '{{range .Mounts}}{{if eq .Destination "/var/lib/postgresql/data"}}{{.Name}}{{end}}{{end}}' \
    "${postgres_id}"
)"
[[ "${postgres_volume}" == "pomegranate_casdoor_test_postgres_data" ]] ||
  casdoor_test_die "postgres-test is not using the approved isolated named volume."

expected_network="pomegranate_casdoor_test_backend"
for container_id in "${postgres_id}" "${casdoor_id}"; do
  network_names="$(
    docker inspect \
      --format '{{range $name, $_ := .NetworkSettings.Networks}}{{println $name}}{{end}}' \
      "${container_id}" |
      sed '/^[[:space:]]*$/d' |
      sort -u
  )"
  [[ "${network_names}" == "${expected_network}" ]] ||
    casdoor_test_die "A Casdoor TEST container is attached to an unexpected network."
done

[[ -z "$(docker port "${postgres_id}" 2>/dev/null || true)" ]] ||
  casdoor_test_die "postgres-test must not publish a host port."
[[ "$(docker port "${casdoor_id}" 8000/tcp)" == "127.0.0.1:18000" ]] ||
  casdoor_test_die "casdoor-test is not bound exclusively to 127.0.0.1:18000."

database_exists="$(
  casdoor_test_compose exec -T postgres-test sh -eu -c \
    'psql --username "$POSTGRES_USER" --dbname postgres --tuples-only --no-align --command "SELECT 1 FROM pg_database WHERE datname = '\''$CASDOOR_TEST_DB_NAME'\'';"' |
    tr -d '[:space:]'
)"
[[ "${database_exists}" == "1" ]] ||
  casdoor_test_die "The isolated casdoor_test database does not exist."

role_exists="$(
  casdoor_test_compose exec -T postgres-test sh -eu -c \
    'psql --username "$POSTGRES_USER" --dbname postgres --tuples-only --no-align --command "SELECT 1 FROM pg_roles WHERE rolname = '\''$CASDOOR_TEST_DB_USER'\'';"' |
    tr -d '[:space:]'
)"
[[ "${role_exists}" == "1" ]] ||
  casdoor_test_die "The isolated casdoor_test_app role does not exist."

for ((attempt = 1; attempt <= 24; attempt += 1)); do
  if curl --fail --silent --show-error --max-time 10 \
    --output /dev/null \
    "${CASDOOR_TEST_PUBLIC_URL}/.well-known/openid-configuration"; then
    break
  fi
  [[ "${attempt}" -lt 24 ]] || casdoor_test_die "Casdoor TEST OIDC discovery is not reachable."
  sleep 5
done

discovery_file="$(mktemp)"
trap 'rm -f "${discovery_file}"' EXIT
curl --fail --silent --show-error --max-time 10 \
  --output "${discovery_file}" \
  "${CASDOOR_TEST_PUBLIC_URL}/.well-known/openid-configuration"
grep -Eq '"issuer"[[:space:]]*:[[:space:]]*"http://127\.0\.0\.1:18000"' \
  "${discovery_file}" ||
  casdoor_test_die "Casdoor TEST discovery issuer does not match the loopback-only origin."

printf 'Casdoor TEST isolation, database, binding, and OIDC checks passed.\n'
