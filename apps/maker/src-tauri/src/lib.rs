//! OpenThor Patch Maker backend: Tauri commands over otp-core. The GUI does nothing `otp` (the CLI) cannot do.

use std::path::{Path, PathBuf};

use otp_core::apply::GrfKey;
use otp_core::patchlist::PatchList;
use otp_core::project::{scan_client, Project, ScanItem, Suggest};
use otp_core::publish::{publish, site_files, PublishReport, SiteFile};
use serde::Serialize;
use tauri::Emitter;

type Res<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
fn scan(dir: String) -> Res<Vec<ScanItem>> {
    scan_client(Path::new(&dir)).map_err(err)
}

#[tauri::command]
fn load_project(path: String) -> Res<Project> {
    Project::load(Path::new(&path)).map_err(err)
}

#[tauri::command]
fn save_project(path: String, project: Project) -> Res<()> {
    project.save(Path::new(&path)).map_err(err)
}

/// A new project from a master client: big GRFs become Thor targets, small files are synced whole.
#[tauri::command]
fn new_project(path: String, server: String, client: String, site: String) -> Res<Project> {
    if Path::new(&path).exists() {
        return Err(format!("{path} exists"));
    }
    let mut p = Project::new(&server, PathBuf::from(&client), PathBuf::from(&site));
    for i in scan_client(Path::new(&client)).map_err(err)? {
        match i.suggest {
            Suggest::Thor => p.thor.push(i.path),
            Suggest::Sync => p.sync.push(i.path),
            Suggest::Ignore => {}
        }
    }
    p.save(Path::new(&path)).map_err(err)?;
    Ok(p)
}

#[derive(Serialize)]
struct Keys {
    seed_path: String,
    pub_hex: String,
}

/// Creates the key pair in `dir` (never overwrites). Returns the paths and the public key as hex.
#[tauri::command]
fn make_keys(dir: String) -> Res<Keys> {
    let d = Path::new(&dir);
    let seed = d.join("signing.seed");
    if !seed.exists() {
        otp_core::sign::keygen(d).map_err(err)?;
    }
    let pub_hex = std::fs::read_to_string(d.join("patcher.pub")).map_err(err)?.trim().to_string();
    Ok(Keys { seed_path: seed.display().to_string(), pub_hex })
}

#[tauri::command]
fn read_pub(dir: String) -> Res<String> {
    Ok(std::fs::read_to_string(Path::new(&dir).join("patcher.pub")).map_err(err)?.trim().to_string())
}

async fn run_publish(app: tauri::AppHandle, project: Project, grf_password: Option<String>, dry: bool) -> Res<PublishReport> {
    tauri::async_runtime::spawn_blocking(move || {
        let key = GrfKey::from_password(grf_password.as_deref()).map_err(err)?.0;
        publish(&project, key, dry, &mut |line| {
            let _ = app.emit("log", line);
        })
        .map_err(err)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn plan(app: tauri::AppHandle, project: Project, grf_password: Option<String>) -> Res<PublishReport> {
    run_publish(app, project, grf_password, true).await
}

#[tauri::command]
async fn publish_site(app: tauri::AppHandle, project: Project, grf_password: Option<String>) -> Res<PublishReport> {
    run_publish(app, project, grf_password, false).await
}

#[tauri::command]
fn files_to_upload(project: Project) -> Vec<SiteFile> {
    site_files(&project)
}

#[derive(Serialize)]
struct SiteStatus {
    published: bool,
    server: String,
    updated: String,
    patches: usize,
    files: usize,
    targets: usize,
    signed: bool,
}

#[tauri::command]
fn site_status(project: Project) -> SiteStatus {
    let list = std::fs::read(project.site.join("patchlist.json")).ok().and_then(|b| PatchList::parse(&b).ok());
    match list {
        Some(l) => SiteStatus {
            published: true,
            server: l.server,
            updated: l.updated,
            patches: l.patches.len(),
            files: l.files.len(),
            targets: l.targets.len(),
            signed: project.site.join("patchlist.json.sig").is_file(),
        },
        None => SiteStatus { published: false, server: String::new(), updated: String::new(), patches: 0, files: 0, targets: 0, signed: false },
    }
}

/// Upload the site, a package and/or the external files to the project's targets (rclone, gh).
#[tauri::command]
async fn upload(project: Project, site: bool, external: bool, package: Option<String>) -> Res<Vec<otp_core::upload::Uploaded>> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut all = Vec::new();
        let mut log = |_: &str| {};
        if external {
            all.extend(otp_core::upload::upload_external(&project, &mut log).map_err(err)?);
        }
        if site {
            all.extend(otp_core::upload::upload_site(&project, &mut log).map_err(err)?);
        }
        if let Some(z) = package {
            all.extend(otp_core::upload::upload_file(&project, std::path::Path::new(&z), &project.upload.package, "package", &mut log).map_err(err)?);
        }
        Ok(all)
    })
    .await
    .map_err(err)?
}

