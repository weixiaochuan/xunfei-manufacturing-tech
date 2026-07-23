#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

require_command docker
require_env_file

# Stop containers without deleting them, networks, named volumes, or user files.
cloud_compose stop caddy account-server account-migrate casdoor postgres
printf 'Cloud services stopped. Containers, named volumes, and user files were preserved.\n'
