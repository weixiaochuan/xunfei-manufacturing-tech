#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

require_command docker
load_cloud_env

required_variables=(
  AUTH_DOMAIN API_DOMAIN ACCOUNT_SERVER_PUBLIC_URL CASDOOR_PUBLIC_URL CASDOOR_REDIRECT_URI
  POSTGRES_ADMIN_USER POSTGRES_ADMIN_PASSWORD
  CASDOOR_DB_HOST CASDOOR_DB_PORT CASDOOR_DB_NAME CASDOOR_DB_USER CASDOOR_DB_PASSWORD
  ACCOUNT_DB_HOST ACCOUNT_DB_PORT ACCOUNT_DB_NAME ACCOUNT_DB_USER ACCOUNT_DB_PASSWORD
  CASDOOR_ORGANIZATION CASDOOR_APPLICATION CASDOOR_CLIENT_ID CASDOOR_CLIENT_SECRET
  DEPLOYMENT_PROFILE ACCOUNT_SERVER_HOST ACCOUNT_SERVER_PORT NODE_ENV
  FILE_STORAGE_BACKEND USER_FILES_ROOT FILE_STORAGE_ALLOW_LEGACY_ROLLBACK
  USER_FILE_MAX_BYTES OIDC_DEBUG_CLAIM_TYPES POMEGRANATE_DATA_DIR
  POMEGRANATE_UID POMEGRANATE_GID ACCOUNT_SERVER_IMAGE_TAG CADDY_MAX_REQUEST_BODY
)

for variable_name in "${required_variables[@]}"; do
  [[ -n "${!variable_name:-}" ]] || die "Required variable is empty: ${variable_name}"
done

for variable_name in "${required_variables[@]}"; do
  value="${!variable_name}"
  case "${value}" in
    *CHANGE_ME*|*REPLACE_WITH*|*example.com*)
      die "${variable_name} still contains a deployment placeholder."
      ;;
  esac
done

[[ "${AUTH_DOMAIN}" != "${API_DOMAIN}" ]] || die "AUTH_DOMAIN and API_DOMAIN must be different."
for public_value in \
  "${AUTH_DOMAIN}" "${API_DOMAIN}" "${ACCOUNT_SERVER_PUBLIC_URL}" \
  "${CASDOOR_PUBLIC_URL}" "${CASDOOR_REDIRECT_URI}"; do
  case "${public_value}" in
    *localhost*|*127.0.0.1*|*0.0.0.0*|*192.168.*|*10.*|*172.16.*|*172.17.*|*172.18.*|*172.19.*|*172.2[0-9].*|*172.3[01].*)
      die "Public cloud URLs and domains must not contain local or LAN addresses."
      ;;
  esac
done
[[ "${ACCOUNT_SERVER_PUBLIC_URL}" == "https://${API_DOMAIN}" ]] ||
  die "ACCOUNT_SERVER_PUBLIC_URL must be https://${API_DOMAIN}."
[[ "${CASDOOR_PUBLIC_URL}" == "https://${AUTH_DOMAIN}" ]] ||
  die "CASDOOR_PUBLIC_URL must be https://${AUTH_DOMAIN}."
[[ "${CASDOOR_REDIRECT_URI}" == "https://${API_DOMAIN}/auth/callback" ]] ||
  die "CASDOOR_REDIRECT_URI must be https://${API_DOMAIN}/auth/callback."

[[ "${DEPLOYMENT_PROFILE}" == "cloud" ]] || die "DEPLOYMENT_PROFILE must be cloud."
[[ "${ACCOUNT_SERVER_HOST}" == "0.0.0.0" ]] || die "Cloud Account Server must listen on 0.0.0.0 inside its container."
[[ "${ACCOUNT_SERVER_PORT}" == "3010" ]] || die "ACCOUNT_SERVER_PORT must be 3010."
[[ "${NODE_ENV}" == "production" ]] || die "NODE_ENV must be production."
[[ "${FILE_STORAGE_BACKEND}" == "filesystem" ]] || die "FILE_STORAGE_BACKEND must be filesystem."
[[ "${FILE_STORAGE_ALLOW_LEGACY_ROLLBACK}" == "false" ]] ||
  die "FILE_STORAGE_ALLOW_LEGACY_ROLLBACK must remain false."
