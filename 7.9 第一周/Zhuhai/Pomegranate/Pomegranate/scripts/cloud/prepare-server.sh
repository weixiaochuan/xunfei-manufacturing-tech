#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

[[ "$(uname -s)" == "Linux" ]] || die "prepare-server.sh must run on Linux."
[[ -r /etc/os-release ]] || die "Unable to identify this Linux distribution."
# shellcheck disable=SC1091
source /etc/os-release
[[ "${ID:-}" == "ubuntu" ]] || die "This deployment baseline is supported on Ubuntu; detected ${ID:-unknown}."
[[ "${EUID}" -eq 0 ]] || die "Run this script with sudo so it can create /srv/pomegranate safely."

require_command docker
docker compose version >/dev/null || die "Docker Compose v2 is required."

runtime_uid=1000
runtime_gid=1000
if [[ -f "${CLOUD_ENV_FILE}" ]]; then
  load_cloud_env
  runtime_uid="${POMEGRANATE_UID:-1000}"
  runtime_gid="${POMEGRANATE_GID:-1000}"
fi

[[ "${runtime_uid}" =~ ^[1-9][0-9]*$ ]] || die "POMEGRANATE_UID must be a positive integer."
[[ "${runtime_gid}" =~ ^[1-9][0-9]*$ ]] || die "POMEGRANATE_GID must be a positive integer."

install -d -m 0750 -o root -g root /srv/pomegranate
install -d -m 0750 -o root -g root /srv/pomegranate/deploy
install -d -m 0750 -o root -g root /srv/pomegranate/data
install -d -m 0750 -o "${runtime_uid}" -g "${runtime_gid}" /srv/pomegranate/data/user-files
install -d -m 0750 -o root -g root /srv/pomegranate/backups
install -d -m 0750 -o root -g root /srv/pomegranate/logs

printf 'Prepared /srv/pomegranate with a private data directory owned by UID:GID %s:%s.\n' \
  "${runtime_uid}" "${runtime_gid}"
