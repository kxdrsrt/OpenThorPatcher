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
use sha2::{Digest, Sha256};

/// Downloads at the same time unless the player or `patcher.json` says otherwise.
pub const DEFAULT_PARALLEL: usize = 4;

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
    /// Headers for a private host (`Authorization: Bearer <token>`, `PRIVATE-TOKEN: <token>`).
    pub headers: Vec<(String, String)>,
    /// Hosts that get them; default: the host of `patch_url` / `list_url`. A token is never sent anywhere else, also not
    /// to the target of a redirect.
    pub header_hosts: Vec<String>,
    /// More patch sites with the same layout as `patch_url` (each has its own `patchlist.json`). The patch list is read from
    /// all of them and the newest valid one wins (`serial`), so one source being down or behind changes nothing. Files are
    /// still checked by SHA-256, so only the master version is ever accepted.
    pub fallback_urls: Vec<String>,
    /// An explicit HTTP(S) or SOCKS5 proxy; without one the system settings apply.
    pub proxy: Option<String>,
    /// Files downloaded at the same time (1 = one after another). Patches are still applied in order.
    pub parallel: usize,
    /// Bandwidth limit for all downloads together in KiB/s; 0 = no limit.
    pub max_kbps: u64,
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
            backup: false,
            dry_run: false,
            force_check: false,
            legacy_default_grf: "data.grf".into(),
            cancel: None,
            headers: vec![],
            header_hosts: vec![],
            fallback_urls: vec![],
            proxy: None,
            parallel: DEFAULT_PARALLEL,
            max_kbps: 0,
        }
    }

    fn apply_auth(&self) {
        let mut hosts = self.header_hosts.clone();
        if hosts.is_empty() {
            for u in std::iter::once(&self.patch_url).chain(self.list_url.iter()) {
                let h = crate::fetch::host_of(&crate::providers::resolve(u));
                if !h.is_empty() && !hosts.contains(&h) {
                    hosts.push(h);
                }
            }
        }
        crate::fetch::set_auth(crate::fetch::Auth { headers: self.headers.clone(), hosts });
        crate::fetch::set_proxy(self.proxy.clone());
        crate::fetch::set_rate_limit(self.max_kbps.saturating_mul(1024));
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
    /// All downloads of the current step together (several run at the same time).
    Total { done: u64, total: u64 },
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
    /// `serial` of the newest patch list this client applied; an older list is refused
    #[serde(default)]
    list_serial: u64,
}

