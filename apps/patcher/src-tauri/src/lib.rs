//! OpenThor Patcher (player) backend: Tauri commands over otp-core.
//!
//! `patcher.json` next to the program says which server to patch from (name, patch URL, public key). The client folder
//! is the program's own folder unless the player picked another one (kept in the app data folder).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use otp_core::launch;
use otp_core::patchlist::{ClientCfg, PatchList};
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
}

#[derive(Deserialize, Serialize, Clone, Default)]
struct Settings {
    client_dir: Option<String>,
    auto_update: Option<bool>,
    keep_backup: Option<bool>,
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
    c.backup = s.keep_backup.unwrap_or(true);
    if let Some(g) = &cfg.legacy_grf {
        c.legacy_default_grf = g.clone();
    }
    c.cancel = Some(st.cancel.clone());
    c
}

#[derive(Serialize)]
struct Init {
    name: String,
    client_dir: String,
    auto_update: bool,
    keep_backup: bool,
    error: Option<String>,
    version: String,
}

#[tauri::command]
fn init(app: tauri::AppHandle) -> Init {
    let s = load_settings(&app);
    let version = env!("CARGO_PKG_VERSION").to_string();
    match load_cfg() {
        Ok(c) => Init { name: c.name, client_dir: client_dir(&app).display().to_string(), auto_update: s.auto_update.unwrap_or(true), keep_backup: s.keep_backup.unwrap_or(true), error: None, version },
        Err(e) => Init { name: "OpenThor Patcher".into(), client_dir: client_dir(&app).display().to_string(), auto_update: true, keep_backup: true, error: Some(e), version },
    }
}

#[tauri::command]
fn set_settings(app: tauri::AppHandle, client_dir: Option<String>, auto_update: bool, keep_backup: bool) -> Res<()> {
    let mut s = load_settings(&app);
    if let Some(d) = client_dir {
        if !Path::new(&d).is_dir() {
            return Err(format!("{d} is not a folder"));
        }
        s.client_dir = Some(d);
    }
    s.auto_update = Some(auto_update);
    s.keep_backup = Some(keep_backup);
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
    summary: Summary,
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
        let prefs = sync::load_prefs(&c.client_dir);
        let packs = l
            .as_ref()
            .and_then(|l| l.data_ini.as_ref())
            .map(|d| d.packs.iter().map(|p| PackInfo { id: p.id.clone(), title: if p.title.is_empty() { p.id.clone() } else { p.title.clone() }, on: p.locked || prefs.packs.get(&p.id).copied().unwrap_or(p.default), locked: p.locked }).collect())
            .unwrap_or_default();
        let game = l.as_ref().and_then(|l| l.client.clone()).filter(|g| !g.exe.is_empty()).unwrap_or_else(|| ClientCfg {
            exe: cfg.game_exe.clone().unwrap_or_default(),
            args: cfg.game_args.clone().unwrap_or_default(),
            setup_exe: cfg.setup_exe.clone().unwrap_or_default(),
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
            summary,
        };
        *st.last.lock().map_err(err)? = l;
        Ok(out)
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
    Ok((ClientCfg { exe, args: c.game_args.unwrap_or_default(), setup_exe: c.setup_exe.unwrap_or_default() }, false))
}

#[tauri::command]
fn play(app: tauri::AppHandle, st: State<'_, Arc<App>>) -> Res<()> {
    let (g, _) = game(&st, false)?;
    launch::start(&client_dir(&app), &g.exe, &g.args).map_err(err)
}

#[tauri::command]
fn setup(app: tauri::AppHandle, st: State<'_, Arc<App>>) -> Res<()> {
    let (g, _) = game(&st, true)?;
    if g.setup_exe.is_empty() {
        return Err("this server has no setup program".into());
    }
    launch::start(&client_dir(&app), &g.setup_exe, "").map_err(err)
}

pub fn run() {
    let state = Arc::new(App { cancel: Arc::new(AtomicBool::new(false)), last: Mutex::new(None), busy: AtomicBool::new(false) });
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![init, set_settings, check, patch, cancel, set_pack, play, setup])
        .run(tauri::generate_context!())
        .expect("error while running OpenThor Patcher");
}
