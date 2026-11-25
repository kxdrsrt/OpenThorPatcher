//! Applying Thor patches: into a GRF (merge through RustyGRF, check, swap) or into the client folder (loose files).
//!
//! A patch wins over the old GRF, delete directives of the patch remove paths. Encrypted GRFs are opened with the key
//! and written encrypted again. Nothing is changed unless the result passed its checks.

use crate::{io_err, msg, safe_rel, Result};
use rgrf_core::grfcrypt::{key_from_password, Key};
use rgrf_core::progress::Control;
use rgrf_core::repack::RepackOptions;
use rgrf_core::verify::{verify, VerifyOptions};
use rgrf_core::{merge, Archive, NameMode};
use std::path::{Path, PathBuf};

#[derive(Default, Clone)]
pub struct GrfKey(pub Option<Key>);

impl GrfKey {
    pub fn from_password(p: Option<&str>) -> Result<Self> {
        match p.filter(|s| !s.is_empty()) {
            None => Ok(Self(None)),
            Some(p) => Ok(Self(Some(key_from_password(p)?))),
        }
    }
}

#[derive(Debug, Default)]
pub struct ApplyReport {
    pub files_in_patch: usize,
    pub removals: usize,
    /// Where the patch went: the GRF name or `(folder)`.
    pub target: String,
}

fn open(path: &Path, key: &GrfKey) -> Result<Archive> {
    Ok(match &key.0 {
        Some(k) => Archive::open_with_key(path, k)?,
        None => Archive::open(path)?,
    })
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut n = path.file_name().unwrap_or_default().to_os_string();
    n.push(suffix);
    path.with_file_name(n)
}

/// What a Thor patch asks for in its header.
pub struct ThorInfo {
    pub merge_into_grf: bool,
    pub target_grf: String,
    pub entries: usize,
}

pub fn thor_info(thor: &Path) -> Result<ThorInfo> {
    let a = Archive::open(thor)?;
    let p = a.header().patch.clone();
    let (merge_into_grf, target) = match p {
        Some(p) => (p.merge_into_grf, p.target_grf),
        None => (true, Vec::new()),
    };
    Ok(ThorInfo {
        merge_into_grf,
        target_grf: rgrf_core::GrfPath::new(target).display(NameMode::Cp949),
        entries: a.len(),
    })
}

/// Merges `thor` into `grf` in place. On any error `grf` is untouched. With `backup` the previous GRF stays as
/// `<name>.bak`.
pub fn apply_thor(grf: &Path, thor: &Path, key: &GrfKey, backup: bool) -> Result<ApplyReport> {
    let old = open(grf, key)?;
    // a patch is never encrypted, so it is opened without the key
    let patch = Archive::open(thor)?;
    let tmp = sibling(grf, ".otp-new");
    let _ = std::fs::remove_file(&tmp);

    let mut opts = RepackOptions::default();
    opts.encrypt = key.0;
    opts.overwrite = true;
    // plain GRFs: copy the compressed streams as they are (no decode/encode of 2 GB); the check below covers the
    // patch entries. Encrypted GRFs need the normal path so the new entries are encrypted.
    opts.copy_streams = key.0.is_none();
    opts.verify = key.0.is_some();
    let rep = match merge::merge(&[&patch, &old], &tmp, &opts, &Control::default()) {
        Ok(r) => r,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
    };

    if let Err(e) = check_result(&tmp, &patch, key) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    drop(old);
    drop(patch);
    swap(grf, &tmp, backup)?;
    let s = rep.sources.first().cloned().unwrap_or_default();
    Ok(ApplyReport {
        files_in_patch: s.files,
        removals: s.removals,
        target: grf.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
    })
}

/// The merged GRF must parse, its file table must be sound, and every file of the patch must read back identical.
fn check_result(merged_path: &Path, patch: &Archive, key: &GrfKey) -> Result<()> {
    let merged = open(merged_path, key)?;
    let v = verify(&merged, VerifyOptions { decode: false }, &Control::default())?;
    if v.exit_code() != 0 {
        return Err(msg(format!("the merged GRF failed verification ({} problems), nothing was changed", v.issues.len())));
    }
    let index = merged.index_by_key();
    for (i, e) in patch.entries().iter().enumerate() {
        let key_name = e.name.key();
        if e.is_removal() {
            if index.get(&key_name).is_some_and(|v| !v.is_empty()) {
                return Err(msg(format!("{} should have been removed", e.name.display(NameMode::Cp949))));
            }
            continue;
        }
        if e.is_marker() {
            continue;
        }
        let want = patch.read(i)?.data;
        let Some(&at) = index.get(&key_name).and_then(|v| v.last()) else {
            return Err(msg(format!("{} is missing in the merged GRF", e.name.display(NameMode::Cp949))));
        };
        if merged.read(at)?.data != want {
            return Err(msg(format!("{} differs after the merge, nothing was changed", e.name.display(NameMode::Cp949))));
        }
    }
    Ok(())
}

/// A Thor patch that is not meant for a GRF: its files go to the client folder, delete directives remove files.
pub fn apply_loose(client_dir: &Path, thor: &Path) -> Result<ApplyReport> {
    let patch = Archive::open(thor)?;
    let mut rep = ApplyReport { target: "(folder)".into(), ..Default::default() };
    for (i, e) in patch.entries().iter().enumerate() {
        let rel = safe_rel(&e.name.display(NameMode::Cp949))?;
        let dest = client_dir.join(&rel);
        if e.is_removal() {
            let _ = std::fs::remove_file(&dest);
            rep.removals += 1;
            continue;
        }
        if e.is_marker() {
            continue;
        }
        let data = patch.read(i)?.data;
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
        }
        let tmp = sibling(&dest, ".otp-new");
        std::fs::write(&tmp, &data).map_err(|e| io_err(&tmp, e))?;
        // a running or locked file cannot be replaced: report it instead of leaving the temp file around
        if let Err(e) = std::fs::rename(&tmp, &dest) {
            let _ = std::fs::remove_file(&tmp);
            return Err(io_err(&dest, e));
        }
        rep.files_in_patch += 1;
    }
    Ok(rep)
}

/// Picks the right way from the patch's own header (what Thor Patcher servers rely on): merge into the GRF the patch
/// names (`default_grf` if it names none) or extract into the client folder.
pub fn apply_auto(client_dir: &Path, default_grf: &str, thor: &Path, key_for: &dyn Fn(&str) -> Result<GrfKey>, backup: bool) -> Result<ApplyReport> {
    let info = thor_info(thor)?;
    if !info.merge_into_grf {
        return apply_loose(client_dir, thor);
    }
    let name = if info.target_grf.is_empty() { default_grf.to_string() } else { info.target_grf };
    let grf = client_dir.join(safe_rel(&name)?);
    if !grf.is_file() {
        return Err(msg(format!("the patch targets {name}, which is not in the client folder")));
    }
    apply_thor(&grf, thor, &key_for(&name)?, backup)
}

/// `new` becomes `target`; the old target is kept as `.bak` or removed. Same folder, so the rename is atomic.
pub fn swap(target: &Path, new: &Path, backup: bool) -> Result<()> {
    let bak = sibling(target, ".bak");
    if target.exists() {
        let _ = std::fs::remove_file(&bak);
        std::fs::rename(target, &bak).map_err(|e| io_err(target, e))?;
    }
    if let Err(e) = std::fs::rename(new, target) {
        if bak.exists() {
            let _ = std::fs::rename(&bak, target);
        }
        return Err(io_err(target, e));
    }
    if !backup {
        let _ = std::fs::remove_file(&bak);
    }
    Ok(())
}
