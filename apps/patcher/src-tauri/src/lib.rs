//! OpenThor Patcher (player) backend: Tauri commands over otp-core.
//!
//! `patcher.json` next to the program says which server to patch from (name, patch URL, public key). The client folder
//! is the program's own folder unless the player picked another one (kept in the app data folder).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use otp_core::launch;
use otp_core::patchlist::{ChangeEntry, ClientCfg, LinkItem, NewsItem, PatchList, ServerEntry};
use otp_core::sync::{self, Config, Event, Summary};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

type Res<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Deserialize, Serialize, Clone, Default)]
struct Cfg {
    name: String,
    patch_url: String,
    #[serde(default)]
    list_url: Option<String>,
    #[serde(default)]
    pub_key: String,
    #[serde(default)]
    news_url: Option<String>,
    #[serde(default)]
    legacy_grf: Option<String>,
    /// For classic Thor Patcher servers, which have no patch list to say what to start.
    #[serde(default)]
    game_exe: Option<String>,
    #[serde(default)]
    game_args: Option<String>,
    #[serde(default)]
    setup_exe: Option<String>,
    /// For classic servers without a patch list.
    #[serde(default)]
    website: Option<String>,
    /// Headers for a private patch host, e.g. {"Authorization": "Bearer <read-only token>"}. Anyone who has the patcher can read
    /// them, so use a read-only token limited to the patch repository.
    #[serde(default)]
    headers: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    header_hosts: Vec<String>,
    /// More patch sites with the same layout as `patch_url`; the newest valid patch list of all of them is used, and one
    /// being down or behind changes nothing.
    #[serde(default)]
    fallback_urls: Vec<String>,
    /// Optional HTTP(S) or SOCKS5 proxy; without it the system settings are used.
    #[serde(default)]
    proxy: Option<String>,
    /// For classic servers without a patch list: what to show as server status.
    #[serde(default)]
    servers: Vec<ServerEntry>,
    /// Downloads at the same time (default 4); the player can change it in the settings.
    #[serde(default)]
    parallel: Option<usize>,
    /// Bandwidth limit in KiB/s (default none); the player can change it in the settings.
    #[serde(default)]
    max_kbps: Option<u64>,
    /// `false` switches the self-update off (default on; it only runs with a signed list).
    #[serde(default)]
    self_update: Option<bool>,
    /// For classic servers without a patch list: how many copies of the game may run (0 = no limit).
    #[serde(default)]
    max_instances: Option<u32>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
struct Settings {
    client_dir: Option<String>,
    auto_update: Option<bool>,
    keep_backup: Option<bool>,
    #[serde(default)]
    parallel: Option<usize>,
    #[serde(default)]
    max_kbps: Option<u64>,
}

struct App {
    cancel: Arc<AtomicBool>,
    last: Mutex<Option<PatchList>>,
    busy: AtomicBool,
}

fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}

fn config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("OPENTHOR_CONFIG") {
        return PathBuf::from(p);
    }
    let beside = exe_dir().join("patcher.json");
    if beside.is_file() {
        return beside;
    }
    PathBuf::from("patcher.json")
}

fn load_cfg() -> Res<Cfg> {
    let p = config_path();
    let b = std::fs::read(&p).map_err(|_| format!("patcher.json not found next to the program ({})", p.display()))?;
    let c: Cfg = serde_json::from_slice(&b).map_err(|e| format!("patcher.json is invalid: {e}"))?;
    if c.patch_url.is_empty() && c.list_url.is_none() {
        return Err("patcher.json has no patch_url".into());
    }
    Ok(c)
}

fn settings_path(app: &tauri::AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap_or_else(|_| exe_dir()).join("settings.json")
}

