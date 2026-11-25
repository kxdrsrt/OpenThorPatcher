//! Downloads. `http(s)://` with resume (Range) and `file://` (tests, local mirrors, a USB stick).

use crate::{io_err, msg, sha256_file, Error, Result};
use std::io::{Read, Write};
use std::path::Path;

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("OpenThorPatcher/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(None)
        .build()?)
}

pub fn join(base: &str, rel: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), rel.trim_start_matches('/'))
}

fn file_path(url: &str) -> Option<std::path::PathBuf> {
    url.strip_prefix("file://").map(std::path::PathBuf::from)
}

pub fn get_bytes(url: &str) -> Result<Vec<u8>> {
    if let Some(p) = file_path(url) {
        return std::fs::read(&p).map_err(|e| io_err(&p, e));
    }
    let r = client()?.get(url).send()?;
    if !r.status().is_success() {
        return Err(msg(format!("{url}: HTTP {}", r.status())));
    }
    Ok(r.bytes()?.to_vec())
}

/// Downloads to `dest` through `dest.part`, resuming a partial file. The result is only moved to `dest` when size and
/// SHA-256 match; a wrong file never replaces anything. `progress(done, total)`.
pub fn download(url: &str, dest: &Path, size: u64, sha256: &str, progress: &mut dyn FnMut(u64, u64)) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
    }
    let part = dest.with_extension(format!("{}part", dest.extension().map(|e| format!("{}.", e.to_string_lossy())).unwrap_or_default()));
    if let Some(src) = file_path(url) {
        std::fs::copy(&src, &part).map_err(|e| io_err(&src, e))?;
        progress(size, size);
    } else {
        let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let have = if have > size { 0 } else { have };
        let mut req = client()?.get(url);
        if have > 0 {
            req = req.header("Range", format!("bytes={have}-"));
        }
        let mut resp = req.send()?;
        let resumed = resp.status().as_u16() == 206;
        if !resp.status().is_success() {
            return Err(msg(format!("{url}: HTTP {}", resp.status())));
        }
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(&part)
            .map_err(|e| io_err(&part, e))?;
        let mut done = if resumed { have } else { 0 };
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            let n = resp.read(&mut buf).map_err(|e| io_err(&part, e))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(|e| io_err(&part, e))?;
            done += n as u64;
            progress(done, size);
        }
        out.flush().map_err(|e| io_err(&part, e))?;
    }
    let got = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if got != size || !sha256_file(&part)?.eq_ignore_ascii_case(sha256) {
        let _ = std::fs::remove_file(&part);
        return Err(Error::Msg(format!("{}: size or SHA-256 differs from the patch list, the download was discarded", dest.display())));
    }
    std::fs::rename(&part, dest).map_err(|e| io_err(dest, e))?;
    Ok(())
}
