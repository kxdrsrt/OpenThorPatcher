//! Starting the game (and its setup program) from the patcher.

use crate::{msg, safe_rel, Result};
use std::path::Path;
use std::process::Command;

/// Starts `exe` (relative to `dir`) with `args` and does not wait. On Windows a program that needs administrator
/// rights (error 740) is started through PowerShell with the UAC prompt.
pub fn start(dir: &Path, exe: &str, args: &str) -> Result<()> {
    let rel = safe_rel(exe)?;
    let full = dir.join(rel);
    if !full.is_file() {
        return Err(msg(format!("{} not found in the client folder", full.display())));
    }
    let list: Vec<&str> = args.split_whitespace().collect();
    match Command::new(&full).args(&list).current_dir(dir).spawn() {
        Ok(_) => Ok(()),
        Err(e) if cfg!(windows) && e.raw_os_error() == Some(740) => {
            let quoted = list.iter().map(|a| format!("'{}'", a.replace('\'', "''"))).collect::<Vec<_>>().join(",");
            let mut ps = format!("Start-Process -FilePath '{}' -WorkingDirectory '{}' -Verb RunAs", full.display(), dir.display());
            if !quoted.is_empty() {
                ps.push_str(&format!(" -ArgumentList {quoted}"));
            }
            Command::new("powershell").args(["-NoProfile", "-Command", &ps]).spawn().map(|_| ()).map_err(|e| crate::io_err(&full, e))
        }
        Err(e) => Err(crate::io_err(&full, e)),
    }
}
