//! One encrypted backup file for the signing key of a project ("Back up key" in the Patch Maker, `otp keys-backup`).
//!
//! File: `OTPKEYS1\n` | salt (16) | nonce (24) | XChaCha20-Poly1305 ciphertext of a JSON document. The key is derived
//! from the password with Argon2id (64 MiB, 3 passes, 1 lane); the magic is the authenticated header. A wrong password
//! and a damaged file give the same message (an AEAD cannot tell them apart). Same design as OdinGuard Studio's backup.

use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand_core::RngCore;
use serde::{Deserialize, Serialize};

use crate::project::Project;
use crate::{io_err, msg, Result};

const MAGIC: &[u8; 9] = b"OTPKEYS1\n";
const MIN_PASSWORD: usize = 8;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KeyFile {
    /// `signing_seed`, `patcher_pub`, or `extra:<file name>`
    pub role: String,
    pub name: String,
    /// file content, hex
    pub data: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Bundle {
    pub product: String,
    pub server: String,
    pub created: u64,
    pub files: Vec<KeyFile>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Restored {
    pub path: String,
    /// `written`, `same` (already identical), `kept` (differs and no force)
    pub action: String,
}

fn derive(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let params = Params::new(64 * 1024, 3, 1, Some(32)).map_err(|e| msg(e.to_string()))?;
    let mut key = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params).hash_password_into(password.as_bytes(), salt, &mut key).map_err(|e| msg(e.to_string()))?;
    Ok(key)
}

pub fn check_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD {
        return Err(msg(format!("the password needs at least {MIN_PASSWORD} characters")));
    }
    Ok(())
}

pub fn seal(bundle: &Bundle, password: &str) -> Result<Vec<u8>> {
    check_password(password)?;
    let (mut salt, mut nonce) = ([0u8; 16], [0u8; 24]);
    rand_core::OsRng.fill_bytes(&mut salt);
    rand_core::OsRng.fill_bytes(&mut nonce);
    let key = derive(password, &salt)?;
    let plain = serde_json::to_vec(bundle)?;
    let ct = XChaCha20Poly1305::new((&key).into()).encrypt(XNonce::from_slice(&nonce), Payload { msg: &plain, aad: MAGIC }).map_err(|_| msg("encryption failed"))?;
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn open(bytes: &[u8], password: &str) -> Result<Bundle> {
    if bytes.len() < MAGIC.len() + 16 + 24 + 16 || &bytes[..MAGIC.len()] != MAGIC {
        return Err(msg("this is not an OpenThor key backup file"));
    }
    let (salt, rest) = bytes[MAGIC.len()..].split_at(16);
    let (nonce, ct) = rest.split_at(24);
    let key = derive(password, salt)?;
    let plain = XChaCha20Poly1305::new((&key).into()).decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad: MAGIC }).map_err(|_| msg("wrong password or damaged file"))?;
    serde_json::from_slice(&plain).map_err(|_| msg("the backup content is not readable"))
}

fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| io_err(path, e))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The signing key of the project, its public key next to it (when present), and `extra` files (e.g. an SFTP key).
pub fn collect(project_path: &Path, extra: &[PathBuf]) -> Result<Bundle> {
    let p = Project::load(project_path)?;
    let seed = p.signing_seed.clone().ok_or_else(|| msg("this project has no signing key yet (Create key pair first)"))?;
    let mut files = vec![KeyFile { role: "signing_seed".into(), name: file_name(&seed), data: hex::encode(read(&seed)?) }];
    let pubf = seed.parent().unwrap_or(Path::new(".")).join("patcher.pub");
    if pubf.is_file() {
        files.push(KeyFile { role: "patcher_pub".into(), name: "patcher.pub".into(), data: hex::encode(read(&pubf)?) });
    }
    for path in extra {
        let name = file_name(path);
        if name.is_empty() {
            return Err(msg("extra file without a name"));
        }
        files.push(KeyFile { role: format!("extra:{name}"), name, data: hex::encode(read(path)?) });
    }
    let created = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    Ok(Bundle { product: "OpenThor Patcher".into(), server: p.server.clone(), created, files })
}

pub fn backup(project_path: &Path, extra: &[PathBuf], out: &Path, password: &str) -> Result<usize> {
    let bundle = collect(project_path, extra)?;
    let bytes = seal(&bundle, password)?;
    // only a file that can be opened again counts
    if open(&bytes, password)?.files.len() != bundle.files.len() {
        return Err(msg("the backup could not be read back"));
    }
    std::fs::write(out, bytes).map_err(|e| io_err(out, e))?;
    Ok(bundle.files.len())
}

