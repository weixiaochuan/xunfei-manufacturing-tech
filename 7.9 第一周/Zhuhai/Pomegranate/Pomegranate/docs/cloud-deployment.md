# Pomegranate cloud server deployment

This package starts a new, empty Linux cloud environment for Pomegranate account,
document, and uploaded-file services. It does **not** migrate the current PostgreSQL
database, Casdoor records, user files, credentials, or backups.

## Scope and security boundary

The deployment contains PostgreSQL 17.6, Casdoor 3.119.0, Account Server, a one-shot
Account Server migration job, and Caddy 2.10.0. Caddy is the only public application
entry point. PostgreSQL, Casdoor, and Account Server have no host port mapping.

Public firewall or security-group rules:

| Port | Protocol | Purpose |
| --- | --- | --- |
| 22 | TCP | SSH administration; restrict source IPs and use keys |
| 80 | TCP | ACME validation and automatic redirect to HTTPS |
| 443 | TCP/UDP | HTTPS and HTTP/3 |

Do not expose 3010, 8000, or 5432. Do not mount the Docker socket into any service.
The user-file directory is a private Account Server bind mount, not a Caddy static
directory. Clients never receive the PostgreSQL credentials or Casdoor Client Secret.

## Server requirements

Recommended starting point for a small pre-release environment:

- Ubuntu 24.04 LTS, x86_64, 2 vCPU, 4 GiB RAM, and at least 40 GiB SSD.
- A static public IP.
- Docker Engine with the Compose v2 plugin.
- Git, Bash, curl, and OpenSSL.
- Two public DNS A/AAAA records:
  - `auth.<domain>` points to the server and serves Casdoor.
  - `api.<domain>` points to the server and serves Account Server.

DNS must resolve before Caddy can obtain trusted certificates. If an AAAA record is
present, IPv6 must actually reach the server. Production sizing and monitoring must
be reviewed by the development owner before launch.

## Directory layout

The scripts use:

```text
/srv/pomegranate/
|-- deploy/                 checked-out deployment code
|-- data/
|   `-- user-files/         persistent private uploaded-file content
|-- backups/                reserved; backup implementation is not in this stage
`-- logs/                   reserved for operator-managed diagnostics
```

PostgreSQL and Caddy certificate data use Docker named volumes. The
`docker-entrypoint-initdb.d` database bootstrap runs **only when the PostgreSQL
volume is empty on its first initialization**. Container restarts do not rerun it.
Never edit an initialized database by changing bootstrap environment variables.

On the server, run:

```bash
sudo ./scripts/cloud/prepare-server.sh
```

This script checks Ubuntu and Docker, creates the directories without `chmod 777`,
and makes `user-files` writable by the non-root Account Server runtime UID/GID.

## Obtain the deployment code

The server operator may clone the approved repository branch into
`/srv/pomegranate/deploy`, or receive an archive built from an approved commit.
The development owner must provide the exact commit ID. Do not deploy from a dirty
working tree.

All commands below run from the Pomegranate project directory containing
`compose.cloud.yml`.

## Create the private environment file

Copy the safe template and restrict it:

```bash
cp .env.cloud.example .env.cloud
chmod 600 .env.cloud
```

Generate a different random value for every password or secret. Hex values avoid
shell parsing surprises:

```bash
openssl rand -hex 32
```

Replace all `CHANGE_ME`, `REPLACE_WITH`, and `example.com` values. Never reuse local
development passwords. Never send `.env.cloud`, database passwords, Casdoor
administrator credentials, the Client Secret, SSH private keys, or tokens in chat.

Public configuration includes the two domain names, public HTTPS URLs, organization
name, application name, port numbers, and non-secret image tag. Server-only secrets
include:

- `POSTGRES_ADMIN_PASSWORD`
- `CASDOOR_DB_PASSWORD`
- `ACCOUNT_DB_PASSWORD`
- `CASDOOR_CLIENT_SECRET`

`CASDOOR_CLIENT_ID` identifies the application but still belongs in the server
environment for this deployment. Keep `.env.cloud` on the server only; Git ignores
it. The committed `.env.cloud.example` contains placeholders only.

The deployed Casdoor application must use:

```text
Redirect URI: https://api.<domain>/auth/callback
Issuer/public URL: https://auth.<domain>
```

Creating or restoring the Casdoor organization, application, administrator, and
Client Secret requires development-owner confirmation. Do not enable email, SMS,
phone-number, or third-party sign-in providers as part of this package.

