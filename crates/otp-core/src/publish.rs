//! Publishing: turn a *master client* folder into a patch site (`patchlist.json`, `files/`, `patches/`).
//!
//! Nobody names or numbers patches by hand: the publisher keeps an entry hash state per Thor target in
//! `<site>/state/` and writes only the entries that changed since the last publish.

use crate::patchlist::{FileEntry, PatchEntry, PatchList, Target, SCHEMA};
use crate::project::Project;
use crate::{io_err, msg, safe_rel, sha256_bytes, sha256_file, sign, Result};
use rgrf_core::grfcrypt::Key;
use rgrf_core::thor::{self, ThorFile, ThorOptions};
use rgrf_core::{Archive, GrfPath};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[derive(Default, Serialize, Deserialize)]
struct State {
    /// hex of the raw entry name -> SHA-256 of the decoded content
    entries: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanItem {
    /// `file`, `patch`, `baseline`, `remove`, `none`
    pub kind: String,
    pub name: String,
    pub detail: String,
    pub size: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct PublishReport {
    pub dry_run: bool,
    pub patches_added: Vec<String>,
    pub files_updated: Vec<String>,
    pub baselines: Vec<String>,
    pub plan: Vec<PlanItem>,
    /// What `git push` said, when the project pushes.
    pub git: Option<String>,
    /// Output of the OdinGuard hook (`og grf-scan`, `og grf-manifest`), when the project has `odinguard`.
    pub odinguard: Option<String>,
}

/// OdinGuard hook: the master client's GRFs into the profile (old versions stay accepted while players patch), then
/// the public GRF manifest into the site. The server config must be rebuilt and deployed before players patch.
pub fn odinguard_hook(p: &Project, log: &mut dyn FnMut(&str)) -> Result<String> {
    let Some(o) = &p.odinguard else { return Ok(String::new()) };
    let og = o.og.clone().unwrap_or_else(|| "og".into());
    let run = |args: Vec<std::ffi::OsString>| -> Result<String> {
        let out = std::process::Command::new(&og).args(&args).output().map_err(|e| msg(format!("OdinGuard: {} could not be started: {e}", og.display())))?;
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        if out.status.success() {
            Ok(text.trim().to_string())
        } else {
            Err(msg(format!("OdinGuard: og {} failed: {}", args.first().map(|a| a.to_string_lossy().into_owned()).unwrap_or_default(), text.trim())))
        }
    };
    let mut scan: Vec<std::ffi::OsString> = vec!["grf-scan".into(), "--keep-previous".into(), "--client".into(), p.client.clone().into()];
    if o.full {
        scan.push("--full".into());
    }
    for opt in &o.optional {
        scan.push("--optional".into());
        scan.push(opt.into());
    }
    scan.push(o.profile.clone().into());
    let a = run(scan)?;
    let b = run(vec!["grf-manifest".into(), o.profile.clone().into(), "-o".into(), p.site.join("grf-manifest.json").into()])?;
    // signed like the patch list, so a mirror cannot change what the patcher compares against
    if let Some(seed) = &p.signing_seed {
        let m = p.site.join("grf-manifest.json");
        let bytes = std::fs::read(&m).map_err(|e| io_err(&m, e))?;
        write_atomic(&p.site.join("grf-manifest.json.sig"), sign::sign(seed, &bytes)?.as_bytes())?;
    }
    let text = format!("{a}\n{b}");
    for l in text.lines() {
        log(&format!("OdinGuard: {l}"));
    }
    log("OdinGuard: rebuild and deploy the server config (og build) before players patch, or they are reported as grf:modified");
    Ok(text)
}

fn open(path: &Path, key: Option<&Key>) -> Result<Archive> {
    Ok(match key {
        Some(k) => Archive::open_with_key(path, k)?,
        None => Archive::open(path)?,
    })
}

type Hashes = BTreeMap<Vec<u8>, (usize, String)>;

/// name key -> (entry index, sha256 of the content); the last entry of a path wins, like in the client.
fn hash_archive(a: &Archive) -> Result<Hashes> {
    let n = a.len();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
    let chunk = n.div_ceil(threads).max(1);
    let mut parts: Vec<Result<Vec<(usize, String)>>> = Vec::new();
    std::thread::scope(|s| {
        let hs: Vec<_> = (0..n)
            .step_by(chunk)
            .map(|from| {
                s.spawn(move || -> Result<Vec<(usize, String)>> {
                    let mut out = Vec::new();
                    for i in from..(from + chunk).min(n) {
                        if a.entries()[i].is_marker() {
                            continue;
                        }
                        out.push((i, sha256_bytes(&a.read(i)?.data)));
                    }
                    Ok(out)
                })
            })
            .collect();
        for h in hs {
            parts.push(h.join().unwrap_or_else(|_| Err(msg("hash thread panicked"))));
        }
    });
    let mut by_key = Hashes::new();
    for p in parts {
        for (i, sha) in p? {
            by_key.insert(a.entries()[i].name.key(), (i, sha));
        }
    }
    Ok(by_key)
}

fn write_atomic(p: &Path, data: &[u8]) -> Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
    }
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, data).map_err(|e| io_err(&tmp, e))?;
    std::fs::rename(&tmp, p).map_err(|e| io_err(p, e))
}

