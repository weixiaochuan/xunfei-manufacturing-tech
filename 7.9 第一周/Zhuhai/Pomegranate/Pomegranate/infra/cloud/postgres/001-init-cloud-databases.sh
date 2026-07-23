#!/bin/sh
set -eu

required_variables="
CASDOOR_DB_NAME
CASDOOR_DB_USER
CASDOOR_DB_PASSWORD
ACCOUNT_DB_NAME
ACCOUNT_DB_USER
ACCOUNT_DB_PASSWORD
"

for variable_name in $required_variables; do
  eval "variable_value=\${$variable_name:-}"
  if [ -z "$variable_value" ]; then
    echo "Missing required PostgreSQL initialization variable: $variable_name" >&2
    exit 1
  fi
done

case "$CASDOOR_DB_NAME:$CASDOOR_DB_USER:$ACCOUNT_DB_NAME:$ACCOUNT_DB_USER" in
  *[!A-Za-z0-9_:]*)
    echo "Database and role names may contain only ASCII letters, digits, and underscores" >&2
    exit 1
    ;;
esac

if [ "$CASDOOR_DB_NAME" = "$ACCOUNT_DB_NAME" ] || [ "$CASDOOR_DB_USER" = "$ACCOUNT_DB_USER" ]; then
  echo "Casdoor and Account Server must use separate databases and roles" >&2
  exit 1
fi

psql \
  --username "$POSTGRES_USER" \
  --dbname "$POSTGRES_DB" \
  --set=ON_ERROR_STOP=1 \
  --set=casdoor_db="$CASDOOR_DB_NAME" \
  --set=casdoor_user="$CASDOOR_DB_USER" \
  --set=casdoor_password="$CASDOOR_DB_PASSWORD" \
  --set=account_db="$ACCOUNT_DB_NAME" \
  --set=account_user="$ACCOUNT_DB_USER" \
  --set=account_password="$ACCOUNT_DB_PASSWORD" <<'SQL'
SELECT format('CREATE ROLE %I LOGIN PASSWORD %L', :'casdoor_user', :'casdoor_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'casdoor_user') \gexec

SELECT format('CREATE ROLE %I LOGIN PASSWORD %L', :'account_user', :'account_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'account_user') \gexec

SELECT format('CREATE DATABASE %I OWNER %I', :'casdoor_db', :'casdoor_user')
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = :'casdoor_db') \gexec

SELECT format('CREATE DATABASE %I OWNER %I', :'account_db', :'account_user')
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = :'account_db') \gexec

SELECT format('REVOKE ALL ON DATABASE %I FROM PUBLIC', :'casdoor_db') \gexec
SELECT format('REVOKE ALL ON DATABASE %I FROM PUBLIC', :'account_db') \gexec
SELECT format('GRANT CONNECT, TEMPORARY ON DATABASE %I TO %I', :'casdoor_db', :'casdoor_user') \gexec
SELECT format('GRANT CONNECT, TEMPORARY ON DATABASE %I TO %I', :'account_db', :'account_user') \gexec
SQL
