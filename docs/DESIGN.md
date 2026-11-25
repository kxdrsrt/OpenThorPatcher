# Design

Labels: **[gelesen]** read in sources or docs, **[gemessen]** measured on files, **[Entscheidung]** decided here,
**[offen]** to be measured or decided later.

## 1. Why not rpatchur, why not Thor Patcher

| | Thor Patcher 2.6.4.13 | rpatchur (L1nkZ) | OpenThor Patcher |
|---|---|---|---|
| State | abandoned, closed source (Delphi), last release 2016 **[gemessen]** (Changelog) | Rust, Apache-2.0/MIT, last push 2024-05, 57 stars **[gelesen]** (GitHub) | new |
| Patch formats | `.thor` (+ GRF 0x102/0x103/0x200 input) | `.thor`, GRF 0x101-0x200 **[gelesen]** | `.thor`, plus GPF/RGZ read via RustyGRF |
| Writes | GRF 0x200 | **only 0x200**, cannot patch GRFs with several entries on one offset, no encrypted GRF **[gelesen]** (README "Known limitations") | 0x200 and 0x300, encrypted GRF, shared blocks (RustyGRF handles them) |
| Self-update | no | **no** ("cannot automatically update the patcher executable") **[gelesen]** | signed self-update |
| UI | skinnable web page | web-view, YAML config | web UI (HTML/CSS) with the same theme idea |
| Safety | "YOU are responsible for data integrity" (changelog 2.6.4.6) **[gelesen]** | n/a | atomic swap + verify + backup |

[Entscheidung] Do not fork rpatchur. Its core idea (web UI, YAML config, Thor compatibility, SSO launcher) is what we
want, but the two hard parts, archive handling and trust (signatures, verified swap), are exactly what RustyGRF
already does better, and a patcher must never corrupt a 2.5 GB `data.grf`. We keep rpatchur as a reference for the
config and UI conventions and for `mkpatch`-style tooling; its code is Apache-2.0/MIT, so ideas and small parts can be
reused with attribution.

## 2. Building blocks

* **`rgrf-core`** from RustyGRF, as a **vendored copy** in `vendor/rgrf-core` (revision in `VENDORED.md`, refreshed with
  `tools/vendor-rgrf-core.sh`): open/merge/patch/verify/encrypt GRF 0x102-0x300, GPF, GSF, Thor, RGZ, repack, hash
  lists. It is **compiled into** the programs, and this repository builds and tests without RustyGRF (no git
  dependency, no `rgrf` program; `tests/e2e.sh` uses the small `otp-testkit` of this workspace). Reusing its source avoids
  a second, worse GRF implementation; fixes are made in RustyGRF and copied here.
* **Tauri 2** (same stack as RustyGRF and the OdinGuard Studio) for the window; the page is plain HTML/CSS/JS that an
  operator can replace. **[offen]** WebView2 does not run on Windows 7/8.1; many Ragnarok players still use them. Two
  answers to measure: a Windows 7 capable build (older WebView runtime, or a tiny native UI), and a `--no-ui` mode that
  patches and starts the game with a console progress line.
* **reqwest/rustls** for HTTP(S), range requests for resuming, several mirrors.
* **Ed25519** (as OdinGuard uses) for the signed patch list and signed self-update.

## 3. What a run looks like

1. Read `config.toml` next to the executable (server URL, mirrors, client exe, default GRF, window, language).
2. Download `patchlist.json` (+ `.sig`), verify the signature with the public key compiled in or shipped as
   `patcher.pub`. Legacy: `plist.txt`.
3. Compare with the local state file (`server.dat`-like: last applied patch id per patch set).
4. Download missing patches (resumable, SHA-256 each), apply them in order. **Per target GRF** build the result in a
   temporary file, run `rgrf verify`-equivalent checks, then swap atomically; the old file is replaced completely (a `<name>.bak` only if the player or `--keep-backup` asks for it).
5. Write `DATA.INI` from the *managed list* plus the player's pack choices (see FORMATS.md).
6. Optional OdinGuard check (ODINGUARD.md), then start the client with its parameters.

## 4. Rules that keep it stable

* Never edit a GRF in place. Temp file in the same folder (same disk), verify, rename.
* Free disk space check before a patch (target size + temp).
* A running client holds its GRFs open: detect it and refuse to patch (clear message).
* A failed patch leaves the old files untouched and the state file unchanged; the next run starts again.
* Large GRFs are patched by streams: only changed entries are rewritten, unchanged streams are copied (RustyGRF
  `edit`/`merge` keep original streams, no recompression).
* One patch list entry = one patch; an interrupted download resumes from the bytes on disk.

## 5. Decisions still open

1. ~~Windows 7 support~~ decided: no (WebView2). The CLI and core have no such limit.
2. Binary delta for big non-GRF files (the game exe): whole-file replacement first, delta only if it is worth it.
3. ~~Who builds patches~~ decided: `otp publish` / the Patch Maker diff the master client against the last publish and
   write the `.thor` files themselves (through `rgrf-core`), see README.