/// What the player chose; lives next to the state in `_patch/`.
#[derive(Default, Serialize, Deserialize, Clone)]
pub struct Prefs {
    /// pack id -> on/off
    pub packs: BTreeMap<String, bool>,
    /// The `[Data]` list the patcher wrote last. When `DATA.INI` differs, something else changed it (OdinGuard's
    /// `!dark`), and the packs it switched count as the player's new choice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub last_data_ini: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Summary {
    pub files_updated: Vec<String>,
    pub patches_applied: Vec<u32>,
    pub repaired: Vec<String>,
    pub removed: Vec<String>,
    pub data_ini_written: bool,
    pub legacy: bool,
    /// GRFs that are not a version the server's OdinGuard accepts (`grf-manifest.json` of the site)
    #[serde(default)]
    pub grf_not_accepted: Vec<String>,
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

fn read_list(cfg: &Config, list_url: &str, need_sig: bool) -> Result<PatchList> {
    let bytes = get_bytes(list_url)?;
    if need_sig {
        let sig_url = match (&cfg.sig_url, &cfg.list_url) {
            (Some(u), Some(l)) if l == list_url => u.clone(),
            _ => format!("{list_url}.sig"),
        };
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

/// Reads the patch list from every source (`patch_url`/`list_url` and `fallback_urls`), checks the signature, and takes the
/// valid list with the highest `serial` (the first source wins a tie). A list older than the one this client already
/// applied is refused: a mirror that is behind must never roll a client back.
pub fn fetch_list(cfg: &Config) -> Result<PatchList> {
    cfg.apply_auth();
    let mut sources: Vec<(String, String)> = Vec::new(); // (list url, site folder or "")
    match &cfg.list_url {
        Some(u) => sources.push((u.clone(), String::new())),
        None => sources.push((join(&cfg.patch_url, "patchlist.json"), cfg.patch_url.clone())),
    }
    for u in &cfg.fallback_urls {
        let key = u.trim_end_matches('/');
        if !key.is_empty() && !sources.iter().any(|(_, f)| f.trim_end_matches('/') == key) {
            sources.push((join(u, "patchlist.json"), u.clone()));
        }
    }
    let need_sig = cfg.pub_key.is_some() || cfg.pub_key_hex.is_some();
    let mut best: Option<PatchList> = None;
    let mut errors: Vec<(String, Error)> = Vec::new();
    for (url, folder) in sources {
        match read_list(cfg, &url, need_sig) {
            Ok(mut l) => {
                l.source = folder;
                if best.as_ref().map_or(true, |b| l.serial > b.serial) {
                    best = Some(l);
                }
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => errors.push((url, e)),
        }
    }
    let Some(list) = best else {
        if errors.len() == 1 || errors.iter().all(|(_, e)| matches!(e, Error::NotFound(_))) {
            return Err(errors.remove(0).1);
        }
        let all: Vec<String> = errors.iter().map(|(u, e)| format!("{u}: {e}")).collect();
        return Err(msg(format!("no patch source answered with a valid patch list: {}", all.join(" | "))));
    };
    let have = load_state(&cfg.client_dir).list_serial;
    if list.serial < have {
        return Err(msg(format!(
            "the newest patch list reachable now has serial {} but this client already applied serial {}: every source is out of date. \
             Nothing was changed; try again later.",
            list.serial, have
        )));
    }
    Ok(list)
}

static DEAD: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn norm_base(b: &str) -> String {
    b.trim_end_matches('/').to_ascii_lowercase()
}

/// Downloads `rel` (relative to a mirror, or a full link) from the first source that has it. The site the patch list came
/// from is asked first; a source that does not answer (connection refused, timeout) is skipped for the rest of the run so a
/// dead one does not cost its timeout again for every file; a file that fails the SHA-256 check is thrown away and the
/// next source tried.
pub(crate) fn get(cfg: &Config, list: &PatchList, rel: &str, dest: &Path, size: u64, sha: &str, ev: &mut dyn FnMut(Event)) -> Result<()> {
    let mut bases: Vec<String> = if is_absolute(rel) {
        vec![String::new()]
    } else {
        let mut v = list.base_url.clone();
        if !list.source.is_empty() {
            v.retain(|b| norm_base(b) != norm_base(&list.source));
            v.insert(0, list.source.clone());
        }
        v
    };
    let dead: Vec<String> = DEAD.lock().map(|g| g.clone()).unwrap_or_default();
    let alive: Vec<String> = bases.iter().filter(|b| !dead.contains(&norm_base(b))).cloned().collect();
    if !alive.is_empty() {
        bases = alive;
    }
    let mut last = msg("the patch list has no mirror");
    let n = bases.len();
    for (i, base) in bases.into_iter().enumerate() {
        let name = rel.rsplit('/').next().unwrap_or(rel).to_string();
        let mut f = |done, total| {
            ev(Event::Download { name: name.clone(), done, total });
            !cfg.cancelled()
        };
        let url = join(&base, rel);
        // a list fetched over the network must not point the patcher at files on the player's own disk
        let local_site = cfg.patch_url.starts_with("file://")
            || cfg.list_url.as_deref().is_some_and(|u| u.starts_with("file://"))
            || cfg.fallback_urls.iter().any(|u| u.starts_with("file://"));
        if url.starts_with("file://") && !local_site {
            last = msg(format!("{url}: the patch list asks for a local file, refused"));
            continue;
        }
        match download(&url, dest, size, sha, &mut f) {
            Ok(()) => return Ok(()),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                if matches!(e, Error::Http(_) | Error::Net(_)) && !base.is_empty() {
                    if let Ok(mut g) = DEAD.lock() {
                        g.push(norm_base(&base));
                    }
                }
                if i + 1 < n {
                    ev(Event::Info(format!("{url}: {e}; trying the next source")));
                }
                last = e;
            }
        }
    }
    Err(last)
}

fn get_file(cfg: &Config, list: &PatchList, f: &FileEntry, dest: &Path, ev: &mut dyn FnMut(Event)) -> Result<()> {
    let rel = if f.url.is_empty() { format!("files/{}", f.path) } else { f.url.clone() };
    let mut r = get(cfg, list, &rel, dest, f.size, &f.sha256, ev);
    for alt in &f.mirrors {
        match r {
            Ok(()) | Err(Error::Cancelled) => break,
            Err(_) => r = get(cfg, list, alt, dest, f.size, &f.sha256, ev),
        }
    }
    r
}

/// One download of a step: site-relative path or full link, destination, size and SHA-256 from the list.
struct Job {
    rel: String,
    /// more full links to the same file, tried in order when `rel` fails
    alts: Vec<String>,
    dest: PathBuf,
    size: u64,
    sha: String,
}

enum Msg {
    Ev(Event),
    Done(Result<()>),
}

/// Downloads every job with up to `cfg.parallel` at the same time. A destination that already holds the right file (an
/// earlier run stopped after its download) is not fetched again. Progress of every file is passed on, plus a `Total`
/// event for all of them. The first error stops new downloads and is returned once the running ones have ended.
fn fetch_all(cfg: &Config, list: &PatchList, jobs: Vec<Job>, ev: &mut dyn FnMut(Event)) -> Result<()> {
    let jobs: Vec<Job> = jobs
        .into_iter()
        .filter(|j| !(j.dest.is_file() && !j.sha.is_empty() && sha256_file(&j.dest).is_ok_and(|h| h.eq_ignore_ascii_case(&j.sha))))
        .collect();
    if jobs.is_empty() {
        return Ok(());
    }
    let total: u64 = jobs.iter().map(|j| j.size).sum();
    let workers = cfg.parallel.clamp(1, 16).min(jobs.len());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    let mut first: Option<Error> = None;
    let mut done_by: BTreeMap<String, u64> = BTreeMap::new();
    std::thread::scope(|s| {
        for _ in 0..workers {
            let tx = tx.clone();
            let (jobs, next, stop) = (&jobs, &next, &stop);
            s.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                if i >= jobs.len() || stop.load(Ordering::SeqCst) || cfg.cancelled() {
                    break;
                }
                let j = &jobs[i];
                let mut r = Err(Error::Cancelled);
                for (n, rel) in std::iter::once(&j.rel).chain(j.alts.iter()).enumerate() {
                    if n > 0 {
                        let _ = tx.send(Msg::Ev(Event::Info(format!("trying the next link for {}", j.dest.file_name().unwrap_or_default().to_string_lossy()))));
                    }
                    r = get(cfg, list, rel, &j.dest, j.size, &j.sha, &mut |e| {
                        let _ = tx.send(Msg::Ev(e));
                    });
                    if !matches!(r, Err(ref e) if !matches!(e, Error::Cancelled)) {
                        break;
                    }
                }
                if r.is_err() {
                    stop.store(true, Ordering::SeqCst);
                }
                let _ = tx.send(Msg::Done(r));
            });
        }
        drop(tx);
        for m in rx {
            match m {
                Msg::Ev(e) => {
                    if let Event::Download { name, done, .. } = &e {
                        done_by.insert(name.clone(), *done);
                        let sum: u64 = done_by.values().sum();
                        ev(e);
                        ev(Event::Total { done: sum.min(total.max(sum)), total });
                    } else {
                        ev(e);
                    }
                }
                Msg::Done(Err(e)) => {
                    if first.is_none() || matches!(first, Some(Error::Cancelled)) {
                        first = Some(e);
                    }
                }
                Msg::Done(Ok(())) => {}
            }
        }
    });
    if let Some(e) = first {
        return Err(e);
    }
    cfg.check_cancel()
}

fn file_job(f: &FileEntry, dest: PathBuf) -> Job {
    Job { rel: if f.url.is_empty() { format!("files/{}", f.path) } else { f.url.clone() }, alts: f.mirrors.clone(), dest, size: f.size, sha: f.sha256.clone() }
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
    if let Ok(mut g) = DEAD.lock() {
        g.clear();
    }
    let list = match fetch_list(cfg) {
        Ok(l) => l,
        Err(Error::NotFound(_)) => return run_legacy(cfg, ev),
        Err(e) => return Err(e),
    };
    if !list.allow_patching {
        return Err(msg(if list.message.is_empty() { "patching is closed by the server".to_string() } else { list.message.clone() }));
    }
    ev(Event::Info(format!(
        "{}: {} files, {} patches (list serial {}{})",
        list.server,
        list.files.len(),
        list.patches.len(),
        list.serial,
        if list.source.is_empty() { String::new() } else { format!(", from {}", list.source) }
    )));
    let mut st = load_state(&cfg.client_dir);
    let mut sum = Summary::default();
    let dl = patch_dir(&cfg.client_dir).join("dl");
    let key_for = |name: &str| GrfKey::from_password(cfg.grf_passwords.get(name).map(|s| s.as_str()));

    let mut stale: Vec<&FileEntry> = Vec::new();
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
        stale.push(f);
    }
    if !cfg.dry_run && !stale.is_empty() {
        let jobs = stale.iter().map(|f| Ok(file_job(f, dl.join(safe_rel(&f.path)?)))).collect::<Result<Vec<_>>>()?;
        fetch_all(cfg, &list, jobs, ev)?;
        for f in &stale {
            let rel = safe_rel(&f.path)?;
            let local = cfg.client_dir.join(&rel);
            if let Some(d) = local.parent() {
                std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
            }
            swap(&local, &dl.join(&rel), false)?;
            st.hashes.remove(&format!("file:{}", f.path));
            save_state(&cfg.client_dir, &st)?;
        }
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
            _ if !target.current_sha256.is_empty() && local_sha.eq_ignore_ascii_case(&target.current_sha256) => Some(target.current_id),
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
        if !cfg.dry_run && !todo.is_empty() {
            // every patch of this GRF is downloaded first (several at a time), then they are applied in order
            let jobs = todo
                .iter()
                .map(|p| Ok(Job { rel: if p.url.is_empty() { p.file.clone() } else { p.url.clone() }, alts: vec![], dest: dl.join(safe_rel(&p.file)?), size: p.size, sha: p.sha256.clone() }))
                .collect::<Result<Vec<_>>>()?;
            fetch_all(cfg, &list, jobs, ev)?;
        }
        for p in todo {
            cfg.check_cancel()?;
            ev(Event::Info(format!("patch {} for {name}", p.id)));
            sum.patches_applied.push(p.id);
            if cfg.dry_run {
                continue;
            }
            let tmp = dl.join(safe_rel(&p.file)?);
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
    if !cfg.dry_run {
        st.list_serial = st.list_serial.max(list.serial);
    }
    save_state(&cfg.client_dir, &st)?;
    if !cfg.dry_run {
        sum.data_ini_written = write_data_ini(cfg, &list)?;
        sum.grf_not_accepted = grf_manifest_check(cfg, &list, ev);
    }
    Ok(sum)
}

/// SHA-256 of the 46-byte GRF header plus everything from the file table to the end (OdinGuard's "tail hash").
pub fn grf_tail_hash(path: &Path) -> Result<(u64, String)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).map_err(|e| io_err(path, e))?;
    let size = f.metadata().map_err(|e| io_err(path, e))?.len();
    let mut h = [0u8; 46];
    f.read_exact(&mut h).map_err(|e| io_err(path, e))?;
    let version = u32::from_le_bytes([h[42], h[43], h[44], h[45]]);
    let off = if version == 0x300 && h[35..38] == [0, 0, 0] {
        u64::from_le_bytes(h[30..38].try_into().unwrap_or_default())
    } else {
        u64::from(u32::from_le_bytes([h[30], h[31], h[32], h[33]]))
    };
    let mut d = Sha256::new();
    d.update(h);
    let pos = 46u64.saturating_add(off);
    if pos < size {
        f.seek(SeekFrom::Start(pos)).map_err(|e| io_err(path, e))?;
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = f.read(&mut buf).map_err(|e| io_err(path, e))?;
            if n == 0 {
                break;
            }
            d.update(&buf[..n]);
        }
    }
    Ok((size, hex::encode(d.finalize())))
}

