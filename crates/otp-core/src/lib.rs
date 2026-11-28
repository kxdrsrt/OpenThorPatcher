//! OpenThor Patcher core.
//!
//! * [`patchlist`]: the signed `patchlist.json` (whole files by hash, Thor patches by id)
//! * [`fetch`]: resumable download with SHA-256 check (`http(s)://` and `file://`)
//! * [`apply`]: merge a Thor patch into a GRF through RustyGRF, verify, swap atomically, keep a backup
//! * [`publish`]: build the patch site from a *master client* folder
//! * [`sign`]: Ed25519 signing of the patch list

pub mod apply;
pub mod dataini;
pub mod fetch;
pub mod keybackup;
pub mod launch;
pub mod mega;
pub mod midgard;
pub mod package;
pub mod patchlist;
pub mod project;
pub mod providers;
pub mod publish;
pub mod selfupdate;
pub mod sign;
pub mod sync;
pub mod upload;

use std::io::Read;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Msg(String),
    #[error("io: {0}: {1}")]
    Io(String, std::io::Error),
    #[error("grf: {0}")]
    Grf(#[from] rgrf_core::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    /// A source stopped answering (no data for a long time).
    #[error("{0}")]
    Net(String),
    /// The file does not exist on the server (HTTP 404, missing file://).
    #[error("not found: {0}")]
    NotFound(String),
    #[error("cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::Io(path.display().to_string(), e)
}

pub fn msg(s: impl Into<String>) -> Error {
    Error::Msg(s.into())
}

/// SHA-256 of a file, streamed (a 2.5 GB GRF needs a few seconds, not memory).
pub fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).map_err(|e| io_err(path, e))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| io_err(path, e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

pub fn sha256_bytes(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(b))
}

/// A path from a patch list or a Thor patch, made safe to join below a root: relative, no `..`, no drive letter.
pub fn safe_rel(p: &str) -> Result<std::path::PathBuf> {
    let norm = p.replace('\\', "/");
    let norm = norm.trim_start_matches("./");
    if norm.is_empty() || norm.starts_with('/') || norm.contains(':') {
        return Err(msg(format!("unsafe path in patch data: {p:?}")));
    }
    let mut out = std::path::PathBuf::new();
    for part in norm.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(msg(format!("unsafe path in patch data: {p:?}")));
        }
        out.push(part);
    }
    if out.as_os_str().is_empty() {
        return Err(msg(format!("unsafe path in patch data: {p:?}")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_rel_rejects_escapes() {
        assert!(safe_rel("../x").is_err());
        assert!(safe_rel("a/../../x").is_err());
        assert!(safe_rel("/etc/passwd").is_err());
        assert!(safe_rel("C:\\Windows\\x").is_err());
        assert!(safe_rel("").is_err());
        assert_eq!(safe_rel("data\\a\\b.txt").unwrap(), std::path::PathBuf::from("data/a/b.txt"));
    }
}
