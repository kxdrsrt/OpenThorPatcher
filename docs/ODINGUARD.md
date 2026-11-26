# OpenThor Patcher with OdinGuard

OdinGuard is optional. With it, the patcher helps keep honest players from being flagged during a patch rollout.

## 1. GRF manifest and the rollout order

OdinGuard checks every GRF in `DATA.INI` against the operator's manifest (name, size, tail hash, optional full hash).
While a patch rolls out, **both the old and the new version must be accepted**, otherwise players who have not
patched yet, or have just patched, are reported. The order that is safe, and that `og grf-scan --keep-previous` supports:

1. Build the new GRF(s) / patch, run `og grf-scan <profile> --client <patched client> --keep-previous`
   (the profile now accepts old and new), rebuild and deploy the **server** config.
2. Publish the new `patchlist.json`.
3. After most players patched: remove the old versions from the profile (`og grf-scan` without `--keep-previous`).

**Automated in the Patch Maker** (`odinguard` in `otp-project.json`, or *OdinGuard Anti Cheat* on the Project page):

```json
"odinguard": { "profile": "../odinguard-profile/myserver.profile.toml", "og": "/usr/local/bin/og", "full": false, "optional": ["odin-dark.grf"] }
```

On every `otp publish` (not on `--dry-run`, which only lists the step) the Maker runs, before it writes the new patch
list:

1. `og grf-scan --keep-previous --client <master client> [--full] [--optional …] <profile>`: the profile accepts the new
   GRF versions and keeps the old ones;
2. `og grf-manifest <profile> -o <site>/grf-manifest.json`: the public list of accepted versions (hashes only), signed
   like the patch list (`grf-manifest.json.sig`).

If `og` is missing or fails, the publish stops and the old patch list stays. The Maker does not deploy your server:
rebuild and deploy the server config (`og build`) **before** you upload the site (with *Git hosting* on, the push
happens right after the publish, so deploy first or switch the push off for that publish).

**Patcher side:** after patching, the patcher reads `grf-manifest.json` from the site (with a public key it must carry a
valid signature) and compares size and tail hash of every listed GRF with the client's file. A GRF that is not an
accepted version, or a required one that is missing, is shown as a warning (`otp patch`: `warning: cdata.grf is not a
version the server's OdinGuard accepts …`; JSON summary `grf_not_accepted`) instead of the guard reporting the player
at login. Sites without the file are not checked.

## 2. Files the patcher ships for OdinGuard

`odinguard.dll`, `ijl15.dll` (the loader), `odin.grf`, `odinguard.ini` and, when the server uses it, `odin-dark.grf`.
They are ordinary entries in `client.files` or `.thor` patches. `odin.grf` and `odin-dark.grf` must be **the exact
bytes** of the release (the guard checks hashes), so the patcher copies them whole, never merges them into another
GRF.

## 3. The competitive pack switch

`data_ini.packs` lists `odin-dark.grf` as an optional pack. The player's choice (on/off) decides whether the file is in
`DATA.INI`. If the server enforces the pack on some maps (`competitive_enforce` in the OdinGuard profile), the patch list
sets `locked_by_server` for the pack, or, when it is only enforced for some game modes, the patcher shows a short note
("battlegrounds need this pack") next to the switch. The server is the authority either way.

The player can also switch the pack in the game (OdinGuard's `!dark on|off`, which edits `DATA.INI` for the next start).
Since 0.4.1 the patcher notices that: it remembers the `[Data]` list it wrote last, and when `DATA.INI` differs, the packs
found there become the player's choice, so the next patch run does not undo `!dark` (and the switch in the patcher shows
the same state). Unit test `dark_toggled_in_game_is_kept_and_the_patcher_switch_still_wins`.

## 4. What the patcher must never do

It must not hide, rename or "repair around" files the guard checks, must not disable the guard, and must not ship an
`odinguard.dll` that is not the operator's release. OdinGuard's loader (`ijl15.dll`) is just a file to the patcher.
