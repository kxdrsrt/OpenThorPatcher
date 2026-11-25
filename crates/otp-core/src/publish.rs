//! Publishing: turn a *master client* folder into a patch site (`patchlist.json`, `files/`, `patches/`).
//!
//! Nobody names or numbers patches by hand: the publisher keeps an entry hash state per Thor target in
//! `<site>/state/` and writes only the entries that changed since the last publish.

use crate::patchlist::{FileEntry, PatchEntry, PatchList, Target, SCHEMA};
use crate::{io_err, msg, sha256_bytes, sha256_file, sign, Result};
use rgrf_core::grfcrypt::Key;
use rgrf_core::thor::{self, ThorFile, ThorOptions};
use rgrf_core::{Archive, GrfPath};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub struct PublishOptions {
    pub client: PathBuf,
    pub site: PathBuf,
    pub server: String,
    pub base_urls: Vec<String>,
    /// GRFs (relative to the client) that are patched entry by entry.
    pub thor_targets: Vec<String>,
    /// Files synced whole (relative to the client).
    pub sync_files: Vec<String>,
    pub signing_seed: Option<PathBuf>,
    pub grf_key: Option<Key>,
    /// Also publish the whole current GRFs (repair/new installs without the client package).
    pub full: bool,
    pub message: Option<String>,
    pub allow_patching: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    /// hex of the raw entry name -> SHA-256 of the decoded content
    entries: BTreeMap<String, String>,
}

pub struct PublishReport {
    pub patches_added: Vec<String>,
    pub files_updated: Vec<String>,
    pub baselines: Vec<String>,
}

fn open(path: &Path, key: Option<&Key>) -> Result<Archive> {
    Ok(match key {
        Some(k) => Archive::open_with_key(path, k)?,
        None => Archive::open(path)?,
    })
}

