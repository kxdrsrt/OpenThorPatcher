//! The player side: check the list, sync whole files, remove files, bring every Thor target up to date, write `DATA.INI`.
//! Also understands the plain Thor Patcher server layout (`plist.txt` + `.thor` files) so an existing patch server works.

use crate::apply::{apply_auto, apply_thor, swap, GrfKey};
use crate::dataini;
use crate::fetch::{download, get_bytes, join};
use crate::patchlist::{parse_plist_txt, FileEntry, PatchList};
use crate::providers::is_absolute;
use crate::{io_err, msg, safe_rel, sha256_file, sign, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct Config {
    /// Folder URL of the patch site (`http(s)://`, `file://`), without the file name.
    pub patch_url: String,
    /// Full link of `patchlist.json` / its signature (cloud share links); default `<patch_url>/patchlist.json`.
    pub list_url: Option<String>,
    pub sig_url: Option<String>,
    pub client_dir: PathBuf,
    /// Public key file, or hex text. With either, the list must be signed.
    pub pub_key: Option<PathBuf>,
    pub pub_key_hex: Option<String>,
    /// GRF password per target name (`data.grf`), for encrypted GRFs.
    pub grf_passwords: BTreeMap<String, String>,
    pub backup: bool,
    /// Only report what would happen.
    pub dry_run: bool,
    /// Re-hash everything instead of trusting the size+time cache (the "check files" button).
    pub force_check: bool,
    /// GRF a legacy Thor patch without a target goes to.
    pub legacy_default_grf: String,
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Config {
    pub fn new(patch_url: impl Into<String>, client_dir: impl Into<PathBuf>) -> Self {
        Self {
            patch_url: patch_url.into(),
            list_url: None,
            sig_url: None,
            client_dir: client_dir.into(),
            pub_key: None,
            pub_key_hex: None,
            grf_passwords: BTreeMap::new(),
            backup: true,
            dry_run: false,
            force_check: false,
            legacy_default_grf: "data.grf".into(),
            cancel: None,
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed))
    }

    fn check_cancel(&self) -> Result<()> {
        if self.cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Info(String),
    Download { name: String, done: u64, total: u64 },
    Applied { target: String, id: u32 },
    Removed(String),
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
    /// last applied id of a legacy Thor Patcher list
    legacy_id: u32,
}

/// What the player chose; lives next to the state in `_patch/`.
#[derive(Default, Serialize, Deserialize, Clone)]
pub struct Prefs {
    /// pack id -> on/off
    pub packs: BTreeMap<String, bool>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Summary {
    pub files_updated: Vec<String>,
    pub patches_applied: Vec<u32>,
    pub repaired: Vec<String>,
    pub removed: Vec<String>,
    pub data_ini_written: bool,
    pub legacy: bool,
}

impl Summary {
    pub fn up_to_date(&self) -> bool {
        self.files_updated.is_empty() && self.patches_applied.is_empty() && self.repaired.is_empty() && self.removed.is_empty()
    }
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

fn cached_sha(st: &mut State, key: &str, p: &Path, force: bool) -> Result<Option<String>> {
    if !p.is_file() {
        return Ok(None);
    }
    let (size, mt) = size_mtime(p);
    if !force {
        if let Some(h) = st.hashes.get(key) {
            if h.size == size && h.mtime == mt {
                return Ok(Some(h.sha256.clone()));
            }
        }
    }
    let sha = sha256_file(p)?;
    st.hashes.insert(key.into(), Applied { id: 0, sha256: sha.clone(), size, mtime: mt });
    Ok(Some(sha))
}

fn patch_dir(dir: &Path) -> PathBuf {
    dir.join("_patch")
}

fn load_state(dir: &Path) -> State {
    std::fs::read(patch_dir(dir).join("state.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_state(dir: &Path, st: &State) -> Result<()> {
    write_json(&patch_dir(dir).join("state.json"), st)
}

fn write_json<T: Serialize>(p: &Path, v: &T) -> Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
    }
    std::fs::write(p, serde_json::to_vec_pretty(v)?).map_err(|e| io_err(p, e))
}

pub fn load_prefs(dir: &Path) -> Prefs {
    std::fs::read(patch_dir(dir).join("prefs.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_prefs(dir: &Path, p: &Prefs) -> Result<()> {
    write_json(&patch_dir(dir).join("prefs.json"), p)
}

pub fn fetch_list(cfg: &Config) -> Result<PatchList> {
    let list_url = cfg.list_url.clone().unwrap_or_else(|| join(&cfg.patch_url, "patchlist.json"));
    let bytes = get_bytes(&list_url)?;
    if cfg.pub_key.is_some() || cfg.pub_key_hex.is_some() {
        let sig_url = cfg.sig_url.clone().unwrap_or_else(|| format!("{list_url}.sig"));
        let sig = get_bytes(&sig_url).map_err(|_| msg("the patch list has no signature but this patcher requires one"))?;
        let sig = String::from_utf8_lossy(&sig).to_string();
        match (&cfg.pub_key_hex, &cfg.pub_key) {
            (Some(h), _) => sign::verify_hex(h, &bytes, &sig)?,
            (None, Some(f)) => sign::verify(f, &bytes, &sig)?,
            _ => {}
        }
    }
    PatchList::parse(&bytes)
}

/// Downloads `rel` (relative to a mirror, or a full link) from the first mirror that has it.
fn get(cfg: &Config, list: &PatchList, rel: &str, dest: &Path, size: u64, sha: &str, ev: &mut dyn FnMut(Event)) -> Result<()> {
    let bases: Vec<String> = if is_absolute(rel) { vec![String::new()] } else { list.base_url.clone() };
    let mut last = msg("the patch list has no mirror");
    for base in bases {
        let name = rel.rsplit('/').next().unwrap_or(rel).to_string();
        let mut f = |done, total| {
            ev(Event::Download { name: name.clone(), done, total });
            !cfg.cancelled()
        };
        match download(&join(&base, rel), dest, size, sha, &mut f) {
            Ok(()) => return Ok(()),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn get_file(cfg: &Config, list: &PatchList, f: &FileEntry, dest: &Path, ev: &mut dyn FnMut(Event)) -> Result<()> {
    let rel = if f.url.is_empty() { format!("files/{}", f.path) } else { f.url.clone() };
    get(cfg, list, &rel, dest, f.size, &f.sha256, ev)
}

fn remove_paths(cfg: &Config, list: &PatchList, sum: &mut Summary, ev: &mut dyn FnMut(Event)) -> Result<()> {
    for r in &list.remove {
        let (base, folder) = match r.strip_suffix("/*").or_else(|| r.strip_suffix("\\*")) {
            Some(b) => (b, true),
            None => (r.as_str(), false),
        };
        let rel = safe_rel(base)?;
        if rel.components().next().is_some_and(|c| c.as_os_str() == "_patch") {
            continue;
        }
        let full = cfg.client_dir.join(&rel);
        let exists = if folder { full.is_dir() } else { full.is_file() };
        if !exists {
            continue;
        }
        sum.removed.push(r.clone());
        if cfg.dry_run {
            continue;
        }
        let res = if folder { std::fs::remove_dir_all(&full) } else { std::fs::remove_file(&full) };
        res.map_err(|e| io_err(&full, e))?;
        ev(Event::Removed(r.clone()));
    }
    Ok(())
}

pub fn run(cfg: &Config, ev: &mut dyn FnMut(Event)) -> Result<Summary> {
    cfg.check_cancel()?;
    let list = match fetch_list(cfg) {
        Ok(l) => l,
        Err(Error::NotFound(_)) => return run_legacy(cfg, ev),
        Err(e) => return Err(e),
    };
    if !list.allow_patching {
        return Err(msg(if list.message.is_empty() { "patching is closed by the server".to_string() } else { list.message.clone() }));
    }
    ev(Event::Info(format!("{}: {} files, {} patches", list.server, list.files.len(), list.patches.len())));
    let mut st = load_state(&cfg.client_dir);
    let mut sum = Summary::default();
    let dl = patch_dir(&cfg.client_dir).join("dl");
    let key_for = |name: &str| GrfKey::from_password(cfg.grf_passwords.get(name).map(|s| s.as_str()));

    for f in &list.files {
        cfg.check_cancel()?;
        let rel = safe_rel(&f.path)?;
        let local = cfg.client_dir.join(&rel);
        let have = cached_sha(&mut st, &format!("file:{}", f.path), &local, cfg.force_check)?;
        if have.as_deref().is_some_and(|h| h.eq_ignore_ascii_case(&f.sha256)) {
            continue;
        }
        ev(Event::Info(format!("update {}", f.path)));
        sum.files_updated.push(f.path.clone());
        if cfg.dry_run {
            continue;
        }
        let tmp = dl.join(&rel);
        get_file(cfg, &list, f, &tmp, ev)?;
        if let Some(d) = local.parent() {
            std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
        }
        swap(&local, &tmp, false)?;
        st.hashes.remove(&format!("file:{}", f.path));
        save_state(&cfg.client_dir, &st)?;
    }

    remove_paths(cfg, &list, &mut sum, ev)?;

    for (name, target) in &list.targets {
        cfg.check_cancel()?;
        let grf = cfg.client_dir.join(safe_rel(name)?);
        let key = key_for(name)?;
        if !grf.is_file() {
            return Err(msg(format!("{name} is missing in the client folder; install the client package first")));
        }
        let local_sha = cached_sha(&mut st, &format!("grf:{name}"), &grf, cfg.force_check)?.unwrap_or_default();
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
                let tmp = dl.join(safe_rel(name)?);
                get_file(cfg, &list, full, &tmp, ev)?;
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
            cfg.check_cancel()?;
            ev(Event::Info(format!("patch {} for {name}", p.id)));
            sum.patches_applied.push(p.id);
            if cfg.dry_run {
                continue;
            }
            let tmp = dl.join(safe_rel(&p.file)?);
            let rel = if p.url.is_empty() { p.file.clone() } else { p.url.clone() };
            get(cfg, &list, &rel, &tmp, p.size, &p.sha256, ev)?;
            apply_thor(&grf, &tmp, &key, cfg.backup)?;
            let _ = std::fs::remove_file(&tmp);
            st.hashes.remove(&format!("grf:{name}"));
            let sha = cached_sha(&mut st, &format!("grf:{name}"), &grf, false)?.unwrap_or_default();
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
    if !cfg.dry_run {
        sum.data_ini_written = write_data_ini(cfg, &list)?;
    }
    Ok(sum)
}

/// Rewrites `[Data]` of `DATA.INI` from the server's order and the player's pack choices. Returns whether it changed.
pub fn write_data_ini(cfg: &Config, list: &PatchList) -> Result<bool> {
    let Some(di) = &list.data_ini else { return Ok(false) };
    let prefs = load_prefs(&cfg.client_dir);
    let path = cfg.client_dir.join("DATA.INI");
    let existing = dataini::read(&path);
    let want = dataini::plan(&di.order, &di.packs, &prefs.packs, &existing);
    if want == existing {
        return Ok(false);
    }
    dataini::write(&path, &want)?;
    Ok(true)
}

/// Switches a pack on or off for this player and rewrites `DATA.INI` (needs the patch list for the pack definitions).
pub fn set_pack(cfg: &Config, pack_id: &str, on: bool) -> Result<bool> {
    let list = fetch_list(cfg)?;
    let Some(di) = &list.data_ini else { return Err(msg("this server has no optional packs")) };
    let pack = di.packs.iter().find(|p| p.id == pack_id).ok_or_else(|| msg(format!("unknown pack {pack_id}")))?;
    if pack.locked && !on {
        return Err(msg("this pack is required by the server"));
    }
    let mut prefs = load_prefs(&cfg.client_dir);
    prefs.packs.insert(pack_id.to_string(), on);
    save_prefs(&cfg.client_dir, &prefs)?;
    write_data_ini(cfg, &list)
}

/// Classic Thor Patcher server: `plist.txt` (`id file`) and the `.thor` files next to it. No hashes, so downloads are
/// not verified; progress is a counter in the state file.
fn run_legacy(cfg: &Config, ev: &mut dyn FnMut(Event)) -> Result<Summary> {
    let text = get_bytes(&join(&cfg.patch_url, "plist.txt"))
        .map_err(|e| if matches!(e, Error::NotFound(_)) { msg("the server has neither patchlist.json nor plist.txt") } else { e })?;
    let mut items = parse_plist_txt(&String::from_utf8_lossy(&text));
    items.sort_by_key(|(id, _)| *id);
    ev(Event::Info(format!("classic patch list (plist.txt): {} entries", items.len())));
    let mut st = load_state(&cfg.client_dir);
    let mut sum = Summary { legacy: true, ..Default::default() };
    let dl = patch_dir(&cfg.client_dir).join("dl");
    let key_for = |name: &str| GrfKey::from_password(cfg.grf_passwords.get(name).map(|s| s.as_str()));
    let from = st.legacy_id;
    for (id, file) in items.into_iter().filter(|(id, _)| *id > from) {
        cfg.check_cancel()?;
        ev(Event::Info(format!("patch {id}: {file}")));
        sum.patches_applied.push(id);
        if cfg.dry_run {
            continue;
        }
        let tmp = dl.join(safe_rel(&file)?);
        let name = file.clone();
        let mut f = |done, total| {
            ev(Event::Download { name: name.clone(), done, total });
            !cfg.cancelled()
        };
        download(&join(&cfg.patch_url, &file), &tmp, 0, "", &mut f)?;
        apply_auto(&cfg.client_dir, &cfg.legacy_default_grf, &tmp, &key_for, cfg.backup)?;
        let _ = std::fs::remove_file(&tmp);
        st.legacy_id = id;
        st.hashes.clear();
        save_state(&cfg.client_dir, &st)?;
        ev(Event::Applied { target: "classic".into(), id });
    }
    Ok(sum)
}
