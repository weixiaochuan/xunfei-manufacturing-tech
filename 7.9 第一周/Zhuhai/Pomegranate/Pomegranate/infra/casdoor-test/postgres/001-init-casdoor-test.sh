#!/bin/sh

set -eu

required_variables="
POSTGRES_USER
CASDOOR_TEST_DB_NAME
CASDOOR_TEST_DB_USER
CASDOOR_TEST_DB_PASSWORD
"

for variable_name in $required_variables; do
  eval "variable_value=\${$variable_name:-}"
  if [ -z "$variable_value" ]; then
    echo "Missing required Casdoor TEST PostgreSQL variable: $variable_name" >&2
    exit 1
  fi
done

case "$POSTGRES_USER:$CASDOOR_TEST_DB_NAME:$CASDOOR_TEST_DB_USER" in
  *[!A-Za-z0-9_:]*)
    echo "Casdoor TEST database and role names may contain only ASCII letters, digits, and underscores" >&2
    exit 1
    ;;
esac

if [ "$POSTGRES_USER" = "$CASDOOR_TEST_DB_USER" ]; then
  echo "Casdoor TEST application role must not reuse the PostgreSQL administrator role" >&2
  exit 1
fi

psql \
  --username "$POSTGRES_USER" \
  --dbname "$POSTGRES_DB" \
  --set=ON_ERROR_STOP=1 \
  --set=casdoor_test_db="$CASDOOR_TEST_DB_NAME" \
  --set=casdoor_test_user="$CASDOOR_TEST_DB_USER" \
  --set=casdoor_test_password="$CASDOOR_TEST_DB_PASSWORD" <<'SQL'
SELECT format(
  'CREATE ROLE %I LOGIN PASSWORD %L',
  :'casdoor_test_user',
  :'casdoor_test_password'
)
WHERE NOT EXISTS (
  SELECT 1 FROM pg_roles WHERE rolname = :'casdoor_test_user'
) \gexec

SELECT format(
  'CREATE DATABASE %I OWNER %I',
  :'casdoor_test_db',
  :'casdoor_test_user'
)
WHERE NOT EXISTS (
  SELECT 1 FROM pg_database WHERE datname = :'casdoor_test_db'
) \gexec

SELECT format('REVOKE ALL ON DATABASE %I FROM PUBLIC', :'casdoor_test_db') \gexec
SELECT format(
  'GRANT CONNECT, TEMPORARY ON DATABASE %I TO %I',
  :'casdoor_test_db',
  :'casdoor_test_user'
) \gexec
SQL
