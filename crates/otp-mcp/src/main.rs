//! OpenThor Patcher MCP server (stdio, newline-delimited JSON-RPC 2.0, tools only).
//!
//! Lets an AI assistant set up a patch project, preview and publish the patch site, and check a client against a
//! site. Tools that write (`otp_project_create`, `otp_project_update`, `otp_publish`, `otp_patch`, `otp_keygen`)
//! need `confirm: true`; `OTP_MCP_READONLY=1` hides them completely. Secrets (signing key, GRF passwords) are never
//! returned.

use otp_core::apply::GrfKey;
use otp_core::project::{scan_client, Project, Suggest};
use otp_core::publish::{publish, site_files};
use otp_core::sync::{run, Config};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;

const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

struct Tool {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    writes: bool,
    /// (name, JSON type, description, required)
    params: &'static [(&'static str, &'static str, &'static str, bool)],
}

const TOOLS: &[Tool] = &[
    Tool {
        name: "otp_scan_client",
        title: "Scan the master client",
        description: "Lists the files of a master client folder with a suggestion per file: thor (big GRF, patched entry by entry), sync (whole file by hash) or ignore.",
        writes: false,
        params: &[("client", "string", "Master client folder", true)],
    },
    Tool {
        name: "otp_project_get",
        title: "Read a project",
        description: "Returns the project file (otp-project.json) of a patch site: server name, mirrors, GRF targets, synced files, removals, DATA.INI packs, cloud links.",
        writes: false,
        params: &[("project", "string", "Path of otp-project.json", true)],
    },
    Tool {
        name: "otp_project_create",
        title: "Create a project",
        description: "Creates a project from a master client folder: big GRFs become Thor targets, small files are synced whole. Does not overwrite an existing project.",
        writes: true,
        params: &[
            ("project", "string", "Path of the new otp-project.json", true),
            ("server", "string", "Server name shown to players", true),
            ("client", "string", "Master client folder", true),
            ("site", "string", "Output folder of the patch site", true),
            ("base_urls", "array", "Mirror base URLs (may be empty when every file gets a full link in url_map)", false),
            ("signing_seed", "string", "Path of signing.seed (see otp_keygen)", false),
            ("confirm", "boolean", "Must be true", true),
        ],
    },
    Tool {
        name: "otp_project_update",
        title: "Change a project",
        description: "Merges the given fields into the project file. Allowed keys: server, base_urls, thor, sync, remove, signing_seed, full, news_url, notice, game {exe,args,setup_exe}, news [{date,title,body}], changelog [{date,version,title,items[]}], data_ini {order,packs[{id,file,title,default,locked}]}, url_map {site path: full link}, allow_patching, message.",
        writes: true,
        params: &[
            ("project", "string", "Path of otp-project.json", true),
            ("changes", "object", "Fields to set (replaces the whole value of each key)", true),
            ("confirm", "boolean", "Must be true", true),
        ],
    },
    Tool {
        name: "otp_publish_plan",
        title: "Preview a publish",
        description: "Dry run: hashes the GRF targets and reports what would be published (new patches with counts, changed files, removals) without writing anything.",
        writes: false,
        params: &[
            ("project", "string", "Path of otp-project.json", true),
            ("grf_password", "string", "Password of encrypted master GRFs", false),
        ],
    },
    Tool {
        name: "otp_publish",
        title: "Publish the patch site",
        description: "Writes the patch site: numbered .thor patches with only the changed entries, whole files, patchlist.json and its signature.",
        writes: true,
        params: &[
            ("project", "string", "Path of otp-project.json", true),
            ("grf_password", "string", "Password of encrypted master GRFs", false),
            ("confirm", "boolean", "Must be true", true),
        ],
    },
    Tool {
        name: "otp_site_files",
        title: "List the files to upload",
        description: "Lists every file of the patch site with its size and the full link from the project's url_map (for cloud hosting where each file has its own share link).",
        writes: false,
        params: &[("project", "string", "Path of otp-project.json", true)],
    },
    Tool {
        name: "otp_patch_check",
        title: "Check a client against a site",
        description: "Dry run of a player update: reports which files and patches a client folder still needs.",
        writes: false,
        params: &[
            ("url", "string", "Folder URL of the patch site (https:// or file://)", true),
            ("client", "string", "Client folder", true),
            ("pub_key", "string", "Public key file (patcher.pub)", false),
        ],
    },
    Tool {
        name: "otp_patch",
        title: "Update a client",
        description: "Updates a client folder from a patch site (downloads, verifies, merges into GRFs, removes files, writes DATA.INI). A GRF that is replaced is kept as .bak.",
        writes: true,
        params: &[
            ("url", "string", "Folder URL of the patch site", true),
            ("client", "string", "Client folder", true),
            ("pub_key", "string", "Public key file (patcher.pub)", false),
            ("confirm", "boolean", "Must be true", true),
        ],
    },
    Tool {
        name: "otp_keygen",
        title: "Create a signing key pair",
        description: "Writes signing.seed (secret) and patcher.pub into a folder; refuses to overwrite an existing key.",
        writes: true,
        params: &[("out", "string", "Folder for the key files", true), ("confirm", "boolean", "Must be true", true)],
    },
];