fn load_settings(app: &tauri::AppHandle) -> Settings {
    std::fs::read(settings_path(app)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_settings(app: &tauri::AppHandle, s: &Settings) -> Res<()> {
    let p = settings_path(app);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(err)?;
    }
    std::fs::write(p, serde_json::to_vec_pretty(s).map_err(err)?).map_err(err)
}

fn client_dir(app: &tauri::AppHandle) -> PathBuf {
    load_settings(app).client_dir.map(PathBuf::from).unwrap_or_else(exe_dir)
}

fn core_config(app: &tauri::AppHandle, cfg: &Cfg, st: &App) -> Config {
    let s = load_settings(app);
    let mut c = Config::new(cfg.patch_url.clone(), client_dir(app));
    c.list_url = cfg.list_url.clone();
    c.pub_key_hex = Some(cfg.pub_key.clone()).filter(|k| !k.is_empty());
    c.backup = s.keep_backup.unwrap_or(false);
    if let Some(g) = &cfg.legacy_grf {
        c.legacy_default_grf = g.clone();
    }
    c.cancel = Some(st.cancel.clone());
    c.headers = cfg.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    c.header_hosts = cfg.header_hosts.clone();
    c.fallback_urls = cfg.fallback_urls.clone();
    c.proxy = cfg.proxy.clone();
    c.parallel = s.parallel.or(cfg.parallel).unwrap_or(sync::DEFAULT_PARALLEL);
    c.max_kbps = s.max_kbps.or(cfg.max_kbps).unwrap_or(0);
    c
}

#[derive(Serialize)]
struct Init {
    name: String,
    client_dir: String,
    auto_update: bool,
    keep_backup: bool,
    parallel: usize,
    max_kbps: u64,
    error: Option<String>,
    version: String,
}

#[tauri::command]
fn init(app: tauri::AppHandle) -> Init {
    let s = load_settings(&app);
    let version = env!("CARGO_PKG_VERSION").to_string();
    if let Ok(exe) = std::env::current_exe() {
        otp_core::selfupdate::cleanup(&exe);
    }
    let cfg = load_cfg();
    let parallel = s.parallel.or(cfg.as_ref().ok().and_then(|c| c.parallel)).unwrap_or(sync::DEFAULT_PARALLEL);
    let max_kbps = s.max_kbps.or(cfg.as_ref().ok().and_then(|c| c.max_kbps)).unwrap_or(0);
    let dir = client_dir(&app).display().to_string();
    match cfg {
        Ok(c) => Init { name: c.name, client_dir: dir, auto_update: s.auto_update.unwrap_or(true), keep_backup: s.keep_backup.unwrap_or(false), parallel, max_kbps, error: None, version },
        Err(e) => Init { name: "OpenThor Patcher".into(), client_dir: dir, auto_update: true, keep_backup: false, parallel, max_kbps, error: Some(e), version },
    }
}

#[tauri::command]
fn set_settings(app: tauri::AppHandle, client_dir: Option<String>, auto_update: bool, keep_backup: bool, parallel: Option<usize>, max_kbps: Option<u64>) -> Res<()> {
    let mut s = load_settings(&app);
    if let Some(d) = client_dir {
        if !Path::new(&d).is_dir() {
            return Err(format!("{d} is not a folder"));
        }
        s.client_dir = Some(d);
    }
    s.auto_update = Some(auto_update);
    s.keep_backup = Some(keep_backup);
    if let Some(n) = parallel {
        s.parallel = Some(n.clamp(1, 16));
    }
    if let Some(k) = max_kbps {
        s.max_kbps = Some(k);
    }
    save_settings(&app, &s)
}

#[derive(Serialize)]
struct PackInfo {
    id: String,
    title: String,
    on: bool,
    locked: bool,
}

#[derive(Serialize)]
struct Checked {
    server: String,
    notice: String,
    news_url: String,
    open: bool,
    message: String,
    packs: Vec<PackInfo>,
    can_play: bool,
    can_setup: bool,
    news: Vec<NewsItem>,
    changelog: Vec<ChangeEntry>,
    website: String,
    links: Vec<LinkItem>,
    summary: Summary,
    /// A newer patcher program the list offers for this computer (installed before patching).
    update: Option<UpdateInfo>,
    has_status: bool,
    update_warning: String,
}

/// `--updated-to <version>`: this process was started by a self-update to that version.
fn updated_to() -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == "--updated-to").and_then(|i| a.get(i + 1).cloned())
}

