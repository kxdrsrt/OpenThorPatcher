//! Self-update of the patcher program. The patch list names the newest patcher per platform (`patcher`); since the list
//! is signed and the download is checked by its SHA-256, only the operator's build can replace the program. A patcher
//! without a public key does not update itself.
//!
//! Windows lets a running program be renamed but not overwritten, so the update renames the running file to `<name>.old`,
//! moves the new one into its place and starts it; the next start deletes `<name>.old`.

use crate::patchlist::{FileEntry, PatchList};
use crate::sync::{Config, Event};
use crate::{io_err, msg, Result};
use std::path::{Path, PathBuf};

/// The platform key of this build in `patcher.files`.
pub fn platform() -> &'static str {
    if cfg!(all(windows, target_arch = "x86_64")) {
        "windows-x64"
    } else if cfg!(all(windows, target_arch = "aarch64")) {
        "windows-arm64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-arm64"
    } else if cfg!(target_os = "macos") {
        "macos-x64"
    } else if cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "linux-x64"
    }
}

fn parts(v: &str) -> Vec<u64> {
    v.trim().trim_start_matches('v').split(['.', '-', '+']).take(3).map(|x| x.parse().unwrap_or(0)).collect()
}

/// Is `a` newer than `b` (`major.minor.patch`)?
pub fn newer(a: &str, b: &str) -> bool {
    let (mut x, mut y) = (parts(a), parts(b));
    x.resize(3, 0);
    y.resize(3, 0);
    x > y
}

/// The update for this program, if the list offers a newer version for `platform`.
pub fn available<'a>(list: &'a PatchList, current: &str, platform: &str) -> Option<(&'a str, &'a FileEntry)> {
    let r = list.patcher.as_ref()?;
    if !newer(&r.version, current) {
        return None;
    }
    r.files.get(platform).map(|f| (r.version.as_str(), f))
}

fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut n = exe.file_name().unwrap_or_default().to_os_string();
    n.push(suffix);
    exe.with_file_name(n)
}

/// Deletes the program file left by the last update (`<name>.old`).
pub fn cleanup(exe: &Path) {
    let _ = std::fs::remove_file(sibling(exe, ".old"));
    let _ = std::fs::remove_file(sibling(exe, ".new"));
}

/// Downloads the new program next to `exe` (`<name>.new`, checked by size and SHA-256) and swaps it in. Returns the
/// path to start (the same as `exe`). Needs a signed list (a public key in `cfg`).
pub fn update(cfg: &Config, list: &PatchList, entry: &FileEntry, exe: &Path, ev: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    if cfg.pub_key.is_none() && cfg.pub_key_hex.is_none() {
        return Err(msg("self-update needs a signed patch list (pub_key in patcher.json)"));
    }
    let new = sibling(exe, ".new");
    let rel = if entry.url.is_empty() { format!("patcher/{}", entry.path) } else { entry.url.clone() };
    crate::sync::get(cfg, list, &rel, &new, entry.size, &entry.sha256, ev)?;
    install(exe, &new)?;
    Ok(exe.to_path_buf())
}

/// Puts `new` in the place of the running program `exe` (the old file becomes `<name>.old`).
pub fn install(exe: &Path, new: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(new, std::fs::Permissions::from_mode(0o755)).map_err(|e| io_err(new, e))?;
    }
    let old = sibling(exe, ".old");
    let _ = std::fs::remove_file(&old);
    if exe.exists() {
        std::fs::rename(exe, &old).map_err(|e| io_err(exe, e))?;
    }
    if let Err(e) = std::fs::rename(new, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(io_err(exe, e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare() {
        assert!(newer("0.2.0", "0.1.9") && newer("1.0.0", "0.99.99") && newer("v0.10.0", "0.9.0"));
        assert!(!newer("0.2.0", "0.2.0") && !newer("0.1.0", "0.2.0") && !newer("garbage", "0.0.1"));
    }

    #[test]
    fn install_swaps_and_keeps_the_old_file() {
        let d = std::env::temp_dir().join(format!("otp-selfupdate-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let exe = d.join("openthor-patcher.exe");
        std::fs::write(&exe, b"old").unwrap();
        let new = sibling(&exe, ".new");
        std::fs::write(&new, b"new").unwrap();
        install(&exe, &new).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(sibling(&exe, ".old")).unwrap(), b"old");
        cleanup(&exe);
        assert!(!sibling(&exe, ".old").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