## Validate before starting

```bash
./scripts/cloud/validate-env.sh
```

It rejects missing variables, placeholders, local/LAN URLs, weak-length secret
placeholders, mismatched HTTPS URLs, Windows paths, shared database roles, and
unexpected publication of ports 3010/8000/5432. It does not print secret values.

The two application databases are created only in a new PostgreSQL volume:

- `casdoor`, owned by the dedicated Casdoor database role.
- `pomegranate_account`, owned by the dedicated Account Server database role.

The bootstrap does not create application tables. Casdoor owns its schema.
Account Server tables are created by the repository's checksummed migrations.

## Start a new empty environment

The supported orchestration command is:

```bash
./scripts/cloud/start-services.sh
```

It performs this sequence:

1. Validate `.env.cloud` and statically render Compose.
2. Build Account Server from the pinned lockfile.
3. Start PostgreSQL and wait for `healthy`.
4. Start Casdoor.
5. Run the real, repeatable `node dist/scripts/migrate.js` entry as the one-shot
   `account-migrate` service.
6. Start Account Server and wait for its `/health/ready` container check.
7. Start Caddy.
8. Verify public OIDC discovery and Account Server live/ready endpoints over HTTPS.

Migration checksums and the existing skip behavior remain active. If migration
fails, Compose does not start Account Server through this dependency chain. The
service itself does not blindly rebuild tables on each start.

This stage intentionally starts an **empty** environment. It must not be pointed at
the current production or local data directories.

## Verify and troubleshoot

Run the supported health check:

```bash
./scripts/cloud/health-check.sh
```

Expected public checks:

```text
https://auth.<domain>/.well-known/openid-configuration
https://api.<domain>/health/live
https://api.<domain>/health/ready
```

Useful read-only diagnostics:

```bash
docker compose --env-file .env.cloud -f compose.cloud.yml ps
docker compose --env-file .env.cloud -f compose.cloud.yml logs --tail=200 postgres
docker compose --env-file .env.cloud -f compose.cloud.yml logs --tail=200 casdoor
docker compose --env-file .env.cloud -f compose.cloud.yml logs --tail=200 account-migrate
docker compose --env-file .env.cloud -f compose.cloud.yml logs --tail=200 account-server
docker compose --env-file .env.cloud -f compose.cloud.yml logs --tail=200 caddy
```

Review logs before sharing them. They must not contain tokens, passwords, complete
database URLs, user document bodies, or uploaded-file content.

Typical failure boundaries:

- Certificate failure: verify DNS, ports 80/443, IPv6 reachability, and Caddy logs.
- PostgreSQL unhealthy: check disk space, volume ownership, and PostgreSQL logs.
- Migration failure: stop and ask the development owner; do not paste or edit SQL.
- Account Server unready: check migration status and safe error codes in its logs.
- Upload failure: verify `user-files` ownership matches `POMEGRANATE_UID/GID` and
  keep `USER_FILE_MAX_BYTES` aligned with Caddy's finite request-body limit.

## Stop, restart, update, and rollback

Safely stop without deleting data:

```bash
./scripts/cloud/stop-services.sh
```

Restart using the normal start script. Never run:

```text
docker compose down -v
```

That command deletes named volumes and can destroy PostgreSQL and certificate data.
The scripts never delete volumes or user files.

For an update, the development owner must provide an approved commit and image tag.
Before deploying it:

1. Confirm the database/file backup and restore plan for that release.
2. Read new migration notes.
3. Pull or check out the approved clean commit.
4. Update `ACCOUNT_SERVER_IMAGE_TAG`.
5. Run validation, build, migration, start, and health checks.

Rollback means returning to an approved application image/commit only when its
database compatibility has been confirmed. Never roll back database schema by
deleting a volume, editing `schema_migrations`, or manually copying old SQL.

## Responsibilities and unfinished work

The server operator may prepare directories, fill the private environment file,
configure DNS/firewall, run the committed scripts, inspect sanitized logs, and stop
services safely.

The development owner must confirm Casdoor production configuration and rotated
credentials, the exact deployment commit, schema compatibility, client public URLs,
and any migration/rollback decision.

Not implemented in this stage:

- Migration of the real PostgreSQL/Casdoor data.
- Migration of real `user-files`.
- PostgreSQL and file backup/restore automation.
- Cloud TEST desktop-client configuration or acceptance.
- Monitoring, alerting, and formal disaster recovery.

Do not describe those items as complete. They are deliberate next-stage work.
