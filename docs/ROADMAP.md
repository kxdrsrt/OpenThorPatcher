# Roadmap

| | Milestone | State |
|---|---|---|
| M0 | Plan | done |
| M1 | Core `otp-core`: patch list, download with resume + SHA-256, Ed25519, state | done |
| M2 | Apply engine: Thor merge into GRFs (plain and encrypted) through `rgrf-core`, check, atomic swap (old file replaced completely, optional `.bak`); loose-file Thor patches; `DATA.INI` writer | done, tested on synthetic GRFs and on the real 2.5 GB `data.grf` |
| M3 | CLI `otp`: `keygen`, `init`, `scan`, `publish` (`--dry-run`, `--json`), `patch`, `pack` | done, `tests/e2e.sh` |
| M3b | Classic Thor Patcher servers (`plist.txt`), removals (GRF entries, files, folders), cloud links, optional packs | done, `tests/e2e.sh` |
| M4 | Apps: **Patcher** (players) and **Patch Maker** (operators) in Tauri 2, themes, news and changelog | built for macOS and Windows x64 (cross-compiled); the Patcher was run on Windows 11 (x64 emulation) against an nginx container; UI logic also tested with a scripted DOM; **not yet clicked through by a person** |
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
* The Windows programs are **not code-signed** (SmartScreen warns), and the apps have only run in a virtual machine, never on
  a real player's PC (slow lines, antivirus, locked files) and not been clicked through by a person. This is why v0.1.0 is
  a **pre-release**; a stable v1.0 waits for both.
