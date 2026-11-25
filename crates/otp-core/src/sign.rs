//! Ed25519 signature of `patchlist.json` (`patchlist.json.sig`, hex).

use crate::{io_err, msg, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use std::path::Path;

/// Writes `signing.seed` (secret, never publish) and `patcher.pub` (ships with the patcher).
pub fn keygen(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    let seed_path = dir.join("signing.seed");
    if seed_path.exists() {
        return Err(msg(format!("{} exists, refusing to overwrite a signing key", seed_path.display())));
    }
    let key = SigningKey::generate(&mut rand_core::OsRng);
    std::fs::write(&seed_path, hex::encode(key.to_bytes())).map_err(|e| io_err(&seed_path, e))?;
    let pub_path = dir.join("patcher.pub");
    std::fs::write(&pub_path, hex::encode(key.verifying_key().to_bytes())).map_err(|e| io_err(&pub_path, e))?;
    Ok(())
}

fn read_hex32(path: &Path) -> Result<[u8; 32]> {
    let t = std::fs::read_to_string(path).map_err(|e| io_err(path, e))?;
    let v = hex::decode(t.trim()).map_err(|_| msg(format!("{}: not hex", path.display())))?;
    v.try_into().map_err(|_| msg(format!("{}: need 32 bytes", path.display())))
}

pub fn sign(seed_file: &Path, data: &[u8]) -> Result<String> {
    let key = SigningKey::from_bytes(&read_hex32(seed_file)?);
    Ok(hex::encode(key.sign(data).to_bytes()))
}

pub fn verify(pub_file: &Path, data: &[u8], sig_hex: &str) -> Result<()> {
    let key = VerifyingKey::from_bytes(&read_hex32(pub_file)?).map_err(|_| msg("public key invalid"))?;
    let raw = hex::decode(sig_hex.trim()).map_err(|_| msg("signature is not hex"))?;
    let sig = Signature::from_slice(&raw).map_err(|_| msg("signature has the wrong length"))?;
    key.verify(data, &sig).map_err(|_| msg("the patch list signature is INVALID (wrong server or tampered list)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_round_trip() {
        let d = std::env::temp_dir().join(format!("otp-sign-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        keygen(&d).unwrap();
        assert!(keygen(&d).is_err());
        let s = sign(&d.join("signing.seed"), b"list").unwrap();
        verify(&d.join("patcher.pub"), b"list", &s).unwrap();
        assert!(verify(&d.join("patcher.pub"), b"list2", &s).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