/// The program that a self-update just installed reports an older version than announced (a wrong build or version on the
/// server): updating again would download the same file forever.
fn update_loop(list: Option<&PatchList>) -> bool {
    let Some(want) = updated_to() else { return false };
    otp_core::selfupdate::newer(&want, env!("CARGO_PKG_VERSION")) && list.and_then(|l| l.patcher.as_ref()).is_some_and(|r| !otp_core::selfupdate::newer(&r.version, &want))
}

#[derive(Serialize)]
struct UpdateInfo {
    version: String,
    notes: String,
}

#[tauri::command]
async fn check(app: tauri::AppHandle, st: State<'_, Arc<App>>, force: bool) -> Res<Checked> {
    let cfg = load_cfg()?;
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut c = core_config(&app, &cfg, &st);
        let list = sync::fetch_list(&c).map_err(err);
        // a classic plist.txt server has no patchlist.json: still report what it needs
        let (open, message) = list.as_ref().map(|l| (l.allow_patching, l.message.clone())).unwrap_or((true, String::new()));
        c.dry_run = true;
        c.force_check = force;
        let summary = if open { sync::run(&c, &mut |_| {}).map_err(err)? } else { Summary::default() };
        let l = list.ok();
        let packs = l
            .as_ref()
            .and_then(|l| l.data_ini.as_ref())
            .map(|d| (d, sync::pack_choices(&c.client_dir, d)))
            .map(|(d, prefs)| d.packs.iter().map(|p| PackInfo { id: p.id.clone(), title: if p.title.is_empty() { p.id.clone() } else { p.title.clone() }, on: p.locked || prefs.packs.get(&p.id).copied().unwrap_or(p.default), locked: p.locked }).collect())
            .unwrap_or_default();
        let game = l.as_ref().and_then(|l| l.client.clone()).filter(|g| !g.exe.is_empty()).unwrap_or_else(|| ClientCfg {
            exe: cfg.game_exe.clone().unwrap_or_default(),
            args: cfg.game_args.clone().unwrap_or_default(),
            setup_exe: cfg.setup_exe.clone().unwrap_or_default(),
            max_instances: cfg.max_instances.unwrap_or(0),
        });
        let out = Checked {
            server: l.as_ref().map(|l| l.server.clone()).unwrap_or_else(|| cfg.name.clone()),
            notice: l.as_ref().map(|l| l.notice.clone()).unwrap_or_default(),
            news_url: cfg.news_url.clone().or_else(|| l.as_ref().map(|l| l.news_url.clone())).unwrap_or_default(),
            open,
            message,
            packs,
            can_play: !game.exe.is_empty(),
            can_setup: !game.setup_exe.is_empty(),
            website: l.as_ref().map(|l| l.website.clone()).filter(|w| !w.is_empty()).or_else(|| cfg.website.clone()).unwrap_or_default(),
            links: l.as_ref().map(|l| l.links.clone()).unwrap_or_default(),
            news: l.as_ref().map(|l| l.news.clone()).unwrap_or_default(),
            changelog: l.as_ref().map(|l| l.changelog.clone()).unwrap_or_default(),
            summary,
            update: l
                .as_ref()
                .filter(|_| cfg.self_update.unwrap_or(true) && !cfg.pub_key.is_empty() && !update_loop(l.as_ref()))
                .and_then(|l| otp_core::selfupdate::available(l, env!("CARGO_PKG_VERSION"), otp_core::selfupdate::platform()).map(|(v, _)| (v, l)))
                .map(|(v, l)| UpdateInfo { version: v.to_string(), notes: l.patcher.as_ref().map(|r| r.notes.clone()).unwrap_or_default() }),
            has_status: l.as_ref().is_some_and(|l| !l.status_url.is_empty()),
            update_warning: if update_loop(l.as_ref()) {
                format!(
                    "The server's patcher build still reports version {} although it was announced as {}; the self-update is stopped (ask the server team to check the build).",
                    env!("CARGO_PKG_VERSION"),
                    updated_to().unwrap_or_default()
                )
            } else {
                String::new()
            },
        };
        *st.last.lock().map_err(err)? = l;
        Ok(out)
    })
    .await
    .map_err(err)?
}

