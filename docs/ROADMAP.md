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
| M6 | OdinGuard integration: `og grf-scan` hook in the Maker (`odinguard` in the project, before the patch list is written), signed `grf-manifest.json` in the site, check of the GRFs against it after patching | done, `tests/e2e.sh` (stand-in `og` and, with `OG=…`, the real one) |
| M7 | Self-update (signed) | **done (0.2.0)**: `patcher` in the list, `otp self-update`, loop guard; `tests/e2e.sh` and on Windows 11 in the VM (0.1.0 → 0.1.1 replaced itself once, restarted, patched; a wrong build stops with a warning) |
| M7b | Installers, languages (themes are done in M4) | planned |
| M8 | Parallel downloads, bandwidth limit | **done (0.2.0)**: `parallel` (default 4), `max_kbps`; `tests/e2e.sh` (12 files with 6 at a time; 768 KiB at 256 KiB/s took ≥ 2 s), VM: 8 files in parallel equal to the master |
| M8b | MEGA file links | **done (0.2.0)**: API + AES-128-CTR + resume; e2e with a stand-in, and a real public MEGA file downloaded and resumed byte-identically; `otp fetch` to check any link |
| M8c | Binary deltas for single huge non-GRF files, zip/7z/rar input | planned |
| M9 | MidgardCP: `news_from` (news from `GET /api/v1/news`, HTML → patcher markup incl. headings, tables, relative links; the last news stay when the panel is down), `status_url` + `status_realm` (players online in the patcher), `otp news`, Maker fields, MCP. The signing key stays with the Patch Maker, never on the panel | **done (0.2.0)**: e2e, and against a running MidgardCP (dev): its two news articles and "51 online" shown in the patcher in the VM |
| M9b | Launch tokens (password-less start after a web login) | later, needs a login-server change and its own security design |

## Known limits

* A patched GRF is **rewritten** (new file, check, swap). That is safe, and takes the time of copying the file
  (12 s for 2.5 GB on an SSD); Thor Patcher appended in place, which is faster but leaves a broken GRF if it is
  interrupted.
* Cloud drives (Google Drive, Dropbox, OneDrive) work through direct-download links but have quotas and can answer with a
  web page; they are untested with live links (MEGA was tested live). Check each link with `otp fetch`. See [HOSTING.md](HOSTING.md).
* No installer yet: ship the patcher in your client package; from then on it updates itself.
* The Windows programs are **not code-signed** (SmartScreen warns), and the apps have only run in a virtual machine, never on
  a real player's PC (slow lines, antivirus, locked files) and not been clicked through by a person. This is why 0.x is
  a **pre-release**; a stable v1.0 waits for both.
