#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=public-ip-test-common.sh
source "${SCRIPT_DIR}/public-ip-test-common.sh"

public_ip_test_require_command docker
"${SCRIPT_DIR}/validate-public-ip-test.sh"
public_ip_test_load_environment

postgres_id="$(public_ip_test_compose ps -q postgres)"
[[ -n "${postgres_id}" ]] ||
  public_ip_test_die "The existing PostgreSQL container is missing; refusing to create a new one."
[[ "$(public_ip_test_container_status postgres)" == "healthy" ]] ||
  public_ip_test_die "The existing PostgreSQL container is not healthy."

postgres_volume="$(
  docker inspect \
    --format '{{range .Mounts}}{{if eq .Destination "/var/lib/postgresql/data"}}{{.Name}}{{end}}{{end}}' \
    "${postgres_id}"
)"
[[ -n "${postgres_volume}" ]] ||
  public_ip_test_die "The existing PostgreSQL named volume could not be identified."
docker volume inspect "${postgres_volume}" >/dev/null
printf 'Preserving PostgreSQL container %s and named volume %s.\n' \
  "${postgres_id:0:12}" "${postgres_volume}"

public_ip_test_compose build account-server
public_ip_test_compose up -d --no-deps --force-recreate casdoor
public_ip_test_compose up -d --no-deps --force-recreate account-server
public_ip_test_wait_for_status account-server healthy 40 3
public_ip_test_compose up -d --no-deps --force-recreate caddy

"${SCRIPT_DIR}/check-public-ip-test.sh"
printf 'Temporary Public IP TEST entry points are active.\n'
