// OpenThor Patch Maker UI. Everything goes through the Tauri commands (otp-core); the UI only edits the project JSON.
"use strict";
const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const icon = (n) => `<svg class="ic" aria-hidden="true"><use href="#i-${n}"/></svg>`;
const el = (tag, cls, html) => { const e = document.createElement(tag); if (cls) e.className = cls; if (html != null) e.innerHTML = html; return e; };
const human = (n) => { const u = ["B", "KB", "MB", "GB"]; let i = 0, v = Number(n) || 0; while (v >= 1024 && i < 3) { v /= 1024; i++; } return (i ? v.toFixed(1) : v) + " " + u[i]; };
const TITLES = { project: "Project", files: "Files", remove: "Removals", players: "Game & DATA.INI", hosting: "Hosting", publish: "Publish" };

let project = null, path = null, tab = "project", dirty = false, scanItems = [], siteFiles = [], toastTimer = 0, pubHex = "";

function status(msg, kind = "ok") {
  const k = kind === true ? "ok" : kind === false ? "bad" : kind;
  const t = $("#toast");
  t.className = "toast " + k;
  t.innerHTML = icon(k === "ok" ? "check" : k === "warn" ? "warn" : "error") + `<span>${esc(msg)}</span>`;
  t.hidden = false;
  clearTimeout(toastTimer);
  t.onclick = () => (t.hidden = true);
  if (k !== "bad") toastTimer = setTimeout(() => (t.hidden = true), 4500);
}

function touch() {
  dirty = true;
  $("#dirty").hidden = false;
  $("#btn-save").disabled = false;
}

function clean() {
  dirty = false;
  $("#dirty").hidden = true;
  $("#btn-save").disabled = !project;
}