#[derive(Serialize)]
struct ServerState {
    name: String,
    ok: bool,
    ms: u64,
}

/// Is the server reachable? A TCP connect with a short timeout, all servers in parallel.
#[tauri::command]
async fn server_status(st: State<'_, Arc<App>>) -> Res<Vec<ServerState>> {
    let cfg = load_cfg()?;
    let list: Vec<ServerEntry> = st.last.lock().map_err(err)?.as_ref().map(|l| l.servers.clone()).filter(|s| !s.is_empty()).unwrap_or(cfg.servers);
    tauri::async_runtime::spawn_blocking(move || {
        let handles: Vec<_> = list
            .into_iter()
            .map(|s| {
                std::thread::spawn(move || {
                    use std::net::ToSocketAddrs;
                    let t = std::time::Instant::now();
                    let ok = (s.host.as_str(), s.port)
                        .to_socket_addrs()
                        .ok()
                        .and_then(|mut a| a.next())
                        .is_some_and(|addr| std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(2500)).is_ok());
                    ServerState { name: s.name, ok, ms: t.elapsed().as_millis() as u64 }
                })
            })
            .collect();
        Ok(handles.into_iter().filter_map(|h| h.join().ok()).collect())
    })
    .await
    .map_err(err)?
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Progress {
    Info { text: String },
    Download { name: String, done: u64, total: u64 },
    Applied { target: String, id: u32 },
    Removed { path: String },
    Total { done: u64, total: u64 },
}

#[tauri::command]
async fn patch(app: tauri::AppHandle, st: State<'_, Arc<App>>, force: bool) -> Res<Summary> {
    let cfg = load_cfg()?;
    let st = st.inner().clone();
    if st.busy.swap(true, Ordering::SeqCst) {
        return Err("already patching".into());
    }
    st.cancel.store(false, Ordering::SeqCst);
    let st2 = st.clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        let mut c = core_config(&app, &cfg, &st2);
        c.force_check = force;
        let a = app.clone();
        let mut last_emit = std::time::Instant::now();
        let mut last_total = std::time::Instant::now();
        sync::run(&c, &mut |e| {
            let p = match e {
                Event::Info(text) => Progress::Info { text },
                Event::Download { name, done, total } => {
                    if done < total && last_emit.elapsed() < std::time::Duration::from_millis(100) {
                        return;
                    }
                    last_emit = std::time::Instant::now();
                    Progress::Download { name, done, total }
                }
                Event::Applied { target, id } => Progress::Applied { target, id },
                Event::Removed(path) => Progress::Removed { path },
                Event::Total { done, total } => {
                    if done < total && last_total.elapsed() < std::time::Duration::from_millis(100) {
                        return;
                    }
                    last_total = std::time::Instant::now();
                    Progress::Total { done, total }
                }
            };
            let _ = a.emit("progress", p);
        })
        .map_err(err)
    })
    .await
    .map_err(err);
    st.busy.store(false, Ordering::SeqCst);
    res?
}

