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
static HELPER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Argument of the elevated helper: `<patcher> --otp-elevated-start <dir> <exe> <token> [game args...]`.
pub const HELPER_ARG: &str = "--otp-elevated-start";

/// Windows does not pass environment variables through the UAC prompt, so a game that needs administrator rights would
/// lose `OPENTHOR_LAUNCH`. With a helper set (the patcher itself), such a game is started by an elevated copy of the
/// helper ([`run_elevated_helper`]), which sets the variable and starts the game directly.
pub fn use_elevated_helper(program: PathBuf) {
    let _ = HELPER.set(program);
}

/// The helper part: call it first thing in `main`. Returns true when this process was started as the helper (it has
/// started the game; the caller should exit).
pub fn run_elevated_helper(args: &[String]) -> bool {
    let Some(i) = args.iter().position(|a| a == HELPER_ARG) else { return false };
    let rest = &args[i + 1..];
    if rest.len() >= 3 {
        let (dir, exe, token) = (PathBuf::from(&rest[0]), PathBuf::from(&rest[1]), &rest[2]);
        let token_ok = token.len() == 32 && token.chars().all(|c| c.is_ascii_hexdigit());
        if token_ok && exe.starts_with(&dir) && exe.is_file() {
            let _ = Command::new(&exe).args(&rest[3..]).current_dir(&dir).env(ENV, token).spawn();
        }
    }
    true
}

/// `"a b"` quoting for a Windows command line (paths with spaces; a trailing backslash would escape the quote).
fn win_quote(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '"']) {
        return a.to_string();
    }
    format!("\"{}\"", a.trim_end_matches('\\').replace('"', "\\\""))
}

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

/// Waits until the process `pid` has ended (at most `max`). A self-update starts the new patcher while the old one is still
/// closing; the new one must not meet the old one's single-instance lock, so it waits for it.
pub fn wait_pid_gone(pid: u32, max: Duration) {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let start = Instant::now();
    let mut s = System::new();
    while start.elapsed() < max {
        s.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
        if s.process(Pid::from_u32(pid)).is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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
    ensure_client_dirs(dir);
    let token = write_ticket(dir)?;
    spawn(dir, &full, args, Some(&token))?;
    if let Ok(mut g) = LAST.lock() {
        *g = Some((full, Instant::now()));
    }
    Ok(())
}

/// Folders the client writes to (chat window layout, screenshots, replays). Neither the sync nor the client package
/// carries them (they hold the player's own files), so a new player's client has none and the game ends with an error
/// ("SaveData\\ChatWndInfo_U.lua") when it saves its settings.
const CLIENT_DIRS: &[&str] = &["SaveData", "ScreenShot", "Replay"];

fn ensure_client_dirs(dir: &Path) {
    for d in CLIENT_DIRS {
        let p = dir.join(d);
        if !p.exists() {
            let _ = std::fs::create_dir_all(&p);
        }
    }
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
        Err(e) if cfg!(windows) && e.raw_os_error() == Some(740) && token.is_some() && HELPER.get().is_some() => {
            let (helper, t) = (HELPER.get().unwrap(), token.unwrap());
            let mut line = vec![HELPER_ARG.to_string(), win_quote(&dir.to_string_lossy()), win_quote(&full.to_string_lossy()), t.to_string()];
            line.extend(list.iter().map(|a| win_quote(a)));
            let ps = format!(
                "Start-Process -FilePath '{}' -Verb RunAs -ArgumentList '{}'",
                helper.display().to_string().replace('\'', "''"),
                line.join(" ").replace('\'', "''")
            );
            Command::new("powershell").args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps]).spawn().map(|_| ()).map_err(|e| crate::io_err(full, e))
        }
        Err(e) if cfg!(windows) && e.raw_os_error() == Some(740) => {
            let quoted = list.iter().map(|a| format!("'{}'", a.replace('\'', "''"))).collect::<Vec<_>>().join(",");
            // no helper: the game starts elevated, but without the launch variable (UAC does not pass it on)
            let mut ps = String::new();
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
    fn wait_pid_gone_returns_when_the_process_ended() {
        #[cfg(unix)]
        {
            let mut c = Command::new("sleep").arg("1").spawn().unwrap();
            let t = Instant::now();
            let pid = c.id();
            let h = std::thread::spawn(move || c.wait().unwrap());
            wait_pid_gone(pid, Duration::from_secs(10));
            h.join().unwrap();
            assert!(t.elapsed() < Duration::from_secs(5), "returns soon after the process ended");
        }
        // a pid that does not exist returns at once
        let t = Instant::now();
        wait_pid_gone(u32::MAX - 7, Duration::from_secs(10));
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn start_creates_the_folders_the_client_saves_to() {
        let d = std::env::temp_dir().join(format!("otp-dirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("SaveData")).unwrap();
        ensure_client_dirs(&d);
        assert!(d.join("SaveData").is_dir() && d.join("ScreenShot").is_dir() && d.join("Replay").is_dir());
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
        assert_eq!(names.iter().filter(|n| *n == "savedata").count(), 1, "an existing folder is not duplicated");
        let _ = std::fs::remove_dir_all(&d);
    }

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

    #[test]
    fn elevated_helper_starts_only_a_game_inside_the_folder_with_a_valid_token() {
        assert_eq!(win_quote("C:\\My Games\\RO\\"), "\"C:\\My Games\\RO\"");
        assert_eq!(win_quote("-1sak1"), "-1sak1");
        assert!(!run_elevated_helper(&["openthor.exe".into()]), "a normal start is not the helper");
        let d = std::env::temp_dir().join(format!("otp-helper-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let outside = std::env::temp_dir().join("otp-helper-outside.bin");
        let a = |exe: &std::path::Path, tok: &str| vec!["x".to_string(), HELPER_ARG.into(), d.display().to_string(), exe.display().to_string(), tok.into()];
        // handled (returns true) but nothing is started: exe outside the folder, bad token, missing file
        assert!(run_elevated_helper(&a(&outside, &"a".repeat(32))));
        assert!(run_elevated_helper(&a(&d.join("game.exe"), "not-hex")));
        let _ = std::fs::remove_dir_all(&d);
    }
}
