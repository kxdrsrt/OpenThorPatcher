//! Downloads. `http(s)://` with resume (Range), `file://` (tests, local mirrors, a USB stick), cloud share links
//! (see [`crate::providers`]) and MEGA file links (see [`crate::mega`]). An optional bandwidth limit applies to all
//! downloads of the process together.

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
static RATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static BUCKET: Mutex<Option<(std::time::Instant, f64)>> = Mutex::new(None);

/// Bandwidth limit for all downloads together, in bytes per second; 0 = no limit.
pub fn set_rate_limit(bytes_per_sec: u64) {
    RATE.store(bytes_per_sec, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut b) = BUCKET.lock() {
        *b = None;
    }
}

/// Token bucket shared by every download thread: takes `n` bytes and sleeps while the bucket is in debt.
fn throttle(n: usize) {
    let rate = RATE.load(std::sync::atomic::Ordering::Relaxed) as f64;
    if rate <= 0.0 {
        return;
    }
    let wait = {
        let Ok(mut g) = BUCKET.lock() else { return };
        let now = std::time::Instant::now();
        let (last, tokens) = g.unwrap_or((now, rate));
        let tokens = (tokens + now.duration_since(last).as_secs_f64() * rate).min(rate) - n as f64;
        *g = Some((now, tokens));
        if tokens < 0.0 { -tokens / rate } else { 0.0 }
    };
    if wait > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(wait.min(5.0)));
    }
}
static PROXY: Mutex<Option<String>> = Mutex::new(None);

/// An explicit HTTP(S) proxy (`http://host:port`, `socks5://host:port`). Without one the system settings and the
/// `HTTP_PROXY` / `HTTPS_PROXY` variables apply.
pub fn set_proxy(p: Option<String>) {
    if let Ok(mut g) = PROXY.lock() {
        *g = p.filter(|x| !x.trim().is_empty());
    }
}

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

fn client(timeout: Option<std::time::Duration>) -> Result<reqwest::blocking::Client> {
    // redirects are followed by hand so a token is never sent to the host a redirect points to
    let mut b = reqwest::blocking::Client::builder()
        .user_agent(concat!("OpenThor-Patcher/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout);
    if let Some(p) = PROXY.lock().ok().and_then(|g| g.clone()) {
        b = b.proxy(reqwest::Proxy::all(p)?);
    }
    Ok(b.build()?)
}

/// How long a download may deliver no data at all before the source counts as stalled (the next one is tried).
const STALL: std::time::Duration = std::time::Duration::from_secs(45);

/// GET with the configured headers for the host, following up to 6 redirects (headers only for hosts that are listed).
fn send(url: &str, range_from: Option<u64>, timeout: Option<std::time::Duration>) -> Result<reqwest::blocking::Response> {
    let c = client(timeout)?;
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

/// MEGA: the temporary download address and the size of a file link (one API call).
fn mega_resolve(url: &str) -> Result<(crate::mega::Link, String, u64)> {
    let link = crate::mega::parse(url)?;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let api = format!("{}/cs?id={seq}", crate::mega::api_base().trim_end_matches('/'));
    let body = serde_json::json!([{ "a": "g", "g": 1, "ssl": 2, "p": link.id }]).to_string();
    let r = client(Some(std::time::Duration::from_secs(60)))?.post(&api).header("Content-Type", "application/json").body(body).send()?;
    if !r.status().is_success() {
        return Err(Error::Net(format!("{url}: MEGA API answered HTTP {}", r.status())));
    }
    let v: serde_json::Value = serde_json::from_slice(&r.bytes()?)?;
    let first = if v.is_array() { v[0].clone() } else { v };
    if let Some(code) = first.as_i64() {
        return Err(crate::mega::api_error(url, code));
    }
    if let Some(code) = first["e"].as_i64() {
        return Err(crate::mega::api_error(url, code));
    }
    let g = first["g"].as_str().filter(|g| g.starts_with("http")).ok_or_else(|| msg(format!("{url}: MEGA sent no download address")))?;
    Ok((link, g.to_string(), first["s"].as_u64().unwrap_or(0)))
}

pub fn get_bytes(url: &str) -> Result<Vec<u8>> {
    if let Some(p) = file_path(url) {
        return std::fs::read(&p).map_err(|e| missing(&p, e));
    }
    if crate::mega::is_mega(url) {
        let (link, real, _) = mega_resolve(url)?;
        let r = send(&real, None, Some(std::time::Duration::from_secs(60)))?;
        if !r.status().is_success() {
            return Err(status_err(url, r.status()));
        }
        let mut b = r.bytes()?.to_vec();
        crate::mega::Decryptor::new(&link).apply(0, &mut b);
        return Ok(b);
    }
    let real = resolve(url);
    let r = send(&real, None, Some(std::time::Duration::from_secs(60)))?;
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
        // MEGA: temporary address, resume as a "/start-end" suffix of it, decryption of every chunk
        let mega = if crate::mega::is_mega(url) { Some(mega_resolve(url)?) } else { None };
        let (resp, resumed) = match &mega {
            Some((_, g, msize)) => {
                let msize = if size > 0 { size } else { *msize };
                let from = if msize > 0 && have >= msize { 0 } else { have };
                let real = if from > 0 && msize > 0 { format!("{g}/{from}-{}", msize - 1) } else { g.clone() };
                let r = send(&real, None, None)?;
                let ok = r.status().is_success();
                (r, ok && from > 0)
            }
            None => {
                let r = send(&resolve(url), if have > 0 { Some(have) } else { None }, None)?;
                let resumed = r.status().as_u16() == 206;
                (r, resumed)
            }
        };
        let decrypt = mega.as_ref().map(|(l, _, _)| crate::mega::Decryptor::new(l));
        if mega.is_some() && resp.status().as_u16() == 509 {
            return Err(msg(format!("{url}: MEGA transfer quota exceeded; try later or use another source")));
        }
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
        // the body is read in a helper thread so that a source that goes silent can be given up on (and the cancel button
        // keeps working while we wait)
        let (tx, rx) = std::sync::mpsc::sync_channel::<std::io::Result<Vec<u8>>>(8);
        std::thread::spawn(move || {
            let mut resp = resp;
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                match resp.read(&mut buf) {
                    Ok(0) => {
                        let _ = tx.send(Ok(Vec::new()));
                        break;
                    }
                    Ok(n) => {
                        if tx.send(Ok(buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });
        let mut silent = std::time::Duration::ZERO;
        let tick = std::time::Duration::from_millis(500);
        loop {
            match rx.recv_timeout(tick) {
                Ok(Ok(chunk)) if chunk.is_empty() => break,
                Ok(Ok(mut chunk)) => {
                    silent = std::time::Duration::ZERO;
                    if let Some(d) = &decrypt {
                        d.apply(done, &mut chunk);
                    }
                    throttle(chunk.len());
                    out.write_all(&chunk).map_err(|e| io_err(&part, e))?;
                    done += chunk.len() as u64;
                    if !progress(done, total) {
                        return Err(Error::Cancelled);
                    }
                }
                Ok(Err(e)) => return Err(Error::Net(format!("{url}: {e}"))),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    silent += tick;
                    if !progress(done, total) {
                        return Err(Error::Cancelled);
                    }
                    if silent >= STALL {
                        return Err(Error::Net(format!("{url}: no data for {} s, the source is stalled", STALL.as_secs())));
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err(Error::Net(format!("{url}: the connection was closed early"))),
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
