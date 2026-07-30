# Account TEST runtime

This repository contains the non-secret files needed to reproduce the Pomegranate account TEST chain on a clean Windows development machine.

## Target chain

```text
Pomegranate desktop
-> local Account Server http://127.0.0.1:18080
-> remote Casdoor TEST http://82.157.119.201:18000
-> Account Server /auth/callback
-> pomegranate://auth/callback?ticket=...
-> Tauri exchanges the ticket for a Pomegranate session
-> Windows Credential Manager stores the session token
-> PostgreSQL stores only the SHA-256 token hash
-> React header shows account information
```

The desktop and React code must not receive the Casdoor client secret, database credentials, Authorization headers, or raw session tokens.

## Directories

Recommended local layout:

```text
D:\ag                    project root
D:\pomegranate-local-test  runtime root, outside the repository
E:\ag-tools\pgsql
```

Runtime data belongs outside the repository:

```text
D:\pomegranate-local-test\postgres-data
D:\pomegranate-local-test\logs
D:\pomegranate-local-test\user-files
D:\pomegranate-local-test\desktop-data
D:\pomegranate-local-test\postgres-password.tmp
D:\pomegranate-local-test\casdoor-client-id.tmp
D:\pomegranate-local-test\casdoor-client-secret.tmp
```

Do not commit, zip, screenshot, or paste the runtime directory or any `.tmp` secret file.

## Fixed TEST values

```text
Casdoor TEST:        http://82.157.119.201:18000
Organization:        pomegranate-test
Application:         app-pomegranate-test
Account Server:      http://127.0.0.1:18080
PostgreSQL:          127.0.0.1:55432
OIDC Redirect URI:   http://127.0.0.1:18080/auth/callback
Deep Link:           pomegranate://auth/callback
```

Do not use production organization/application values, `127.0.0.1:3010`, public Account Server `:8080`, Casdoor `:8000`, or PostgreSQL `:5432` for this TEST chain.

## Local secrets

Create these files manually under the runtime root:

```text
postgres-password.tmp
casdoor-client-id.tmp
casdoor-client-secret.tmp
```

`postgres-password.tmp` is a local TEST password you generate. The Casdoor client id and secret must be copied from the Casdoor TEST application `app-pomegranate-test`. They are required from you or an authorized Casdoor administrator; Codex cannot invent or safely supply them.

## Casdoor TEST checks

Casdoor TEST must have:

```text
Organization: pomegranate-test
Application: app-pomegranate-test
Redirect URI: http://127.0.0.1:18080/auth/callback
Grant types: Authorization Code, Refresh Token
Token format: JWT-Custom
Signing method: RS256
Token fields: Owner, Name, DisplayName, Email
```

TEST users such as `test001` and `test002` must belong to `pomegranate-test`.