fn stem(rel: &str) -> String {
    Path::new(rel)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "grf".into())
        .replace(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_', "_")
}

fn norm(rel: &str) -> String {
    rel.replace('\\', "/").trim_start_matches("./").to_string()
}

fn check_project(p: &Project) -> Result<()> {
    if p.server.trim().is_empty() {
        return Err(msg("the project needs a server name"));
    }
    if !p.client.is_dir() {
        return Err(msg(format!("master client folder {} does not exist", p.client.display())));
    }
    if p.base_urls.is_empty() && p.url_map.is_empty() {
        return Err(msg("give at least one mirror base URL (or a full link for every file in the URL map)"));
    }
    for t in &p.thor {
        if p.sync.iter().any(|s| norm(s) == norm(t)) {
            return Err(msg(format!("{t} is listed for patching and for whole-file sync; pick one")));
        }
    }
    for r in &p.remove {
        let base = r.trim_end_matches("/*").trim_end_matches("\\*");
        safe_rel(base)?;
        if p.sync.iter().chain(&p.thor).any(|s| norm(s) == norm(base)) {
            return Err(msg(format!("{r} is removed and also published; remove one of the two")));
        }
    }
    Ok(())
}

pub fn publish(p: &Project, grf_key: Option<Key>, dry_run: bool, log: &mut dyn FnMut(&str)) -> Result<PublishReport> {
    check_project(p)?;
    let list_path = p.site.join("patchlist.json");
    let mut list: PatchList = match std::fs::read(&list_path) {
        Ok(b) => PatchList::parse(&b)?,
        Err(_) => PatchList { schema: SCHEMA, ..Default::default() },
    };
    list.schema = SCHEMA;
    list.server = p.server.clone();
    list.base_url = p.base_urls.clone();
    list.allow_patching = p.allow_patching;
    list.message = p.message.clone();
    list.notice = p.notice.clone();
    list.news_url = p.news_url.clone();
    list.remove = p.remove.iter().map(|r| norm(r)).collect();
    list.client = p.game.clone();
    list.data_ini = p.data_ini.clone();
    list.servers = p.servers.clone();
    list.website = p.website.clone();
    list.links = p.links.clone();
    list.news = p.news.clone();
    list.changelog = p.changelog.clone();
    let mut rep = PublishReport { dry_run, ..Default::default() };
    let plan = |rep: &mut PublishReport, kind: &str, name: &str, detail: String, size: u64| {
        rep.plan.push(PlanItem { kind: kind.into(), name: name.into(), detail, size });
    };

    // whole files
    let old_files: HashMap<String, FileEntry> = list.files.drain(..).map(|f| (f.path.clone(), f)).collect();
    for rel in &p.sync {
        let rel = norm(rel);
        let src = p.client.join(&rel);
        if !src.is_file() {
            return Err(msg(format!("sync file {rel} is not in the master client")));
        }
        let sha = sha256_file(&src)?;
        let size = std::fs::metadata(&src).map_err(|e| io_err(&src, e))?.len();
        let dest = p.site.join("files").join(&rel);
        let unchanged = old_files.get(&rel).is_some_and(|f| f.sha256 == sha) && (dest.is_file() || dry_run);
        if !unchanged {
            if !dry_run {
                if let Some(d) = dest.parent() {
                    std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
                }
                std::fs::copy(&src, &dest).map_err(|e| io_err(&src, e))?;
            }
            log(&format!("file   {rel} ({size} bytes)"));
            plan(&mut rep, "file", &rel, "new or changed".into(), size);
            rep.files_updated.push(rel.clone());
        }
        list.files.push(FileEntry { path: rel.clone(), size, sha256: sha, url: format!("files/{rel}") });
    }

    // Thor targets
    let known: Vec<String> = p.thor.iter().map(|t| norm(t)).collect();
    list.targets.retain(|k, _| known.contains(k));
    list.patches.retain(|x| known.contains(&x.target));
    for rel in &known {
        let src = p.client.join(rel);
        log(&format!("hashing {rel} ..."));
        let arc = open(&src, grf_key.as_ref())?;
        let now = hash_archive(&arc)?;
        let state_path = p.site.join("state").join(format!("{}.json", stem(rel)));
        let prev: Option<State> = match std::fs::read(&state_path) {
            Ok(b) => Some(serde_json::from_slice(&b)?),
            Err(_) => None,
        };
        let new_state = State {
            entries: now.iter().map(|(_, (i, sha))| (hex::encode(arc.entries()[*i].name.as_bytes()), sha.clone())).collect(),
        };

        let Some(prev) = prev else {
            let base = sha256_file(&src)?;
            list.targets.insert(rel.clone(), Target { baseline_sha256: base, full: None, full_id: 0 });
            if !dry_run {
                write_atomic(&state_path, &serde_json::to_vec(&new_state)?)?;
                if p.full {
                    publish_full(p, &mut list, rel, &src)?;
                }
            }
            log(&format!("baseline {rel}: patching starts from this version ({} entries)", now.len()));
            plan(&mut rep, "baseline", rel, format!("{} entries; players need this version as their starting point", now.len()), 0);
            rep.baselines.push(rel.clone());
            continue;
        };

        let mut old: HashMap<Vec<u8>, (Vec<u8>, String)> = HashMap::new();
        for (h, sha) in &prev.entries {
            let raw = hex::decode(h).map_err(|_| msg("state file is damaged"))?;
            old.insert(GrfPath::new(raw.clone()).key(), (raw, sha.clone()));
        }
        let (mut added, mut changed) = (0usize, 0usize);
        let mut changed_idx: Vec<usize> = Vec::new();
        for (key, (i, sha)) in &now {
            match old.get(key) {
                None => {
                    added += 1;
                    changed_idx.push(*i);
                }
                Some((_, s)) if s != sha => {
                    changed += 1;
                    changed_idx.push(*i);
                }
                _ => {}
            }
        }
        let removed: Vec<&Vec<u8>> = old.iter().filter(|(k, _)| !now.contains_key(*k)).map(|(_, (raw, _))| raw).collect();
        if changed_idx.is_empty() && removed.is_empty() {
            log(&format!("{rel}: no changes"));
        } else {
            let id = list.next_id();
            let file = format!("patches/{id:04}-{}.thor", stem(rel));
            let detail = format!("{added} new, {changed} changed, {} removed", removed.len());
            let mut size = 0;
            if !dry_run {
                let mut files: Vec<ThorFile> = Vec::with_capacity(changed_idx.len() + removed.len());
                for i in &changed_idx {
                    files.push(ThorFile { name: arc.entries()[*i].name.clone(), data: Some(arc.read(*i)?.data) });
                }
                for raw in &removed {
                    files.push(ThorFile { name: GrfPath::new((*raw).clone()), data: None });
                }
                let dest = p.site.join(&file);
                if let Some(d) = dest.parent() {
                    std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
                }
                let opts = ThorOptions { merge_into_grf: true, target_grf: rel.as_bytes().to_vec(), overwrite: true, integrity: false, ..Default::default() };
                thor::write(&dest, &files, &opts)?;
                size = std::fs::metadata(&dest).map_err(|e| io_err(&dest, e))?.len();
                list.patches.push(PatchEntry { id, target: rel.clone(), file: file.clone(), size, sha256: sha256_file(&dest)?, url: String::new() });
            }
            log(&format!("patch  {file}: {detail}, {size} bytes"));
            plan(&mut rep, "patch", &file, detail.clone(), size);
            rep.patches_added.push(file);
        }
        if !dry_run {
            write_atomic(&state_path, &serde_json::to_vec(&new_state)?)?;
            if p.full {
                publish_full(p, &mut list, rel, &src)?;
            }
        }
    }
    for r in &list.remove {
        plan(&mut rep, "remove", r, "deleted from player clients".into(), 0);
    }

    // full links for cloud hosting
    for f in list.files.iter_mut() {
        if let Some(u) = p.url_map.get(&format!("files/{}", f.path)) {
            f.url = u.clone();
        }
    }
    for pe in list.patches.iter_mut() {
        if let Some(u) = p.url_map.get(&pe.file) {
            pe.url = u.clone();
        }
    }
    for (name, t) in list.targets.iter_mut() {
        if let (Some(full), Some(u)) = (t.full.as_mut(), p.url_map.get(&format!("files/{name}"))) {
            full.url = u.clone();
        }
    }
    let missing: Vec<String> = if p.base_urls.is_empty() {
        list.files.iter().filter(|f| !crate::providers::is_absolute(&f.url)).map(|f| f.path.clone()).chain(
            list.patches.iter().filter(|x| !crate::providers::is_absolute(&x.url)).map(|x| x.file.clone())).collect()
    } else {
        vec![]
    };
    if !missing.is_empty() {
        return Err(msg(format!("no mirror base URL and no full link for: {}", missing.join(", "))));
    }

    if rep.plan.is_empty() {
        plan(&mut rep, "none", "", "the patch site is up to date".into(), 0);
    }
    if let Some(o) = &p.odinguard {
        plan(&mut rep, "odinguard", "grf-manifest.json", format!("og grf-scan into {}, then the GRF manifest", o.profile.display()), 0);
        if dry_run {
            log(&format!("OdinGuard: would run og grf-scan into {} and write grf-manifest.json", o.profile.display()));
        }
    }
    if dry_run {
        return Ok(rep);
    }
    // OdinGuard first: when og fails, the old patch list stays the published one
    if p.odinguard.is_some() {
        rep.odinguard = Some(odinguard_hook(p, log)?);
    }
    // the serial (and the time) only move when the list really changed, so a publish without changes keeps every mirror valid
    let strip = |mut v: serde_json::Value| {
        if let Some(o) = v.as_object_mut() {
            o.remove("updated");
            o.remove("serial");
        }
        v
    };
    let previous = std::fs::read(&list_path).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    let changed = match previous {
        Some(prev) => strip(prev) != strip(serde_json::to_value(&list)?),
        None => true,
    };
    if changed {
        list.serial += 1;
        list.updated = utc_now();
    }
    let json = serde_json::to_vec_pretty(&list)?;
    write_atomic(&list_path, &json)?;
    if let Some(seed) = &p.signing_seed {
        write_atomic(&p.site.join("patchlist.json.sig"), sign::sign(seed, &json)?.as_bytes())?;
    } else {
        log("warning: no signing key, the patch list is not signed");
    }
    if p.git_push {
        let msg = format!("Publish {}", list.updated);
        rep.git = Some(git_publish(&p.site, &msg, log)?);
    }
    Ok(rep)
}

