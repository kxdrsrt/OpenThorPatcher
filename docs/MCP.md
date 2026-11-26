# MCP server (`otp-mcp`)

[MCP](https://modelcontextprotocol.io) lets an AI assistant call tools. `otp-mcp` exposes the Patch Maker's functions so
you can set up and publish a patch site by asking ("add the new map to the patch and remove `old.dll`, show me what
would change, then publish").

```json
{ "mcpServers": { "openthor": { "command": "/path/to/otp-mcp" } } }
```

Transport: stdio, newline-delimited JSON-RPC 2.0, tools only. `OTP_MCP_READONLY=1` hides every tool that writes.

| Tool | Writes | What it does |
|---|---|---|
| `otp_scan_client` | – | Files of the master client with a suggestion (thor / sync / ignore) |
| `otp_project_get` | – | Reads `otp-project.json` |
| `otp_project_create` | yes | New project from a master client folder |
| `otp_project_update` | yes | Merges fields (mirrors, `remove`, `data_ini`, `url_map`, `game`, maintenance, `news_from`/`status_url` for MidgardCP, `patcher` self-update, `external`, `upload` …) |
| `otp_publish_plan` | – | Dry run: new patches with counts, files, removals |
| `otp_publish` | yes | Writes the site |
| `otp_site_files` | – | Files to upload with their cloud links |
| `otp_package` | yes | Client package ZIP for new players (managed files, `openthor.exe`, `patcher.json`) |
| `otp_upload` | yes | Uploads the site (list last), a package and/or the external files to the project's `upload` targets (rclone, GitHub releases) |
| `otp_patch_check` | – | What a client folder still needs |
| `otp_patch` | yes | Updates a client folder (`parallel`, `max_kbps`) |
| `otp_news_preview` | – | News a web panel (MidgardCP `/api/v1/news`) would put into the patch list |
| `otp_keygen` | yes | Key pair; refuses to overwrite |

**Safety.** Every writing tool needs `confirm: true`, which the assistant should only send after you agreed. Secrets
are never returned: the signing key is a path, GRF passwords are only accepted as arguments, never stored. The server
runs with your user's rights; point it only at folders you want it to touch.

OdinGuard has the same for its profiles and releases: `og-mcp` ([docs](https://github.com/kxdrsrt/OdinGuard)).
