//! The operator's project file (`otp-project.json`): everything `otp publish` and the Patch Maker need.

use crate::patchlist::{ClientCfg, DataIniCfg};
use crate::{io_err, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub server: String,
    /// The master client: the folder you keep up to date. Players are synced to it.
    pub client: PathBuf,
    /// Where the patch site is written (upload this folder, or parts of it, to your host).
    pub site: PathBuf,
    /// Mirror base URLs, first one is tried first. May be empty when every file has a full link in `url_map`.
    #[serde(default)]
    pub base_urls: Vec<String>,
    /// GRFs (relative to the master client) patched entry by entry with Thor patches.
    #[serde(default)]
    pub thor: Vec<String>,
    /// Files synced whole by hash (small GRFs, dll, exe).
    #[serde(default)]
    pub sync: Vec<String>,
    /// Paths removed from player clients (`dir/*` = whole folder).
    #[serde(default)]
    pub remove: Vec<String>,
    /// Secret signing key file (`signing.seed`); never leaves the operator's machine.
    #[serde(default)]
    pub signing_seed: Option<PathBuf>,
    /// Also publish whole GRFs so a damaged client can be repaired without reinstalling.
    #[serde(default)]
    pub full: bool,
    #[serde(default)]
    pub news_url: String,
    #[serde(default)]
    pub notice: String,
    #[serde(default)]
    pub game: Option<ClientCfg>,
    #[serde(default)]
    pub data_ini: Option<DataIniCfg>,
    /// Site-relative path (`files/odin.grf`, `patches/0001-data.thor`) → full link, for cloud hosting where every file has
    /// its own share link (Google Drive, Dropbox, OneDrive).
    #[serde(default)]
    pub url_map: BTreeMap<String, String>,
    #[serde(default = "yes")]
    pub allow_patching: bool,
    #[serde(default)]
    pub message: String,
}

impl Project {
    pub fn new(server: &str, client: PathBuf, site: PathBuf) -> Self {
        Self {
            server: server.into(),
            client,
            site,
            base_urls: vec![],
            thor: vec![],
            sync: vec![],
            remove: vec![],
            signing_seed: None,
            full: false,
            news_url: String::new(),
            notice: String::new(),
            game: None,
            data_ini: None,
            url_map: BTreeMap::new(),
            allow_patching: true,
            message: String::new(),
        }
    }

    /// Relative paths are resolved against the folder of the project file.
    pub fn load(path: &Path) -> Result<Self> {
        let b = std::fs::read(path).map_err(|e| io_err(path, e))?;
        let mut p: Project = serde_json::from_slice(&b)?;
        let base = path.parent().unwrap_or(Path::new("."));
        let fix = |x: &mut PathBuf| {
            if x.is_relative() {
                *x = base.join(&*x);
            }
        };
        fix(&mut p.client);
        fix(&mut p.site);
        if let Some(s) = p.signing_seed.as_mut() {
            fix(s);
        }
        Ok(p)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?).map_err(|e| io_err(&tmp, e))?;
        std::fs::rename(&tmp, path).map_err(|e| io_err(path, e))
    }
}

/// How the Patch Maker suggests handling a file of the master client.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Suggest {
    /// A big GRF: patched entry by entry (only the changed files are downloaded).
    Thor,
    /// A small file: synced whole when its hash changes.
    Sync,
    /// Not synced (saves, logs, screenshots, backups).
    Ignore,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanItem {
    pub path: String,
    pub size: u64,
    pub suggest: Suggest,
}

const IGNORE_DIRS: &[&str] = &["screenshot", "replay", "savedata", "_patch", "memo", "_tmpemblem", "emblem", "_backup", "backup"];

/// Lists the master client folder (top level and one folder deep for loose files) with a suggestion per file.
pub fn scan_client(dir: &Path) -> Result<Vec<ScanItem>> {
    let mut out = Vec::new();
    fn walk(root: &Path, dir: &Path, depth: u32, out: &mut Vec<ScanItem>) -> Result<()> {
        let rd = std::fs::read_dir(dir).map_err(|e| io_err(dir, e))?;
        for ent in rd.flatten() {
            let p = ent.path();
            let name = ent.file_name().to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                if IGNORE_DIRS.iter().any(|d| lower.starts_with(d)) || lower == "data" && depth == 0 {
                    continue;
                }
                if depth < 2 {
                    walk(root, &p, depth + 1, out)?;
                }
                continue;
            }
            let size = ent.metadata().map(|m| m.len()).unwrap_or(0);
            let ext = lower.rsplit('.').next().unwrap_or("");
            let suggest = if lower.ends_with(".bak") || lower.ends_with(".log") || lower.ends_with(".tmp") || lower.contains("otp-new") || lower.ends_with(".part") {
                Suggest::Ignore
            } else if matches!(ext, "grf" | "gpf") && size > 64 << 20 {
                Suggest::Thor
            } else {
                Suggest::Sync
            };
            out.push(ScanItem { path: rel, size, suggest });
        }
        Ok(())
    }
    walk(dir, dir, 0, &mut out)?;
    out.sort_by(|a, b| a.path.to_ascii_lowercase().cmp(&b.path.to_ascii_lowercase()));
    Ok(out)
}
