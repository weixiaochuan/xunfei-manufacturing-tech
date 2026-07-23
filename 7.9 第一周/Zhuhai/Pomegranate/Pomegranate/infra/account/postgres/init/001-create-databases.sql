\set ON_ERROR_STOP on

-- The official PostgreSQL image runs files in /docker-entrypoint-initdb.d only
-- when it initializes a new data directory. Database names are intentionally
-- fixed because they are part of the local account-infrastructure contract.
SELECT 'CREATE DATABASE casdoor'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'casdoor')\gexec

SELECT 'CREATE DATABASE pomegranate_account'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'pomegranate_account')\gexec

COMMENT ON DATABASE casdoor IS 'Casdoor identity and authentication data';
COMMENT ON DATABASE pomegranate_account IS 'Future Pomegranate Account Server data';
