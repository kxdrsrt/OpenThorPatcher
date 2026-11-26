//! The client package for new players (`otp package`): a ZIP of the master client exactly as it was published, with
//! the patcher and its `patcher.json`. External files (`data.grf` on MEGA or in a release) are left out by default:
//! the patcher downloads them on the first start. The package is checked against the published patch list first, so
//! a player who unpacks it and starts the patcher has nothing (or only the external files) to download.

use crate::patchlist::PatchList;
use crate::project::Project;
use crate::{io_err, msg, sha256_file, Result};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct Options {
    /// The ZIP to write.
    pub out: PathBuf,
    /// The patcher program to put next to the game; it is always named `openthor.exe` in the package.
    pub patcher: Option<PathBuf>,
    /// Content of `patcher.json` for the players.
    pub patcher_json: Option<String>,
    /// Also pack the external files (one big download instead of a smaller package plus the patcher's download).
    pub include_external: bool,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub out: PathBuf,
    pub files: usize,
    pub size: u64,
    pub sha256: String,
    /// External files left out (the patcher downloads them).
    pub left_out: Vec<String>,
    pub warnings: Vec<String>,
}

const SKIP_DIRS: &[&str] = &["_patch", "screenshot", "replay", "savedata", "memo", "_tmpemblem", "emblem", "_backup", "backup"];

fn skip_file(lower: &str) -> bool {
    [".part", ".bak", ".log", ".tmp", ".old", ".new"].iter().any(|e| lower.ends_with(e)) || lower == ".ds_store" || lower == "thumbs.db"
}

fn stored(lower: &str) -> bool {
    ["grf", "gpf", "mp3", "ogg", "wav", "bik", "zip", "7z", "rar", "jpg", "png"].contains(&lower.rsplit('.').next().unwrap_or(""))
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let mut ents: Vec<_> = std::fs::read_dir(dir).map_err(|e| io_err(dir, e))?.flatten().collect();
    ents.sort_by_key(|e| e.file_name());
    for e in ents {
        let p = e.path();
        let lower = e.file_name().to_string_lossy().to_ascii_lowercase();
        if p.is_dir() {
            if dir == root && SKIP_DIRS.iter().any(|d| lower.starts_with(d)) {
                continue;
            }
            walk(root, &p, out)?;
        } else if !skip_file(&lower) {
            let rel = p.strip_prefix(root).unwrap_or(&p).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
            out.push((rel, p));
        }
    }
    Ok(())
}

/// Checks that the master client is the published state: every file of the list has its hash, every patched GRF is
/// the version the last publish saw.
fn check_published(p: &Project, list: &PatchList, include_external: bool) -> Result<()> {
    let mut stale = Vec::new();
    for f in &list.files {
        if p.external.contains_key(&f.path) && !include_external {
            continue;
        }
        let src = p.client.join(&f.path);
        if !src.is_file() || std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0) != f.size || !sha256_file(&src)?.eq_ignore_ascii_case(&f.sha256) {
            stale.push(f.path.clone());
        }
    }
    for (name, t) in &list.targets {
        let src = p.client.join(name);
        let want = if t.current_sha256.is_empty() { &t.baseline_sha256 } else { &t.current_sha256 };
        if !src.is_file() || !sha256_file(&src)?.eq_ignore_ascii_case(want) {
            stale.push(name.clone());
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(msg(format!("the master client changed since the last publish ({}); publish first, then package", stale.join(", "))))
    }
}

type Zip = zip::ZipWriter<std::io::BufWriter<std::fs::File>>;

fn zerr(e: zip::result::ZipError) -> crate::Error {
    msg(format!("zip: {e}"))
}

fn add(z: &mut Zip, name: &str, src: Option<&Path>, data: Option<&[u8]>) -> Result<()> {
    let method = if stored(&name.to_ascii_lowercase()) { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated };
    z.start_file(name, zip::write::SimpleFileOptions::default().compression_method(method).large_file(true)).map_err(zerr)?;
    match (src, data) {
        (Some(s), _) => {
            let mut r = std::fs::File::open(s).map_err(|e| io_err(s, e))?;
            std::io::copy(&mut r, z).map_err(|e| io_err(s, e))?;
        }
        (None, Some(d)) => z.write_all(d).map_err(|e| msg(format!("zip: {e}")))?,
        _ => {}
    }
    Ok(())
}

