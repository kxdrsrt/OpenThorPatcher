//! Starting the game (and its setup program) from the patcher.
//!
//! **Launch ticket.** Every start through the patcher gets a one-time token: the patcher writes
//! `_patch/launch-<token>.ticket` in the client folder and starts the game with the environment variable
//! `OPENTHOR_LAUNCH=<token>`. A guard inside the game (OdinGuard's `launcher` module, or any other) can require both, so
//! the game only runs when it was started by the patcher, i.e. after the client was checked. The ticket is not a security
//! boundary (a player can write one by hand); it keeps players from skipping updates by accident.
//!
//! **Several clicks.** A second start of the same program within a few seconds is refused ("already starting"), and
//! `max_instances` limits how many copies of the game may run at the same time (0 = no limit).

use crate::{msg, safe_rel, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Environment variable with the launch token.
pub const ENV: &str = "OPENTHOR_LAUNCH";
/// How long a ticket stays valid for the guard (seconds since it was written).
pub const TICKET_SECS: u64 = 300;
/// A second click within this time does not start the game again.
pub const DEBOUNCE: Duration = Duration::from_secs(8);

static LAST: Mutex<Option<(PathBuf, Instant)>> = Mutex::new(None);

#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// The same program was started a moment ago.
    Starting,
    /// `max_instances` copies are running.
    Limit(usize),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::Starting => write!(f, "The game is starting, please wait."),
            Refused::Limit(1) => write!(f, "The game is already running. Only one game window is allowed."),
            Refused::Limit(n) => write!(f, "The game is already running {n} times. No more game windows are allowed."),
        }
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    let (a, b) = (a.canonicalize().unwrap_or_else(|_| a.to_path_buf()), b.canonicalize().unwrap_or_else(|_| b.to_path_buf()));
    if cfg!(windows) {
        a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

/// How many processes of this program file run right now (any user session of this computer that we may see).
pub fn running(exe: &Path) -> usize {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut s = System::new();
    s.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(UpdateKind::Always));
    s.processes().values().filter(|p| p.exe().is_some_and(|e| same_file(e, exe))).count()
}

/// May `exe` be started now? Checks the double-click guard and the instance limit.
pub fn may_start(exe: &Path, max_instances: u32) -> std::result::Result<(), Refused> {
    if let Ok(g) = LAST.lock() {
        if let Some((p, t)) = g.as_ref() {
            if same_file(p, exe) && t.elapsed() < DEBOUNCE {
                return Err(Refused::Starting);
            }
        }
    }
    if max_instances > 0 {
        let n = running(exe);
        if n >= max_instances as usize {
            return Err(Refused::Limit(n));
        }
    }
    Ok(())
}

fn new_token() -> String {
    use rand_core::RngCore;
    let mut b = [0u8; 16];
    rand_core::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

/// Writes a launch ticket for a start now and removes tickets older than [`TICKET_SECS`]. Returns the token.
pub fn write_ticket(dir: &Path) -> Result<String> {
    let pd = dir.join("_patch");
    std::fs::create_dir_all(&pd).map_err(|e| crate::io_err(&pd, e))?;
    for e in std::fs::read_dir(&pd).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let old = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|a| a.as_secs() > TICKET_SECS);
        if name.starts_with("launch-") && name.ends_with(".ticket") && old {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let token = new_token();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let p = pd.join(format!("launch-{token}.ticket"));
    std::fs::write(&p, format!("{{\"token\":\"{token}\",\"time\":{now}}}\n")).map_err(|e| crate::io_err(&p, e))?;
    Ok(token)
}

/// Starts `exe` (relative to `dir`) with `args` and a launch ticket, and does not wait. `max_instances` 0 = no limit.
/// On Windows a program that needs administrator rights (error 740) is started through PowerShell with the UAC prompt.
pub fn start_checked(dir: &Path, exe: &str, args: &str, max_instances: u32) -> Result<()> {
    let full = dir.join(safe_rel(exe)?);
    if !full.is_file() {
        return Err(msg(format!("{} not found in the client folder", full.display())));
    }
    may_start(&full, max_instances).map_err(|r| msg(r.to_string()))?;
    let token = write_ticket(dir)?;
    spawn(dir, &full, args, Some(&token))?;
    if let Ok(mut g) = LAST.lock() {
        *g = Some((full, Instant::now()));
    }
    Ok(())
}

/// Starts a program (setup) without ticket and limits.
pub fn start(dir: &Path, exe: &str, args: &str) -> Result<()> {
    let full = dir.join(safe_rel(exe)?);
    if !full.is_file() {
        return Err(msg(format!("{} not found in the client folder", full.display())));
    }
    spawn(dir, &full, args, None)
}

fn spawn(dir: &Path, full: &Path, args: &str, token: Option<&str>) -> Result<()> {
    let list: Vec<&str> = args.split_whitespace().collect();
    let mut c = Command::new(full);
    c.args(&list).current_dir(dir);
    if let Some(t) = token {
        c.env(ENV, t);
    }
    match c.spawn() {
        Ok(_) => Ok(()),
        Err(e) if cfg!(windows) && e.raw_os_error() == Some(740) => {
            let quoted = list.iter().map(|a| format!("'{}'", a.replace('\'', "''"))).collect::<Vec<_>>().join(",");
            // the elevated process inherits the environment of the PowerShell that starts it
            let mut ps = String::new();
            if let Some(t) = token {
                ps.push_str(&format!("$env:{ENV}='{t}'; "));
            }
            ps.push_str(&format!("Start-Process -FilePath '{}' -WorkingDirectory '{}' -Verb RunAs", full.display(), dir.display()));
            if !quoted.is_empty() {
                ps.push_str(&format!(" -ArgumentList {quoted}"));
            }
            Command::new("powershell").args(["-NoProfile", "-Command", &ps]).spawn().map(|_| ()).map_err(|e| crate::io_err(full, e))
        }
        Err(e) => Err(crate::io_err(full, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tickets_and_double_click() {
        let d = std::env::temp_dir().join(format!("otp-launch-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let t = write_ticket(&d).unwrap();
        let body = std::fs::read_to_string(d.join("_patch").join(format!("launch-{t}.ticket"))).unwrap();
        assert!(body.contains(&t) && t.len() == 32);
        assert_ne!(write_ticket(&d).unwrap(), t, "every start gets its own token");
        let exe = d.join("game.bin");
        std::fs::write(&exe, b"x").unwrap();
        *LAST.lock().unwrap() = Some((exe.clone(), Instant::now()));
        assert_eq!(may_start(&exe, 0), Err(Refused::Starting));
        *LAST.lock().unwrap() = Some((exe.clone(), Instant::now() - DEBOUNCE - Duration::from_secs(1)));
        assert_eq!(may_start(&exe, 0), Ok(()));
        // the test runner itself is running once
        let me = std::env::current_exe().unwrap();
        assert!(running(&me) >= 1);
        assert!(matches!(may_start(&me, 1), Err(Refused::Limit(_))));
        let _ = std::fs::remove_dir_all(&d);
    }
}