/// The client package for new players (`otp package`): ZIP of the published master client, patcher and patcher.json.
#[tauri::command]
async fn make_package(project: Project, out: String, patcher: Option<String>, patcher_json: Option<String>, include_external: bool) -> Res<otp_core::package::Report> {
    tauri::async_runtime::spawn_blocking(move || {
        let o = otp_core::package::Options {
            out: out.into(),
            patcher: patcher.filter(|p| !p.is_empty()).map(Into::into),
            patcher_json: patcher_json.filter(|p| !p.is_empty()),
            include_external,
        };
        otp_core::package::package(&project, &o, &mut |_| {}).map_err(err)
    })
    .await
    .map_err(err)?
}

/// The `patcher.json` a player's OpenThor Patcher needs next to its executable.
#[tauri::command]
fn patcher_config(project: Project, pub_hex: String, list_url: Option<String>, header_name: Option<String>, header_value: Option<String>) -> Res<String> {
    let base = project.base_urls.first().cloned().unwrap_or_default();
    let mut v = serde_json::json!({
        "name": project.server,
        "patch_url": base,
        "pub_key": pub_hex,
    });
    if let Some(u) = list_url.filter(|u| !u.is_empty()) {
        v["list_url"] = serde_json::Value::String(u);
    }
    // every further mirror is also a place to read the patch list from, so the first one being down does not stop patching
    let more: Vec<&String> = project.base_urls.iter().skip(1).filter(|u| !u.is_empty()).collect();
    if !more.is_empty() {
        v["fallback_urls"] = serde_json::json!(more);
    }
    if let (Some(n), Some(val)) = (header_name.filter(|n| !n.is_empty()), header_value.filter(|n| !n.is_empty())) {
        v["headers"] = serde_json::json!({ n: val });
    }
    serde_json::to_string_pretty(&v).map_err(err)
}

#[tauri::command]
fn write_text(path: String, text: String) -> Res<()> {
    std::fs::write(&path, text).map_err(err)
}

const THEME_FILES: &[(&str, &[u8])] = &[
    ("theme.json", include_bytes!("../../../patcher/ui/themes/default/theme.json")),
    ("theme.css", include_bytes!("../../../patcher/ui/themes/default/theme.css")),
    ("logo.png", include_bytes!("../../../patcher/ui/themes/default/logo.png")),
    ("fonts/Cinzel.ttf", include_bytes!("../../../patcher/ui/themes/default/fonts/Cinzel.ttf")),
    ("fonts/OFL.txt", include_bytes!("../../../patcher/ui/themes/default/fonts/OFL.txt")),
];

/// Writes the built-in theme into `<dir>/patcher-theme/` as a starting point for an own theme.
#[tauri::command]
fn export_theme(dir: String) -> Res<String> {
    let root = Path::new(&dir).join("patcher-theme");
    if root.exists() {
        return Err(format!("{} exists; rename or remove it first", root.display()));
    }
    for (name, bytes) in THEME_FILES {
        let p = root.join(name);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(err)?;
        }
        std::fs::write(&p, bytes).map_err(err)?;
    }
    Ok(root.display().to_string())
}

#[tauri::command]
fn reveal(path: String) -> Res<()> {
    let p = Path::new(&path);
    let dir = if p.is_dir() { p } else { p.parent().unwrap_or(p) };
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let cmd = "xdg-open";
    std::process::Command::new(cmd).arg(dir).spawn().map(|_| ()).map_err(err)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            app_version,
            scan,
            load_project,
            save_project,
            new_project,
            make_keys,
            read_pub,
            plan,
            publish_site,
            files_to_upload,
            site_status,
            patcher_config,
            make_package,
            upload,
            write_text,
            export_theme,
            reveal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenThor Patch Maker");
}