/// name bytes -> sha256 of the content, for every file entry (last entry of a path wins, like the client).
fn hash_archive(a: &Archive) -> Result<BTreeMap<Vec<u8>, (usize, String)>> {
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
                        let e = &a.entries()[i];
                        if e.is_marker() {
                            continue;
                        }
                        let d = a.read(i)?;
                        out.push((i, sha256_bytes(&d.data)));
                    }
                    Ok(out)
                })
            })
            .collect();
        for h in hs {
            parts.push(h.join().unwrap_or_else(|_| Err(msg("hash thread panicked"))));
        }
    });
    let mut by_key: BTreeMap<Vec<u8>, (usize, String)> = BTreeMap::new();
    for p in parts {
        for (i, sha) in p? {
            by_key.insert(a.entries()[i].name.key(), (i, sha));
        }
    }
    Ok(by_key)
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(p: &Path) -> Result<T> {
    match std::fs::read(p) {
        Ok(b) => Ok(serde_json::from_slice(&b)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(io_err(p, e)),
    }
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

pub fn publish(o: &PublishOptions, log: &mut dyn FnMut(&str)) -> Result<PublishReport> {
    let list_path = o.site.join("patchlist.json");
    let mut list: PatchList = match std::fs::read(&list_path) {
        Ok(b) => PatchList::parse(&b)?,
        Err(_) => PatchList { schema: SCHEMA, ..Default::default() },
    };
    list.schema = SCHEMA;
    list.server = o.server.clone();
    list.base_url = o.base_urls.clone();
    list.allow_patching = o.allow_patching;
    list.message = o.message.clone().unwrap_or_default();
    let mut rep = PublishReport { patches_added: vec![], files_updated: vec![], baselines: vec![] };

    // whole files
    let old_files: HashMap<String, FileEntry> = list.files.drain(..).map(|f| (f.path.clone(), f)).collect();
    for rel in &o.sync_files {
        let src = o.client.join(rel);
        if !src.is_file() {
            return Err(msg(format!("sync file {rel} is not in the master client")));
        }
        let sha = sha256_file(&src)?;
        let size = std::fs::metadata(&src).map_err(|e| io_err(&src, e))?.len();
        let dest = o.site.join("files").join(rel);
        let unchanged = old_files.get(rel).is_some_and(|f| f.sha256 == sha) && dest.is_file();
        if !unchanged {
            if let Some(d) = dest.parent() {
                std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
            }
            std::fs::copy(&src, &dest).map_err(|e| io_err(&src, e))?;
            log(&format!("file   {rel} ({size} bytes)"));
            rep.files_updated.push(rel.clone());
        }
        list.files.push(FileEntry { path: rel.replace('\\', "/"), size, sha256: sha, url: format!("files/{}", rel.replace('\\', "/")) });
    }

    // Thor targets
    for rel in &o.thor_targets {
        let src = o.client.join(rel);
        let name = rel.replace('\\', "/");
        log(&format!("hashing {rel} ..."));
        let arc = open(&src, o.grf_key.as_ref())?;
        let now = hash_archive(&arc)?;
        let state_path = o.site.join("state").join(format!("{}.json", stem(rel)));
        let prev: Option<State> = if state_path.exists() { Some(read_json(&state_path)?) } else { None };
        let new_state = State {
            entries: now
                .iter()
                .map(|(_, (i, sha))| (hex::encode(arc.entries()[*i].name.as_bytes()), sha.clone()))
                .collect(),
        };

        let Some(prev) = prev else {
            let base = sha256_file(&src)?;
            list.targets.insert(name.clone(), Target { baseline_sha256: base, full: None, full_id: 0 });
            write_atomic(&state_path, &serde_json::to_vec(&new_state)?)?;
            if o.full {
                publish_full(o, &mut list, &name, &src)?;
            }
            log(&format!("baseline {rel}: patching starts from this version ({} entries)", now.len()));
            rep.baselines.push(name);
            continue;
        };

        let mut old: HashMap<Vec<u8>, (Vec<u8>, String)> = HashMap::new();
        for (h, sha) in &prev.entries {
            let raw = hex::decode(h).map_err(|_| msg("state file is damaged"))?;
            old.insert(GrfPath::new(raw.clone()).key(), (raw, sha.clone()));
        }
        let mut files: Vec<ThorFile> = Vec::new();
        for (key, (i, sha)) in &now {
            if old.get(key).map(|(_, s)| s) != Some(sha) {
                let e = &arc.entries()[*i];
                files.push(ThorFile { name: e.name.clone(), data: Some(arc.read(*i)?.data) });
            }
        }
        let changed = files.len();
        for (key, (raw, _)) in &old {
            if !now.contains_key(key) {
                files.push(ThorFile { name: GrfPath::new(raw.clone()), data: None });
            }
        }
        if files.is_empty() {
            log(&format!("{rel}: no changes"));
        } else {
            let id = list.next_id();
            let file = format!("patches/{id:04}-{}.thor", stem(rel));
            let dest = o.site.join(&file);
            if let Some(d) = dest.parent() {
                std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
            }
            let opts = ThorOptions { merge_into_grf: true, target_grf: name.as_bytes().to_vec(), overwrite: true, integrity: false, ..Default::default() };
            let r = thor::write(&dest, &files, &opts)?;
            let size = std::fs::metadata(&dest).map_err(|e| io_err(&dest, e))?.len();
            list.patches.push(PatchEntry { id, target: name.clone(), file: file.clone(), size, sha256: sha256_file(&dest)? });
            log(&format!("patch  {file}: {changed} new/changed, {} removed, {size} bytes", r.removals));
            rep.patches_added.push(file);
        }
        write_atomic(&state_path, &serde_json::to_vec(&new_state)?)?;
        if o.full {
            publish_full(o, &mut list, &name, &src)?;
        }
    }

    list.updated = utc_now();
    let json = serde_json::to_vec_pretty(&list)?;
    write_atomic(&list_path, &json)?;
    if let Some(seed) = &o.signing_seed {
        write_atomic(&o.site.join("patchlist.json.sig"), sign::sign(seed, &json)?.as_bytes())?;
    } else {
        log("warning: no signing key, the patch list is not signed");
    }
    Ok(rep)
}

fn publish_full(o: &PublishOptions, list: &mut PatchList, name: &str, src: &Path) -> Result<()> {
    let dest = o.site.join("files").join(name);
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
    }
    std::fs::copy(src, &dest).map_err(|e| io_err(src, e))?;
    let size = std::fs::metadata(&dest).map_err(|e| io_err(&dest, e))?.len();
    let latest = list.patches.iter().filter(|p| p.target == name).map(|p| p.id).max().unwrap_or(0);
    let entry = FileEntry { path: name.to_string(), size, sha256: sha256_file(&dest)?, url: format!("files/{name}") };
    if let Some(t) = list.targets.get_mut(name) {
        t.full_id = latest;
        t.full = Some(entry);
    }
    Ok(())
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
