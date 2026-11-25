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
downloads resumable; several `base_urls` give you mirrors (the first that works wins).

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
| GitHub Releases, S3, R2, own server | direct URL | unchanged |

**Be honest about the limits.** Cloud drives are the fragile option:

* Google Drive throttles files that many people download (quota) and can answer with a web page; the patcher detects
  that, discards it and says so, but cannot work around it.
* A file you re-upload gets a new link on most drives, so the URL map must be updated after every publish. Google Drive
  keeps the link if you replace the file through *Manage versions*.
* Big GRFs are never sent whole during normal patching (only `.thor` diffs), which is what keeps cloud quotas workable.
* The cloud conversion is covered by unit tests of the link rewriting and an end-to-end test with local file links;
  it has **not** been tested against the live services.

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
{ "headers": ["Authorization: Bearer ghp_xxx"], "header_hosts": ["raw.githubusercontent.com"] }
```

GitLab uses `"PRIVATE-TOKEN: glpat-xxx"` with `gitlab.com`. The CLI takes `--header "Name: value" --header-host host`.
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
