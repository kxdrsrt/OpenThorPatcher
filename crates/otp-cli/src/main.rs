use clap::{Parser, Subcommand};
use otp_core::project::{scan_client, Project, Suggest};
use otp_core::publish::publish;
use otp_core::sync::{run, set_pack, Config, Event};
use otp_core::{apply::GrfKey, sign};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "otp", version, about = "OpenThor Patcher: publish a master client, patch player clients")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create signing.seed (secret) and patcher.pub (ships with the patcher)
    Keygen {
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Back up the signing key of a project into ONE password-protected file (keep it away from the server and from git)
    KeysBackup {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// more files to include, e.g. the SFTP key of the upload
        #[arg(long = "also")]
        also: Vec<PathBuf>,
    },
    /// Put the key of a backup file back (a different existing key is only replaced with --force)
    KeysRestore {
        file: PathBuf,
        #[arg(long)]
        project: PathBuf,
        /// where extra files of the backup go
        #[arg(long)]
        extra_dir: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
    /// List the master client folder with a suggestion per file (thor / sync / ignore)
    Scan {
        #[arg(long)]
        client: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Create a project file from a master client folder (big GRFs become Thor targets, small files are synced)
    Init {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        server: String,
        #[arg(long)]
        client: PathBuf,
        #[arg(long)]
        site: PathBuf,
        /// Mirror base URL, repeatable
        #[arg(long = "base-url")]
        base_url: Vec<String>,
        #[arg(long)]
        seed: Option<PathBuf>,
    },
    /// Build or update the patch site of a project (only changed entries become a new numbered .thor)
    Publish {
        #[arg(long)]
        project: PathBuf,
        /// Show what would be published, write nothing
        #[arg(long)]
        dry_run: bool,
        /// Password of encrypted master GRFs
        #[arg(long)]
        grf_password: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Update a client folder from a patch site
    Patch {
        /// Folder URL of the patch site (http(s):// or file://)
        #[arg(long)]
        url: String,
        #[arg(long)]
        client: PathBuf,
        #[arg(long = "pub")]
        pub_key: Option<PathBuf>,
        /// `name=password` for an encrypted GRF, repeatable
        #[arg(long = "grf-password")]
        grf_password: Vec<String>,
        /// Keep the replaced GRF as <name>.bak (default: the old file is replaced completely)
        #[arg(long)]
        keep_backup: bool,
        #[arg(long)]
        dry_run: bool,
        /// Re-hash every file instead of trusting the cache (repair)
        #[arg(long)]
        check: bool,
        /// Request header for a private host, `"Authorization: Bearer <token>"`, repeatable
        #[arg(long = "header")]
        header: Vec<String>,
        /// Host that gets the headers (default: the host of --url)
        #[arg(long = "header-host")]
        header_host: Vec<String>,
        /// More patch sites with the same layout as --url, repeatable: the newest valid patch list of all sources wins
        #[arg(long = "fallback-url")]
        fallback_url: Vec<String>,
        /// HTTP(S) or SOCKS5 proxy (default: the system settings)
        #[arg(long)]
        proxy: Option<String>,
        /// GRF a classic Thor patch without a target goes to
        #[arg(long, default_value = "data.grf")]
        legacy_grf: String,
        /// Files downloaded at the same time
        #[arg(long, default_value_t = otp_core::sync::DEFAULT_PARALLEL)]
        parallel: usize,
        /// Bandwidth limit for all downloads together in KiB/s (0 = none)
        #[arg(long, default_value_t = 0)]
        max_kbps: u64,
    },
    /// Replace a patcher program with the newer build a signed patch list offers (what the patcher does at start)
    SelfUpdate {
        #[arg(long)]
        url: String,
        #[arg(long = "pub")]
        pub_key: PathBuf,
        /// The program file to replace
        #[arg(long)]
        exe: PathBuf,
        /// Its version
        #[arg(long)]
        current: String,
        /// Platform key in the patch list (default: this computer's)
        #[arg(long)]
        platform: Option<String>,
        #[arg(long = "fallback-url")]
        fallback_url: Vec<String>,
    },
    /// Download one link the way the patcher does (MEGA, Google Drive, Dropbox, OneDrive, http): check a cloud link
    Fetch {
        #[arg(long)]
        url: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Make the client package for new players (ZIP of the published master client + patcher + patcher.json)
    Package {
        #[arg(long)]
        project: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// The patcher program to put next to the game
        #[arg(long)]
        patcher: Option<PathBuf>,
        /// patcher.json for the players
        #[arg(long)]
        patcher_json: Option<PathBuf>,
        /// Also pack the external files (data.grf ...) instead of letting the patcher download them
        #[arg(long)]
        include_external: bool,
    },
    /// Upload the patch site (files first, patch list last), a client package and/or the external files (rclone, gh)
    Upload {
        #[arg(long)]
        project: PathBuf,
        /// Upload the patch site to upload.site
        #[arg(long)]
        site: bool,
        /// Upload this client package to upload.package
        #[arg(long)]
        package: Option<PathBuf>,
        /// Upload the external files (data.grf …) to upload.external
        #[arg(long)]
        external: bool,
    },
    /// Show the news a web panel (MidgardCP /api/v1/news) would put into the patch list
    News {
        #[arg(long)]
        from: String,
    },
    /// Switch an optional pack on or off for this client and rewrite DATA.INI
    Pack {
        #[arg(long)]
        url: String,
        #[arg(long)]
        client: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long, conflicts_with = "off")]
        on: bool,
        #[arg(long)]
        off: bool,
    },
}

fn main() {
    if let Err(e) = real() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn human(n: u64) -> String {
    let u = ["B", "KB", "MB", "GB"];
    let (mut v, mut i) = (n as f64, 0);
    while v >= 1024.0 && i < 3 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", u[i])
}

/// Password for key backups: OTP_KEYS_PASSWORD, otherwise asked on the terminal (not echoed).
fn password(confirm: bool) -> otp_core::Result<String> {
    if let Ok(p) = std::env::var("OTP_KEYS_PASSWORD") {
        return Ok(p);
    }
    let pw = rpassword::prompt_password("Password for the key backup: ").map_err(|e| otp_core::msg(e.to_string()))?;
    if confirm {
        otp_core::keybackup::check_password(&pw)?;
        if rpassword::prompt_password("Repeat the password: ").map_err(|e| otp_core::msg(e.to_string()))? != pw {
            return Err(otp_core::msg("the passwords differ"));
        }
    }
    Ok(pw)
}

fn real() -> otp_core::Result<()> {
    match Cli::parse().cmd {
        Cmd::Keygen { out } => {
            sign::keygen(&out)?;
            println!("wrote {}/signing.seed (keep secret) and patcher.pub", out.display());
        }
        Cmd::KeysBackup { project, out, also } => {
            let n = otp_core::keybackup::backup(&project, &also, &out, &password(true)?)?;
            println!("{n} key file(s) saved encrypted to {} and read back successfully. Keep it in two places and remember the password.", out.display());
        }
        Cmd::KeysRestore { file, project, extra_dir, force } => {
            let bytes = std::fs::read(&file).map_err(|e| otp_core::io_err(&file, e))?;
            let bundle = otp_core::keybackup::open(&bytes, &password(false)?)?;
            for r in otp_core::keybackup::restore(&bundle, &project, extra_dir.as_deref(), force)? {
                println!("{:8} {}", r.action, r.path);
            }
        }
        Cmd::Scan { client, json } => {
            let items = scan_client(&client)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else {
                for i in items {
                    println!("{:7} {:>10}  {}", format!("{:?}", i.suggest).to_lowercase(), human(i.size), i.path);
                }
            }
        }
        Cmd::Init { project, server, client, site, base_url, seed } => {
            let mut p = Project::new(&server, client.clone(), site);
            p.base_urls = base_url;
            p.signing_seed = seed;
            for i in scan_client(&client)? {
                match i.suggest {
                    Suggest::Thor => p.thor.push(i.path),
                    Suggest::Sync => p.sync.push(i.path),
                    Suggest::Ignore => {}
                }
            }
            p.save(&project)?;
            println!("wrote {} ({} GRF target(s), {} synced file(s)); edit it, then run `otp publish --project`", project.display(), p.thor.len(), p.sync.len());
        }
        Cmd::Publish { project, dry_run, grf_password, json } => {
            let p = Project::load(&project)?;
            let key = GrfKey::from_password(grf_password.as_deref())?.0;
            let r = publish(&p, key, dry_run, &mut |s| {
                if !json {
                    println!("{s}")
                }
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                println!(
                    "{}: {} patch(es), {} file(s) updated, {} baseline(s)",
                    if dry_run { "dry run" } else { "done" },
                    r.patches_added.len(),
                    r.files_updated.len(),
                    r.baselines.len()
                );
            }
        }
        Cmd::Patch { url, client, pub_key, grf_password, keep_backup, dry_run, check, header, header_host, fallback_url, proxy, legacy_grf, parallel, max_kbps } => {
            let mut cfg = Config::new(url, client);
            for s in grf_password {
                let (n, p) = s.split_once('=').ok_or_else(|| otp_core::msg("--grf-password needs name=password"))?;
                cfg.grf_passwords.insert(n.to_string(), p.to_string());
            }
            for h in header {
                let (k, v) = h.split_once(':').ok_or_else(|| otp_core::msg("--header needs \"Name: value\""))?;
                cfg.headers.push((k.trim().to_string(), v.trim().to_string()));
            }
            cfg.header_hosts = header_host;
            cfg.fallback_urls = fallback_url;
            cfg.proxy = proxy;
            cfg.pub_key = pub_key;
            cfg.backup = keep_backup;
            cfg.dry_run = dry_run;
            cfg.force_check = check;
            cfg.legacy_default_grf = legacy_grf;
            cfg.parallel = parallel;
            cfg.max_kbps = max_kbps;
            let mut last: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
            let s = run(&cfg, &mut |e| match e {
                Event::Info(s) => println!("{s}"),
                Event::Download { name, done, total } => {
                    let l = last.entry(name.clone()).or_insert(0);
                    if (done == total && *l != done) || done.saturating_sub(*l) > 8 << 20 {
                        println!("  {name}: {}/{}", human(done), human(total));
                        *l = done;
                    }
                }
                Event::Total { .. } => {}
                Event::Applied { target, id } => println!("  {target}: patch {id} applied"),
                Event::Removed(p) => println!("  removed {p}"),
            })?;
            println!(
                "done: {} file(s), {} patch(es), {} repaired, {} removed{}",
                s.files_updated.len(),
                s.patches_applied.len(),
                s.repaired.len(),
                s.removed.len(),
                if s.data_ini_written { ", DATA.INI updated" } else { "" }
            );
        }
        Cmd::SelfUpdate { url, pub_key, exe, current, platform, fallback_url } => {
            let mut cfg = Config::new(url, exe.parent().map(PathBuf::from).unwrap_or_default());
            cfg.pub_key = Some(pub_key);
            cfg.fallback_urls = fallback_url;
            let list = otp_core::sync::fetch_list(&cfg)?;
            let platform = platform.unwrap_or_else(|| otp_core::selfupdate::platform().to_string());
            match otp_core::selfupdate::available(&list, &current, &platform) {
                None => println!("no newer patcher for {platform} (have {current})"),
                Some((v, f)) => {
                    let f = f.clone();
                    let v = v.to_string();
                    otp_core::selfupdate::update(&cfg, &list, &f, &exe, &mut |_| {})?;
                    println!("updated {} from {current} to {v}", exe.display());
                }
            }
        }
        Cmd::Fetch { url, out } => {
            let mut last = 0u64;
            otp_core::fetch::download(&url, &out, 0, "", &mut |done, total| {
                if done.saturating_sub(last) > 8 << 20 {
                    println!("  {}/{}", human(done), human(total));
                    last = done;
                }
                true
            })?;
            let size = std::fs::metadata(&out).map_err(|e| otp_core::io_err(&out, e))?.len();
            println!("{}: {} bytes, sha256 {}", out.display(), size, otp_core::sha256_file(&out)?);
        }
        Cmd::Package { project, out, patcher, patcher_json, include_external } => {
            let p = Project::load(&project)?;
            let patcher_json = match patcher_json {
                Some(f) => Some(std::fs::read_to_string(&f).map_err(|e| otp_core::io_err(&f, e))?),
                None => None,
            };
            let r = otp_core::package::package(&p, &otp_core::package::Options { out, patcher, patcher_json, include_external }, &mut |s| println!("{s}"))?;
            for w in &r.warnings {
                println!("warning: {w}");
            }
            if !r.left_out.is_empty() {
                println!("left out (the patcher downloads them): {}", r.left_out.join(", "));
            }
            println!("{}: {} files, {} bytes, sha256 {}", r.out.display(), r.files, r.size, r.sha256);
        }
        Cmd::Upload { project, site, package, external } => {
            let p = Project::load(&project)?;
            if !site && package.is_none() && !external {
                return Err(otp_core::msg("give --site, --package <zip> and/or --external"));
            }
            let mut log = |s: &str| println!("{s}");
            let mut all = Vec::new();
            if external {
                all.extend(otp_core::upload::upload_external(&p, &mut log)?);
            }
            if site {
                all.extend(otp_core::upload::upload_site(&p, &mut log)?);
            }
            if let Some(z) = package {
                all.extend(otp_core::upload::upload_file(&p, &z, &p.upload.package, "package", &mut log)?);
            }
            for u in all {
                println!("uploaded {} to {}{}", u.what, u.target, u.link.map(|l| format!(" ({l})")).unwrap_or_default());
            }
        }
        Cmd::News { from } => {
            for n in otp_core::midgard::fetch_news(&from)? {
                println!("{} {}\n{}\n", n.date, n.title, n.body);
            }
        }
        Cmd::Pack { url, client, id, on, off } => {
            if !on && !off {
                return Err(otp_core::msg("give --on or --off"));
            }
            let cfg = Config::new(url, client);
            let changed = set_pack(&cfg, &id, on)?;
            println!("pack {id} {}; DATA.INI {}", if on { "on" } else { "off" }, if changed { "updated" } else { "unchanged" });
        }
    }
    Ok(())
}