/// After patching: compares the client's GRFs with `grf-manifest.json` of the site (written by the Patch Maker's OdinGuard
/// hook). A GRF the server does not accept would be reported by OdinGuard in game; the patcher says so now. Sites without
/// the file are skipped; with a public key the manifest must carry a valid signature.
fn grf_manifest_check(cfg: &Config, list: &PatchList, ev: &mut dyn FnMut(Event)) -> Vec<String> {
    let base = if list.source.is_empty() { cfg.patch_url.clone() } else { list.source.clone() };
    if base.is_empty() {
        return vec![];
    }
    let url = join(&base, "grf-manifest.json");
    let Ok(bytes) = get_bytes(&url) else { return vec![] };
    if cfg.pub_key.is_some() || cfg.pub_key_hex.is_some() {
        let sig = get_bytes(&format!("{url}.sig")).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
        let ok = match (&cfg.pub_key_hex, &cfg.pub_key) {
            (Some(h), _) => sign::verify_hex(h, &bytes, &sig).is_ok(),
            (None, Some(f)) => sign::verify(f, &bytes, &sig).is_ok(),
            _ => true,
        };
        if !ok {
            ev(Event::Info("grf-manifest.json has no valid signature, not used".into()));
            return vec![];
        }
    }
    let Ok(m) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return vec![] };
    let mut versions: BTreeMap<String, Vec<(u64, String, bool)>> = BTreeMap::new();
    for f in m["files"].as_array().into_iter().flatten() {
        let (Some(name), Some(size), Some(tail)) = (f["name"].as_str(), f["size"].as_u64(), f["tail_sha256"].as_str()) else { continue };
        versions.entry(name.to_ascii_lowercase()).or_default().push((size, tail.to_ascii_lowercase(), f["required"].as_bool().unwrap_or(true)));
    }
    let mut bad = vec![];
    for (name, vs) in &versions {
        let Ok(rel) = safe_rel(name) else { continue };
        let path = cfg.client_dir.join(rel);
        if !path.is_file() {
            if vs.iter().any(|v| v.2) {
                ev(Event::Info(format!("warning: {name} is missing; the server's OdinGuard expects it")));
                bad.push(name.clone());
            }
            continue;
        }
        let accepted = match grf_tail_hash(&path) {
            Ok((size, tail)) => vs.iter().any(|v| v.0 == size && v.1 == tail),
            Err(_) => false,
        };
        if !accepted {
            ev(Event::Info(format!(
                "warning: {name} is not a version the server's OdinGuard accepts (the server config may not be updated yet, or the file was changed)"
            )));
            bad.push(name.clone());
        }
    }
    if bad.is_empty() && !versions.is_empty() {
        ev(Event::Info(format!("OdinGuard GRF check: {} GRF(s) accepted", versions.len())));
    }
    bad
}

