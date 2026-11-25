const { JSDOM } = require("jsdom");
const fs = require("fs");
const base = require("path").join(__dirname, "../../apps") + "/";
async function mk(app, mock) {
  const dir = base + app + "/ui/";
  const html = fs.readFileSync(dir + "index.html", "utf8").replace('<script src="app.js"></script>', "");
  const dom = new JSDOM(html, { runScripts: "outside-only", pretendToBeVisual: true });
  const w = dom.window; w.errors = [];
  w.addEventListener("error", (e) => w.errors.push(e.message));
  w.HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  w.__TAURI__ = mock(w);
  w.eval(fs.readFileSync(dir + "app.js", "utf8"));
  return w;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ok = (c, m) => { console.log((c ? "PASS " : "FAIL ") + m); if (!c) process.exitCode = 1; };
(async () => {
  // ---- maker
  const project = { server: "TestRO", client: "/m", site: "/s", base_urls: ["https://p.example/"], thor: ["data.grf"], sync: ["odin.grf"], remove: ["old.dll"], signing_seed: "/k/signing.seed", full: true, news_url: "", notice: "", game: null, data_ini: { order: ["odin.grf", "@pack:comp"], packs: [{ id: "comp", file: "odin-dark.grf", title: "Comp", default: true, locked: false }] }, url_map: {}, allow_patching: true, message: "" };
  const saved = [];
  let w = await mk("maker", (w) => ({
    core: { invoke: async (c, a) => { switch (c) {
      case "app_version": return "0.1.0";
      case "load_project": return JSON.parse(JSON.stringify(project));
      case "save_project": saved.push(a.project); return null;
      case "scan": return [{ path: "data.grf", size: 2.5e9, suggest: "thor" }, { path: "odin.grf", size: 3e6, suggest: "sync" }, { path: "ragexe.exe", size: 8e6, suggest: "sync" }];
      case "site_status": return { published: true, server: "TestRO", updated: "2026-11-25T00:00:00Z", patches: 3, files: 2, targets: 1, signed: true };
      case "read_pub": return "ab".repeat(32);
      case "patcher_config": return JSON.stringify({ name: a.project.server, pub_key: a.pubHex });
      case "files_to_upload": return [{ rel: "files/odin.grf", size: 3e6, url: "" }, { rel: "patches/0001-data.thor", size: 1000, url: "" }, { rel: "patchlist.json", size: 10, url: "" }];
      case "export_theme": return "/x/patcher-theme";
      case "plan": return { dry_run: true, plan: [{ kind: "patch", name: "patches/0002-data.thor", detail: "1 new", size: 99 }, { kind: "remove", name: "old.dll", detail: "x", size: 0 }], patches_added: [], files_updated: [] };
      default: return null; } } },
    dialog: { open: async () => "/x/p.json", save: async () => "/x/p.json" },
    event: { listen: async () => () => {} },
  }));
  await sleep(50);
  let d = w.document;
  ok(!d.getElementById("welcome").hidden, "maker: welcome without project");
  d.getElementById("btn-open").click(); await sleep(100);
  ok(d.getElementById("welcome").hidden && !d.body.classList.contains("no-project"), "maker: project opened");
  ok(d.getElementById("f-server").value === "TestRO", "maker: server field");
  await sleep(50);
  ok(d.getElementById("f-pub").value.startsWith("abab"), "maker: public key shown");
  ok(d.querySelectorAll("#status-stats div").length === 5, "maker: site status");
  d.querySelector("nav button[data-tab=files]").click(); await sleep(80);
  ok(d.querySelectorAll("#files tbody tr").length === 3, "maker: files table");
  d.querySelector('#files tbody tr:nth-child(3) input[value=sync]').checked = true;
  d.querySelector('#files tbody tr:nth-child(3) input[value=sync]').dispatchEvent(new w.Event("change"));
  ok(d.getElementById("scan-summary").textContent.includes("1 patched") && d.getElementById("scan-summary").textContent.includes("2 synced"), "maker: mode change summary");
  ok(!d.getElementById("dirty").hidden, "maker: dirty flag");
  d.querySelector("nav button[data-tab=remove]").click(); await sleep(30);
  d.getElementById("remove-new").value = "oldfolder/*"; d.getElementById("btn-remove-add").click();
  ok(d.querySelectorAll("#remove-list .item").length === 2, "maker: removal added");
  d.querySelector("nav button[data-tab=players]").click(); await sleep(30);
  ok(d.querySelectorAll("#order-list .item").length === 2 && d.querySelectorAll("#packs tbody tr").length === 1, "maker: DATA.INI order and packs");
  d.querySelector("nav button[data-tab=news]").click(); await sleep(30);
  d.getElementById("btn-news-add").click(); d.getElementById("btn-change-add").click();
  ok(d.querySelectorAll("#news-list .card").length === 1 && d.querySelectorAll("#change-list .card").length === 1, "maker: news and changelog entries added");
  const ta = d.querySelector("#change-list textarea"); ta.value = "a\nb\n"; ta.dispatchEvent(new w.Event("input"));
  d.querySelector("nav button[data-tab=hosting]").click(); await sleep(80);
  ok(d.querySelectorAll("#links tbody tr").length === 2, "maker: upload list (patchlist excluded)");
  const li = d.querySelector("#links tbody tr input"); li.value = "https://drive.google.com/file/d/X/view"; li.dispatchEvent(new w.Event("input"));
  ok(d.getElementById("links-summary").textContent.includes("1 of 2"), "maker: link summary");
  d.querySelector("nav button[data-tab=publish]").click(); await sleep(30);
  d.getElementById("btn-plan").click(); await sleep(150);
  ok(!d.getElementById("plan-wrap").hidden && d.querySelectorAll("#plan tbody tr").length === 2, "maker: plan table");
  ok(saved.length >= 1 && saved[saved.length - 1].url_map["files/odin.grf"], "maker: saved before plan with link");
  ok(w.errors.length === 0, "maker: no script errors " + w.errors.join(";"));
  // ---- patcher
  const mkp = (summary, extra = {}) => (w) => ({
    core: { invoke: async (c, a) => { switch (c) {
      case "init": return { name: "TestRO", client_dir: "/c", auto_update: false, keep_backup: true, error: null, version: "0.1.0" };
      case "get_theme": return null;
      case "check": return Object.assign({ server: "TestRO", notice: "Welcome!", news: [{ date: "2026-11-25", title: "Hello <b>", body: "Para one\n\n- item **bold**\n- [link](https://x.y/z)" }], changelog: [{ date: "2026-11-25", version: "Patch 3", title: "T", items: ["one", "two"] }], news_url: "", open: true, message: "", packs: [{ id: "comp", title: "Competitive", on: true, locked: false }], can_play: true, can_setup: true, summary }, extra);
      case "patch": return summary;
      default: return null; } } },
    dialog: { open: async () => "/c2" }, event: { listen: async () => () => {} },
  });
  w = await mk("patcher", mkp({ files_updated: ["a"], patches_applied: [1, 2], repaired: [], removed: [] }));
  await sleep(150); d = w.document;
  ok(d.getElementById("state-title").textContent === "Updates available", "patcher: updates shown");
  ok(!d.getElementById("btn-update").hidden && d.getElementById("btn-update").classList.contains("primary"), "patcher: Update is primary");
  ok(d.querySelectorAll("#p-news .news-item").length === 1 && d.querySelector("#p-news h4").textContent === "Hello <b>", "patcher: news rendered and escaped");
  ok(d.querySelectorAll("#p-news li").length === 2 && d.querySelector("#p-news strong") && d.querySelector("#p-news a").href.startsWith("https://x.y"), "patcher: news markup");
  ok(d.querySelectorAll("#p-changelog .change-entry li").length === 2, "patcher: changelog");
  d.querySelector('#tabs button[data-tab=changelog]').click();
  ok(d.getElementById("p-changelog").classList.contains("active") && !d.getElementById("p-news").classList.contains("active"), "patcher: tab switch");
  ok(d.querySelectorAll("#packs label").length === 1, "patcher: pack switch");
  w = await mk("patcher", mkp({ files_updated: [], patches_applied: [], repaired: [], removed: [] }));
  await sleep(150); d = w.document;
  ok(d.getElementById("state-title").textContent === "Up to date" && d.getElementById("btn-update").hidden && !d.getElementById("btn-play").disabled, "patcher: up to date, Play enabled");
  ok(!d.getElementById("btn-setup").hidden, "patcher: setup button");
  w = await mk("patcher", mkp({ files_updated: [], patches_applied: [], repaired: [], removed: [] }, { open: false, message: "Maintenance" }));
  await sleep(150); d = w.document;
  ok(d.getElementById("state-title").textContent === "Patching is closed", "patcher: maintenance");
  ok(w.errors.length === 0, "patcher: no script errors " + w.errors.join(";"));
  process.exit();
})();
