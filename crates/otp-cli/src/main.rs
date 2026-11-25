use clap::{Parser, Subcommand};
use otp_core::publish::{publish, PublishOptions};
use otp_core::sync::{run, Config, Event};
use otp_core::{apply::GrfKey, sign};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "otp", version, about = "OpenThorPatcher: publish a master client, patch player clients")]
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
    /// Build or update a patch site from a master client folder
    Publish {
        #[arg(long)]
        client: PathBuf,
        #[arg(long)]
        site: PathBuf,
        #[arg(long)]
        server: String,
        /// Mirror base URL, repeatable (first one is tried first)
        #[arg(long = "base-url", required = true)]
        base_url: Vec<String>,
        /// GRF patched entry by entry with Thor patches, relative to the client, repeatable
        #[arg(long = "thor")]
        thor: Vec<String>,
        /// File synced whole by hash (small GRFs, dll, exe), relative to the client, repeatable
        #[arg(long = "sync")]
        sync: Vec<String>,
        #[arg(long)]
        seed: Option<PathBuf>,
        /// Password of encrypted master GRFs
        #[arg(long)]
        grf_password: Option<String>,
        /// Also publish whole GRFs for repair
        #[arg(long)]
        full: bool,
        /// Close patching with this message (maintenance)
        #[arg(long)]
        closed: Option<String>,
    },
    /// Update a client folder from a patch site
    Patch {
        #[arg(long)]
        url: String,
        #[arg(long)]
        client: PathBuf,
        #[arg(long = "pub")]
        pub_key: Option<PathBuf>,
        /// `name=password` for an encrypted GRF, repeatable
        #[arg(long = "grf-password")]
        grf_password: Vec<String>,
        #[arg(long)]
        no_backup: bool,
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() {
    if let Err(e) = real() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn real() -> otp_core::Result<()> {
    match Cli::parse().cmd {
        Cmd::Keygen { out } => {
            sign::keygen(&out)?;
            println!("wrote {}/signing.seed (keep secret) and patcher.pub", out.display());
        }
        Cmd::Publish { client, site, server, base_url, thor, sync, seed, grf_password, full, closed } => {
            let key = GrfKey::from_password(grf_password.as_deref())?.0;
            let o = PublishOptions {
                client,
                site,
                server,
                base_urls: base_url,
                thor_targets: thor,
                sync_files: sync,
                signing_seed: seed,
                grf_key: key,
                full,
                allow_patching: closed.is_none(),
                message: closed,
            };
            let r = publish(&o, &mut |s| println!("{s}"))?;
            println!("done: {} patch(es), {} file(s) updated, {} baseline(s)", r.patches_added.len(), r.files_updated.len(), r.baselines.len());
        }
        Cmd::Patch { url, client, pub_key, grf_password, no_backup, dry_run } => {
            let mut pw = std::collections::BTreeMap::new();
            for s in grf_password {
                let (n, p) = s.split_once('=').ok_or_else(|| otp_core::msg("--grf-password needs name=password"))?;
                pw.insert(n.to_string(), p.to_string());
            }
            let cfg = Config { patch_url: url, client_dir: client, pub_key, grf_passwords: pw, backup: !no_backup, dry_run };
            let mut last = 0u64;
            let s = run(&cfg, &mut |e| match e {
                Event::Info(s) => println!("{s}"),
                Event::Download { name, done, total } => {
                    if done == total || done.saturating_sub(last) > 8 << 20 {
                        println!("  {name}: {done}/{total}");
                        last = done;
                    }
                }
                Event::Applied { target, id } => println!("  {target}: patch {id} applied"),
            })?;
            println!("done: {} file(s), {} patch(es), {} repaired", s.files_updated.len(), s.patches_applied.len(), s.repaired.len());
        }
    }
    Ok(())
}