[[ "${OIDC_DEBUG_CLAIM_TYPES}" == "false" ]] || die "OIDC_DEBUG_CLAIM_TYPES must remain false."
[[ "${POMEGRANATE_DATA_DIR}" == "/srv/pomegranate/data" ]] ||
  die "POMEGRANATE_DATA_DIR must be /srv/pomegranate/data."
[[ "${USER_FILES_ROOT}" == "/srv/pomegranate/data/user-files" ]] ||
  die "USER_FILES_ROOT must be /srv/pomegranate/data/user-files."
[[ "${USER_FILES_ROOT}" == /* ]] || die "USER_FILES_ROOT must be an absolute Linux path."
[[ "${POMEGRANATE_DATA_DIR}" == /* ]] || die "POMEGRANATE_DATA_DIR must be an absolute Linux path."

[[ "${CASDOOR_DB_HOST}" == "postgres" && "${ACCOUNT_DB_HOST}" == "postgres" ]] ||
  die "Both application database hosts must use the internal postgres service."
[[ "${CASDOOR_DB_PORT}" == "5432" && "${ACCOUNT_DB_PORT}" == "5432" ]] ||
  die "Both application database ports must be 5432."
[[ "${CASDOOR_DB_NAME}" != "${ACCOUNT_DB_NAME}" ]] || die "Casdoor and Account Server must use separate databases."
[[ "${CASDOOR_DB_USER}" != "${ACCOUNT_DB_USER}" ]] || die "Casdoor and Account Server must use separate database roles."
[[ "${POSTGRES_ADMIN_USER}" != "${CASDOOR_DB_USER}" && "${POSTGRES_ADMIN_USER}" != "${ACCOUNT_DB_USER}" ]] ||
  die "Application roles must not reuse the PostgreSQL administrator role."

[[ "${USER_FILE_MAX_BYTES}" =~ ^[1-9][0-9]*$ ]] || die "USER_FILE_MAX_BYTES must be a positive integer."
[[ "${POMEGRANATE_UID}" =~ ^[1-9][0-9]*$ ]] || die "POMEGRANATE_UID must be a positive integer."
[[ "${POMEGRANATE_GID}" =~ ^[1-9][0-9]*$ ]] || die "POMEGRANATE_GID must be a positive integer."
[[ "${ACCOUNT_SERVER_IMAGE_TAG}" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] ||
  die "ACCOUNT_SERVER_IMAGE_TAG contains unsupported characters."

for secret_name in POSTGRES_ADMIN_PASSWORD CASDOOR_DB_PASSWORD ACCOUNT_DB_PASSWORD CASDOOR_CLIENT_SECRET; do
  secret_value="${!secret_name}"
  [[ "${#secret_value}" -ge 24 ]] || die "${secret_name} must contain at least 24 characters."
done
[[ "${POSTGRES_ADMIN_PASSWORD}" != "${CASDOOR_DB_PASSWORD}" &&
   "${POSTGRES_ADMIN_PASSWORD}" != "${ACCOUNT_DB_PASSWORD}" &&
   "${POSTGRES_ADMIN_PASSWORD}" != "${CASDOOR_CLIENT_SECRET}" &&
   "${CASDOOR_DB_PASSWORD}" != "${ACCOUNT_DB_PASSWORD}" &&
   "${CASDOOR_DB_PASSWORD}" != "${CASDOOR_CLIENT_SECRET}" &&
   "${ACCOUNT_DB_PASSWORD}" != "${CASDOOR_CLIENT_SECRET}" ]] ||
  die "Every database password and Client Secret must use a distinct random value."

docker compose version >/dev/null
cloud_compose config --quiet

rendered_config="$(cloud_compose config)"
if grep -Eq 'published:[[:space:]]*"?((3010)|(5432)|(8000))"?' <<<"${rendered_config}"; then
  die "PostgreSQL, Casdoor, or Account Server is unexpectedly published on a host port."
fi

printf 'Cloud environment and Compose configuration are valid.\n'
