//! Applying one Thor patch to a GRF: merge through RustyGRF into a temporary GRF, verify, swap.
//!
//! The patch wins over the old GRF (`merge(&[thor, old])`: first archive wins), delete directives of the
//! patch remove paths. Encrypted GRFs are opened with the key and written encrypted again.

use crate::{io_err, msg, Result};
use rgrf_core::grfcrypt::{key_from_password, Key};
use rgrf_core::progress::Control;
use rgrf_core::repack::RepackOptions;
use rgrf_core::verify::{verify, VerifyOptions};
use rgrf_core::{merge, Archive};
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
    let rep = merge::merge(&[&patch, &old], &tmp, &opts, &Control::default());
    let rep = match rep {
        Ok(r) => r,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
    };

    let check = (|| -> Result<()> {
        let merged = open(&tmp, key)?;
        let v = verify(&merged, VerifyOptions { decode: true }, &Control::default())?;
        if v.exit_code() != 0 {
            return Err(msg(format!("the merged GRF failed verification ({} problems), nothing was changed", v.issues.len())));
        }
        Ok(())
    })();
    if let Err(e) = check {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    drop(old);
    drop(patch);
    swap(grf, &tmp, backup)?;
    let s = rep.sources.first().cloned().unwrap_or_default();
    Ok(ApplyReport { files_in_patch: s.files, removals: s.removals })
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