/// The player's pack choices, with changes made to `DATA.INI` outside the patcher (OdinGuard's `!dark`) taken over.
pub fn pack_choices(dir: &Path, di: &crate::patchlist::DataIniCfg) -> Prefs {
    let mut prefs = load_prefs(dir);
    let existing = dataini::read(&dir.join("DATA.INI"));
    let changed_outside = prefs.last_data_ini.is_empty() || !same_list(&existing, &prefs.last_data_ini);
    if changed_outside && !existing.is_empty() {
        for p in &di.packs {
            let listed = existing.iter().any(|f| f.eq_ignore_ascii_case(&p.file));
            // before the patcher ever wrote DATA.INI, only a pack that is listed says something; a missing one keeps
            // the stored choice or the pack's default
            if listed || !prefs.last_data_ini.is_empty() {
                prefs.packs.insert(p.id.clone(), listed);
            }
        }
    }
    prefs
}

fn same_list(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
}

/// Rewrites `[Data]` of `DATA.INI` from the server's order and the player's pack choices. Returns whether it changed.
pub fn write_data_ini(cfg: &Config, list: &PatchList) -> Result<bool> {
    let Some(di) = &list.data_ini else { return Ok(false) };
    let mut prefs = pack_choices(&cfg.client_dir, di);
    let path = cfg.client_dir.join("DATA.INI");
    let existing = dataini::read(&path);
    let want = dataini::plan(&di.order, &di.packs, &prefs.packs, &existing);
    let changed = want != existing;
    if changed {
        dataini::write(&path, &want)?;
    }
    prefs.last_data_ini = want;
    save_prefs(&cfg.client_dir, &prefs)?;
    Ok(changed)
}

