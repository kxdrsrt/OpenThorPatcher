# Roadmap

| | Milestone | State |
|---|---|---|
| M0 | Plan | done |
| M1 | Core `otp-core`: patch list, download with resume + SHA-256, Ed25519, state | done |
| M2 | Apply engine: Thor merge into GRFs (plain and encrypted) through `rgrf-core`, check, atomic swap, `.bak`; loose-file Thor patches; `DATA.INI` writer | done, tested on synthetic GRFs and on the real 2.5 GB `data.grf` |
| M3 | CLI `otp`: `keygen`, `init`, `scan`, `publish` (`--dry-run`, `--json`), `patch`, `pack` | done, `tests/e2e.sh` |
| M3b | Classic Thor Patcher servers (`plist.txt`), removals (GRF entries, files, folders), cloud links, optional packs | done, `tests/e2e.sh` |
| M4 | Apps: **Patcher** (players) and **Patch Maker** (operators) in Tauri 2 | built; UI logic tested with a scripted DOM; **not yet clicked through on a real desktop** |
| M5 | MCP server `otp-mcp` | done (smoke tested over stdio) |
| M6 | OdinGuard integration: `og grf-scan` hook in the Maker, manifest check after patching | planned |
| M7 | Self-update (signed), installers, themes (HTML/CSS skin), languages | planned |
| M8 | Parallel downloads, bandwidth limit, binary deltas for single huge non-GRF files, zip/7z/rar input | planned |

## Known limits

* A patched GRF is **rewritten** (new file, check, swap). That is safe, and takes the time of copying the file
  (12 s for 2.5 GB on an SSD); Thor Patcher appended in place, which is faster but leaves a broken GRF if it is
  interrupted.
* Cloud drives (Google Drive, Dropbox, OneDrive) work through direct-download links but have quotas and can answer with a
  web page; they are untested against the real services. See [HOSTING.md](HOSTING.md).
* Self-update and installers are not there yet; ship the program in your client package.
