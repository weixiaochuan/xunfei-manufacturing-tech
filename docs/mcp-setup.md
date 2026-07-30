# MCP Setup

Pomegranate can expose the local knowledge base as an MCP server named
`knowledge-base`. External MCP clients can use it to search notes, read notes,
inspect folders and tags, and, when explicitly enabled, write back changes.

## Build The Sidecar

Build the MCP sidecar before configuring an external client:

```powershell
pnpm build:mcp
```

The desktop settings page shows the exact sidecar path and database path for the
current machine.

## Read-Only Mode

Use read-only mode when the client should only inspect your knowledge base:

```json
{
  "mcpServers": {
    "knowledge-base": {
      "command": "<path-to-kb-mcp>",
      "args": ["--db-path", "<path-to-app-db>"]
    }
  }
}
```

## Writable Mode

Writable mode allows the client to create and update notes, folders, tags, and
tasks. Enable it only for clients you trust:

```json
{
  "mcpServers": {
    "knowledge-base": {
      "command": "<path-to-kb-mcp>",
      "args": ["--db-path", "<path-to-app-db>", "--writable"]
    }
  }
}
```

## Client Locations

Typical configuration files:

- Claude Desktop: `%APPDATA%\Claude\claude_desktop_config.json`
- Claude Code CLI: `%USERPROFILE%\.claude.json`

The Pomegranate settings page can generate the final JSON snippets with the
correct local paths.

## Notes

- Restart the external MCP client after changing its configuration.
- If the sidecar path is missing, run `pnpm build:mcp` again.
- Keep writable mode disabled unless you intentionally want AI-assisted edits.