fn schema(t: &Tool) -> Value {
    let mut props = serde_json::Map::new();
    let mut req = vec![];
    for (n, ty, d, r) in t.params {
        props.insert((*n).into(), json!({"type": ty, "description": d}));
        if *r {
            req.push(json!(n));
        }
    }
    json!({"type": "object", "properties": props, "required": req, "additionalProperties": false})
}

fn s(a: &Value, k: &str) -> Option<String> {
    a.get(k).and_then(Value::as_str).map(str::to_string)
}

fn need(a: &Value, k: &str) -> Result<String, String> {
    s(a, k).filter(|x| !x.is_empty()).ok_or_else(|| format!("missing argument {k}"))
}

fn confirmed(a: &Value) -> Result<(), String> {
    if a.get("confirm").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err("this tool changes files: call it again with confirm: true after the user agreed".into())
    }
}

fn e<T: std::fmt::Display>(x: T) -> String {
    x.to_string()
}

fn call(name: &str, a: &Value) -> Result<Value, String> {
    match name {
        "otp_scan_client" => {
            let items = scan_client(&PathBuf::from(need(a, "client")?)).map_err(e)?;
            Ok(json!({"files": items}))
        }
        "otp_project_get" => {
            let p = Project::load(&PathBuf::from(need(a, "project")?)).map_err(e)?;
            Ok(serde_json::to_value(p).map_err(e)?)
        }
        "otp_project_create" => {
            confirmed(a)?;
            let path = PathBuf::from(need(a, "project")?);
            if path.exists() {
                return Err(format!("{} exists; use otp_project_update", path.display()));
            }
            let client = PathBuf::from(need(a, "client")?);
            let mut p = Project::new(&need(a, "server")?, client.clone(), PathBuf::from(need(a, "site")?));
            if let Some(b) = a.get("base_urls").and_then(Value::as_array) {
                p.base_urls = b.iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
            }
            p.signing_seed = s(a, "signing_seed").map(PathBuf::from);
            for i in scan_client(&client).map_err(e)? {
                match i.suggest {
                    Suggest::Thor => p.thor.push(i.path),
                    Suggest::Sync => p.sync.push(i.path),
                    Suggest::Ignore => {}
                }
            }
            p.save(&path).map_err(e)?;
            Ok(json!({"project": path, "thor": p.thor, "sync": p.sync}))
        }
        "otp_project_update" => {
            confirmed(a)?;
            let path = PathBuf::from(need(a, "project")?);
            let p = Project::load(&path).map_err(e)?;
            let mut v = serde_json::to_value(&p).map_err(e)?;
            let ch = a.get("changes").and_then(Value::as_object).ok_or("changes must be an object")?;
            const ALLOWED: &[&str] = &[
                "server", "base_urls", "thor", "sync", "remove", "signing_seed", "full", "news_url", "notice", "game", "data_ini", "url_map",
                "allow_patching", "message", "news", "changelog",
            ];
            for (k, val) in ch {
                if !ALLOWED.contains(&k.as_str()) {
                    return Err(format!("{k} cannot be changed here (allowed: {})", ALLOWED.join(", ")));
                }
                v[k] = val.clone();
            }
            let np: Project = serde_json::from_value(v).map_err(|x| format!("the changed project is invalid: {x}"))?;
            np.save(&path).map_err(e)?;
            Ok(serde_json::to_value(np).map_err(e)?)
        }
        "otp_publish_plan" | "otp_publish" => {
            let dry = name == "otp_publish_plan";
            if !dry {
                confirmed(a)?;
            }
            let p = Project::load(&PathBuf::from(need(a, "project")?)).map_err(e)?;
            let key = GrfKey::from_password(s(a, "grf_password").as_deref()).map_err(e)?.0;
            let mut log = vec![];
            let r = publish(&p, key, dry, &mut |l| log.push(l.to_string())).map_err(e)?;
            let mut v = serde_json::to_value(r).map_err(e)?;
            v["log"] = json!(log);
            Ok(v)
        }
        "otp_site_files" => {
            let p = Project::load(&PathBuf::from(need(a, "project")?)).map_err(e)?;
            Ok(json!({"files": site_files(&p)}))
        }
        "otp_patch_check" | "otp_patch" => {
            let dry = name == "otp_patch_check";
            if !dry {
                confirmed(a)?;
            }
            let mut cfg = Config::new(need(a, "url")?, PathBuf::from(need(a, "client")?));
            cfg.pub_key = s(a, "pub_key").map(PathBuf::from);
            cfg.dry_run = dry;
            let mut log = vec![];
            let sum = run(&cfg, &mut |ev| {
                if let otp_core::sync::Event::Info(i) = ev {
                    log.push(i)
                }
            })
            .map_err(e)?;
            let mut v = serde_json::to_value(sum).map_err(e)?;
            v["log"] = json!(log);
            Ok(v)
        }
        "otp_keygen" => {
            confirmed(a)?;
            let out = PathBuf::from(need(a, "out")?);
            otp_core::sign::keygen(&out).map_err(e)?;
            Ok(json!({"public_key_file": out.join("patcher.pub"), "secret_key_file": out.join("signing.seed"), "note": "keep signing.seed secret; ship patcher.pub with the patcher"}))
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

fn reply(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: &Value, code: i64, m: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": m}})
}

fn handle(msg: &Value, readonly: bool) -> Option<Value> {
    let method = msg.get("method")?.as_str()?;
    let id = msg.get("id")?; // notifications get no reply
    let params = msg.get("params").unwrap_or(&Value::Null);
    Some(match method {
        "initialize" => {
            let wanted = params.get("protocolVersion").and_then(Value::as_str);
            let v = PROTOCOL_VERSIONS.iter().find(|v| Some(**v) == wanted).unwrap_or(&PROTOCOL_VERSIONS[0]);
            reply(
                id,
                json!({
                    "protocolVersion": v,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "otp-mcp", "title": "OpenThor Patcher", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": "OpenThor Patcher builds and applies Ragnarok Online client patches. Typical flow: otp_scan_client, otp_keygen, otp_project_create, otp_project_update (mirrors, data_ini, removals), otp_publish_plan, otp_publish, then upload the folder from otp_site_files. Tools that write need confirm: true.",
                }),
            )
        }
        "ping" => reply(id, json!({})),
        "tools/list" => {
            let tools: Vec<Value> = TOOLS
                .iter()
                .filter(|t| !(readonly && t.writes))
                .map(|t| {
                    json!({"name": t.name, "title": t.title, "description": t.description, "inputSchema": schema(t),
                    "annotations": {"readOnlyHint": !t.writes, "destructiveHint": false, "openWorldHint": false}})
                })
                .collect();
            reply(id, json!({"tools": tools}))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let Some(t) = TOOLS.iter().find(|t| t.name == name && !(readonly && t.writes)) else {
                return Some(error(id, -32602, &format!("unknown tool '{name}'")));
            };
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            for (n, _, _, r) in t.params {
                if *r && args.get(n).is_none() {
                    return Some(reply(id, json!({"content": [{"type": "text", "text": format!("missing argument {n}")}], "isError": true})));
                }
            }
            match call(name, &args) {
                Ok(v) => reply(id, json!({"content": [{"type": "text", "text": v.to_string()}], "structuredContent": v, "isError": false})),
                Err(m) => reply(id, json!({"content": [{"type": "text", "text": m}], "isError": true})),
            }
        }
        other => error(id, -32601, &format!("method '{other}' is not supported")),
    })
}

fn main() {
    let readonly = std::env::var("OTP_MCP_READONLY").is_ok_and(|v| v == "1");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(m) => handle(&m, readonly),
            Err(x) => Some(error(&Value::Null, -32700, &x.to_string())),
        };
        if let Some(r) = reply {
            let _ = writeln!(out, "{r}").and_then(|()| out.flush());
        }
    }
}
