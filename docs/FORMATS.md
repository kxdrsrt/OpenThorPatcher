# Formats

## 1. The patch container: keep `.thor`

[Entscheidung] OpenThor Patcher uses **Thor patches (`.thor`)** as the only patch container. No new container.

Why not a new extension (`.othor`, `.patch`, `.gpf`)?

* Every tool that matters already writes `.thor`: GRF Editor, ThorGenerator, `rgrf thor create` (RustyGRF), rpatchur's `mkpatch`.
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
  "serial": 12,
  "allow_patching": true,
  "message": "",
  "notice": "Welcome back!",
  "news_url": "https://example.com/news",
  "base_url": ["https://patch.example.com/", "https://mirror.example.net/patch/"],
  "sites": ["https://new-host.example.com/"],
  "files": [
    { "path": "odin.grf", "size": 3057437, "sha256": "…", "url": "files/odin.grf" }
  ],
  "targets": {
    "data.grf": { "baseline_sha256": "…", "full": { "path": "data.grf", "size": 0, "sha256": "…", "url": "files/data.grf" }, "full_id": 7 }
  },
  "patches": [
    { "id": 1, "target": "data.grf", "file": "patches/0001-data.thor", "size": 1234567, "sha256": "…" }
  ],
  "website": "https://burning.example",
  "links": [{ "title": "Discord", "url": "https://discord.gg/x" }],
  "servers": [{ "name": "Login", "host": "play.burning.example", "port": 6900 }, { "name": "Char", "host": "play.burning.example", "port": 6121 }, { "name": "Map", "host": "play.burning.example", "port": 5121 }],
  "remove": ["old.dll", "oldfolder/*"],
  "news": [{ "date": "2026-11-25", "title": "Welcome", "body": "The **test server** is open.\n\n- 75x base", "lang": "en" }],
  "changelog": [{ "date": "2026-11-25", "version": "Patch 12", "title": "LGP colours", "items": ["Magnus tiles are red"] }],
  "client": { "exe": "alpha.exe", "args": "-1sak1", "setup_exe": "Setup.exe", "max_instances": 1 },
  "data_ini": {
    "order": ["odin.grf", "@pack:competitive", "cdata.grf", "core.grf", "data.grf"],
    "packs": [{ "id": "competitive", "file": "odin-dark.grf", "title": "Competitive visuals", "default": true, "locked": false }]
  },
  "patcher": {
    "version": "0.2.0", "notes": "faster downloads",
    "files": { "windows-x64": { "path": "windows-x64/openthor.exe", "size": 14050304, "sha256": "…", "url": "patcher/windows-x64/openthor.exe" } }
  },
  "status_url": "https://cp.example/api/v1/status",
  "status_realm": "hr"
}
```

*Implemented (schema 1).* `serial` grows by one with every publish that changes something (not for a publish without changes); a patcher reads the list from all of its sources, takes the highest valid one and refuses a list lower than the last one it applied (see [HOSTING.md](HOSTING.md)). Lists without it count as 0. `files` are kept equal to the master by hash (small GRFs, dll, exe). `targets` are big GRFs
patched entry by entry: `baseline_sha256` is the version patch 0 starts from (what a fresh client package contains),
`full` is an optional whole copy for repair and `full_id` the patch id it corresponds to. `patches` are applied in id
order; ids are assigned by `otp publish`, nobody names or numbers them. A relative `url`/`file` is appended to the first
mirror in `base_url` that has it; a full link (`https://…`, `gdrive:ID`, a share link) is used as it is, see
[HOSTING.md](HOSTING.md). `remove` paths (relative, no `..`; `dir/*` = folder) are deleted from every client; entries
inside GRFs are removed by Thor delete directives. `client` says what the Play and Setup buttons start; `max_instances`
(0.4.0, 0 or missing = no limit) is how many copies of the game may run at the same time when started from the patcher (see
§6). `data_ini`
gives the GRF load order; `@pack:<id>` is replaced by the pack's file only when the player switched the pack on (or
`locked`), and GRFs the player added stay after the managed ones. `servers` are TCP-checked by the player app every 30 s (green/red dot, 2.5 s timeout); `website` and `links` become the Website pill and the icon buttons (icon chosen by host name; only http/https are opened). `notice`/`news_url`/`news`/`changelog` fill the patcher's news area (see [THEMES.md](THEMES.md)); a `lang` on a news or
changelog entry and `titles` on a pack (0.5.0) are explained there under *Languages*.
Unknown fields are ignored, so lists can grow without a new schema number.

