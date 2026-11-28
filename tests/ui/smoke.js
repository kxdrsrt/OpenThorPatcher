const { JSDOM } = require("jsdom");
const fs = require("fs");
const base = require("path").join(__dirname, "../../apps") + "/";
async function mk(app, mock) {
  const dir = base + app + "/ui/";
  const html = fs.readFileSync(dir + "index.html", "utf8").replace('<script src="app.js"></script>', "");
  const dom = new JSDOM(html, { runScripts: "outside-only", pretendToBeVisual: true, url: "https://app.test/" });
  const w = dom.window; w.errors = [];
  w.addEventListener("error", (e) => w.errors.push(e.message));
  w.HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  w.HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
  w.__TAURI__ = mock(w);
  // the scripts index.html loads before app.js (the patcher's languages); the Maker has none
  for (const f of ["i18n.js", "locales/en.js", "locales/de.js"]) if (fs.existsSync(dir + f)) w.eval(fs.readFileSync(dir + f, "utf8"));
  w.eval(fs.readFileSync(dir + "app.js", "utf8"));
  return w;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ok = (c, m) => { console.log((c ? "PASS " : "FAIL ") + m); if (!c) process.exitCode = 1; };
(async () => {
  // ---- maker
  const project = { server: "TestRO", client: "/m", site: "/s", base_urls: ["https://p.example/"], thor: ["data.grf"], sync: ["odin.grf"], remove: ["old.dll"], signing_seed: "/k/signing.seed", full: true, news_url: "", notice: "", game: null, data_ini: { order: ["odin.grf", "@pack:comp"], packs: [{ id: "comp", file: "odin-dark.grf", title: "Comp", default: true, locked: false }] }, url_map: {}, allow_patching: true, message: "" };
  const saved = [], calls = [];
  let w = await mk("maker", (w) => ({
    core: { invoke: async (c, a) => { switch (c) {
      case "app_version": return "0.1.0";
      case "load_project": return JSON.parse(JSON.stringify(project));
      case "save_project": saved.push(a.project); return null;
      case "scan": return [{ path: "data.grf", size: 2.5e9, suggest: "thor" }, { path: "odin.grf", size: 3e6, suggest: "sync" }, { path: "ragexe.exe", size: 8e6, suggest: "sync" }];
      case "site_status": return { published: true, server: "TestRO", updated: "2026-11-25T00:00:00Z", patches: 3, files: 2, targets: 1, signed: true };
      case "read_pub": return "ab".repeat(32);
      case "keys_backup": calls.push(["keys_backup", a]); return 2;
      case "keys_restore": calls.push(["keys_restore", a]); return [{ path: "/k/signing.seed", action: "written" }, { path: "/k/patcher.pub", action: "kept" }];
      case "patcher_config": return JSON.stringify({ name: a.project.server, pub_key: a.pubHex, h: a.headerName });
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
  d.querySelector("nav button[data-tab=players]").click(); await sleep(30);
  d.getElementById("btn-server-std").click();
  ok(d.querySelectorAll("#servers-list .item").length === 3, "maker: login/char/map servers");
  const gm = d.getElementById("g-max"); gm.value = "1"; gm.dispatchEvent(new w.Event("input"));
  d.querySelector("nav button[data-tab=news]").click(); await sleep(30);
  d.getElementById("btn-link-add").click();
  ok(d.querySelectorAll("#links-list .item").length === 1, "maker: link added");
  d.getElementById("btn-news-add").click(); d.getElementById("btn-change-add").click();
  ok(d.querySelectorAll("#news-list .card").length === 1 && d.querySelectorAll("#change-list .card").length === 1, "maker: news and changelog entries added");
  const ta = d.querySelector("#change-list textarea"); ta.value = "a\nb\n"; ta.dispatchEvent(new w.Event("input"));
  const nsel = d.querySelector("#news-list select[data-lang]"); nsel.value = "de"; nsel.dispatchEvent(new w.Event("change"));
  d.querySelector('#news-list [data-act="copy"]').click(); await sleep(20);
  const langs = [...d.querySelectorAll("#news-list select[data-lang]")].map((x) => x.value).join(",");
  ok(langs === "de,en", "maker: news language and copy for the other language " + langs);
  d.querySelector("nav button[data-tab=hosting]").click(); await sleep(80);
  ok(d.querySelectorAll("#links tbody tr").length === 2, "maker: upload list (patchlist excluded)");
  const li = d.querySelector("#links tbody tr input"); li.value = "https://drive.google.com/file/d/X/view"; li.dispatchEvent(new w.Event("input"));
  ok(d.getElementById("links-summary").textContent.includes("1 of 2"), "maker: link summary");
  d.querySelector("nav button[data-tab=publish]").click(); await sleep(30);
  d.getElementById("btn-plan").click(); await sleep(150);
  ok(!d.getElementById("plan-wrap").hidden && d.querySelectorAll("#plan tbody tr").length === 2, "maker: plan table");
  ok(saved.length >= 1 && saved[saved.length - 1].url_map["files/odin.grf"], "maker: saved before plan with link");
  ok(saved[saved.length - 1].news.map((n) => n.lang).join(",") === "de,en", "maker: news languages saved");
  ok(saved[saved.length - 1].game && saved[saved.length - 1].game.max_instances === 1, "maker: game windows limit saved as a number");
  // server move: other addresses of the patch site
  d.querySelector("nav button[data-tab=hosting]").click(); await sleep(80);
  ok(d.querySelectorAll("#site-list .item").length === 0 && d.getElementById("site-list").textContent.includes("only needed when"), "maker: no other patch site address by default");
  d.getElementById("site-new").value = "ftp://x"; d.getElementById("btn-site-add").click(); await sleep(30);
  ok(d.getElementById("toast").textContent.includes("starts with https") && d.querySelectorAll("#site-list .item").length === 0, "maker: a bad patch site address is refused");
  d.getElementById("site-new").value = "https://new-host.example.com/"; d.getElementById("btn-site-add").click(); await sleep(30);
  d.getElementById("site-new").value = "https://new-host.example.com"; d.getElementById("btn-site-add").click(); await sleep(30);
  ok(d.querySelectorAll("#site-list .item").length === 1 && !d.getElementById("dirty").hidden, "maker: a patch site address is added once (a trailing slash is the same address)");
  d.querySelector("#site-list .item button").click(); await sleep(30);
  ok(d.querySelectorAll("#site-list .item").length === 0, "maker: the address can be removed again");

  // key backup: one password-protected file, "not backed up" until it was done
  d.querySelector("nav button[data-tab=project]").click(); await sleep(30);
  const dlg = d.getElementById("keys-dlg"), go = () => d.getElementById("keys-form").dispatchEvent(new w.Event("submit", { cancelable: true }));
  ok(d.getElementById("keys-state").textContent.includes("Not backed up yet") && d.getElementById("keys-state").classList.contains("warn"), "maker: warns while the key was never backed up");
  d.getElementById("btn-keys-backup").click(); await sleep(30);
  ok(dlg.hasAttribute("open") && !d.getElementById("keys-pw2-row").hidden, "maker: the password dialog opens");
  d.getElementById("keys-pw").value = "short"; go(); await sleep(30);
  ok(d.getElementById("keys-err").textContent.includes("8 characters"), "maker: a short password is refused");
  d.getElementById("keys-pw").value = "long enough pw"; d.getElementById("keys-pw2").value = "another one"; go(); await sleep(30);
  ok(d.getElementById("keys-err").textContent.includes("differ"), "maker: two different passwords are refused");
  d.getElementById("keys-pw2").value = "long enough pw"; go(); await sleep(120);
  const bk = calls.find((c) => c[0] === "keys_backup");
  ok(bk && bk[1].out === "/x/p.json" && bk[1].password === "long enough pw", "maker: the backup is made with the chosen file and password");
  ok(!dlg.hasAttribute("open") && d.getElementById("keys-state").textContent.includes("backed up on") && !d.getElementById("keys-state").classList.contains("warn"), "maker: afterwards the date is shown, no warning");
  d.getElementById("btn-keys-restore").click(); await sleep(30);
  d.getElementById("keys-pw").value = "long enough pw"; go(); await sleep(150);
  ok(d.getElementById("toast").textContent.includes("1 written") && d.getElementById("toast").textContent.includes("1 differ"), "maker: restore reports what was written and what was kept");
  ok(w.errors.length === 0, "maker: no script errors " + w.errors.join(";"));
  // ---- patcher
  const mkp = (summary, extra = {}) => (w) => ({
    core: { invoke: async (c, a) => { switch (c) {
      case "init": return { name: "TestRO", client_dir: "/c", auto_update: false, keep_backup: true, error: null, version: "0.1.0", language: extra.language || "auto" };
      case "get_locales": return extra.locales || {};
      case "set_settings": w.savedLang = a.language; return null;
      case "get_theme": return null;
      case "server_status": return [{ name: "Login", ok: true, ms: 12 }, { name: "Char", ok: true, ms: 15 }, { name: "Map", ok: false, ms: 2500 }];
      case "open_url": w.opened = (w.opened || []).concat(a.url); return null;
      case "check": return Object.assign({ server: "TestRO", notice: "Welcome!", news: [{ date: "2026-11-25", title: "Hello <b>", body: "Para one\n\n- item **bold**\n- [link](https://x.y/z)" }], changelog: [{ date: "2026-11-25", version: "Patch 3", title: "T", items: ["one", "two"] }], website: "https://burning.example", links: [{ title: "Discord", url: "https://discord.gg/x" }, { title: "bad", url: "javascript:alert(1)" }], news_url: "", open: true, message: "", packs: [{ id: "comp", title: "Competitive", on: true, locked: false }], can_play: true, can_setup: true, summary }, extra);
      case "patch": return summary;
      case "play": w.plays = (w.plays || 0) + 1; if (w.playErr) throw w.playErr; return null;
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
  ok(!d.getElementById("btn-web").hidden, "patcher: website button");
  ok(d.querySelectorAll("#srv .srv-item").length === 3 && d.querySelectorAll("#srv .srv-item.bad").length === 1, "patcher: server status dots");
  ok(d.querySelector("#foot-links a use").getAttribute("href") === "#b-discord", "patcher: Discord link icon");
  ok(d.getElementById("btn-setup").textContent.includes("RO Setup"), "patcher: RO Setup label");
  d.getElementById("btn-web").click(); await sleep(20);
  ok(d.querySelectorAll("#foot-links a").length === 1, "patcher: footer links (non-http dropped)");
  d.querySelector("#p-news a").click(); await sleep(20);
  ok(w.opened && w.opened.includes("https://burning.example") && w.opened.some((u) => u.startsWith("https://x.y")), "patcher: website and news links open in the browser");
  d.querySelector('#tabs button[data-tab=changelog]').click();
  ok(d.getElementById("p-changelog").classList.contains("active") && !d.getElementById("p-news").classList.contains("active"), "patcher: tab switch");
  ok(d.querySelectorAll("#packs label").length === 1, "patcher: pack switch");
  w = await mk("patcher", mkp({ files_updated: [], patches_applied: [], repaired: [], removed: [] }));
  await sleep(150); d = w.document;
  ok(d.getElementById("state-title").textContent === "Up to date" && d.getElementById("btn-update").hidden && !d.getElementById("btn-play").disabled, "patcher: up to date, Play enabled");
  ok(!d.getElementById("btn-setup").hidden, "patcher: setup button");
  d.getElementById("btn-play").click(); await sleep(30); d.getElementById("btn-play").click(); await sleep(30);
  ok(w.plays === 1 && d.getElementById("btn-play").disabled, "patcher: Play is locked after a start (second click ignored)");
  w = await mk("patcher", mkp({ files_updated: [], patches_applied: [], repaired: [], removed: [] }));
  await sleep(150); d = w.document; w.playErr = "The game is already running. Only one game window is allowed.";
  d.getElementById("btn-play").click(); await sleep(30);
  ok(d.getElementById("toast").classList.contains("warn") && d.getElementById("toast").textContent.includes("already running"), "patcher: refused start shown as a warning");
  w = await mk("patcher", mkp({ files_updated: [], patches_applied: [], repaired: [], removed: [] }, { open: false, message: "Maintenance" }));
  await sleep(150); d = w.document;
  ok(d.getElementById("state-title").textContent === "Patching is closed", "patcher: maintenance");
  // ---- languages
  const deNews = [{ date: "2026-11-26", title: "Hallo", body: "Deutsch", lang: "de" }, { date: "2026-11-26", title: "Hello", body: "English", lang: "en" }, { date: "2026-11-25", title: "Both", body: "für alle" }];
  w = await mk("patcher", mkp({ files_updated: [], patches_applied: [], repaired: [], removed: [] }, { language: "de", news: deNews, changelog: [{ date: "x", items: ["eins"], lang: "de" }, { date: "x", items: ["one"], lang: "en" }], packs: [{ id: "comp", title: "Competitive", titles: { de: "Darkworld-Karten" }, on: false, locked: false }] }));
  await sleep(150); d = w.document;
  ok(d.getElementById("state-title").textContent === "Aktuell" && d.documentElement.lang === "de", "patcher: German from the player's setting");
  ok(d.querySelector("#btn-play span").textContent === "Spielen" && d.getElementById("btn-settings").title === "Einstellungen", "patcher: static texts in German");
  const titles = [...d.querySelectorAll("#p-news h4")].map((h) => h.textContent);
  ok(titles.join("|") === "Hallo|Both", "patcher: German news plus news without a language, English left out");
  ok(d.querySelectorAll("#p-changelog li").length === 1 && d.querySelector("#p-changelog li").textContent === "eins", "patcher: changelog in German");
  ok(d.querySelector("#packs label span").textContent === "Darkworld-Karten", "patcher: pack title in German");
  ok(w.eval('I18N.err("The game is already running 3 times. No more game windows are allowed.")') === "Das Spiel läuft schon 3-mal. Mehr Spielfenster sind nicht erlaubt.", "patcher: program messages translated");
  ok(w.eval('I18N.err("something new")') === "something new", "patcher: unknown messages stay as they are");
  d.getElementById("btn-settings").click(); await sleep(20);
  const sel = d.getElementById("s-lang");
  ok([...sel.options].map((o) => o.value).join(",") === "auto,de,en", "patcher: language list (auto, Deutsch, English)");
  sel.value = "en"; sel.dispatchEvent(new w.Event("change")); await sleep(20);
  ok(d.getElementById("state-title").textContent === "Up to date" && d.querySelector("#p-news h4").textContent === "Hello", "patcher: switching to English re-renders state and news");
  d.getElementById("dlg").dispatchEvent(new w.Event("close")); await sleep(20);
  ok(w.savedLang === "en", "patcher: the language is saved");
  w = await mk("patcher", mkp({ files_updated: [], patches_applied: [], repaired: [], removed: [] }, { language: "fr", locales: { fr: { _name: "Français", play: "Jouer", "state.ok": "À jour" } } }));
  await sleep(150); d = w.document;
  ok(d.querySelector("#btn-play span").textContent === "Jouer" && d.getElementById("state-title").textContent === "À jour" && d.getElementById("btn-check").textContent.includes("Check files"), "patcher: a language from the server's theme folder, English for missing keys");
  ok(w.errors.length === 0, "patcher: no script errors " + w.errors.join(";"));
  process.exit();
})();
