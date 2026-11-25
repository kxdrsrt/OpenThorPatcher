# OpenThor Patcher

> _A free, fast and stable patcher for Ragnarok Online clients: the open successor of Thor Patcher and NeonCube._

MIT or Apache-2.0. Status: **working core, CLI, MCP server and both apps**; see [docs/ROADMAP.md](docs/ROADMAP.md).

Three programs and one library:

| | For | What it is |
|---|---|---|
| **OpenThor Patcher** (`apps/patcher`) | players | The window that keeps a client up to date: news, progress, *Play*, *Check files*, optional packs on/off |
| **OpenThor Patch Maker** (`apps/maker`) | operators | Point it at your *master client*; it works out what changed and writes the patch site |
| **`otp`** (`crates/otp-cli`) | operators, CI | The same from the command line (`otp publish`, `otp patch`, `--dry-run`, `--json`) |
| **`otp-mcp`** (`crates/otp-mcp`) | AI assistants | An [MCP](docs/MCP.md) server: set up, preview and publish a patch site by asking |
| `otp-core` | everyone | The library under all of them |

## How it works (short)

```
 operator                                              player
 ┌──────────────┐  otp publish    ┌───────────────┐   ┌──────────────┐
 │ master client│ ──────────────► │  patch site   │ ◄─│ OpenThor     │
 │ (you edit it)│  only the diff  │ patchlist.json│   │ Patcher      │
 └──────────────┘                 │ patches/*.thor│   └──────────────┘
                                  │ files/…       │   merges into the GRF,
                                  └───────────────┘   verifies, swaps atomically
```

1. You keep **one master client** up to date (change a map, add a texture, delete a file).
2. `otp publish` (or the Patch Maker) compares every entry of your big GRFs with what you published last time and writes
   **only the difference** as a numbered `.thor` patch. Small files (a 3 MB `odin.grf`, a dll) are synced whole by hash.
   Nobody names or numbers patches by hand.
3. The patch list (`patchlist.json`, signed with Ed25519) says what exists. Upload the **site folder** to any static
   host, or to a cloud drive ([hosting options](docs/HOSTING.md)).
4. The player's patcher downloads what it is missing (resumable, mirrors, SHA-256), merges patches **into the GRF**
   (also encrypted GRFs), deletes what you removed, writes `DATA.INI` (with optional packs the player can switch) and
   starts the game.

Changing **one map file** means one small `.thor` containing that file, not a 2.5 GB download. Measured on the real
2.5 GB client `data.grf` (202,355 entries): publishing the baseline hashes everything in 36 s, a publish with 4
changes writes a 166-byte patch, applying it takes 12 s of disk work (a new 2.5 GB GRF is written, checked, and
swapped in; the old file is **replaced completely**, a `.bak` only stays with `--keep-backup`; for a moment both exist, so a 2.5 GB GRF needs 2.5 GB free), and a start without changes takes 0.0 s.

## Why it is better

[docs/COMPARISON.md](docs/COMPARISON.md) has the full table against Thor Patcher, NeonCube and rpatchur. The short list:

- **Diff publishing:** a master-client workflow instead of hand-made, hand-numbered patches.
- **Removal everywhere:** entries inside GRFs (Thor delete directives), loose files and whole folders.
- **Integrity:** per-file SHA-256, signed patch list, a damaged download never replaces anything, a damaged GRF is
  repaired from a full copy, the merged GRF is checked before it is swapped in.
- **GRF 0x102–0x300 and GRF Editor encryption** (through [RustyGRF](https://github.com/kxdrsrt/RustyGRF) as a library
  linked into the program; players do **not** install anything).
- **Still `.thor`:** reads every Thor patch and the classic `plist.txt` server layout, so an existing patch server works
  unchanged (merge patches and loose-file patches).
- **Hosting freedom:** static host, S3/R2, GitHub Releases, or Google Drive / Dropbox / OneDrive share links.
- **Themes, news and changelog:** the player window is a themeable folder (default: a Ragnarok-style night-blue theme with the logo sticking out of the window); news and changelog are written in the Patch Maker and travel in the signed patch list.
- **Optional packs** (for example OdinGuard's competitive pack) with a per-player switch that survives patching.
- **Scriptable and AI-ready:** CLI with `--json`, MCP server.

## Quick start (operator)

```sh
cargo build --release                      # otp, otp-mcp, and the two apps
otp keygen --out keys                      # signing.seed (secret) + patcher.pub
otp init --project otp-project.json --server MyRO --client /path/to/master-client \
         --site /path/to/site --base-url https://patch.example.com/ --seed keys/signing.seed
otp publish --project otp-project.json --dry-run    # what would change
otp publish --project otp-project.json              # write the site, upload it
```

Players get `patcher.json` next to the patcher (the Patch Maker writes it):

```json
{ "name": "MyRO", "patch_url": "https://patch.example.com/", "pub_key": "<contents of patcher.pub>" }
```

## Documents

| | |
|---|---|
| [docs/COMPARISON.md](docs/COMPARISON.md) | Thor Patcher, NeonCube, rpatchur and OpenThor Patcher side by side |
| [docs/DEPLOY.md](docs/DEPLOY.md) | Live-test setup, Docker (nginx site, `otp` image), checklist with the honest state |
| [docs/THEMES.md](docs/THEMES.md) | Themes (a folder, the Ragnarok-style default), news and changelog |
| [docs/HOSTING.md](docs/HOSTING.md) | Static hosts and cloud drives, what works, what does not |
| [docs/MCP.md](docs/MCP.md) | The MCP server: tools, safety, an example session |
| [docs/DESIGN.md](docs/DESIGN.md) | Architecture and decisions |
| [docs/FORMATS.md](docs/FORMATS.md) | `.thor`, `patchlist.json`, `plist.txt`, `DATA.INI`, signatures |
| [docs/ODINGUARD.md](docs/ODINGUARD.md) | Using it with OdinGuard Anti Cheat |
| [docs/ROADMAP.md](docs/ROADMAP.md) | What is done, what is next |

## Not goals

No anti-cheat of its own ([OdinGuard](https://github.com/kxdrsrt/OdinGuard) does that), no account system, no
payments. Windows 7 is not supported (the window uses WebView2); the CLI and the core have no such limit.
