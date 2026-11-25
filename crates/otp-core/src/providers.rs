//! Share links of cloud storage → direct download links. Static hosting (own server, S3/R2, GitHub Releases) needs none.
//!
//! Supported: Google Drive (`drive.google.com/file/d/ID/...`, `...?id=ID`, `gdrive:ID`), Dropbox (`dl=0` → `dl=1`),
//! OneDrive (`onedrive.live.com`). A link that returns a web page instead of the file is reported by the downloader.

fn drive_id(url: &str) -> Option<String> {
    if let Some(id) = url.strip_prefix("gdrive:") {
        return Some(id.trim().to_string());
    }
    if !(url.contains("drive.google.com") || url.contains("docs.google.com") || url.contains("drive.usercontent.google.com")) {
        return None;
    }
    if let Some(i) = url.find("/d/") {
        let rest = &url[i + 3..];
        let id: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        if !id.is_empty() {
            return Some(id);
        }
    }
    let q = url.split_once('?')?.1;
    q.split('&').find_map(|kv| kv.strip_prefix("id=")).map(|s| s.to_string())
}

pub fn resolve(url: &str) -> String {
    if let Some(id) = drive_id(url) {
        // the usercontent host serves large files without the "virus scan" page when confirm=t is given
        return format!("https://drive.usercontent.google.com/download?id={id}&export=download&confirm=t");
    }
    if url.contains("dropbox.com") {
        let (base, frag) = match url.split_once('#') {
            Some((b, f)) => (b.to_string(), format!("#{f}")),
            None => (url.to_string(), String::new()),
        };
        let out = if base.contains("dl=0") {
            base.replace("dl=0", "dl=1")
        } else if base.contains("dl=1") {
            base
        } else if base.contains('?') {
            format!("{base}&dl=1")
        } else {
            format!("{base}?dl=1")
        };
        return out + &frag;
    }
    if url.contains("onedrive.live.com") {
        let u = url.replace("/redir?", "/download?").replace("/embed?", "/download?");
        if !u.contains("download") {
            return if u.contains('?') { format!("{u}&download=1") } else { format!("{u}?download=1") };
        }
        return u;
    }
    url.to_string()
}

/// Is `s` a full link (not relative to a mirror)?
pub fn is_absolute(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("file://") || s.starts_with("gdrive:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_links() {
        let want = "https://drive.usercontent.google.com/download?id=1AbC-d_9&export=download&confirm=t";
        assert_eq!(resolve("https://drive.google.com/file/d/1AbC-d_9/view?usp=sharing"), want);
        assert_eq!(resolve("https://drive.google.com/open?id=1AbC-d_9"), want);
        assert_eq!(resolve("https://drive.google.com/uc?export=download&id=1AbC-d_9"), want);
        assert_eq!(resolve("gdrive:1AbC-d_9"), want);
    }

    #[test]
    fn dropbox_and_plain() {
        assert_eq!(resolve("https://www.dropbox.com/s/x/a.thor?dl=0"), "https://www.dropbox.com/s/x/a.thor?dl=1");
        assert_eq!(resolve("https://www.dropbox.com/s/x/a.thor"), "https://www.dropbox.com/s/x/a.thor?dl=1");
        assert_eq!(resolve("https://patch.example.com/a.thor"), "https://patch.example.com/a.thor");
        assert!(is_absolute("gdrive:abc") && !is_absolute("patches/a.thor"));
    }
}