`files[].mirrors` (0.3.0) are more full links to the same file, tried in order (external files such as `data.grf`,
written by the project's `external` map; those files are never copied into the site). `targets[].current_sha256` /
`current_id` (0.3.0) are the GRF's hash at the last publish and the patch id it corresponds to: a client package made with
`otp package` ships that version, so a fresh install starts there instead of at the baseline. `sync` entries ending in
`/*` (project file only) mean every file below that folder.

`patcher` (0.2.0) offers a newer patcher program per platform (`windows-x64`, `windows-arm64`, `macos-arm64`,
`macos-x64`, `linux-x64`): a patcher with a public key and an older version downloads it (SHA-256 checked), renames
itself to `<name>.old`, puts the new file in its place and restarts with `--updated-to <version>`; the new process deletes
`.old`. If the restarted program still reports an older version than announced (a wrong build on the site), it does not
update again and shows a warning instead of looping. `status_url`/`status_realm` (0.2.0) point at a web panel's JSON
status (MidgardCP `GET /api/v1/status`: `[{id, name, state, players}]`); the patcher shows the realm's players online next
to the server dots, every 30 s. Since 0.2.0 the patcher downloads up to 4 files at the same time (`parallel` in
`patcher.json` or the player's settings) and can limit the bandwidth (`max_kbps`, all downloads together); Thor patches
are still applied in id order after their downloads.

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

`_patch/master` (0.4.3) is written by `otp publish` into the master client. A patcher started in that folder does not
patch it (it reports the folder as the operator's master and only starts the game), so an unpublished change in the
master is never reverted to the published state. Delete the file to patch the folder like a player's.

## 6. Starting the game: launch ticket (0.4.0)

Play installs pending updates first, then starts `client.exe` with a **launch ticket**:

1. The patcher writes `_patch/launch-<token>.ticket` in the client folder (`<token>` = 32 hex characters, content
   `{"token":"<token>","time":<unix seconds>}`) and removes tickets older than 300 s.
2. It starts the game with the environment variable `OPENTHOR_LAUNCH=<token>`. Windows does not pass environment
   variables through the UAC prompt, so when the game needs administrator rights (most RO clients ask for them in their
   manifest) the patcher starts an elevated copy of itself (`openthor.exe --otp-elevated-start <dir> <exe> <token> [args]`,
   one UAC prompt as before), which starts the game with the variable and exits. Since 0.4.2; 0.4.0/0.4.1 lost the
   variable on that path.
3. A guard inside the game may require both: the variable is set, the file `_patch/launch-<token>.ticket` exists and is at
   most 300 s old. It should delete the ticket and clear the variable so the token is used once. OdinGuard's `launcher`
   module does exactly this and otherwise offers to start the patcher (`openthor.exe`) instead.

The ticket is **not a security boundary**: anybody can write such a file and set the variable. It stops players from
skipping updates by accident (an old desktop shortcut to `alpha.exe`), nothing more. OpenThor does not need a guard, and the
guard does not need OpenThor: the two only share this protocol.

**Several clicks.** The Play button is disabled for 8 s after a start, and the patcher refuses a second start of the same
program within 8 s ("The game is starting, please wait."). With `max_instances` it counts the running copies of that exact
program file and refuses more ("The game is already running. Only one game window is allowed."). Copies started some other
way (a second client folder, a renamed exe) are not counted; a hard limit belongs to the server or the guard (OdinGuard:
`og_max_windows`). *Tested:* unit test of the ticket and the 8 s guard; Windows 11 VM with BurningRO's client: a direct
start of `alpha.exe` was refused by OdinGuard's launcher module, Play started the game, a second Play was refused with
`max_instances: 1`. 0.4.2 measured again with the patcher started normally (not elevated) and BurningRO's
`alpha.exe` (`requireAdministrator`): `launcher: started by the patcher`, ticket used, the guard connected to the live login
server.

