# Account TEST quick start

## Prerequisites

- Node 22.
- `pnpm.cmd install` has been run at the project root.
- Native PostgreSQL tools are available at `E:\ag-tools\pgsql` or another path passed as `-PostgresRoot`.
- You have created the three runtime secret files documented in `docs/account-test-runtime.md`.

## Start

```powershell
$ProjectRoot = "D:\ag"
$RuntimeRoot = "D:\pomegranate-local-test"
$PostgresRoot = "E:\ag-tools\pgsql"

& "$ProjectRoot\scripts\account-test\start-all.ps1" `
  -ProjectRoot $ProjectRoot `
  -RuntimeRoot $RuntimeRoot `
  -PostgresRoot $PostgresRoot `
  -StartDesktop
```

To start in steps:

```powershell
& "$ProjectRoot\scripts\account-test\start-postgres.ps1" -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
& "$ProjectRoot\scripts\account-test\start-account-server.ps1" -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
& "$ProjectRoot\scripts\account-test\check-runtime.ps1" -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
& "$ProjectRoot\scripts\account-test\start-desktop.ps1" -ProjectRoot $ProjectRoot -RuntimeRoot $RuntimeRoot -PostgresRoot $PostgresRoot
```

## Stop

```powershell
& "$ProjectRoot\scripts\account-test\stop-all.ps1" `
  -ProjectRoot $ProjectRoot `
  -RuntimeRoot $RuntimeRoot `
  -PostgresRoot $PostgresRoot
```

## Acceptance checks

- `GET http://127.0.0.1:18080/health/live` returns 200.
- `GET http://127.0.0.1:18080/health/ready` returns 200.
- `GET http://127.0.0.1:18080/auth/login?client=desktop` returns 302 to Casdoor TEST.
- `test001` can log in and return to the desktop.
- The desktop header shows the corresponding platform account.
- Restarting the desktop can restore the session.
- Logout clears the session.
- `test002` can log in.
- `test002` cannot see `test001` files, documents, or learning projects.
- Cross-account resource access returns 404 or an equivalent safe error.
- Upload, download, and delete work inside the current account.

## Common failures

- `3010` and `18080` are mixed up.
- Redirect URI is not exactly `http://127.0.0.1:18080/auth/callback`.
- `invalid_state` means the callback state no longer matches the login attempt.
- `organization_forbidden` usually means the user or app is not in `pomegranate-test`.
- `invalid_client_id` means the runtime client id does not match `app-pomegranate-test`.
- `oidc_unavailable` means Casdoor discovery or JWKS cannot be reached.
- PostgreSQL is not listening on `127.0.0.1:55432`.
- The database data directory was created by a newer PostgreSQL version than the tool you are starting.