/// Switches a pack on or off for this player and rewrites `DATA.INI` (needs the patch list for the pack definitions).
pub fn set_pack(cfg: &Config, pack_id: &str, on: bool) -> Result<bool> {
    let list = fetch_list(cfg)?;
    let Some(di) = &list.data_ini else { return Err(msg("this server has no optional packs")) };
    let pack = di.packs.iter().find(|p| p.id == pack_id).ok_or_else(|| msg(format!("unknown pack {pack_id}")))?;
    if pack.locked && !on {
        return Err(msg("this pack is required by the server"));
    }
    let mut prefs = pack_choices(&cfg.client_dir, di);
    prefs.packs.insert(pack_id.to_string(), on);
    // the choice is made now; the current DATA.INI must not override it
    prefs.last_data_ini = dataini::read(&cfg.client_dir.join("DATA.INI"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patchlist::{DataIniCfg, Pack};

    #[test]
    fn dark_toggled_in_game_is_kept_and_the_patcher_switch_still_wins() {
        let d = std::env::temp_dir().join(format!("otp-packs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(patch_dir(&d)).unwrap();
        let ini = d.join("DATA.INI");
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let di = DataIniCfg {
            order: s(&["odin.grf", "@pack:comp", "data.grf"]),
            packs: vec![Pack { id: "comp".into(), file: "odin-dark.grf".into(), ..Default::default() }],
        };
        // a player who used !dark before the patcher existed keeps the pack
        dataini::write(&ini, &s(&["odin.grf", "odin-dark.grf", "data.grf"])).unwrap();
        assert_eq!(pack_choices(&d, &di).packs.get("comp"), Some(&true));
        // a fresh client without it: no choice yet, the pack's default applies
        dataini::write(&ini, &s(&["odin.grf", "data.grf"])).unwrap();
        assert_eq!(pack_choices(&d, &di).packs.get("comp"), None);
        // the patcher wrote the list; then !dark on in the game
        save_prefs(&d, &Prefs { packs: BTreeMap::new(), last_data_ini: s(&["odin.grf", "data.grf"]) }).unwrap();
        dataini::write(&ini, &s(&["odin.grf", "odin-dark.grf", "data.grf"])).unwrap();
        let p = pack_choices(&d, &di);
        assert_eq!(p.packs.get("comp"), Some(&true));
        assert_eq!(dataini::plan(&di.order, &di.packs, &p.packs, &dataini::read(&ini)), s(&["odin.grf", "odin-dark.grf", "data.grf"]));
        // !dark off in the game is taken over too
        save_prefs(&d, &Prefs { packs: p.packs.clone(), last_data_ini: s(&["odin.grf", "odin-dark.grf", "data.grf"]) }).unwrap();
        dataini::write(&ini, &s(&["odin.grf", "data.grf"])).unwrap();
        assert_eq!(pack_choices(&d, &di).packs.get("comp"), Some(&false));
        // nothing changed outside: the stored choice (the patcher's switch) stays
        let mut on = BTreeMap::new();
        on.insert("comp".to_string(), true);
        save_prefs(&d, &Prefs { packs: on, last_data_ini: s(&["odin.grf", "data.grf"]) }).unwrap();
        assert_eq!(pack_choices(&d, &di).packs.get("comp"), Some(&true));
        let _ = std::fs::remove_dir_all(&d);
    }
}
