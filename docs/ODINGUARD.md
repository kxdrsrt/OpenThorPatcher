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

The patcher can automate step 2's check: `odinguard.manifest` in the patch list points to the release's
`manifest.json` (signed); after patching, the patcher compares the GRFs it wrote with the accepted list and warns the
player ("your `cdata.grf` is not the version this server accepts, run the repair") instead of letting the guard flag
them at login.

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

## 4. What the patcher must never do

It must not hide, rename or "repair around" files the guard checks, must not disable the guard, and must not ship an
`odinguard.dll` that is not the operator's release. OdinGuard's loader (`ijl15.dll`) is just a file to the patcher.
