#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

require_command docker
"${SCRIPT_DIR}/validate-env.sh"
load_cloud_env

[[ -d "${POMEGRANATE_DATA_DIR}/user-files" ]] ||
  die "Missing ${POMEGRANATE_DATA_DIR}/user-files. Run prepare-server.sh first."

cloud_compose build account-server account-migrate
cloud_compose up -d postgres
wait_for_service_health postgres healthy 40 3

cloud_compose up -d casdoor
cloud_compose up account-migrate

cloud_compose up -d account-server
wait_for_service_health account-server healthy 40 3

cloud_compose up -d caddy
"${SCRIPT_DIR}/health-check.sh"

printf 'Pomegranate cloud services started successfully.\n'
