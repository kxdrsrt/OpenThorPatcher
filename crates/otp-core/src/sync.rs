//! The client side: check the list, sync whole files, bring every Thor target up to date.

use crate::apply::{apply_thor, swap, GrfKey};
use crate::fetch::{download, get_bytes, join};
use crate::patchlist::{FileEntry, PatchList};
use crate::{io_err, msg, sha256_file, sign, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Config {
    /// Where `patchlist.json` lives (http(s):// or file://), without the file name.
    pub patch_url: String,
    pub client_dir: PathBuf,
    /// Public key (`patcher.pub`). With it the list must be signed.
    pub pub_key: Option<PathBuf>,
    /// GRF password per target name (`data.grf`), for encrypted GRFs.
    pub grf_passwords: BTreeMap<String, String>,
    pub backup: bool,
    /// Only report what would happen.
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub enum Event {
    Info(String),
    Download { name: String, done: u64, total: u64 },
    Applied { target: String, id: u32 },
}

#[derive(Default, Serialize, Deserialize, Clone)]
struct Applied {
    id: u32,
    sha256: String,
    size: u64,
    mtime: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    targets: BTreeMap<String, Applied>,
    /// size+mtime cache of hashes, so a start without changes reads nothing
    hashes: BTreeMap<String, Applied>,
}

#[derive(Debug, Default)]
pub struct Summary {
    pub files_updated: Vec<String>,
    pub patches_applied: Vec<u32>,
    pub repaired: Vec<String>,
}

fn size_mtime(p: &Path) -> (u64, u64) {
    match std::fs::metadata(p) {
        Ok(m) => (
            m.len(),
            m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
        ),
        Err(_) => (0, 0),
    }
}

fn cached_sha(st: &mut State, key: &str, p: &Path) -> Result<Option<String>> {
    if !p.is_file() {
        return Ok(None);
    }
    let (size, mt) = size_mtime(p);
    if let Some(h) = st.hashes.get(key) {
        if h.size == size && h.mtime == mt {
            return Ok(Some(h.sha256.clone()));
        }
    }
    let sha = sha256_file(p)?;
    st.hashes.insert(key.into(), Applied { id: 0, sha256: sha.clone(), size, mtime: mt });
    Ok(Some(sha))
}

fn state_path(dir: &Path) -> PathBuf {
    dir.join("_patch").join("state.json")
}

fn load_state(dir: &Path) -> State {
    std::fs::read(state_path(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_state(dir: &Path, st: &State) -> Result<()> {
    let p = state_path(dir);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
    }
    std::fs::write(&p, serde_json::to_vec_pretty(st)?).map_err(|e| io_err(&p, e))
}

pub fn fetch_list(cfg: &Config) -> Result<PatchList> {
    let bytes = get_bytes(&join(&cfg.patch_url, "patchlist.json"))?;
    if let Some(pk) = &cfg.pub_key {
        let sig = get_bytes(&join(&cfg.patch_url, "patchlist.json.sig"))
            .map_err(|_| msg("the patch list has no signature but this patcher requires one"))?;
        sign::verify(pk, &bytes, &String::from_utf8_lossy(&sig))?;
    }
    PatchList::parse(&bytes)
}

/// Downloads `rel` from the first mirror that has it.
fn get(list: &PatchList, rel: &str, dest: &Path, size: u64, sha: &str, ev: &mut dyn FnMut(Event)) -> Result<()> {
    let mut last = msg("the patch list has no mirror");
    for base in &list.base_url {
        let name = rel.to_string();
        let mut f = |done, total| ev(Event::Download { name: name.clone(), done, total });
        match download(&join(base, rel), dest, size, sha, &mut f) {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn get_file(list: &PatchList, f: &FileEntry, dest: &Path, ev: &mut dyn FnMut(Event)) -> Result<()> {
    let rel = if f.url.is_empty() { format!("files/{}", f.path) } else { f.url.clone() };
    get(list, &rel, dest, f.size, &f.sha256, ev)
}

pub fn run(cfg: &Config, ev: &mut dyn FnMut(Event)) -> Result<Summary> {
    let list = fetch_list(cfg)?;
    if !list.allow_patching {
        return Err(msg(if list.message.is_empty() { "patching is closed by the server".to_string() } else { list.message.clone() }));
    }
    ev(Event::Info(format!("{}: {} files, {} patches", list.server, list.files.len(), list.patches.len())));
    let mut st = load_state(&cfg.client_dir);
    let mut sum = Summary::default();
    let dl = cfg.client_dir.join("_patch").join("dl");

    for f in &list.files {
        let local = cfg.client_dir.join(&f.path);
        let have = cached_sha(&mut st, &format!("file:{}", f.path), &local)?;
        if have.as_deref().is_some_and(|h| h.eq_ignore_ascii_case(&f.sha256)) {
            continue;
        }
        ev(Event::Info(format!("update {}", f.path)));
        sum.files_updated.push(f.path.clone());
        if cfg.dry_run {
            continue;
        }
        let tmp = dl.join(&f.path);
        get_file(&list, f, &tmp, ev)?;
        if let Some(d) = local.parent() {
            std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
        }
        swap(&local, &tmp, false)?;
        st.hashes.remove(&format!("file:{}", f.path));
        save_state(&cfg.client_dir, &st)?;
    }

    for (name, target) in &list.targets {
        let grf = cfg.client_dir.join(name);
        let key = GrfKey::from_password(cfg.grf_passwords.get(name).map(|s| s.as_str()))?;
        if !grf.is_file() {
            return Err(msg(format!("{name} is missing in the client folder; install the client package first")));
        }
        let local_sha = cached_sha(&mut st, &format!("grf:{name}"), &grf)?.unwrap_or_default();
        let current = match st.targets.get(name) {
            Some(a) if a.sha256 == local_sha => Some(a.id),
            _ if local_sha.eq_ignore_ascii_case(&target.baseline_sha256) => Some(0),
            _ => None,
        };
        let mut id = match current {
            Some(i) => i,
            None => {
                // modified, damaged or from another build: only a full copy helps
                let Some(full) = &target.full else {
                    return Err(msg(format!(
                        "{name} does not match any published version and the server offers no full copy; reinstall the client package"
                    )));
                };
                ev(Event::Info(format!("{name} is not a published version, restoring the full copy")));
                sum.repaired.push(name.clone());
                if cfg.dry_run {
                    continue;
                }
                let tmp = dl.join(name);
                get_file(&list, full, &tmp, ev)?;
                swap(&grf, &tmp, cfg.backup)?;
                st.hashes.remove(&format!("grf:{name}"));
                let (size, mtime) = size_mtime(&grf);
                st.targets.insert(name.clone(), Applied { id: target.full_id, sha256: full.sha256.clone(), size, mtime });
                save_state(&cfg.client_dir, &st)?;
                target.full_id
            }
        };
        let mut todo: Vec<_> = list.patches.iter().filter(|p| &p.target == name && p.id > id).collect();
        todo.sort_by_key(|p| p.id);
        for p in todo {
            ev(Event::Info(format!("patch {} for {name}", p.id)));
            sum.patches_applied.push(p.id);
            if cfg.dry_run {
                continue;
            }
            let tmp = dl.join(&p.file);
            get(&list, &p.file, &tmp, p.size, &p.sha256, ev)?;
            apply_thor(&grf, &tmp, &key, cfg.backup)?;
            let _ = std::fs::remove_file(&tmp);
            st.hashes.remove(&format!("grf:{name}"));
            let sha = cached_sha(&mut st, &format!("grf:{name}"), &grf)?.unwrap_or_default();
            let (size, mtime) = size_mtime(&grf);
            id = p.id;
            st.targets.insert(name.clone(), Applied { id, sha256: sha, size, mtime });
            save_state(&cfg.client_dir, &st)?;
            ev(Event::Applied { target: name.clone(), id });
        }
        if !cfg.dry_run && !st.targets.contains_key(name) {
            let (size, mtime) = size_mtime(&grf);
            st.targets.insert(name.clone(), Applied { id, sha256: local_sha, size, mtime });
        }
    }
    save_state(&cfg.client_dir, &st)?;
    Ok(sum)
}