#[tauri::command]
fn cancel(st: State<'_, Arc<App>>) {
    st.cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn set_pack(app: tauri::AppHandle, st: State<'_, Arc<App>>, id: String, on: bool) -> Res<bool> {
    let cfg = load_cfg()?;
    let c = core_config(&app, &cfg, &st);
    sync::set_pack(&c, &id, on).map_err(err)
}

fn game(st: &App, _setup: bool) -> Res<(ClientCfg, bool)> {
    let l = st.last.lock().map_err(err)?;
    if let Some(g) = l.as_ref().and_then(|l| l.client.clone()).filter(|g| !g.exe.is_empty()) {
        return Ok((g, false));
    }
    let c = load_cfg()?;
    let exe = c.game_exe.ok_or("the server did not say which program to start")?;
    Ok((ClientCfg { exe, args: c.game_args.unwrap_or_default(), setup_exe: c.setup_exe.unwrap_or_default(), max_instances: c.max_instances.unwrap_or(0) }, false))
}

#[tauri::command]
fn play(app: tauri::AppHandle, st: State<'_, Arc<App>>) -> Res<()> {
    let (g, _) = game(&st, false)?;
    launch::start_checked(&client_dir(&app), &g.exe, &g.args, g.max_instances).map_err(err)
}

#[tauri::command]
fn setup(app: tauri::AppHandle, st: State<'_, Arc<App>>) -> Res<()> {
    let (g, _) = game(&st, true)?;
    if g.setup_exe.is_empty() {
        return Err("this server has no setup program".into());
    }
    launch::start(&client_dir(&app), &g.setup_exe, "").map_err(err)
}

/// Opens a web link in the player's browser. Only http(s); no shell is involved, so the link cannot run anything.
#[tauri::command]
fn open_url(url: String) -> Res<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) || url.chars().any(|c| c.is_control() || c == ' ') || url.len() > 2000 {
        return Err("only http(s) links can be opened".into());
    }
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(&url).spawn().map(|_| ()).map_err(err)
}

#[derive(Deserialize, Default)]
struct ThemeJson {
    #[serde(default)]
    name: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    css: Option<String>,
    #[serde(default)]
    logo: Option<String>,
    #[serde(default)]
    vars: std::collections::BTreeMap<String, String>,
}

#[derive(Serialize)]
struct Theme {
    name: String,
    author: String,
    css: String,
    vars: std::collections::BTreeMap<String, String>,
    logo: Option<String>,
}

fn data_uri(p: &Path) -> Option<String> {
    use std::io::Write;
    let ext = p.extension()?.to_str()?.to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => return None,
    };
    let bytes = std::fs::read(p).ok().filter(|b| b.len() <= 8 << 20)?;
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() * 4 / 3 + 8);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63]);
        out.push(T[(n >> 12) as usize & 63]);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] } else { b'=' });
        out.push(if c.len() > 2 { T[n as usize & 63] } else { b'=' });
    }
    let mut s = format!("data:{mime};base64,").into_bytes();
    s.write_all(&out).ok()?;
    String::from_utf8(s).ok()
}

/// Replaces `url(file)` of files that exist in the theme folder with data URIs, so a theme is just a folder.
fn inline_urls(css: &str, dir: &Path) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("url(") {
        out.push_str(&rest[..i + 4]);
        rest = &rest[i + 4..];
        let Some(j) = rest.find(')') else { break };
        let raw = rest[..j].trim().trim_matches(|c| c == '"' || c == '\'');
        let local = otp_core::safe_rel(raw).ok().map(|r| dir.join(r));
        match local.as_deref().and_then(data_uri) {
            Some(d) => out.push_str(&format!("\"{d}\"")),
            None => out.push_str(&rest[..j]),
        }
        rest = &rest[j..];
    }
    out.push_str(rest);
    out
}

