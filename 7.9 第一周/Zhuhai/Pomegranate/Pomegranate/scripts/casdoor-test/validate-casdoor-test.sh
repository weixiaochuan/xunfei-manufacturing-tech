#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

casdoor_test_require_command docker
casdoor_test_load_environment
docker compose version >/dev/null

required_variables=(
  CASDOOR_TEST_POSTGRES_ADMIN_USER
  CASDOOR_TEST_POSTGRES_ADMIN_PASSWORD
  CASDOOR_TEST_DB_HOST
  CASDOOR_TEST_DB_PORT
  CASDOOR_TEST_DB_NAME
  CASDOOR_TEST_DB_USER
  CASDOOR_TEST_DB_PASSWORD
  CASDOOR_TEST_HOST_PORT
  CASDOOR_TEST_PUBLIC_URL
  CASDOOR_TEST_ORGANIZATION
  CASDOOR_TEST_APPLICATION
)

for variable_name in "${required_variables[@]}"; do
  [[ -n "${!variable_name:-}" ]] ||
    casdoor_test_die "Required variable is empty: ${variable_name}"
done

for secret_name in \
  CASDOOR_TEST_POSTGRES_ADMIN_PASSWORD CASDOOR_TEST_DB_PASSWORD; do
  secret_value="${!secret_name}"
  case "${secret_value}" in
    *CHANGE_ME*|*REPLACE_WITH*|*example*|*placeholder*)
      casdoor_test_die "${secret_name} still contains a placeholder."
      ;;
  esac
  [[ "${#secret_value}" -ge 24 ]] ||
    casdoor_test_die "${secret_name} must contain at least 24 characters."
done

[[ "${CASDOOR_TEST_POSTGRES_ADMIN_PASSWORD}" != "${CASDOOR_TEST_DB_PASSWORD}" ]] ||
  casdoor_test_die "PostgreSQL administrator and Casdoor TEST database passwords must differ."
[[ "${CASDOOR_TEST_POSTGRES_ADMIN_USER}" != "${CASDOOR_TEST_DB_USER}" ]] ||
  casdoor_test_die "Casdoor TEST must not use the PostgreSQL administrator as its application role."

for identifier_name in \
  CASDOOR_TEST_POSTGRES_ADMIN_USER CASDOOR_TEST_DB_NAME CASDOOR_TEST_DB_USER; do
  identifier_value="${!identifier_name}"
  [[ "${identifier_value}" =~ ^[A-Za-z0-9_]+$ ]] ||
    casdoor_test_die "${identifier_name} may contain only ASCII letters, digits, and underscores."
done

[[ "${CASDOOR_TEST_DB_HOST}" == "postgres-test" ]] ||
  casdoor_test_die "CASDOOR_TEST_DB_HOST must be the isolated postgres-test service."
[[ "${CASDOOR_TEST_DB_PORT}" == "5432" ]] ||
  casdoor_test_die "CASDOOR_TEST_DB_PORT must remain the internal port 5432."
[[ "${CASDOOR_TEST_DB_NAME}" == "casdoor_test" ]] ||
  casdoor_test_die "CASDOOR_TEST_DB_NAME must be casdoor_test."
[[ "${CASDOOR_TEST_DB_USER}" == "casdoor_test_app" ]] ||
  casdoor_test_die "CASDOOR_TEST_DB_USER must be casdoor_test_app."
[[ "${CASDOOR_TEST_HOST_PORT}" == "18000" ]] ||
  casdoor_test_die "CASDOOR_TEST_HOST_PORT must remain 18000."
[[ "${CASDOOR_TEST_PUBLIC_URL}" == "http://127.0.0.1:18000" ]] ||
  casdoor_test_die "CASDOOR_TEST_PUBLIC_URL must remain the loopback-only test origin."
[[ "${CASDOOR_TEST_ORGANIZATION}" == "pomegranate-test" ]] ||
  casdoor_test_die "CASDOOR_TEST_ORGANIZATION must be pomegranate-test."
[[ "${CASDOOR_TEST_APPLICATION}" == "app-pomegranate-test" ]] ||
  casdoor_test_die "CASDOOR_TEST_APPLICATION must be app-pomegranate-test."

casdoor_test_compose config --quiet
rendered_config="$(casdoor_test_compose config)"

service_names="$(casdoor_test_compose config --services | sort)"
[[ "${service_names}" == $'casdoor-test\npostgres-test' ]] ||
  casdoor_test_die "The test stack must contain only casdoor-test and postgres-test."

published_ports="$(
  awk '/published:/ {gsub(/"/, "", $2); print $2}' <<<"${rendered_config}" |
    sort -n |
    uniq
)"
[[ "${published_ports}" == "18000" ]] ||
  casdoor_test_die "Only host port 18000 may be published by the Casdoor TEST stack."
grep -Fq 'host_ip: 127.0.0.1' <<<"${rendered_config}" ||
  casdoor_test_die "Casdoor TEST must bind only to 127.0.0.1."
grep -Fq 'name: pomegranate_casdoor_test_postgres_data' <<<"${rendered_config}" ||
  casdoor_test_die "The isolated PostgreSQL volume name is missing."
grep -Fq 'name: pomegranate_casdoor_test_backend' <<<"${rendered_config}" ||
  casdoor_test_die "The isolated Casdoor TEST network name is missing."

for forbidden_value in \
  pomegranate_backend pomegranate_edge pomegranate-cloud \
  account-server /srv/pomegranate/data; do
  if grep -Fq "${forbidden_value}" <<<"${rendered_config}"; then
    casdoor_test_die "Rendered test configuration references forbidden formal resource: ${forbidden_value}"
  fi
done

printf 'Casdoor TEST environment and Compose configuration are valid.\n'
