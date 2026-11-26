# Hosting a patch site

The patch site is a folder:

```
patchlist.json          the list (signed)
patchlist.json.sig      its Ed25519 signature
patches/0001-data.thor  numbered by `otp publish`
files/odin.grf          whole files kept equal by hash
files/data.grf          (only with --full) whole GRFs for repair
```

Players need nothing but `https` access to it. **Static hosting is the recommended way**: your own web server, an
S3-compatible bucket (Cloudflare R2, Backblaze B2, MinIO), GitHub Releases/Pages, or a CDN. HTTP `Range` requests make
downloads resumable; several `base_urls` give you mirrors (see [Several sources](#several-sources-mirrors-and-the-master-version)).

## Where to put what

There are two different jobs, and they need different places:

1. **The first full client** (a zip of the master client, a few GB): every new player downloads it **once**, by hand, from
   your website. It does not have to be patchable and can sit anywhere that gives a direct download link: your own web
   server, Cloudflare R2 / Backblaze B2, a torrent, Google Drive, MEGA. The patcher is not involved.
2. **The patch site** (`patchlist.json`, small whole files, `.thor` diffs): the patcher reads it at every start. It must
   be plain `https` with direct file URLs.

Recommended for the patch site, in this order:

| Place | Why |
|---|---|
| **Your own server** (nginx, see [DEPLOY.md](DEPLOY.md), runs next to your game server) | Simplest, no limits but your line, `Range` works. The `deploy/` folder has the container. |
| **Cloudflare R2** (S3 compatible) | No traffic fees, a custom domain, fast worldwide. Good as the second source. |
| Backblaze B2 / MinIO / any S3 bucket | Same idea, cheap storage. |
| GitHub / GitLab repository | Free, versioned, public or private ([below](#github-and-gitlab)). Only for `patchlist.json` and the diffs: 100 MB per file in git. |
| Google Drive / Dropbox / OneDrive / MEGA | Works through share links ([Cloud drives](#cloud-drives)), but quotas and changing links make it the fragile choice. |

**MEGA (mega.nz) works as a source for single files** (since 0.2.0), with the URL map like the other cloud drives: paste
the file's link (`https://mega.nz/file/<id>#<key>`, the old `https://mega.nz/#!<id>!<key>` or `mega:<id>#<key>`). The
patcher asks MEGA's API for a temporary download address, decrypts the file (AES-128-CTR with the key from the link) and
checks the SHA-256 from the signed list as for every other source; interrupted downloads resume. The key never goes to
anyone but MEGA's own address scheme. Limits: free accounts have a **transfer quota** (the patcher says so clearly and
tries the next source), and **folder links are not supported** (every file needs its own link). Because `patchlist.json`
itself must be fetched at every start, put the list on a static host and use MEGA for the bigger files, or as the place
of the one-time full client. *Tested:* the end-to-end script against a local stand-in of MEGA's API (encryption, resume,
quota); a real public MEGA file was downloaded, decrypted to a valid ZIP and resumed byte-identically (`otp fetch`).

Normal patches are small: `otp publish` writes a `.thor` diff with only what changed (a 4-entry change to a 2.5 GB
`data.grf` is a 166-byte patch), so a patch site needs little space and little traffic. Whole GRFs are only uploaded with
`--full` (for repairing a damaged client) and can live on a bigger, slower place.

## The master client stays on your computer

The **master client** is the source of truth, but it is never uploaded as it is: `otp publish` turns it into the patch
site (signed `patchlist.json` with the SHA-256 of every file, the files, the `.thor` diffs). The patch list *is* the
published master: players are synced to the hashes in it, wherever the bytes come from. Keep a backup of the master folder
and of `site/state/` (the entry hashes the next diff is computed from); without `state/` the next publish starts a new
baseline.

What goes where, for a typical client (measured with BurningRO's: 3.2 GB, 4262 files):

| Part | How | Where |
|---|---|---|
| Custom GRFs (`cdata.grf` ~100 MB, `odin.grf`, `core.grf` …) | **packed, never unpacked**: big ones as Thor targets (only the changed entries travel), small ones synced whole | patch site |
| BGM, skins, `System/`, DLLs, `alpha.exe` | synced whole by hash (folders as `skin/name/*`) | patch site |
| `data.grf` (2.4 GB, 10 GB+ unpacked) | **external** (since 0.3.0): only size + SHA-256 + links in the list, never copied into the site | MEGA, a GitHub release (≤ 2 GB per file, so a release asset only after splitting, or another host), your server |
| Client package for new players | `otp package`: ZIP of the published master **without** external files, with the patcher and `patcher.json` (BurningRO: 719 MB) | MEGA, GitHub release, your website |

```json
"external": { "data.grf": ["https://mega.nz/file/ID#KEY", "https://dl.example.com/data.grf"] }
```

A new player unpacks the package and starts the patcher; it sees that `data.grf` is missing, downloads it from the first
link that works (checked by its SHA-256) and is done. `otp package` refuses to pack a master that changed since the last
publish, so the package always matches the list; a player who installs an older package just gets the patches since then.
Change an external file rarely: every change is a full download for every player. Content that changes belongs into a
custom GRF (patched entry by entry).

*Tested:* `tests/e2e.sh` (external file with a dead first link, package without it, fresh install needs no patch, refusal
of an unpublished master, `skin/…/*` folder sync); on Windows 11 in the VM with BurningRO's real client: 719 MB package,
patcher fetched the 2.4 GB `data.grf` from the second link (the first answered 404) in 26 s over the local network,
SHA-256 equal to the master, the game started.

## Several sources (mirrors) and the master version

You can list more than one place. **Every source must hold the same site** (the same publish). What the patcher does with
them:

* **The patch list is read from all sources** and the one with the highest `serial` that has a valid signature wins
  (the first source wins a tie). `serial` grows by one with every publish that changes something. So a source that is
  down, slow or **behind** simply does not matter.
* **A client never goes back.** It remembers the serial of the last list it applied and refuses an older one ("every
  source is out of date"), also when only a stale source is reachable. A mirror that has not synced yet cannot roll players
  back to an old version.
* **Every file is checked by its SHA-256 from the signed list.** A mirror with an old, partial or damaged file is thrown
  away for that file and the next source is asked; only the master version is ever installed.
* A source that does not answer (connection refused, or **no data for 45 s**) is skipped for the rest of the run, so a dead
  mirror does not cost its timeout for every file. The player's cancel button keeps working while it waits.
* The source the winning list came from is asked first for the files.

Set it up in the Patch Maker (Hosting → *Mirrors*: the first is the primary, the others are written into `patcher.json` as
`fallback_urls`), or by hand:

```json
{
  "name": "MyRO",
  "patch_url": "https://patch.myro.example/",
  "fallback_urls": ["https://myro.r2.example.com/patch/", "gh:me/patches@main"],
  "pub_key": "<hex>",
  "proxy": "http://proxy.example:3128"
}
```

On the command line: `otp patch --url <primary> --fallback-url <second> --fallback-url <third> ...`.

**Publishing to several places without ever showing a half-uploaded site:** publish once, then upload `files/` and `patches/`
to every source **first** and `patchlist.json` + `patchlist.json.sig` **last**. A client that sees the new list can then
always find the files it names. With rclone, for example:

```sh
for dest in r2:myro-patch myserver:/var/www/patch; do
  rclone copy site/files   "$dest/files";   rclone copy site/patches "$dest/patches"
  rclone copy site/patchlist.json "$dest";  rclone copy site/patchlist.json.sig "$dest"
done
```

A token for a private host (`headers`) is only sent to `patch_url`'s host unless you list others in `header_hosts`, so a
GitHub token never goes to the second source.

**Proxy:** without a setting the system proxy (Windows settings, `HTTP_PROXY` / `HTTPS_PROXY`) is used. `"proxy"` in
`patcher.json` (or `--proxy`) forces one (`http://`, `https://` or `socks5://`).

*Tested:* the end-to-end script covers a stale first source with a current fallback, a client refusing an older list than the
one it applied, a missing primary, and a damaged file on one source (`tests/e2e.sh`). The 45-second stall limit and the proxy
setting are in the code and have **not** been tested against a real stalled server or a real proxy.

## Cloud drives

A cloud drive gives every file its **own** share link instead of a folder structure, so the patcher cannot guess the
address of `patches/0002-data.thor`. The project therefore has a **URL map** (Patch Maker → Hosting → *Cloud links*):

1. Publish. The Hosting page lists every file that has to be uploaded.
2. Upload them, copy each file's share link into the table (or `url_map` in `otp-project.json`).
3. Publish again. `patchlist.json` now carries a full link per file; mirrors are not needed.
4. Upload `patchlist.json` and `patchlist.json.sig`, and put their share links into the player's `patcher.json` as
   `list_url` (the signature link is `list_url` + `.sig` unless `sig_url` is given).

Links are converted to direct downloads automatically:

| Provider | Link you paste | What the patcher uses |
|---|---|---|
| Google Drive | `https://drive.google.com/file/d/ID/view`, `…?id=ID`, `gdrive:ID` | `drive.usercontent.google.com/download?id=ID&export=download&confirm=t` |
| Dropbox | `…?dl=0` | `…?dl=1` |
| OneDrive | `onedrive.live.com/redir?…` | `…/download?…` |
| MEGA | `https://mega.nz/file/ID#KEY`, `mega:ID#KEY` | MEGA API (`"a":"g"`) → temporary address, decrypted while downloading |
| GitHub Releases, S3, R2, own server | direct URL | unchanged |

**Be honest about the limits.** Cloud drives are the fragile option:

* Google Drive throttles files that many people download (quota) and can answer with a web page; the patcher detects
  that, discards it and says so, but cannot work around it.
* A file you re-upload gets a new link on most drives, so the URL map must be updated after every publish. Google Drive
  keeps the link if you replace the file through *Manage versions*.
* Big GRFs are never sent whole during normal patching (only `.thor` diffs), which is what keeps cloud quotas workable.
* MEGA: transfer quota per IP for free accounts; a new upload is a new link.
* The cloud conversion is covered by unit tests of the link rewriting and an end-to-end test with local file links.
  MEGA was tested against the live service (a public file, see above). Google Drive, Dropbox and OneDrive have **not**
  been tested with a live link yet. Check your own links before publishing: `otp fetch --url <link> --out test.bin`
  downloads one link exactly as the patcher does and prints size and SHA-256 (a page instead of the file is reported).

For anything with more than a few dozen players use a static host.

## GitHub and GitLab

Both work as a plain static host. Use the shorthands in `base_urls` / `list_url`:

| Shorthand | Resolves to |
|---|---|
| `gh:owner/repo@main` | `https://raw.githubusercontent.com/owner/repo/main/` |
| `gl:group/project@main` | `https://gitlab.com/group/project/-/raw/main/` |

**Publishing.** Set *Git hosting* in the Patch Maker (or `"git_push": true` in `otp-project.json`): after each publish
the site folder is committed and pushed with **your own git login** (OpenThor never sees your credentials; `state/` is
git-ignored automatically). Without it, commit and push the site folder yourself.

**Private repository.** The player needs a read access. Create a **read-only** token (GitHub fine-grained token with
*Contents: read*, GitLab project access token with `read_repository`) and put it into the client's `patcher.json`:

```json
{ "headers": { "Authorization": "Bearer ghp_xxx" }, "header_hosts": ["raw.githubusercontent.com"] }
```

GitLab uses `{ "PRIVATE-TOKEN": "glpat-xxx" }` with `gitlab.com`. The CLI takes `--header "Name: value" --header-host host`.
The header is sent **only** to the listed hosts and never to a redirect target (redirects are followed manually).
A 401/403 is reported as "access denied".

**Limits, honestly.**

* The token is in `patcher.json`, so **every player can read it**. Use a read-only token on a repo that holds only
  patch data, and rotate it when a player leaves. For real secrecy put a small authenticated proxy in front.
* GitHub rejects files over 100 MB in git; use `.thor` diffs (small) and keep full GRFs elsewhere, or use Releases
  through the URL map (release assets are flat, so every file needs its own entry).
* Raw endpoints are rate limited and are not a CDN: fine for a test server, use a static host or R2 for a big player base.
* The git push, the shorthands and the header/redirect logic are covered by local end-to-end tests (a fake private
  host and a local git remote). They have **not** been tried against the live GitHub/GitLab services.

## Which file goes where

* `--thor` GRFs (big, patched entry by entry): `patches/…` only. Add `--full` to also publish the whole GRF for repair.
* `--sync` files: `files/<path>`.
* The first publish of a GRF records a **baseline**: players must start from that exact version (your client package).
  A later publish writes a `.thor` per change.
