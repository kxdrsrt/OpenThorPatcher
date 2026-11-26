# Deploying for a live test

## What runs where

| Part | Runs on | Docker? |
|---|---|---|
| **OpenThor Patcher** (`openthor.exe` in the client folder) | every player's Windows PC | no, it is a desktop program |
| **Patch Maker** (`openthor-maker.exe`) / `otp publish` | the operator's PC (or the server that holds the master client) | optional: `otp` has a container image |
| **The patch site** (a folder of files) | any web server | optional: an nginx image is provided |

So **Docker is not necessary**: the server side of OpenThor Patcher is plain files behind HTTPS. It is **possible and
convenient** if you already run your game server with Docker/Easypanel: a tiny nginx container serves the folder.

## Minimal live-test setup (nothing but files)

1. On your PC: Patch Maker → *New project* (master client folder, output folder) → create the key pair → set the mirror
   (`https://patch.example.com/`) → *Publish*.
2. Upload the output folder to a host that serves it over HTTPS (any static host works, see [HOSTING.md](HOSTING.md)).
3. Put `patcher.json` (Patch Maker → Hosting → *Save patcher.json*) next to `openthor.exe` and hand both to the
   testers together with the baseline client (the first publish's GRF versions).
4. For every update: change the master client, *Preview*, *Publish*, upload the changed files. Players update at the
   next start.

## With Docker (nginx container for the site)

```sh
cd deploy
mkdir -p data/site data/master data/cfg          # site = what players download, master = your client, cfg = project + key
docker compose up -d site                        # nginx on port 8080 serving data/site (read-only)
docker compose run --rm publish                  # runs `otp publish` against data/master, writes data/site
```

* `deploy/site/nginx.conf`: `patchlist.json` is never cached, numbered patches are immutable, `/state/` is hidden,
  `Range` requests work (resumable downloads). Tested: `206 Partial Content`, headers as above, a real patch over HTTP.
* `deploy/otp/Dockerfile`: builds the CLI (`otp`, `otp-mcp`) into a small Debian image. The project file inside `data/cfg`
  must use the container paths (`client: /master`, `site: /site`, `signing_seed: /cfg/signing.seed`).
* **Easypanel**: create an *App* from the `deploy/site` Dockerfile (or the `nginx:alpine` image with `deploy/site/nginx.conf`
  as `/etc/nginx/conf.d/default.conf`), mount a volume at `/usr/share/nginx/html` (read-only), attach your domain with
  HTTPS. Upload the site folder into that volume (SFTP, rsync) or run the publisher container against it.
* Put the **signing key** only where you publish; it is never part of the site and never goes into an image.

## Checklist before the live test (honest state)

| | |
|---|---|
| Patching, GRF merge (also encrypted), removals, `DATA.INI` packs, news/changelog, themes | done, covered by `tests/e2e.sh` and a UI smoke test |
| Windows x64 build of the patcher | built (cross-compiled), **run on Windows 11 in a VM (x64 emulation): window, news, HTTP update from the nginx container work** |
| Real 2.5 GB `data.grf` | patched and verified (see README) |
| A real player machine with a slow connection, antivirus, a locked file | **not tested**; the first live tester should be a person who can report back |
| Code signing of the `.exe` | **not done**: Windows SmartScreen will warn ("unknown publisher") until it is signed |
| Self-update of the patcher | not there yet: ship a new `.exe` in the client package |
| HTTPS | use it; the patch list is signed anyway, but downloads should not be readable/modifiable in transit |
