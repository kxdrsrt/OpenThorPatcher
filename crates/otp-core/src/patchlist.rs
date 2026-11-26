//! `patchlist.json`: what the server publishes. See docs/FORMATS.md.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA: u32 = 1;

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PatchList {
    pub schema: u32,
    pub server: String,
    /// UTC time of the last publish (informational).
    #[serde(default)]
    pub updated: String,
    /// Grows with every publish that changes something. A client takes the newest valid list of all the sources it knows
    /// and never accepts one with a lower number than the last list it applied (a stale mirror, a rollback). Lists made
    /// before this field existed have 0.
    #[serde(default)]
    pub serial: u64,
    /// Not part of the file: the patch site folder this list was read from (set by the patcher).
    #[serde(skip)]
    pub source: String,
    /// `false` = the operator closed patching (maintenance); `message` says why.
    #[serde(default = "yes")]
    pub allow_patching: bool,
    #[serde(default)]
    pub message: String,
    /// Short text shown in the patcher window.
    #[serde(default)]
    pub notice: String,
    /// Web page shown in the patcher window (news).
    #[serde(default)]
    pub news_url: String,
    /// Mirrors; a relative `url` is appended to the first one that works.
    pub base_url: Vec<String>,
    /// Whole files kept equal to the master by hash (small GRFs, dll, exe).
    #[serde(default)]
    pub files: Vec<FileEntry>,
    /// Big GRFs that are patched entry by entry with Thor patches.
    #[serde(default)]
    pub targets: BTreeMap<String, Target>,
    /// Thor patches in the order they must be applied (ids grow).
    #[serde(default)]
    pub patches: Vec<PatchEntry>,
    /// Paths deleted from the client folder (`dir/*` removes a folder). Entries inside GRFs are removed by Thor patches.
    #[serde(default)]
    pub remove: Vec<String>,
    #[serde(default)]
    pub client: Option<ClientCfg>,
    #[serde(default)]
    pub data_ini: Option<DataIniCfg>,
    /// The server's website: the patcher's *Website* button opens it in the player's browser.
    #[serde(default)]
    pub website: String,
    /// Servers whose reachability the patcher shows at the top left (login, char, map).
    #[serde(default)]
    pub servers: Vec<ServerEntry>,
    /// More links (Discord, forum, vote ...), shown at the bottom of the patcher window.
    #[serde(default)]
    pub links: Vec<LinkItem>,
    /// News shown in the patcher's News tab (newest first, written by the operator).
    #[serde(default)]
    pub news: Vec<NewsItem>,
    /// Changelog shown in the patcher's Changelog tab (newest first).
    #[serde(default)]
    pub changelog: Vec<ChangeEntry>,
    /// Newest patcher program; a patcher with an older version replaces itself before it patches (only with a signed
    /// list).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patcher: Option<PatcherRelease>,
    /// JSON status of a web panel (MidgardCP `/api/v1/status`): the patcher shows the realm's state and players online.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status_url: String,
    /// Realm id in that status (default: the first realm).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub status_realm: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PatcherRelease {
    /// `major.minor.patch`
    pub version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Platform (`windows-x64`, `macos-arm64`, `linux-x64`) → the program file.
    pub files: BTreeMap<String, FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerEntry {
    pub name: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinkItem {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct NewsItem {
    /// Free text, usually `YYYY-MM-DD`.
    #[serde(default)]
    pub date: String,
    pub title: String,
    /// Plain text with a tiny markup: blank line = paragraph, `- ` = list item, `**bold**`.
    #[serde(default)]
    pub body: String,
    /// Language of this item (`de`, `en`, ...). Empty = for everybody. When items carry languages, the patcher shows the
    /// player's language, else English, else the first language there is.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lang: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChangeEntry {
    #[serde(default)]
    pub date: String,
    /// Version or patch label, for example `2026-11-25` or `Patch 12`.
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub title: String,
    /// One line per change.
    #[serde(default)]
    pub items: Vec<String>,
    /// Language of this entry, as for news.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lang: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientCfg {
    /// Game executable, relative to the client folder.
    #[serde(default)]
    pub exe: String,
    #[serde(default)]
    pub args: String,
    /// Setup program (resolution, sound), relative to the client folder.
    #[serde(default)]
    pub setup_exe: String,
    /// How many copies of the game the Play button may run at the same time (0 = no limit). A second click within a few
    /// seconds never starts a second copy.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_instances: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// The patcher owns the `[Data]` list of `DATA.INI`. `order` may contain `@pack:<id>`, replaced by the pack's GRF
/// only when the player has the pack switched on (or the server locks it).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DataIniCfg {
    pub order: Vec<String>,
    #[serde(default)]
    pub packs: Vec<Pack>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Pack {
    pub id: String,
    pub file: String,
    #[serde(default)]
    pub title: String,
    /// The title in other languages (`{"de": "Darkworld-Karten"}`); `title` is the fallback.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub titles: std::collections::BTreeMap<String, String>,
    /// Switched on for a new player.
    #[serde(default)]
    pub default: bool,
    /// The player cannot switch it off.
    #[serde(default)]
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileEntry {
    /// Path inside the client folder, `/` separated.
    pub path: String,
    pub size: u64,
    pub sha256: String,
    /// Relative to a base URL or a full link (cloud share link); default `files/<path>`.
    #[serde(default)]
    pub url: String,
    /// More full links to the same file, tried in order when `url` fails (external files such as `data.grf` on MEGA and
    /// in a GitHub release). Older patchers ignore them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mirrors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    /// SHA-256 of the GRF as it was when patching started (id 0). A fresh client package ships this version.
    pub baseline_sha256: String,
    /// Optional whole current GRF for repair (`otp publish --full`), a file of the patch site.
    #[serde(default)]
    pub full: Option<FileEntry>,
    /// Patch id the `full` copy corresponds to.
    #[serde(default)]
    pub full_id: u32,
    /// SHA-256 of the master GRF at the last publish and the patch id it corresponds to: a client package made with
    /// `otp package` ships this version, so a fresh client starts there instead of at the baseline.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub current_sha256: String,
    #[serde(default)]
    pub current_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchEntry {
    pub id: u32,
    pub target: String,
    pub file: String,
    pub size: u64,
    /// SHA-256 of the downloaded `.thor`.
    pub sha256: String,
    /// Full link instead of `file` relative to a mirror (cloud share link).
    #[serde(default)]
    pub url: String,
}

impl PatchList {
    pub fn parse(bytes: &[u8]) -> crate::Result<Self> {
        let p: PatchList = serde_json::from_slice(bytes)?;
        if p.schema != SCHEMA {
            return Err(crate::msg(format!("patch list schema {} is not supported (this patcher knows {})", p.schema, SCHEMA)));
        }
        Ok(p)
    }

    pub fn next_id(&self) -> u32 {
        self.patches.iter().map(|p| p.id).max().unwrap_or(0) + 1
    }
}

/// Thor Patcher's legacy `plist.txt`: `<id> <file>` per line, `//` comments. Returns (id, file name).
pub fn parse_plist_txt(text: &str) -> Vec<(u32, String)> {
    text.lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}'))
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .filter_map(|l| {
            let mut it = l.splitn(2, char::is_whitespace);
            let id = it.next()?.parse().ok()?;
            Some((id, it.next()?.trim().to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_txt_is_read() {
        let v = parse_plist_txt("//1 2007-11-13data.thor\n1 data.thor\n  2   second patch.thor \n");
        assert_eq!(v, vec![(1, "data.thor".to_string()), (2, "second patch.thor".to_string())]);
    }

    #[test]
    fn schema_is_checked() {
        assert!(PatchList::parse(br#"{"schema":9,"server":"x","base_url":[]}"#).is_err());
        let p = PatchList::parse(br#"{"schema":1,"server":"x","base_url":["a"]}"#).unwrap();
        assert!(p.allow_patching);
        assert_eq!(p.next_id(), 1);
    }
}
