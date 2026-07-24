#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=public-ip-test-common.sh
source "${SCRIPT_DIR}/public-ip-test-common.sh"

public_ip_test_require_command docker
public_ip_test_require_files

# Closing only Caddy removes the temporary public listeners without touching data services or volumes.
public_ip_test_compose stop caddy
printf 'Temporary ports 8000 and 8080 are closed. PostgreSQL, Casdoor, Account Server, volumes, and user files were preserved.\n'
printf 'Restore the private formal cloud environment, then use scripts/cloud/start-services.sh to return to HTTPS.\n'
