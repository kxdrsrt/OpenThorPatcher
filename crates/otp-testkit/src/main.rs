//! Test helper for `tests/e2e.sh`: builds and unpacks GRFs and makes Thor patches, with the same flags the e2e script
//! used to take from the RustyGRF CLI, so the tests need nothing but this repository.
//!
//!   otp-testkit build [--password P --encrypt] -o OUT.grf SRC_DIR
//!   otp-testkit extract [--password P] IN.grf DEST_DIR
//!   otp-testkit thor [--merge-into-grf] [--target GRF] [--remove ARCHIVE_PATH]... -o OUT.thor SRC_DIR

use std::path::{Path, PathBuf};

use rgrf_core::build::{build, BuildOptions};
use rgrf_core::extract::{extract, ExtractOptions};
use rgrf_core::grfcrypt::key_from_password;
use rgrf_core::progress::Control;
use rgrf_core::thor::{self, ThorFile, ThorOptions};
use rgrf_core::{Archive, GrfPath};

type R<T> = Result<T, String>;

fn e(x: impl std::fmt::Display) -> String {
    x.to_string()
}

struct Opts {
    password: Option<String>,
    encrypt: bool,
    merge: bool,
    target: String,
    remove: Vec<String>,
    output: Option<PathBuf>,
    rest: Vec<String>,
}

fn parse(args: &[String]) -> R<Opts> {
    let mut o = Opts { password: None, encrypt: false, merge: false, target: String::new(), remove: vec![], output: None, rest: vec![] };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "-q" => {}
            "--password" => o.password = Some(val("--password")?),
            "--encrypt" => o.encrypt = true,
            "--merge-into-grf" => o.merge = true,
            "--target" => o.target = val("--target")?,
            "--remove" => o.remove.push(val("--remove")?),
            "-o" => o.output = Some(PathBuf::from(val("-o")?)),
            _ => o.rest.push(a.clone()),
        }
    }
    Ok(o)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> R<()> {
    let mut items: Vec<_> = std::fs::read_dir(dir).map_err(e)?.filter_map(|x| x.ok()).collect();
    items.sort_by_key(|x| x.file_name());
    for it in items {
        let p = it.path();
        if p.is_dir() {
            walk(root, &p, out)?;
        } else {
            let rel = p.strip_prefix(root).map_err(e)?.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("\\");
            out.push((rel, p));
        }
    }
    Ok(())
}

fn run(args: &[String]) -> R<()> {
    let (cmd, rest) = args.split_first().ok_or("usage: otp-testkit build|extract|thor ...")?;
    let o = parse(rest)?;
    let ctl = Control::default();
    match cmd.as_str() {
        "build" => {
            let [src] = o.rest.as_slice() else { return Err("build needs SRC_DIR".into()) };
            let out = o.output.ok_or("build needs -o")?;
            let mut opts = BuildOptions { overwrite: true, ..Default::default() };
            if o.encrypt {
                opts.encrypt = Some(key_from_password(o.password.as_deref().ok_or("--encrypt needs --password")?).map_err(e)?);
            }
            build(Path::new(src), &out, &opts, &ctl).map_err(e)?;
        }
        "extract" => {
            let [grf, dest] = o.rest.as_slice() else { return Err("extract needs IN.grf DEST_DIR".into()) };
            let a = match &o.password {
                Some(p) => Archive::open_with_key(grf, &key_from_password(p).map_err(e)?),
                None => Archive::open(grf),
            }
            .map_err(e)?;
            let all: Vec<usize> = a.entries().iter().enumerate().filter(|(_, x)| x.is_file()).map(|(i, _)| i).collect();
            let r = extract(&a, &all, Path::new(dest), &ExtractOptions::default(), &ctl).map_err(e)?;
            if r.skipped_encrypted > 0 {
                return Err(format!("{} entries are encrypted and were not extracted", r.skipped_encrypted));
            }
        }
        "thor" => {
            let [src] = o.rest.as_slice() else { return Err("thor needs SRC_DIR".into()) };
            let out = o.output.ok_or("thor needs -o")?;
            let root = Path::new(src);
            let mut found = Vec::new();
            walk(root, root, &mut found)?;
            let mut files: Vec<ThorFile> = Vec::new();
            for (rel, p) in found {
                files.push(ThorFile { name: GrfPath::new(rel.into_bytes()), data: Some(std::fs::read(p).map_err(e)?) });
            }
            for r in &o.remove {
                files.push(ThorFile { name: GrfPath::new(r.clone().into_bytes()), data: None });
            }
            let opts = ThorOptions { merge_into_grf: o.merge, target_grf: o.target.into_bytes(), overwrite: true, ..Default::default() };
            thor::write(&out, &files, &opts).map_err(e)?;
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(m) = run(&args) {
        eprintln!("otp-testkit: {m}");
        std::process::exit(1);
    }
}