pub fn package(p: &Project, o: &Options, log: &mut dyn FnMut(&str)) -> Result<Report> {
    let list_path = p.site.join("patchlist.json");
    let list = PatchList::parse(&std::fs::read(&list_path).map_err(|_| msg("no patch list yet: publish first, then package"))?)?;
    log("checking the master client against the published patch list ...");
    check_published(p, &list, o.include_external)?;
    // exactly what the patch list manages: synced files, patched GRFs, and (on request) external files
    let mut left_out = Vec::new();
    let mut managed: Vec<String> = Vec::new();
    for f in &list.files {
        if p.external.contains_key(&f.path) && !o.include_external {
            left_out.push(f.path.clone());
        } else if !(list.patcher.as_ref().is_some_and(|r| r.files.values().any(|x| x.path == f.path))) {
            managed.push(f.path.clone());
        }
    }
    managed.extend(list.targets.keys().cloned());
    let mut warnings = Vec::new();
    let mut all = Vec::new();
    walk(&p.client, &p.client, &mut all)?;
    let lower: Vec<String> = managed.iter().chain(left_out.iter()).map(|m| m.to_ascii_lowercase()).collect();
    let unmanaged: Vec<String> = all
        .iter()
        .map(|(r, _)| r.clone())
        .filter(|r| !lower.contains(&r.to_ascii_lowercase()) && !r.eq_ignore_ascii_case("patcher.json") && !r.to_ascii_lowercase().starts_with("openthor"))
        .collect();
    if !unmanaged.is_empty() {
        let show: Vec<&str> = unmanaged.iter().take(5).map(|s| s.as_str()).collect();
        warnings.push(format!(
            "{} file(s) of the master client are not in the patch list and not packed (add them to sync if players need them): {}{}",
            unmanaged.len(),
            show.join(", "),
            if unmanaged.len() > 5 { ", …" } else { "" }
        ));
    }
    let files: Vec<(String, PathBuf)> = managed.iter().map(|m| (m.clone(), p.client.join(m))).collect();
    // the patcher is always `openthor.exe` in the client folder, whatever the file on the operator's disk is called
    let patcher_name = o.patcher.as_ref().map(|x| if x.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) { "openthor.exe".to_string() } else { "openthor".to_string() });
    if o.patcher.is_none() && !all.iter().any(|(r, _)| r.to_ascii_lowercase().starts_with("openthor")) {
        warnings.push("the package has no patcher program (give --patcher)".into());
    }
    if o.patcher_json.is_none() && !files.iter().any(|(r, _)| r.eq_ignore_ascii_case("patcher.json")) {
        warnings.push("the package has no patcher.json (give --patcher-json)".into());
    }
    if let Some(j) = &o.patcher_json {
        serde_json::from_str::<serde_json::Value>(j).map_err(|e| msg(format!("patcher.json is not valid JSON: {e}")))?;
    }
    if let Some(d) = o.out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
    }
    let tmp = o.out.with_extension("zip.part");
    let f = std::fs::File::create(&tmp).map_err(|e| io_err(&tmp, e))?;
    let mut z = zip::ZipWriter::new(std::io::BufWriter::new(f));
    let mut count = 0usize;
    for (rel, src) in &files {
        let replaced = patcher_name.as_deref().is_some_and(|n| rel.eq_ignore_ascii_case(n)) || (o.patcher_json.is_some() && rel.eq_ignore_ascii_case("patcher.json"));
        if !replaced {
            add(&mut z, rel, Some(src), None)?;
            count += 1;
        }
    }
    if let (Some(exe), Some(name)) = (&o.patcher, &patcher_name) {
        add(&mut z, name, Some(exe), None)?;
        count += 1;
    }
    if let Some(j) = &o.patcher_json {
        add(&mut z, "patcher.json", None, Some(j.as_bytes()))?;
        count += 1;
    }
    let w = z.finish().map_err(zerr)?;
    w.into_inner().map_err(|e| msg(format!("zip: {e}")))?.sync_all().map_err(|e| io_err(&tmp, e))?;
    std::fs::rename(&tmp, &o.out).map_err(|e| io_err(&o.out, e))?;
    let size = std::fs::metadata(&o.out).map_err(|e| io_err(&o.out, e))?.len();
    if size > 2_000 << 20 {
        warnings.push(format!("the package is {} MB: GitHub release files may be at most 2 GB (leave big files external)", size >> 20));
    }
    log(&format!("{}: {count} files, {} MB", o.out.display(), size >> 20));
    Ok(Report { out: o.out.clone(), files: count, size, sha256: sha256_file(&o.out)?, left_out, warnings })
}
