//! Uploading from the operator's computer: the patch site, the client package and external files (`data.grf`).
//!
//! A target is a string:
//! * an [rclone](https://rclone.org) destination, `remote:path` (SFTP to your own server, S3/R2/B2, MEGA, Google Drive,
//!   Dropbox, OneDrive … set up once with `rclone config`) or a local folder (a mounted share);
//! * `gh-release:owner/repo@tag` for a GitHub release (needs the `gh` program, logged in), for packages and external files.
//!
//! The patch site is uploaded in the order that never shows a half-uploaded site: every file first, `patchlist.json` and
//! its signature last. `state/` stays on the operator's computer. After uploading a package or an external file the
//! share link is asked for (`rclone link`), so it can be pasted into the project (`external`) or the website.

use crate::project::Project;
use crate::{msg, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Uploaded {
    pub target: String,
    pub what: String,
    /// Public link reported by the host (`rclone link`, release asset URL), when there is one.
    pub link: Option<String>,
}

fn tool(name: &str, cfg: Option<&Path>) -> PathBuf {
    cfg.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(name))
}

fn run(cmd: &mut Command, what: &str, log: &mut dyn FnMut(&str)) -> Result<String> {
    let o = cmd.output().map_err(|e| msg(format!("{what}: the program could not be started ({e}); is it installed?")))?;
    let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
    let errs = String::from_utf8_lossy(&o.stderr).trim().to_string();
    if !o.status.success() {
        return Err(msg(format!("{what} failed: {}", if errs.is_empty() { &out } else { &errs })));
    }
    if !errs.is_empty() {
        for l in errs.lines().filter(|l| !l.trim().is_empty()).take(5) {
            log(l);
        }
    }
    Ok(out)
}

fn rclone(p: &Project) -> Command {
    Command::new(tool("rclone", p.upload.rclone.as_deref()))
}

fn join_target(t: &str, name: &str) -> String {
    if t.ends_with('/') || t.ends_with(':') {
        format!("{t}{name}")
    } else {
        format!("{t}/{name}")
    }
}

/// `owner/repo@tag` of a `gh-release:` target.
fn gh_target(t: &str) -> Option<(&str, &str)> {
    let r = t.strip_prefix("gh-release:")?;
    Some(r.split_once('@').unwrap_or((r, "client")))
}

/// Uploads the patch site to every `upload.site` target: all files, then the patch list and its signature.
pub fn upload_site(p: &Project, log: &mut dyn FnMut(&str)) -> Result<Vec<Uploaded>> {
    if p.upload.site.is_empty() {
        return Err(msg("no upload target for the patch site (upload.site)"));
    }
    if !p.site.join("patchlist.json").is_file() {
        return Err(msg("nothing to upload yet: publish first"));
    }
    let mut done = Vec::new();
    for t in &p.upload.site {
        if t.starts_with("gh-release:") {
            return Err(msg(format!("{t}: a GitHub release cannot hold the patch site (it needs folders); use a repository (git_push) or a web host")));
        }
        log(&format!("{t}: files …"));
        let mut c = rclone(p);
        c.arg("copy").arg(&p.site).arg(t);
        for x in ["patchlist.json", "patchlist.json.sig", "state/**", "*.tmp", ".git/**"] {
            c.arg("--exclude").arg(x);
        }
        run(&mut c, &format!("upload to {t}"), log)?;
        log(&format!("{t}: patch list last …"));
        for f in ["patchlist.json", "patchlist.json.sig"] {
            let src = p.site.join(f);
            if src.is_file() {
                run(rclone(p).arg("copyto").arg(&src).arg(join_target(t, f)), &format!("upload of {f} to {t}"), log)?;
            }
        }
        done.push(Uploaded { target: t.clone(), what: "site".into(), link: None });
    }
    Ok(done)
}

/// Uploads one file to the targets (`rclone copyto` or a GitHub release asset) and asks for its public link.
pub fn upload_file(p: &Project, file: &Path, targets: &[String], what: &str, log: &mut dyn FnMut(&str)) -> Result<Vec<Uploaded>> {
    if !file.is_file() {
        return Err(msg(format!("{} does not exist", file.display())));
    }
    if targets.is_empty() {
        return Err(msg(format!("no upload target for the {what}")));
    }
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut done = Vec::new();
    for t in targets {
        log(&format!("{t}: {name} …"));
        let link = if let Some((repo, tag)) = gh_target(t) {
            let gh = || Command::new(tool("gh", p.upload.gh.as_deref()));
            // the release is created once (tag on the default branch); an existing asset is replaced
            if run(gh().args(["release", "view", tag, "--repo", repo]), "gh release view", log).is_err() {
                run(gh().args(["release", "create", tag, "--repo", repo, "--title", tag, "--notes", "Client files for new players"]), "gh release create", log)?;
            }
            run(gh().args(["release", "upload", tag, "--repo", repo, "--clobber"]).arg(file), &format!("upload to {t}"), log)?;
            Some(format!("https://github.com/{repo}/releases/download/{tag}/{name}"))
        } else {
            let dest = join_target(t, &name);
            run(rclone(p).arg("copyto").arg(file).arg(&dest), &format!("upload to {t}"), log)?;
            run(rclone(p).arg("link").arg(&dest), "rclone link", &mut |_| {}).ok().filter(|l| l.starts_with("http"))
        };
        if let Some(l) = &link {
            log(&format!("{t}: link {l}"));
        }
        done.push(Uploaded { target: t.clone(), what: format!("{what} {name}"), link });
    }
    Ok(done)
}

/// Uploads every external file of the project (`external`) to `upload.external`.
pub fn upload_external(p: &Project, log: &mut dyn FnMut(&str)) -> Result<Vec<Uploaded>> {
    let mut done = Vec::new();
    for rel in p.external.keys() {
        done.extend(upload_file(p, &p.client.join(crate::safe_rel(rel)?), &p.upload.external, "external file", log)?);
    }
    Ok(done)
}
