# Formats

## 1. The patch container: keep `.thor`

[Entscheidung] OpenThor Patcher uses **Thor patches (`.thor`)** as the only patch container. No new container.

Why not a new extension (`.othor`, `.patch`, `.gpf`)?

* Every tool that matters already writes `.thor`: GRF Editor, ThorGenerator, `rgrf thor create`, rpatchur's `mkpatch`.
  A new container would be read by nobody but us.
* A `.thor` already carries what we need: files and folders for a GRF (or loose files), per-file removal, zlib. What
  it lacks (signature, hash, order, target) belongs into the **patch list**, not into the container.
* `.gpf` and `.rgz` are older formats: read-only support through RustyGRF, so old patch sets still work.

If binary deltas for very large single files are ever needed, that would be a *separate* file kind listed in the
patch list (`kind = "delta"`), never a change of `.thor`.

## 2. The patch list: `patchlist.json` (+ `patchlist.json.sig`)

```json
{
  "schema": 1,
  "server": "BurningRO",
  "updated": "2026-11-25T03:00:00Z",
  "allow_patching": true,
  "message": "",
  "notice": "Welcome back!",
  "news_url": "https://example.com/news",
  "base_url": ["https://patch.example.com/", "https://mirror.example.net/patch/"],
  "files": [
    { "path": "odin.grf", "size": 3057437, "sha256": "…", "url": "files/odin.grf" }
  ],
  "targets": {
    "data.grf": { "baseline_sha256": "…", "full": { "path": "data.grf", "size": 0, "sha256": "…", "url": "files/data.grf" }, "full_id": 7 }
  },
  "patches": [
    { "id": 1, "target": "data.grf", "file": "patches/0001-data.thor", "size": 1234567, "sha256": "…" }
  ],
  "remove": ["old.dll", "oldfolder/*"],
  "client": { "exe": "alpha.exe", "args": "-1sak1", "setup_exe": "Setup.exe" },
  "data_ini": {
    "order": ["odin.grf", "@pack:competitive", "cdata.grf", "core.grf", "data.grf"],
    "packs": [{ "id": "competitive", "file": "odin-dark.grf", "title": "Competitive visuals", "default": true, "locked": false }]
  }
}
```

*Implemented (schema 1).* `files` are kept equal to the master by hash (small GRFs, dll, exe). `targets` are big GRFs
patched entry by entry: `baseline_sha256` is the version patch 0 starts from (what a fresh client package contains),
`full` is an optional whole copy for repair and `full_id` the patch id it corresponds to. `patches` are applied in id
order; ids are assigned by `otp publish`, nobody names or numbers them. A relative `url`/`file` is appended to the first
mirror in `base_url` that has it; a full link (`https://…`, `gdrive:ID`, a share link) is used as it is, see
[HOSTING.md](HOSTING.md). `remove` paths (relative, no `..`; `dir/*` = folder) are deleted from every client; entries
inside GRFs are removed by Thor delete directives. `client` says what the Play and Setup buttons start. `data_ini`
gives the GRF load order; `@pack:<id>` is replaced by the pack's file only when the player switched the pack on (or
`locked`), and GRFs the player added stay after the managed ones. `notice`/`news_url` fill the patcher's news area.
Unknown fields are ignored, so lists can grow without a new schema number.

Not in schema 1 yet: an `odinguard` hook (manifest check after patching).

* The file is signed with Ed25519 (`patchlist.json.sig`); the public key ships with the patcher. An unsigned or
  wrongly signed list is refused unless the operator switched signatures off in `config.toml` (development only).
* `plist.txt` (Thor Patcher: `id name.thor`, `//` comments) is still read when there is no `patchlist.json`; hashes
  and signatures do not exist there, so those downloads cannot be verified (the patcher says so in the log).

## 3. `DATA.INI`

The patcher owns the `[Data]` list. The operator's `data_ini.order` gives the order; `@pack:<id>` is replaced by the
pack's GRF **only if** the player has it switched on (or the server locks it). Players' own extra GRFs (skins,
translations) are kept in their place after the managed ones, unless OdinGuard's manifest says they are not allowed.

## 4. Config (`config.toml`)

```toml
[server]
name = "BurningRO"
patchlist = "https://patch.example.com/patchlist.json"
public_key = "patcher.pub"
[client]
exe = "alpha.exe"
default_grf = "data.grf"
[window]
theme = "web/"          # replaceable HTML/CSS
language = "auto"
[behaviour]
finish_on_connection_failure = false
keep_backup = true
```

## 5. State file

`patcher.state` (JSON, next to the executable): last applied id per target, hashes of the GRFs after the last
successful patch, the player's pack choices. Deleting it forces a full check against the patch list.
