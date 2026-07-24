#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=public-ip-test-common.sh
source "${SCRIPT_DIR}/public-ip-test-common.sh"

public_ip_test_require_command docker
public_ip_test_load_environment
docker compose version >/dev/null

required_variables=(
  DEPLOYMENT_PROFILE ALLOW_INSECURE_PUBLIC_IP_TEST
  ACCOUNT_SERVER_HOST ACCOUNT_SERVER_PORT ACCOUNT_SERVER_PUBLIC_URL
  CASDOOR_PUBLIC_URL CASDOOR_REDIRECT_URI ACCOUNT_SERVER_IMAGE_TAG
  POSTGRES_ADMIN_PASSWORD CASDOOR_DB_PASSWORD ACCOUNT_DB_PASSWORD
  CASDOOR_CLIENT_ID CASDOOR_CLIENT_SECRET
  POSTGRES_ADMIN_USER
  ACCOUNT_DB_HOST ACCOUNT_DB_PORT ACCOUNT_DB_NAME ACCOUNT_DB_USER
  CASDOOR_DB_HOST CASDOOR_DB_PORT CASDOOR_DB_NAME CASDOOR_DB_USER
  POMEGRANATE_DATA_DIR USER_FILES_ROOT
)

for variable_name in "${required_variables[@]}"; do
  [[ -n "${!variable_name:-}" ]] ||
    public_ip_test_die "Required variable is empty: ${variable_name}"
done

for secret_name in \
  POSTGRES_ADMIN_PASSWORD CASDOOR_DB_PASSWORD ACCOUNT_DB_PASSWORD CASDOOR_CLIENT_SECRET; do
  secret_value="${!secret_name}"
  case "${secret_value}" in
    *CHANGE_ME*|*REPLACE_WITH*|*example*)
      public_ip_test_die "${secret_name} still contains a placeholder."
      ;;
  esac
  [[ "${#secret_value}" -ge 24 ]] ||
    public_ip_test_die "${secret_name} must contain at least 24 characters."
done

[[ "${DEPLOYMENT_PROFILE}" == "public-ip-test" ]] ||
  public_ip_test_die "DEPLOYMENT_PROFILE must be public-ip-test."
[[ "${ALLOW_INSECURE_PUBLIC_IP_TEST}" == "true" ]] ||
  public_ip_test_die "ALLOW_INSECURE_PUBLIC_IP_TEST must be explicitly true."
[[ "${ACCOUNT_SERVER_HOST}" == "0.0.0.0" ]] ||
  public_ip_test_die "ACCOUNT_SERVER_HOST must be 0.0.0.0 inside the container."
[[ "${ACCOUNT_SERVER_PORT}" == "3010" ]] ||
  public_ip_test_die "ACCOUNT_SERVER_PORT must remain the internal port 3010."
[[ "${ACCOUNT_SERVER_PUBLIC_URL}" == "http://82.157.119.201:8080" ]] ||
  public_ip_test_die "ACCOUNT_SERVER_PUBLIC_URL is not the approved temporary origin."
[[ "${CASDOOR_PUBLIC_URL}" == "http://82.157.119.201:8000" ]] ||
  public_ip_test_die "CASDOOR_PUBLIC_URL is not the approved temporary origin."
[[ "${CASDOOR_REDIRECT_URI}" == "http://82.157.119.201:8080/auth/callback" ]] ||
  public_ip_test_die "CASDOOR_REDIRECT_URI is not the approved temporary callback."
[[ "${ACCOUNT_DB_HOST}" == "postgres" && "${CASDOOR_DB_HOST}" == "postgres" ]] ||
  public_ip_test_die "Database hosts must remain on the internal postgres service."
[[ "${ACCOUNT_DB_PORT}" == "5432" && "${CASDOOR_DB_PORT}" == "5432" ]] ||
  public_ip_test_die "Database ports must remain the internal port 5432."
[[ "${ACCOUNT_DB_NAME}" != "${CASDOOR_DB_NAME}" ]] ||
  public_ip_test_die "Account Server and Casdoor must keep separate databases."
[[ "${ACCOUNT_DB_USER}" != "${CASDOOR_DB_USER}" ]] ||
  public_ip_test_die "Account Server and Casdoor must keep separate database roles."
[[ "${POSTGRES_ADMIN_USER}" != "${ACCOUNT_DB_USER}" &&
   "${POSTGRES_ADMIN_USER}" != "${CASDOOR_DB_USER}" ]] ||
  public_ip_test_die "Application roles must not reuse the PostgreSQL administrator."
[[ "${POSTGRES_ADMIN_PASSWORD}" != "${CASDOOR_DB_PASSWORD}" &&
   "${POSTGRES_ADMIN_PASSWORD}" != "${ACCOUNT_DB_PASSWORD}" &&
   "${POSTGRES_ADMIN_PASSWORD}" != "${CASDOOR_CLIENT_SECRET}" &&
   "${CASDOOR_DB_PASSWORD}" != "${ACCOUNT_DB_PASSWORD}" &&
   "${CASDOOR_DB_PASSWORD}" != "${CASDOOR_CLIENT_SECRET}" &&
   "${ACCOUNT_DB_PASSWORD}" != "${CASDOOR_CLIENT_SECRET}" ]] ||
  public_ip_test_die "Every database password and Client Secret must remain distinct."
[[ "${POMEGRANATE_DATA_DIR}" == "/srv/pomegranate/data" ]] ||
  public_ip_test_die "POMEGRANATE_DATA_DIR must remain /srv/pomegranate/data."
[[ "${USER_FILES_ROOT}" == "/srv/pomegranate/data/user-files" ]] ||
  public_ip_test_die "USER_FILES_ROOT must remain private under /srv/pomegranate/data."

public_ip_test_compose config --quiet
rendered_config="$(public_ip_test_compose config)"
published_ports="$(
  awk '/published:/ {gsub(/"/, "", $2); print $2}' <<<"${rendered_config}" |
    sort -n |
    uniq
)"
[[ "${published_ports}" == $'8000\n8080' ]] ||
  public_ip_test_die "Only Caddy host ports 8000 and 8080 may be published."

grep -Fq 'Caddyfile.public-ip-test' <<<"${rendered_config}" ||
  public_ip_test_die "Temporary Caddyfile override is not active."

printf 'Public IP TEST environment and Compose override are valid.\n'
