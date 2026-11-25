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

fn real() -> otp_core::Result<()> {
    match Cli::parse().cmd {
        Cmd::Keygen { out } => {
            sign::keygen(&out)?;
            println!("wrote {}/signing.seed (keep secret) and patcher.pub", out.display());
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
        Cmd::Patch { url, client, pub_key, grf_password, keep_backup, dry_run, check, header, header_host, fallback_url, proxy, legacy_grf } => {
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
            let mut last = 0u64;
            let s = run(&cfg, &mut |e| match e {
                Event::Info(s) => println!("{s}"),
                Event::Download { name, done, total } => {
                    if done == total || done.saturating_sub(last) > 8 << 20 {
                        println!("  {name}: {}/{}", human(done), human(total));
                        last = done;
                    }
                }
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
