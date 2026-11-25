//! Downloads. `http(s)://` with resume (Range), `file://` (tests, local mirrors, a USB stick), and cloud share links
//! (see [`crate::providers`]).

use crate::providers::{is_absolute, resolve};
use crate::{io_err, msg, sha256_file, Error, Result};
use std::io::{Read, Write};
use std::path::Path;

use std::sync::Mutex;

/// Request headers for private hosts (a GitHub/GitLab token, basic auth of a proxy), sent only to the listed hosts.
#[derive(Clone, Default)]
pub struct Auth {
    pub headers: Vec<(String, String)>,
    pub hosts: Vec<String>,
}

static AUTH: Mutex<Option<Auth>> = Mutex::new(None);

pub fn set_auth(a: Auth) {
    if let Ok(mut g) = AUTH.lock() {
        *g = Some(a);
    }
}

pub fn host_of(url: &str) -> String {
    url.split("://").nth(1).unwrap_or("").split(['/', '?', '#']).next().unwrap_or("").rsplit('@').next().unwrap_or("").split(':').next().unwrap_or("").to_ascii_lowercase()
}

fn auth_for(url: &str) -> Vec<(String, String)> {
    let host = host_of(url);
    match AUTH.lock().ok().and_then(|g| g.clone()) {
        Some(a) if a.hosts.iter().any(|h| h.eq_ignore_ascii_case(&host)) => a.headers,
        _ => vec![],
    }
}

fn client() -> Result<reqwest::blocking::Client> {
    // redirects are followed by hand so a token is never sent to the host a redirect points to
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("OpenThor-Patcher/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(None)
        .build()?)
}

/// GET with the configured headers for the host, following up to 6 redirects (headers only for hosts that are listed).
fn send(url: &str, range_from: Option<u64>) -> Result<reqwest::blocking::Response> {
    let c = client()?;
    let mut cur = url.to_string();
    for _ in 0..7 {
        let mut req = c.get(&cur);
        for (k, v) in auth_for(&cur) {
            req = req.header(k, v);
        }
        if let Some(n) = range_from {
            req = req.header("Range", format!("bytes={n}-"));
        }
        let r = req.send()?;
        if r.status().is_redirection() {
            let Some(loc) = r.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()) else { return Ok(r) };
            cur = if loc.starts_with("http") { loc.to_string() } else { format!("{}://{}{}", cur.split("://").next().unwrap_or("https"), host_of(&cur), loc) };
            continue;
        }
        return Ok(r);
    }
    Err(msg(format!("{url}: too many redirects")))
}

pub fn join(base: &str, rel: &str) -> String {
    if is_absolute(rel) {
        return rel.to_string();
    }
    format!("{}/{}", base.trim_end_matches('/'), rel.trim_start_matches('/'))
}

fn file_path(url: &str) -> Option<std::path::PathBuf> {
    url.strip_prefix("file://").map(std::path::PathBuf::from)
}

fn is_html(resp: &reqwest::blocking::Response) -> bool {
    resp.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().contains("text/html"))
}

fn status_err(url: &str, st: reqwest::StatusCode) -> Error {
    if st.as_u16() == 404 {
        Error::NotFound(url.to_string())
    } else if st.as_u16() == 401 || st.as_u16() == 403 {
        msg(format!("{url}: HTTP {st}: access denied (missing or wrong token for a private host?)"))
    } else {
        msg(format!("{url}: HTTP {st}"))
    }
}

fn missing(p: &Path, e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        Error::NotFound(p.display().to_string())
    } else {
        io_err(p, e)
    }
}

pub fn get_bytes(url: &str) -> Result<Vec<u8>> {
    if let Some(p) = file_path(url) {
        return std::fs::read(&p).map_err(|e| missing(&p, e));
    }
    let real = resolve(url);
    let r = send(&real, None)?;
    if !r.status().is_success() {
        return Err(status_err(url, r.status()));
    }
    Ok(r.bytes()?.to_vec())
}

/// Downloads to `dest` through `dest.part`, resuming a partial file. The result is only moved to `dest` when size and
/// SHA-256 match; a wrong file never replaces anything. `progress(done, total)` returns `false` to cancel.
/// `size` 0 / empty `sha256` skip the checks (classic Thor Patcher lists carry neither).
pub fn download(url: &str, dest: &Path, size: u64, sha256: &str, progress: &mut dyn FnMut(u64, u64) -> bool) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
    }
    let mut part_name = dest.file_name().unwrap_or_default().to_os_string();
    part_name.push(".part");
    let part = dest.with_file_name(part_name);
    if let Some(src) = file_path(url) {
        std::fs::copy(&src, &part).map_err(|e| missing(&src, e))?;
        let n = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if !progress(n, n) {
            return Err(Error::Cancelled);
        }
    } else {
        let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let have = if size > 0 && have > size { 0 } else { have };
        let real = resolve(url);
        let mut resp = send(&real, if have > 0 { Some(have) } else { None })?;
        let resumed = resp.status().as_u16() == 206;
        if resp.status().as_u16() == 416 {
            // the partial file is already complete (or wrong): start over once
            let _ = std::fs::remove_file(&part);
            return download(url, dest, size, sha256, progress);
        }
        if !resp.status().is_success() {
            return Err(status_err(url, resp.status()));
        }
        if is_html(&resp) {
            return Err(msg(format!(
                "{url}: the server sent a web page instead of the file (link not public, sign-in required or download quota exceeded)"
            )));
        }
        let total = if size > 0 { size } else { resp.content_length().map(|l| l + if resumed { have } else { 0 }).unwrap_or(0) };
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
            if !progress(done, total) {
                return Err(Error::Cancelled);
            }
        }
        out.flush().map_err(|e| io_err(&part, e))?;
    }
    let got = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let size_bad = size > 0 && got != size;
    let hash_bad = !sha256.is_empty() && !sha256_file(&part)?.eq_ignore_ascii_case(sha256);
    if size_bad || hash_bad {
        let _ = std::fs::remove_file(&part);
        return Err(Error::Msg(format!("{}: size or SHA-256 differs from the patch list, the download was discarded", dest.display())));
    }
    std::fs::rename(&part, dest).map_err(|e| io_err(dest, e))?;
    Ok(())
}