fn write_secret(dest: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))?;
    }
    std::fs::write(dest, data).map_err(|e| io_err(dest, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Puts the files back. The signing key goes where the project points at; a project without a key gets
/// `<project folder>/keys/signing.seed` and the project file is updated. Existing different files are kept unless `force`.
pub fn restore(bundle: &Bundle, project_path: &Path, extra_dir: Option<&Path>, force: bool) -> Result<Vec<Restored>> {
    let mut p = Project::load(project_path)?;
    let base = project_path.parent().unwrap_or(Path::new("."));
    let (seed, new_seed) = match &p.signing_seed {
        Some(s) => (s.clone(), false),
        None => (base.join("keys").join("signing.seed"), true),
    };
    // resolve everything first: a backup that cannot be restored completely must not be restored halfway
    let mut plan = Vec::new();
    for f in &bundle.files {
        let data = hex::decode(&f.data).map_err(|_| msg("the backup content is not readable"))?;
        let dest = match f.role.as_str() {
            "signing_seed" => seed.clone(),
            "patcher_pub" => seed.parent().unwrap_or(Path::new(".")).join("patcher.pub"),
            r if r.starts_with("extra:") => {
                let dir = extra_dir.ok_or_else(|| msg(format!("the backup holds {} (an extra file): say where it goes", f.name)))?;
                dir.join(Path::new(&f.name).file_name().ok_or_else(|| msg("bad file name in the backup"))?)
            }
            other => return Err(msg(format!("unknown entry {other} in the backup"))),
        };
        plan.push((dest, data));
    }
    if new_seed {
        p.signing_seed = Some(seed.clone());
        p.save(project_path)?;
    }
    let mut out = Vec::new();
    for (dest, data) in plan {
        let action = match std::fs::read(&dest) {
            Ok(cur) if cur == data => "same",
            Ok(_) if !force => "kept",
            _ => {
                write_secret(&dest, &data)?;
                "written"
            }
        };
        out.push(Restored { path: dest.display().to_string(), action: action.into() });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_in(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        crate::sign::keygen(&dir.join("keys")).unwrap();
        let mut p = Project::new("Test", dir.join("client"), dir.join("site"));
        p.signing_seed = Some(dir.join("keys/signing.seed"));
        let path = dir.join("otp-project.json");
        p.save(&path).unwrap();
        path
    }

    #[test]
    fn backup_and_restore_roundtrip() {
        let root = std::env::temp_dir().join(format!("otp-kb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let a = root.join("a");
        let prof = project_in(&a);
        let extra = a.join("sftp_key");
        std::fs::write(&extra, "extra secret\n").unwrap();
        let out = a.join("k.otpkeys");
        assert_eq!(backup(&prof, std::slice::from_ref(&extra), &out, "correct horse").unwrap(), 3);
        let raw = std::fs::read(&out).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("extra secret"), "the file must be encrypted");
        assert!(open(&raw, "wrong password").unwrap_err().to_string().contains("wrong password"));
        assert!(open(b"hello", "correct horse").unwrap_err().to_string().contains("not an OpenThor key backup"));
        let mut damaged = raw.clone();
        *damaged.last_mut().unwrap() ^= 1;
        assert!(open(&damaged, "correct horse").is_err());
        assert!(backup(&prof, &[], &a.join("x"), "short").is_err());

        // a new machine: a project without a key
        let b = root.join("b");
        std::fs::create_dir_all(&b).unwrap();
        let prof_b = b.join("otp-project.json");
        Project::new("Test", b.join("client"), b.join("site")).save(&prof_b).unwrap();
        let bundle = open(&raw, "correct horse").unwrap();
        assert!(restore(&bundle, &prof_b, None, false).is_err(), "an extra file needs a folder");
        assert!(!b.join("keys").exists() && Project::load(&prof_b).unwrap().signing_seed.is_none(), "a restore that fails writes nothing");
        let done = restore(&bundle, &prof_b, Some(&b), false).unwrap();
        assert!(done.iter().all(|r| r.action == "written"));
        assert_eq!(std::fs::read(a.join("keys/signing.seed")).unwrap(), std::fs::read(b.join("keys/signing.seed")).unwrap());
        assert_eq!(std::fs::read(a.join("keys/patcher.pub")).unwrap(), std::fs::read(b.join("keys/patcher.pub")).unwrap());
        assert!(Project::load(&prof_b).unwrap().signing_seed.is_some(), "the project now points at the restored key");
        // the restored key signs what the original public key verifies
        let sig = crate::sign::sign(&b.join("keys/signing.seed"), b"list").unwrap();
        crate::sign::verify(&a.join("keys/patcher.pub"), b"list", &sig).unwrap();

        // a different key is kept unless forced
        std::fs::write(b.join("keys/signing.seed"), "other\n").unwrap();
        let done = restore(&bundle, &prof_b, Some(&b), false).unwrap();
        assert_eq!(done.iter().find(|r| r.path.ends_with("signing.seed")).unwrap().action, "kept");
        restore(&bundle, &prof_b, Some(&b), true).unwrap();
        assert_eq!(std::fs::read(a.join("keys/signing.seed")).unwrap(), std::fs::read(b.join("keys/signing.seed")).unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }
}
