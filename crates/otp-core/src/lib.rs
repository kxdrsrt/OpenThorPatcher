//! OpenThorPatcher core.
//!
//! * [`patchlist`]: the signed `patchlist.json` (whole files by hash, Thor patches by id)
//! * [`fetch`]: resumable download with SHA-256 check (`http(s)://` and `file://`)
//! * [`apply`]: merge a Thor patch into a GRF through RustyGRF, verify, swap atomically, keep a backup
//! * [`publish`]: build the patch site from a *master client* folder
//! * [`sign`]: Ed25519 signing of the patch list

pub mod apply;
pub mod fetch;
pub mod patchlist;
pub mod publish;
pub mod sign;
pub mod sync;

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