/// A theme folder `patcher-theme/` in the client folder (shipped like any other file, so operators can update it through
/// the patcher itself). `None` = use the built-in Ragnarok theme.
#[tauri::command]
fn get_theme(app: tauri::AppHandle) -> Option<Theme> {
    let dir = client_dir(&app).join("patcher-theme");
    let j: ThemeJson = serde_json::from_slice(&std::fs::read(dir.join("theme.json")).ok()?).ok()?;
    let css_file = j.css.unwrap_or_else(|| "theme.css".into());
    let css = std::fs::read_to_string(dir.join(otp_core::safe_rel(&css_file).ok()?)).unwrap_or_default();
    let logo = j.logo.and_then(|l| otp_core::safe_rel(&l).ok()).and_then(|l| data_uri(&dir.join(l)));
    Some(Theme { name: j.name, author: j.author, css: inline_urls(&css, &dir), vars: j.vars, logo })
}

/// Downloads the newer patcher the signed list offers, puts it in place of this program, starts it and exits.
#[tauri::command]
async fn update_patcher(app: tauri::AppHandle, st: State<'_, Arc<App>>) -> Res<String> {
    let cfg = load_cfg()?;
    if cfg.pub_key.is_empty() || !cfg.self_update.unwrap_or(true) {
        return Err("self-update is off (it needs pub_key in patcher.json)".into());
    }
    let st = st.inner().clone();
    if st.busy.swap(true, Ordering::SeqCst) {
        return Err("busy".into());
    }
    let app2 = app.clone();
    let st2 = st.clone();
    let res = tauri::async_runtime::spawn_blocking(move || -> Res<String> {
        let c = core_config(&app2, &cfg, &st2);
        let list = sync::fetch_list(&c).map_err(err)?;
        let (v, f) = otp_core::selfupdate::available(&list, env!("CARGO_PKG_VERSION"), otp_core::selfupdate::platform()).ok_or("no newer patcher")?;
        let (v, f) = (v.to_string(), f.clone());
        let exe = std::env::current_exe().map_err(err)?;
        let a = app2.clone();
        otp_core::selfupdate::update(&c, &list, &f, &exe, &mut |e| {
            if let Event::Download { name, done, total } = e {
                let _ = a.emit("progress", Progress::Download { name, done, total });
            }
        })
        .map_err(err)?;
        std::process::Command::new(&exe).arg("--updated-to").arg(&v).current_dir(exe_dir()).spawn().map_err(err)?;
        Ok(v)
    })
    .await
    .map_err(err);
    st.busy.store(false, Ordering::SeqCst);
    let v = res??;
    app.exit(0);
    Ok(v)
}

/// The realm state from the web panel (`status_url` of the list, MidgardCP `/api/v1/status`).
#[tauri::command]
async fn panel_status(app: tauri::AppHandle, st: State<'_, Arc<App>>) -> Res<Option<otp_core::midgard::RealmState>> {
    let (url, realm) = match st.last.lock().map_err(err)?.as_ref() {
        Some(l) if !l.status_url.is_empty() => (l.status_url.clone(), l.status_realm.clone()),
        _ => return Ok(None),
    };
    let cfg = load_cfg()?;
    let c = core_config(&app, &cfg, &st);
    tauri::async_runtime::spawn_blocking(move || {
        crate::sync_auth(&c);
        let b = otp_core::fetch::get_bytes(&url).map_err(err)?;
        otp_core::midgard::parse_status(&b, &realm).map(Some).map_err(err)
    })
    .await
    .map_err(err)?
}

fn sync_auth(c: &Config) {
    otp_core::fetch::set_proxy(c.proxy.clone());
}

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    if otp_core::launch::run_elevated_helper(&args) {
        return;
    }
    if let Ok(me) = std::env::current_exe() {
        otp_core::launch::use_elevated_helper(me);
    }
    let state = Arc::new(App { cancel: Arc::new(AtomicBool::new(false)), last: Mutex::new(None), busy: AtomicBool::new(false) });
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![init, set_settings, check, patch, cancel, set_pack, play, setup, get_theme, open_url, server_status, update_patcher, panel_status])
        .run(tauri::generate_context!())
        .expect("error while running OpenThor Patcher");
}
