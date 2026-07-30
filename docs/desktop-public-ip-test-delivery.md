# Pomegranate Public IP TEST Delivery

This package targets the shared test Account Server:

- Account Server: `http://82.157.119.201:8080`
- Casdoor: `http://82.157.119.201:18000`
- Desktop profile: `public-ip-test`

Build the installer from the repository root:

```powershell
powershell -ExecutionPolicy Bypass -File D:\ag\scripts\package-desktop-public-ip-test.ps1
```

The installer is written to:

```text
D:\ag\src-tauri\target\release\bundle\nsis
```

Before sharing an installer, confirm the Account Server is healthy:

```powershell
Invoke-WebRequest -UseBasicParsing http://82.157.119.201:8080/health/ready
```

Expected response:

```json
{"status":"ok","service":"pomegranate-account-server","database":"ready"}
```

Feature ownership for later changes:

- Frontend pages and workflows: `src/pages`, `src/components`, `src/lib`
- Desktop commands, local storage, account isolation bridge: `src-tauri/src`
- Account Server, registration, files, cloud document/project APIs: `services/account-server`
- Bundled PPT generation resources: `src-tauri/resources/ppt-master`
- Bundled learning assistant resources: `src-tauri/resources/learning-assistant`
- Plugin package/verification path: `plugins`, `dev-plugins`, `scripts/plugin-package.mjs`

Keep account-scoped features behind the existing Rust account bridge. Frontend code should call Tauri commands instead of fetching the Account Server directly, so tokens stay out of the web layer and each user's cloud documents/projects remain isolated by the server-side session.
