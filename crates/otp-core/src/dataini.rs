//! `DATA.INI`: the client's GRF load order. The patcher rewrites only the `[Data]` section, in the order the server
//! gives, with optional packs switched per player; GRFs the player added themselves stay after the managed ones.

use crate::patchlist::Pack;
use crate::{io_err, Result};
use std::collections::BTreeMap;
use std::path::Path;

/// The GRF names of the `[Data]` section, in order (`N=name` lines).
pub fn read(path: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(path) else { return vec![] };
    let text = String::from_utf8_lossy(&bytes);
    let mut in_data = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_data = l.eq_ignore_ascii_case("[data]");
            continue;
        }
        if in_data {
            if let Some((k, v)) = l.split_once('=') {
                if k.trim().chars().all(|c| c.is_ascii_digit()) && !v.trim().is_empty() {
                    out.push(v.trim().to_string());
                }
            }
        }
    }
    out
}

/// Writes `[Data]` with `list`; other sections of the file are kept, line endings are CRLF like the original.
pub fn write(path: &Path, list: &[String]) -> Result<()> {
    let old = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let mut out = String::new();
    let mut skipping = false;
    let mut wrote = false;
    let emit = |out: &mut String| {
        out.push_str("[Data]\r\n");
        for (i, f) in list.iter().enumerate() {
            out.push_str(&format!("{i}={f}\r\n"));
        }
    };
    for line in old.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            skipping = l.eq_ignore_ascii_case("[data]");
            if skipping {
                if !wrote {
                    emit(&mut out);
                    wrote = true;
                }
                continue;
            }
        }
        if !skipping {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }
    if !wrote {
        emit(&mut out);
    }
    let tmp = path.with_extension("ini.otp-new");
    std::fs::write(&tmp, out).map_err(|e| io_err(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| io_err(path, e))
}

/// The final list: the server's `order` with `@pack:<id>` resolved by the player's choice, then the player's own
/// extra GRFs. `enabled` holds the player's pack choices (missing = the pack's default).
pub fn plan(order: &[String], packs: &[Pack], enabled: &BTreeMap<String, bool>, existing: &[String]) -> Vec<String> {
    let mut managed: Vec<String> = Vec::new();
    for item in order {
        if let Some(id) = item.strip_prefix("@pack:") {
            if let Some(p) = packs.iter().find(|p| p.id == id) {
                if p.locked || enabled.get(id).copied().unwrap_or(p.default) {
                    managed.push(p.file.clone());
                }
            }
        } else {
            managed.push(item.clone());
        }
    }
    let known = |f: &str| {
        managed.iter().any(|m| m.eq_ignore_ascii_case(f)) || packs.iter().any(|p| p.file.eq_ignore_ascii_case(f))
    };
    let extras: Vec<String> = existing.iter().filter(|f| !known(f)).cloned().collect();
    managed.extend(extras);
    managed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> Pack {
        Pack { id: "comp".into(), file: "odin-dark.grf".into(), default: true, ..Default::default() }
    }

    #[test]
    fn packs_follow_the_player_and_extras_stay() {
        let order = vec!["odin.grf".to_string(), "@pack:comp".into(), "data.grf".into()];
        let ex = vec!["data.grf".to_string(), "myskin.grf".into(), "odin-dark.grf".into()];
        let mut en = BTreeMap::new();
        assert_eq!(plan(&order, &[pack()], &en, &ex), vec!["odin.grf", "odin-dark.grf", "data.grf", "myskin.grf"]);
        en.insert("comp".to_string(), false);
        assert_eq!(plan(&order, &[pack()], &en, &ex), vec!["odin.grf", "data.grf", "myskin.grf"]);
        let mut locked = pack();
        locked.locked = true;
        assert_eq!(plan(&order, &[locked], &en, &ex)[1], "odin-dark.grf");
    }

    #[test]
    fn write_keeps_other_sections() {
        let d = std::env::temp_dir().join(format!("otp-ini-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("DATA.INI");
        std::fs::write(&p, "[Other]\r\nx=1\r\n[Data]\r\n0=a.grf\r\n1=b.grf\r\n[Tail]\r\ny=2\r\n").unwrap();
        write(&p, &["c.grf".to_string(), "a.grf".to_string()]).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.contains("[Other]\r\nx=1\r\n[Data]\r\n0=c.grf\r\n1=a.grf\r\n[Tail]\r\ny=2"));
        assert_eq!(read(&p), vec!["c.grf", "a.grf"]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