fn git(site: &Path, args: &[&str]) -> Result<String> {
    let o = std::process::Command::new("git").arg("-C").arg(site).args(args).output().map_err(|e| msg(format!("git could not be started: {e}")))?;
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    if o.status.success() {
        Ok(out.trim().to_string())
    } else {
        Err(msg(format!("git {} failed: {}", args.join(" "), out.trim())))
    }
}

/// `git add -A`, commit, push in the site folder. `state/` (publisher bookkeeping) is kept out of the repository.
pub fn git_publish(site: &Path, message: &str, log: &mut dyn FnMut(&str)) -> Result<String> {
    git(site, &["rev-parse", "--is-inside-work-tree"]).map_err(|_| msg(format!("{} is not a git working tree: run `git init` / `git clone` there first", site.display())))?;
    let ignore = site.join(".gitignore");
    let cur = std::fs::read_to_string(&ignore).unwrap_or_default();
    if !cur.lines().any(|l| l.trim() == "state/") {
        std::fs::write(&ignore, format!("{cur}{}state/\n", if cur.is_empty() || cur.ends_with('\n') { "" } else { "\n" })).map_err(|e| io_err(&ignore, e))?;
    }
    git(site, &["add", "-A"])?;
    if git(site, &["status", "--porcelain"])?.is_empty() {
        return Ok("nothing to commit".into());
    }
    git(site, &["commit", "-m", message])?;
    log("git: committed, pushing ...");
    git(site, &["push"])
}

