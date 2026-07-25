#!/usr/bin/env bash

set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

casdoor_test_require_command docker
bash "${SCRIPT_DIR}/validate-casdoor-test.sh"
casdoor_test_load_environment

casdoor_test_compose up -d postgres-test
casdoor_test_wait_for_status postgres-test healthy 40 3

casdoor_test_compose up -d casdoor-test
casdoor_test_wait_for_status casdoor-test running 40 3

bash "${SCRIPT_DIR}/check-casdoor-test.sh"
printf 'Isolated Casdoor TEST services started successfully.\n'
