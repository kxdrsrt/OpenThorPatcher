# OpenThor Patcher compared

What the older patchers do, and what OpenThor Patcher does. How sure each column is: **NeonCube** from its
`ChangeLog.txt` and `TODO.txt` (read); **rpatchur** from its README (read in an earlier session); **Thor Patcher** from
its known behaviour and the `.thor`/`plist.txt` formats (*not re-verified*, treat the cells as likely, not proven).
"geplant" = not there yet.

| | Thor Patcher (2.6.x) | NeonCube (1.x) | rpatchur | **OpenThor Patcher** |
|---|---|---|---|---|
| Source | closed, abandoned | open (GPL-2.0), 2005–2006 | open (MIT) | open (MIT / Apache-2.0) |
| Patch container | `.thor` | RAR/GRF/folder archives | `.thor` | **`.thor`** (reads every existing one) |
| Patch list | `plist.txt` | its own ini | `plist.txt` | **signed `patchlist.json`**, and `plist.txt` still works |
| Operator workflow | make each patch by hand, number it, edit `plist.txt` | same | same | **master client → `otp publish` → only the diff, numbered automatically** |
| Diff of a big GRF | no (you build the patch) | no | no | **yes, per entry (hash state per target)** |
| GRF versions | 0x102, 0x103, 0x200 | libgrf-based | 0x200 only (README) | **0x102 – 0x300** |
| GRF Editor encryption | no | no | no | **yes** |
| Merge into GRF | in place (append) | repack | in place | **new file → check → atomic swap, `.bak`** |
| Interrupted patch | can leave a broken GRF | can | can | **never touches the GRF until the result checked out** |
| Delete a file inside a GRF | yes (Thor flag) | yes | yes | **yes** |
| Delete loose files / folders | no | folder (`path\*`) | no | **yes (`remove` list, `dir/*`)** |
| Loose-file patches | yes | yes (FLD mode) | partly | **yes** |
| Download integrity | none | size check | none | **SHA-256 per file, size, signature on the list** |
| Resume | no | planned | yes | **yes** |
| Mirrors | one URL | one URL | one URL | **several, first working wins** |
| Cloud share links | no | no | no | **Google Drive, Dropbox, OneDrive, GitHub Releases, any static host** |
| Repair damaged GRF | reinstall | reinstall | reinstall | **restore from a full copy (`--full`)** |
| Optional packs (on/off per player) | no | no | no | **yes, written into `DATA.INI`** |
| `DATA.INI` handling | manual | manual | manual | **managed order, the player's own GRFs stay** |
| Maintenance switch | no | no | no | **yes (`allow_patching` + message)** |
| Operator UI | none | none | none | **Patch Maker (Tauri)** |
| Player UI | skinned HTML | skinned | HTML | **Patcher window with news page** |
| Scripting | no | no | no | **`otp` with `--json`, MCP server** |
| Platforms | Windows | Windows | Win/Linux/Mac | **Windows, Linux, macOS (WebView2 needed on Windows, no Windows 7)** |
| Self-update | yes | no | yes | geplant |
| Binary delta | no | geplant | no | geplant |
| zip / 7z / rar patches | no | rar (3rd party lib) | no | geplant |
| Parallel downloads | no | "NO :P" | no | geplant |

Corrections welcome: if a cell is wrong, open an issue.

## What was deliberately not copied

* **No scripting language or plug-ins in the patcher.** Operators get a signed list and a theme, nothing that runs
  code on a player's machine.
* **No in-place append to a GRF.** It is faster for huge GRFs but a power cut leaves a client that cannot start.
* **No unsigned patch lists** when a public key is configured.
