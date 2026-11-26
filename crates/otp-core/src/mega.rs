//! MEGA (mega.nz) public file links. A MEGA link is not a plain file URL: the file is stored encrypted (AES-128-CTR)
//! and the key is the part after `#`, which never reaches MEGA's servers. The patcher asks MEGA's API for a temporary
//! download address (`"a":"g"`), downloads the encrypted bytes and decrypts them on the fly. The SHA-256 from the signed
//! patch list is checked on the decrypted file as for every other source.
//!
//! Supported links: `https://mega.nz/file/<id>#<key>`, the old `https://mega.nz/#!<id>!<key>` (also `mega.co.nz`) and
//! the short form `mega:<id>#<key>`. Folder links are not supported (give every file its own link in the URL map).

use crate::{msg, Error, Result};
use aes::cipher::{BlockEncrypt, KeyInit};
use base64::Engine;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub id: String,
    pub key: [u8; 16],
    pub nonce: [u8; 8],
}

fn b64url(s: &str) -> Option<Vec<u8>> {
    let clean: String = s.trim().trim_end_matches('=').replace('+', "-").replace('/', "_");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(clean).ok()
}

/// Is this a MEGA link (file or folder)?
pub fn is_mega(url: &str) -> bool {
    url.starts_with("mega:") || ["mega.nz/", "mega.co.nz/", "mega.io/"].iter().any(|h| url.contains(h) && url.contains('#'))
}

/// Parses a MEGA file link. The 32-byte key of the link holds the AES key (first half XOR second half) and the CTR
/// nonce (bytes 16..24).
pub fn parse(url: &str) -> Result<Link> {
    let (id, key) = if let Some(rest) = url.strip_prefix("mega:") {
        rest.split_once('#').ok_or_else(|| msg(format!("{url}: MEGA link without key (#...)")))?
    } else if let Some(i) = url.find("/file/") {
        url[i + 6..].split_once('#').ok_or_else(|| msg(format!("{url}: MEGA link without key (#...)")))?
    } else if let Some(i) = url.find("#!") {
        url[i + 2..].split_once('!').ok_or_else(|| msg(format!("{url}: MEGA link without key")))?
    } else if url.contains("/folder/") || url.contains("#F!") {
        return Err(msg(format!("{url}: MEGA folder links are not supported, give every file its own file link")));
    } else {
        return Err(msg(format!("{url}: not a MEGA file link")));
    };
    let id = id.split(['?', '/']).next().unwrap_or("").to_string();
    let key = key.split(['/', '?']).next().unwrap_or("");
    let raw = b64url(key).ok_or_else(|| msg(format!("{url}: MEGA key is not valid base64")))?;
    if raw.len() != 32 || id.is_empty() {
        return Err(msg(format!("{url}: MEGA file key must be 32 bytes (a folder or decryption key link?)")));
    }
    let mut k = [0u8; 16];
    for i in 0..16 {
        k[i] = raw[i] ^ raw[i + 16];
    }
    let mut nonce = [0u8; 8];
    nonce.copy_from_slice(&raw[16..24]);
    Ok(Link { id, key: k, nonce })
}

/// MEGA's API host; `OTP_MEGA_API` replaces it (tests).
pub fn api_base() -> String {
    std::env::var("OTP_MEGA_API").unwrap_or_else(|_| "https://g.api.mega.co.nz".into())
}

/// Explains MEGA's negative API answers.
pub fn api_error(url: &str, code: i64) -> Error {
    match code {
        -9 | -2 => Error::NotFound(format!("{url} (MEGA: file not found or link removed)")),
        -11 => msg(format!("{url}: MEGA denied access")),
        -16 => msg(format!("{url}: MEGA: the file or its owner is blocked")),
        -17 => msg(format!("{url}: MEGA transfer quota exceeded; try later or use another source")),
        -3 | -4 | -18 => Error::Net(format!("{url}: MEGA is temporarily unavailable ({code})")),
        _ => msg(format!("{url}: MEGA API error {code}")),
    }
}

/// AES-128-CTR keystream as MEGA uses it: counter block = nonce (8 bytes) || block number (u64 big endian).
pub struct Decryptor {
    aes: aes::Aes128,
    nonce: [u8; 8],
}

impl Decryptor {
    pub fn new(l: &Link) -> Self {
        Self { aes: aes::Aes128::new(&l.key.into()), nonce: l.nonce }
    }

    fn keystream(&self, block: u64) -> [u8; 16] {
        let mut ctr = [0u8; 16];
        ctr[..8].copy_from_slice(&self.nonce);
        ctr[8..].copy_from_slice(&block.to_be_bytes());
        let mut ks = ctr.into();
        self.aes.encrypt_block(&mut ks);
        ks.into()
    }

    /// Decrypts (or encrypts) `data`, which starts at byte `offset` of the file.
    pub fn apply(&self, offset: u64, data: &mut [u8]) {
        let mut pos = 0usize;
        let mut off = offset;
        while pos < data.len() {
            let skip = (off % 16) as usize;
            let ks = self.keystream(off / 16);
            let n = (16 - skip).min(data.len() - pos);
            for i in 0..n {
                data[pos + i] ^= ks[skip + i];
            }
            pos += n;
            off += n as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_parsed() {
        // 32 key bytes 0..32: AES key = i ^ (i + 16), nonce = 16..24
        let raw: Vec<u8> = (0u8..32).collect();
        let key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&raw);
        let want = Link { id: "AbCd1234".into(), key: std::array::from_fn(|i| i as u8 ^ (i as u8 + 16)), nonce: [16, 17, 18, 19, 20, 21, 22, 23] };
        assert_eq!(parse(&format!("https://mega.nz/file/AbCd1234#{key}")).unwrap(), want);
        assert_eq!(parse(&format!("https://mega.nz/#!AbCd1234!{key}")).unwrap(), want);
        assert_eq!(parse(&format!("mega:AbCd1234#{key}")).unwrap(), want);
        assert!(parse("https://mega.nz/folder/AbCd#xyz").is_err());
        assert!(parse("https://mega.nz/file/AbCd#short").is_err());
        assert!(is_mega("https://mega.nz/file/x#y") && !is_mega("https://example.com/a#b"));
    }

    #[test]
    fn ctr_matches_nist_and_resumes_mid_block() {
        // NIST SP 800-38A F.5.1 (CTR-AES128): counter block f0f1f2f3f4f5f6f7 f8f9fafbfcfdfeff
        let key: [u8; 16] = hex::decode("2b7e151628aed2a6abf7158809cf4f3c").unwrap().try_into().unwrap();
        let d = Decryptor { aes: aes::Aes128::new(&key.into()), nonce: hex::decode("f0f1f2f3f4f5f6f7").unwrap().try_into().unwrap() };
        let start = u64::from_be_bytes(hex::decode("f8f9fafbfcfdfeff").unwrap().try_into().unwrap());
        let plain = hex::decode("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51").unwrap();
        let cipher: Vec<u8> = plain.iter().enumerate().map(|(i, b)| b ^ d.keystream(start + (i / 16) as u64)[i % 16]).collect();
        assert_eq!(hex::encode(&cipher), "874d6191b620e3261bef6864990db6ce9806f66b7970fdff8617187bb9fffdff");
        // decrypting in odd pieces gives the same bytes as in one go
        let plain: Vec<u8> = (0..100u8).collect();
        let mut whole = plain.clone();
        d.apply(5, &mut whole);
        let mut parts = plain.clone();
        let (a, b) = parts.split_at_mut(23);
        d.apply(5, a);
        d.apply(28, b);
        assert_eq!(whole, parts);
    }
}