fn publish_full(p: &Project, list: &mut PatchList, name: &str, src: &Path) -> Result<()> {
    let dest = p.site.join("files").join(name);
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
    }
    std::fs::copy(src, &dest).map_err(|e| io_err(src, e))?;
    let size = std::fs::metadata(&dest).map_err(|e| io_err(&dest, e))?.len();
    let latest = list.patches.iter().filter(|x| x.target == name).map(|x| x.id).max().unwrap_or(0);
    let entry = FileEntry { path: name.to_string(), size, sha256: sha256_file(&dest)?, url: format!("files/{name}") };
    if let Some(t) = list.targets.get_mut(name) {
        t.full_id = latest;
        t.full = Some(entry);
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteFile {
    pub rel: String,
    pub size: u64,
    /// Full link from the URL map, if any.
    pub url: String,
}

/// Every file of the patch site that has to be uploaded (for a cloud host: one share link each).
pub fn site_files(p: &Project) -> Vec<SiteFile> {
    let mut out = Vec::new();
    fn walk(root: &Path, dir: &Path, p: &Project, out: &mut Vec<SiteFile>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "state") {
                    continue;
                }
                walk(root, &path, p, out);
            } else {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                if rel.ends_with(".tmp") {
                    continue;
                }
                out.push(SiteFile { size: e.metadata().map(|m| m.len()).unwrap_or(0), url: p.url_map.get(&rel).cloned().unwrap_or_default(), rel });
            }
        }
    }
    walk(&p.site, &p.site, p, &mut out);
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

fn utc_now() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = (s / 86400, s % 86400);
    // civil-from-days (Howard Hinnant)
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}
