# Formats

## 1. The patch container: keep `.thor`

[Entscheidung] OpenThorPatcher uses **Thor patches (`.thor`)** as the only patch container. No new container.

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
  "base_url": ["https://patch.example.com/files/", "https://mirror.example.net/files/"],
  "patches": [
    {
      "id": 1,
      "file": "0001-data.thor",
      "kind": "thor",
      "size": 1234567,
      "sha256": "…",
      "target": "data.grf"
    },
    { "id": 2, "file": "0002-odin.thor", "kind": "thor", "size": 3057437, "sha256": "…", "target": "odin.grf" }
  ],
  "client": {
    "exe": "alpha.exe", "args": "-1sak1",
    "files": [{ "path": "odinguard.dll", "url": "client/odinguard.dll", "sha256": "…" }]
  },
  "data_ini": {
    "order": ["odin.grf", "@pack:competitive", "cdata.grf", "core.grf", "cpal.grf", "data.grf"],
    "packs": [
      { "id": "competitive", "file": "odin-dark.grf", "title": "Competitive visuals", "default": true, "locked_by_server": false }
    ]
  },
  "odinguard": { "manifest": "odinguard/manifest.json" }
}
```

* `target` is the GRF the Thor patch is merged into (a patch's own target field is only a default).
* The file is signed with Ed25519 (`patchlist.json.sig`); the public key ships with the patcher. An unsigned or
  wrongly signed list is refused unless the operator switched signatures off in `config.toml` (development only).
* `plist.txt` (Thor Patcher: `id name.thor`, `//` comments) is still read when there is no `patchlist.json`; hashes
  and signatures do not exist there, so the patcher warns once.

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