const norm = (p) => String(p).replace(/\\/g, "/").replace(/^\.\//, "");
const without = (list, v) => list.filter((x) => norm(x) !== norm(v));

function bind(id, get, set) {
  const i = $(id);
  if (i.type === "checkbox") { i.checked = !!get(); i.onchange = () => { set(i.checked); touch(); }; }
  else { i.value = get() ?? ""; i.oninput = () => { set(i.value); touch(); }; }
}

function renderProject() {
  const p = project;
  bind("#f-server", () => p.server, (v) => (p.server = v));
  bind("#f-client", () => p.client, (v) => (p.client = v));
  bind("#f-site", () => p.site, (v) => (p.site = v));
  bind("#f-seed", () => p.signing_seed || "", (v) => { p.signing_seed = v || null; loadPub(); });
  bind("#f-full", () => p.full, (v) => (p.full = v));
  renderStatus();
  loadPub();
}

async function renderStatus() {
  const box = $("#status-stats");
  try {
    const s = await invoke("site_status", { project });
    box.innerHTML = s.published
      ? `<div><b>${s.patches}</b><span>Patches</span></div><div><b>${s.files}</b><span>Synced files</span></div><div><b>${s.targets}</b><span>Patched GRFs</span></div><div><b>${s.signed ? "Signed" : "Unsigned"}</b><span>Patch list</span></div><div><b style="font-size:13px">${esc(s.updated || "–")}</b><span>Last publish</span></div>`
      : `<div><b>Not published</b><span>Patch site</span></div>`;
  } catch { box.innerHTML = ""; }
}

async function loadPub() {
  const seed = project?.signing_seed;
  pubHex = "";
  if (seed) {
    const dir = seed.replace(/[\\/][^\\/]*$/, "");
    try { pubHex = await invoke("read_pub", { dir }); } catch { pubHex = ""; }
  }
  $("#f-pub").value = pubHex || (seed ? "patcher.pub not found next to the key file" : "No key yet: create a key pair");
  renderConfig();
}

async function renderFiles() {
  try { scanItems = await invoke("scan", { dir: project.client }); } catch (e) { scanItems = []; status("Scan failed: " + e, false); }
  drawFiles();
}

function modeOf(path) {
  const n = norm(path);
  if (project.thor.some((x) => norm(x) === n)) return "thor";
  if (project.sync.some((x) => norm(x) === n)) return "sync";
  return "ignore";
}

function setMode(path, mode) {
  project.thor = without(project.thor, path);
  project.sync = without(project.sync, path);
  if (mode === "thor") project.thor.push(path);
  if (mode === "sync") project.sync.push(path);
  touch();
  summaryFiles();
}

function summaryFiles() {
  $("#scan-summary").textContent = `${project.thor.length} patched entry by entry · ${project.sync.length} synced whole`;
}

function drawFiles() {
  const t = $("#files");
  const known = new Set(scanItems.map((i) => norm(i.path)));
  const missing = [...project.thor, ...project.sync].filter((p) => !known.has(norm(p))).map((p) => ({ path: p, size: 0, missing: true }));
  const rows = scanItems.concat(missing);
  t.innerHTML = '<thead><tr><th>File</th><th class="num">Size</th><th>How players get it</th></tr></thead>';
  const body = el("tbody");
  rows.forEach((r, i) => {
    const mode = modeOf(r.path);
    const tr = el("tr", null, `<td><b>${esc(r.path)}</b>${r.missing ? '<span class="sub" style="color:var(--danger)">not in the master client any more</span>' : ""}</td><td class="num">${r.missing ? "–" : human(r.size)}</td>
      <td><span class="seg" role="radiogroup" aria-label="Mode for ${esc(r.path)}">${[["thor", "Patch entries"], ["sync", "Sync whole"], ["ignore", "Ignore"]].map(([v, l]) => `<label><input type="radio" name="m${i}" value="${v}" ${mode === v ? "checked" : ""}><span>${l}</span></label>`).join("")}</span></td>`);
    tr.querySelectorAll("input").forEach((inp) => inp.addEventListener("change", () => setMode(r.path, inp.value)));
    body.appendChild(tr);
  });
  if (!rows.length) body.appendChild(el("tr", "empty-row", '<td colspan="3">No files found. Check the master client folder on the Project page.</td>'));
  t.appendChild(body);
  summaryFiles();
}

function list(sel, items, label, onremove, extra) {
  const root = $(sel);
  root.innerHTML = "";
  if (!items.length) root.appendChild(el("div", "note", label));
  items.forEach((v, i) => {
    const row = el("div", "item", `<span class="grow mono">${esc(v)}</span>`);
    if (extra) extra(row, i);
    const b = el("button", "btn icon small danger", icon("trash"));
    b.title = "Remove"; b.setAttribute("aria-label", "Remove " + v);
    b.onclick = () => { onremove(i); touch(); };
    row.appendChild(b);
    root.appendChild(row);
  });
}

function renderRemove() {
  list("#remove-list", project.remove, "Nothing is removed.", (i) => { project.remove.splice(i, 1); renderRemove(); });
}

function ensureIni() { project.data_ini = project.data_ini || { order: [], packs: [] }; return project.data_ini; }

function renderPlayers() {
  const g = () => (project.game = project.game || { exe: "", args: "", setup_exe: "" });
  bind("#g-exe", () => project.game?.exe, (v) => (g().exe = v));
  bind("#g-args", () => project.game?.args, (v) => (g().args = v));
  bind("#g-setup", () => project.game?.setup_exe, (v) => (g().setup_exe = v));
  bind("#n-url", () => project.news_url, (v) => (project.news_url = v));
  bind("#n-notice", () => project.notice, (v) => (project.notice = v));
  bind("#m-open", () => project.allow_patching, (v) => (project.allow_patching = v));
  bind("#m-msg", () => project.message, (v) => (project.message = v));
  renderOrder();
  renderPacks();
}

function renderOrder() {
  const ini = project.data_ini || { order: [] };
  list("#order-list", ini.order, "No order set: the patcher leaves DATA.INI alone.", (i) => { ini.order.splice(i, 1); renderOrder(); }, (row, i) => {
    const mk = (ic, d, label) => { const b = el("button", "btn icon small", icon(ic)); b.title = label; b.setAttribute("aria-label", label); b.disabled = i + d < 0 || i + d >= ini.order.length; b.onclick = () => { [ini.order[i], ini.order[i + d]] = [ini.order[i + d], ini.order[i]]; touch(); renderOrder(); }; row.appendChild(b); };
    mk("up", -1, "Move up"); mk("down", 1, "Move down");
    if (String(ini.order[i]).startsWith("@pack:")) row.querySelector(".grow").insertAdjacentHTML("afterend", '<span class="tag">optional pack</span>');
  });
}

function renderPacks() {
  const ini = project.data_ini || { packs: [] };
  const t = $("#packs");
  t.innerHTML = '<thead><tr><th>Id</th><th>GRF file</th><th>Title</th><th>On by default</th><th>Required</th><th></th></tr></thead>';
  const body = el("tbody");
  ini.packs.forEach((p, i) => {
    const tr = el("tr", null, `<td><input value="${esc(p.id)}" size="10" aria-label="Pack id"></td><td><input value="${esc(p.file)}" size="16" aria-label="Pack file"></td><td><input value="${esc(p.title)}" size="16" aria-label="Pack title"></td>
      <td><span class="switch"><input type="checkbox" ${p.default ? "checked" : ""} aria-label="On by default"><i></i></span></td><td><span class="switch"><input type="checkbox" ${p.locked ? "checked" : ""} aria-label="Required"><i></i></span></td>
      <td><button class="btn icon small danger" title="Remove" aria-label="Remove pack">${icon("trash")}</button></td>`);
    const ins = tr.querySelectorAll("input");
    const keys = ["id", "file", "title"];
    keys.forEach((k, n) => ins[n].addEventListener("input", () => { p[k] = ins[n].value; touch(); }));
    ins[3].addEventListener("change", () => { p.default = ins[3].checked; touch(); });
    ins[4].addEventListener("change", () => { p.locked = ins[4].checked; touch(); });
    tr.querySelector("button").onclick = () => { ini.packs.splice(i, 1); touch(); renderPacks(); };
    body.appendChild(tr);
  });
  if (!ini.packs.length) body.appendChild(el("tr", "empty-row", '<td colspan="6">No optional packs.</td>'));
  t.appendChild(body);
}

function renderMirrors() {
  list("#mirror-list", project.base_urls, "No mirror yet: add one, or give every file a cloud link below.", (i) => { project.base_urls.splice(i, 1); renderMirrors(); renderConfig(); });
}

async function renderLinks() {
  try { siteFiles = (await invoke("files_to_upload", { project })).filter((f) => f.rel.startsWith("files/") || f.rel.startsWith("patches/")); } catch { siteFiles = []; }
  const t = $("#links");
  t.innerHTML = '<thead><tr><th>Site file</th><th class="num">Size</th><th>Full link (optional)</th></tr></thead>';
  const body = el("tbody");
  siteFiles.forEach((f) => {
    const tr = el("tr", null, `<td><b>${esc(f.rel)}</b></td><td class="num">${human(f.size)}</td><td><input value="${esc(project.url_map[f.rel] || "")}" placeholder="https://…  (share link)" style="min-width:300px" aria-label="Link for ${esc(f.rel)}"></td>`);
    tr.querySelector("input").addEventListener("input", (e) => { if (e.target.value.trim()) project.url_map[f.rel] = e.target.value.trim(); else delete project.url_map[f.rel]; touch(); linksSummary(); });
    body.appendChild(tr);
  });
  if (!siteFiles.length) body.appendChild(el("tr", "empty-row", '<td colspan="3">Publish once, then the files that need uploading appear here.</td>'));
  t.appendChild(body);
  linksSummary();
}

function linksSummary() {
  const n = Object.keys(project.url_map).length;
  $("#links-summary").textContent = siteFiles.length ? `${n} of ${siteFiles.length} files have a link` : "";
}

async function renderConfig() {
  if (!project) return;
  try {
    $("#cfg-out").textContent = await invoke("patcher_config", { project, pubHex: pubHex || "", listUrl: $("#list-url").value || null });
  } catch (e) { $("#cfg-out").textContent = String(e); }
}

function setTab(name) {
  tab = name;
  document.querySelectorAll("nav button").forEach((b) => { const on = b.dataset.tab === name; b.classList.toggle("active", on); b.setAttribute("aria-current", on ? "page" : "false"); });
  document.querySelectorAll(".tab").forEach((s) => s.classList.toggle("active", s.id === "tab-" + name));
  $("#page-title").textContent = TITLES[name];
  $("#welcome").hidden = !!project;
  $("#main").scrollTop = 0;
  if (!project) return;
  if (name === "files") renderFiles();
  if (name === "hosting") { renderMirrors(); renderLinks(); renderConfig(); }
  if (name === "players") renderPlayers();
  if (name === "remove") renderRemove();
  if (name === "project") renderProject();
}

function render() {
  document.body.classList.toggle("no-project", !project);
  $("#path").textContent = path || "No project open";
  $("#path").title = path || "";
  clean();
  setTab(tab);
}

async function openProject(p) {
  project = await invoke("load_project", { path: p });
  project.url_map = project.url_map || {};
  path = p;
  render();
  status("Project opened.");
}

async function save() {
  if (!project) return;
  try { await invoke("save_project", { path, project }); clean(); status("Saved."); } catch (e) { status("Not saved: " + e, false); }
}

async function openDialog() {
  const p = await T.dialog.open({ filters: [{ name: "OpenThor project", extensions: ["json"] }] });
  if (p) openProject(p).catch((e) => status(String(e), false));
}

async function newProject() {
  const client = await T.dialog.open({ directory: true, title: "Master client folder" });
  if (!client) return;
  const site = await T.dialog.open({ directory: true, title: "Folder for the patch site (output)" });
  if (!site) return;
  const file = await T.dialog.save({ defaultPath: "otp-project.json", filters: [{ name: "OpenThor project", extensions: ["json"] }] });
  if (!file) return;
  const server = (prompt("Server name shown to players", "My Ragnarok Server") || "").trim();
  if (!server) return;
  try {
    project = await invoke("new_project", { path: file, server, client, site });
    project.url_map = project.url_map || {};
    path = file;
    tab = "project";
    render();
    status(`Project created: ${project.thor.length} GRF to patch, ${project.sync.length} files to sync. Check the Files page.`);
  } catch (e) { status(String(e), false); }
}

async function busy(btn, label, fn) {
  const html = btn.innerHTML;
  btn.disabled = true;
  btn.innerHTML = icon("patch") + label;
  $("#busy").hidden = false;
  try { return await fn(); } finally { btn.disabled = false; btn.innerHTML = html; $("#busy").hidden = true; }
}

function drawPlan(r) {
  $("#plan-wrap").hidden = false;
  const t = $("#plan");
  const label = { file: ["file", "Whole file"], patch: ["patch", "Thor patch"], remove: ["ban", "Removal"], baseline: ["flag", "Starting point"], none: ["check", "Up to date"] };
  t.innerHTML = '<thead><tr><th>Kind</th><th>What</th><th>Detail</th><th class="num">Size</th></tr></thead>';
  const body = el("tbody");
  r.plan.forEach((p) => {
    const [ic, l] = label[p.kind] || ["file", p.kind];
    body.appendChild(el("tr", null, `<td><span class="kind ${esc(p.kind)}">${icon(ic)}${l}</span></td><td><b>${esc(p.name)}</b></td><td class="muted">${esc(p.detail)}</td><td class="num">${p.size ? human(p.size) : ""}</td>`));
  });
  t.appendChild(body);
}

async function doPublish(dry) {
  if (!project) return;
  await save();
  $("#log").textContent = "";
  const pass = $("#p-pass").value || null;
  const r = await invoke(dry ? "plan" : "publish_site", { project, grfPassword: pass });
  drawPlan(r);
  if (!dry) { status(`Published: ${r.patches_added.length} patch(es), ${r.files_updated.length} file(s). Upload the site folder.`); renderStatus(); }
  else status(r.plan.length === 1 && r.plan[0].kind === "none" ? "Nothing to publish." : `${r.plan.length} change(s) would be published.`, "ok");
}

document.querySelectorAll("nav button").forEach((b) => b.addEventListener("click", () => setTab(b.dataset.tab)));
$("#btn-open").onclick = openDialog; $("#btn-open2").onclick = openDialog;
$("#btn-new").onclick = newProject; $("#btn-new2").onclick = newProject;
$("#btn-save").onclick = save;
$("#btn-scan").onclick = () => busy($("#btn-scan"), "Scanning…", renderFiles);
$("#btn-refresh-files").onclick = renderLinks;
$("#btn-plan").onclick = () => busy($("#btn-plan"), "Comparing…", () => doPublish(true).catch((e) => status(String(e), false)));
$("#btn-publish").onclick = () => busy($("#btn-publish"), "Publishing…", () => doPublish(false).catch((e) => status(String(e), false)));
$("#btn-reveal").onclick = () => project && invoke("reveal", { path: project.site }).catch((e) => status(String(e), false));
$("#btn-remove-add").onclick = () => { const v = $("#remove-new").value.trim(); if (v) { project.remove.push(v); $("#remove-new").value = ""; touch(); renderRemove(); } };
$("#remove-new").onkeydown = (e) => e.key === "Enter" && $("#btn-remove-add").click();
$("#btn-order-add").onclick = () => { const v = $("#order-new").value.trim(); if (v) { ensureIni().order.push(v); $("#order-new").value = ""; touch(); renderOrder(); } };
$("#order-new").onkeydown = (e) => e.key === "Enter" && $("#btn-order-add").click();
$("#btn-pack-add").onclick = () => { ensureIni().packs.push({ id: "pack" + (project.data_ini.packs.length + 1), file: "", title: "", default: true, locked: false }); touch(); renderPacks(); };
$("#btn-mirror-add").onclick = () => { const v = $("#mirror-new").value.trim(); if (v) { project.base_urls.push(v); $("#mirror-new").value = ""; touch(); renderMirrors(); renderConfig(); } };
$("#mirror-new").onkeydown = (e) => e.key === "Enter" && $("#btn-mirror-add").click();
$("#list-url").oninput = renderConfig;
$("#btn-cfg-copy").onclick = async () => { await navigator.clipboard.writeText($("#cfg-out").textContent); status("patcher.json copied."); };
$("#btn-cfg-save").onclick = async () => {
  const p = await T.dialog.save({ defaultPath: "patcher.json", filters: [{ name: "JSON", extensions: ["json"] }] });
  if (p) invoke("write_text", { path: p, text: $("#cfg-out").textContent }).then(() => status("Saved.")).catch((e) => status(String(e), false));
};
$("#btn-keygen").onclick = async () => {
  const dir = await T.dialog.open({ directory: true, title: "Folder for the new key pair (keep it private)" });
  if (!dir) return;
  try {
    const k = await invoke("make_keys", { dir });
    project.signing_seed = k.seed_path; pubHex = k.pub_hex; touch(); renderProject();
    status("Key pair ready. Back up signing.seed; without it you cannot publish for existing players.");
  } catch (e) { status(String(e), false); }
};
document.querySelectorAll(".pick").forEach((b) => b.addEventListener("click", async () => {
  const p = await T.dialog.open({ directory: !!b.dataset.dir });
  if (p) { const i = $("#" + b.dataset.for); i.value = p; i.dispatchEvent(new Event("input")); }
}));
document.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") { e.preventDefault(); save(); }
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "o") { e.preventDefault(); openDialog(); }
});
window.addEventListener("beforeunload", (e) => { if (dirty) { e.preventDefault(); e.returnValue = ""; } });

(async () => {
  try {
    $("#ver").textContent = "v" + (await invoke("app_version"));
    await T.event.listen("log", (ev) => { const l = $("#log"); l.textContent += (l.textContent ? "\n" : "") + ev.payload; l.scrollTop = l.scrollHeight; });
  } catch (e) { status(String(e), false); }
  render();
})();
