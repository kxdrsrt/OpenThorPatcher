// OpenThor Patcher window. All work happens in the Tauri backend (otp-core); this file shows state, news and changelog.
"use strict";
const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const icon = (n) => `<svg class="ic" aria-hidden="true"><use href="#i-${n}"/></svg>`;
const human = (n) => { const u = ["B", "KB", "MB", "GB"]; let i = 0, v = Number(n) || 0; while (v >= 1024 && i < 3) { v /= 1024; i++; } return (i ? v.toFixed(1) : Math.round(v)) + " " + u[i]; };

let info = null, checked = null, patching = false, toastTimer = 0, multi = false;

/* Tiny markup for news bodies, escaped first: blank line = paragraph, "- " = list item, **bold**, [text](https://link). */
function markup(text) {
  const inline = (s) => esc(s)
    .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
    .replace(/\[([^\]]+)\]\((https:\/\/[^)\s]+)\)/g, '<a href="$2" target="_blank" rel="noreferrer noopener">$1</a>');
  const out = [];
  let list = null, para = [];
  const flushPara = () => { if (para.length) { out.push("<p>" + para.map(inline).join("<br>") + "</p>"); para = []; } };
  const flushList = () => { if (list) { out.push("<ul>" + list.map((x) => "<li>" + inline(x) + "</li>").join("") + "</ul>"); list = null; } };
  for (const line of String(text ?? "").split(/\r?\n/)) {
    if (!line.trim()) { flushPara(); flushList(); }
    else if (/^\s*[-*] /.test(line)) { flushPara(); (list = list || []).push(line.replace(/^\s*[-*] /, "")); }
    else { flushList(); para.push(line); }
  }
  flushPara(); flushList();
  return out.join("");
}

function renderNews(c) {
  const root = $("#p-news");
  const items = I18N.pick(c?.news);
  if (!items.length) {
    root.innerHTML = c?.notice ? `<article class="news-item"><h4>${esc(c.server || "")}</h4>${markup(c.notice)}</article>` : `<div class="empty-note">${esc(t("news.none"))}</div>`;
    return;
  }
  root.innerHTML = items.map((n) => `<article class="news-item"><time>${esc(n.date)}</time><h4>${esc(n.title)}</h4>${markup(n.body)}</article>`).join("");
}

function renderChangelog(c) {
  const root = $("#p-changelog");
  const items = I18N.pick(c?.changelog);
  if (!items.length) { root.innerHTML = `<div class="empty-note">${esc(t("changelog.none"))}</div>`; return; }
  root.innerHTML = items.map((e) => `<article class="change-entry"><div>${e.version ? `<span class="ver">${esc(e.version)}</span>` : ""}<time>${esc(e.date)}</time></div>${e.title ? `<h4>${esc(e.title)}</h4>` : ""}<ul>${(e.items || []).map((x) => `<li>${esc(x)}</li>`).join("")}</ul></article>`).join("");
}

/* Players online from the web panel (MidgardCP), next to the server dots. */
async function refreshOnline() {
  const o = $("#online");
  if (!checked || !checked.has_status) { o.hidden = true; return; }
  let r = null;
  try { r = await invoke("panel_status"); } catch { r = null; }
  if (!r) { o.hidden = true; return; }
  o.hidden = false;
  const down = r.state === "offline";
  o.className = "online " + (r.state === "online" ? "ok" : r.state === "degraded" ? "warn" : down ? "bad" : "neutral");
  o.textContent = down ? t("online.offline") : r.players != null ? t("online.n", { n: r.players }) : r.state === "unknown" ? "" : r.state;
  o.hidden = !o.textContent;
  o.title = `${r.name}: ${r.state}`;
}

/* Server status at the top left: one dot per server (login, char, map), refreshed every 30 s. */
async function refreshServers() {
  const root = $("#srv");
  let list = [];
  try { list = await invoke("server_status"); } catch { list = []; }
  root.hidden = !list.length;
  root.innerHTML = "";
  for (const s of list) {
    const e = document.createElement("span");
    e.className = "srv-item " + (s.ok ? "ok" : "bad");
    e.title = s.ok ? t("srv.ok", { name: s.name, ms: s.ms }) : t("srv.bad", { name: s.name });
    e.innerHTML = '<i class="dot"></i><b></b>';
    e.querySelector("b").textContent = s.name;
    root.appendChild(e);
  }
}

function setTab(name) {
  document.querySelectorAll("#tabs button").forEach((b) => { const on = b.dataset.tab === name; b.classList.toggle("active", on); b.setAttribute("aria-selected", on ? "true" : "false"); });
  document.querySelectorAll(".pane").forEach((p) => p.classList.toggle("active", p.id === "p-" + name));
}

const openUrl = (u) => invoke("open_url", { url: u }).catch((e) => toast(I18N.err(e), "bad"));

const ICONS = [[/discord\./, "discord"], [/github\./, "github"], [/gitlab\./, "gitlab"], [/youtu(\.be|be\.)/, "youtube"], [/(^|\.)(x|twitter)\.com/, "x"], [/facebook\.|fb\.com/, "facebook"], [/reddit\./, "reddit"], [/twitch\./, "twitch"], [/(^|\.)t\.me|telegram\./, "telegram"], [/instagram\./, "instagram"], [/tiktok\./, "tiktok"]];
function iconFor(l) {
  let host = "";
  try { host = new URL(l.url).hostname.toLowerCase(); } catch { /* not a URL */ }
  for (const [re, id] of ICONS) if (re.test(host)) return id;
  return /forum|board|community|foro/i.test(l.title + " " + l.url) ? "forum" : "link";
}

function renderLinks(c) {
  const root = $("#foot-links");
  root.innerHTML = "";
  for (const l of (c?.links || [])) {
    if (!/^https?:\/\//.test(l.url)) continue;
    const a = document.createElement("a");
    a.innerHTML = `<svg class="ic" aria-hidden="true"><use href="#b-${iconFor(l)}"/></svg><span></span>`;
    a.querySelector("span").textContent = l.title || l.url;
    a.title = l.url;
    a.tabIndex = 0;
    a.onclick = () => openUrl(l.url);
    a.onkeydown = (e) => { if (e.key === "Enter") openUrl(l.url); };
    root.appendChild(a);
  }
}

function news() {
  const c = checked;
  renderLinks(c);
  renderNews(c);
  renderChangelog(c);
  const url = c?.news_url;
  const web = $("#tab-web");
  if (url && /^https:\/\//.test(url)) {
    web.hidden = false;
    const f = $("#news-frame");
    if (f.getAttribute("src") !== url) f.src = url;
    // with no written news, the website is the news
    if (!(c.news || []).length) setTab("web");
  } else web.hidden = true;
}

function toast(msg, kind = "ok") {
  const t = $("#toast");
  t.className = "toast " + kind;
  t.innerHTML = icon(kind === "ok" ? "check" : kind === "warn" ? "warn" : "error") + `<span>${esc(msg)}</span>`;
  t.hidden = false;
  clearTimeout(toastTimer);
  t.onclick = () => (t.hidden = true);
  if (kind !== "bad") toastTimer = setTimeout(() => (t.hidden = true), 4500);
}

function state(kind, title, text, ic) {
  const s = $("#state");
  s.className = "state " + kind;
  s.querySelector("svg").innerHTML = `<use href="#i-${ic}"/>`;
  $("#state-title").textContent = title;
  $("#state-text").textContent = text || "";
  $("#sub").textContent = title;
}

function bar(done, total) {
  const b = $("#bar");
  if (total > 0) {
    const pct = Math.min(100, (done / total) * 100);
    b.classList.remove("indeterminate"); b.querySelector("i").style.width = pct + "%";
    $("#pct").textContent = Math.floor(pct) + " %";
    b.setAttribute("aria-valuenow", String(Math.floor(pct)));
  } else { b.classList.add("indeterminate"); b.querySelector("i").style.width = ""; $("#pct").textContent = ""; }
}

function idle() {
  $("#bar").classList.remove("indeterminate");
  $("#bar").querySelector("i").style.width = "0";
  $("#pct").textContent = "";
  $("#p-name").textContent = "";
  $("#p-size").textContent = "";
}

function log(line) {
  const l = $("#log");
  l.textContent += (l.textContent ? "\n" : "") + line;
  l.scrollTop = l.scrollHeight;
}

const upToDate = (s) => !s.files_updated.length && !s.patches_applied.length && !s.repaired.length && !s.removed.length;

function need(s) {
  const parts = [];
  if (s.patches_applied.length) parts.push(t("need.patches", { n: s.patches_applied.length }));
  if (s.files_updated.length) parts.push(t("need.files", { n: s.files_updated.length }));
  if (s.repaired.length) parts.push(t("need.repair", { n: s.repaired.length }));
  if (s.removed.length) parts.push(t("need.remove", { n: s.removed.length }));
  return parts.join(", ");
}

function buttons() {
  const has = checked && !upToDate(checked.summary) && checked.open;
  $("#btn-play").disabled = patching || !checked || !checked.can_play;
  $("#btn-update").hidden = patching || !has;
  $("#btn-cancel").hidden = !patching;
  $("#btn-check").disabled = patching;
  $("#btn-setup").hidden = !(checked && checked.can_setup);
  $("#btn-setup").disabled = patching;
  $("#btn-web").hidden = !(checked && checked.website);
  $("#btn-update").classList.toggle("primary", !!has);
  $("#btn-play").classList.toggle("primary", !has);
}

/* The state box for the last check, in the current language (also used when the player switches the language). */
function showChecked() {
  if (!checked) return;
  news();
  if (!checked.open) state("warn", t("state.closed"), checked.message || t("state.maintenance"), "warn");
  else if (upToDate(checked.summary)) state("ok", t("state.ok"), checked.server, "check");
  else state("warn", t("state.updates"), need(checked.summary), "update");
  buttons();
  renderPacks();
}

async function check(force = false) {
  state("busy", t("state.checking"), "", "update");
  bar(0, 0);
  try {
    checked = await invoke("check", { force });
  } catch (e) {
    checked = null;
    idle();
    state("bad", t("state.unreachable"), I18N.err(e), "error");
    buttons();
    return null;
  }
  idle();
  showChecked();
  refreshServers();
  refreshOnline();
  if (checked.update_warning) { toast(checked.update_warning, "warn"); log(checked.update_warning); }
  return checked;
}

/* A newer patcher from the server: install it first (the patcher restarts itself), then patch with the new one. */
async function selfUpdate() {
  patching = true;
  buttons();
  state("busy", t("state.selfupdate", { v: checked.update.version }), t("state.selfupdate.text"), "update");
  bar(0, 0);
  try { await invoke("update_patcher"); }
  catch (e) { patching = false; toast(t("toast.selfUpdateFailed", { e: I18N.err(e) }), "bad"); log(t("log.error", { e: String(e) })); buttons(); return false; }
  return true;
}

async function patch(force = false) {
  if (checked && checked.update && await selfUpdate()) return;
  patching = true;
  multi = false;
  $("#log").textContent = "";
  state("busy", t("state.updating"), t("state.updating.text"), "update");
  bar(0, 0);
  buttons();
  try {
    const s = await invoke("patch", { force });
    toast(upToDate(s) ? t("toast.allGood") : t("toast.updated"));
  } catch (e) {
    const m = String(e);
    if (m === "cancelled") toast(t("toast.cancelled"), "warn");
    else { toast(I18N.err(m), "bad"); log(t("log.error", { e: m })); }
  } finally { patching = false; }
  await check(false);
}

function renderPacks() {
  const root = $("#packs");
  const list = checked?.packs || [];
  $("#packs-title").hidden = !list.length;
  root.hidden = !list.length;
  root.innerHTML = "";
  list.forEach((p) => {
    const l = document.createElement("label");
    l.className = "switch-row";
    const title = I18N.local(p.titles, p.title);
    l.innerHTML = `<span>${esc(title)}${p.locked ? `<small>${esc(t("pack.required"))}</small>` : ""}</span><span class="switch"><input type="checkbox" ${p.on ? "checked" : ""} ${p.locked ? "disabled" : ""}><i></i></span>`;
    l.querySelector("input").addEventListener("change", async (e) => {
      try { await invoke("set_pack", { id: p.id, on: e.target.checked }); p.on = e.target.checked; toast(t(e.target.checked ? "pack.on" : "pack.off", { title })); }
      catch (err) { e.target.checked = !e.target.checked; toast(I18N.err(err), "bad"); }
    });
    root.appendChild(l);
  });
}

async function openSettings() {
  $("#s-dir").value = info.client_dir;
  $("#s-auto").checked = info.auto_update;
  $("#s-bak").checked = info.keep_backup;
  $("#s-par").value = info.parallel;
  $("#s-kbps").value = info.max_kbps;
  langOptions();
  $("#scrim").hidden = false;
  $("#dlg").showModal();
}

async function saveSettings() {
  try {
    const parallel = Math.min(16, Math.max(1, parseInt($("#s-par").value, 10) || 4));
    const maxKbps = Math.max(0, parseInt($("#s-kbps").value, 10) || 0);
    await invoke("set_settings", { clientDir: $("#s-dir").value, autoUpdate: $("#s-auto").checked, keepBackup: $("#s-bak").checked, parallel, maxKbps, language: info.language });
    const changed = $("#s-dir").value !== info.client_dir;
    info.client_dir = $("#s-dir").value; info.auto_update = $("#s-auto").checked; info.keep_backup = $("#s-bak").checked;
    info.parallel = parallel; info.max_kbps = maxKbps;
    if (changed) { await loadTheme(); check(false); }
  } catch (e) { toast(I18N.err(e), "bad"); }
}

/* Language: the player's choice from the settings, "auto" = the system language. */
function langOptions() {
  const sel = $("#s-lang");
  sel.innerHTML = "";
  const auto = document.createElement("option");
  auto.value = "auto";
  auto.textContent = t("set.language.auto");
  sel.appendChild(auto);
  for (const l of I18N.available()) {
    const o = document.createElement("option");
    o.value = l.id;
    o.textContent = l.name;
    sel.appendChild(o);
  }
  sel.value = info.language && (info.language === "auto" || I18N.has(info.language)) ? info.language : "auto";
}

function useLanguage(pref) {
  I18N.set(I18N.resolve(pref, navigator.languages || [navigator.language]));
  footTheme();
  showChecked();
}

/* An operator theme (client folder / patcher-theme) replaces the built-in "Midgard Night": its CSS goes after the
   default's, its variables and logo override the defaults. No theme folder = the built-in look. */
async function loadTheme() {
  let th = null;
  try { th = await invoke("get_theme"); } catch { th = null; }
  const style = $("#theme-custom");
  const root = document.documentElement;
  style.textContent = th ? th.css : "";
  for (const [k, v] of Object.entries(th?.vars || {})) root.style.setProperty(k, v);
  if (th?.logo) $("#logo").src = th.logo;
  theme = th;
  footTheme();
}

let theme = null;
function footTheme() {
  const name = theme ? theme.name : "Midgard Night";
  $("#foot-theme").textContent = theme && theme.author ? t("theme.by", { name, author: theme.author }) : t("theme", { name });
}

/* Languages the server ships in its theme folder (patcher-theme/locales/<lang>.json). */
async function loadLocales() {
  let extra = {};
  try { extra = (await invoke("get_locales")) || {}; } catch { extra = {}; }
  for (const [l, d] of Object.entries(extra)) I18N.add(l, d);
}

function winCall(fn) {
  try { const w = T.window?.getCurrentWindow?.(); if (w) w[fn](); } catch { /* not running in a window */ }
}

$("#btn-settings").onclick = openSettings;
$("#btn-min").onclick = () => winCall("minimize");
$("#btn-close").onclick = () => winCall("close");
$("#s-pick").onclick = async () => { const d = await T.dialog.open({ directory: true, title: t("set.dir") }); if (d) $("#s-dir").value = d; };
$("#s-lang").onchange = () => { info.language = $("#s-lang").value; useLanguage(info.language); langOptions(); };
$("#dlg").addEventListener("close", () => { $("#scrim").hidden = true; saveSettings(); });
$("#btn-update").onclick = () => patch(false);
$("#btn-check").onclick = async () => { const c = await check(true); if (c && !upToDate(c.summary)) patch(true); else if (c) toast(t("toast.filesOk")); };
$("#btn-cancel").onclick = () => { invoke("cancel"); $("#btn-cancel").disabled = true; };
$("#btn-web").onclick = () => checked?.website && openUrl(checked.website);
// links inside news text open in the player's browser, never inside the patcher window
document.addEventListener("click", (e) => {
  const a = e.target.closest && e.target.closest("a[href]");
  if (a && /^https?:\/\//.test(a.getAttribute("href"))) { e.preventDefault(); openUrl(a.getAttribute("href")); }
});
/* Play: pending updates are installed first; a second click while the game starts does nothing. */
$("#btn-play").onclick = async () => {
  if (patching || $("#btn-play").disabled) return;
  if (checked && checked.open && !upToDate(checked.summary)) {
    await patch(false);
    if (!checked || !upToDate(checked.summary)) { toast(t("toast.notReady"), "warn"); return; }
  }
  const b = $("#btn-play");
  b.disabled = true;
  try { await invoke("play"); toast(t("toast.starting")); }
  catch (e) { const m = String(e); toast(I18N.err(m), m.startsWith("The game is") ? "warn" : "bad"); }
  setTimeout(() => { b.disabled = false; buttons(); }, 8000);
};
$("#btn-setup").onclick = () => invoke("setup").catch((e) => toast(I18N.err(e), "bad"));
document.querySelectorAll("#tabs button").forEach((b) => b.addEventListener("click", () => setTab(b.dataset.tab)));

setInterval(() => { if (!patching) { refreshServers(); refreshOnline(); } }, 30000);

(async () => {
  await T.event.listen("progress", (ev) => {
    const p = ev.payload;
    if (p.kind === "info") log(p.text);
    else if (p.kind === "download") {
      $("#p-name").textContent = p.name;
      if (!multi) { $("#p-size").textContent = p.total ? `${human(p.done)} / ${human(p.total)}` : human(p.done); bar(p.done, p.total); }
    } else if (p.kind === "total") {
      multi = true;
      $("#p-size").textContent = `${human(p.done)} / ${human(p.total)}`;
      bar(p.done, p.total);
    } else if (p.kind === "applied") { log(t("log.applied", { target: p.target, id: p.id })); bar(0, 0); $("#p-name").textContent = t("state.applying"); $("#p-size").textContent = ""; }
    else if (p.kind === "removed") log(t("log.removed", { path: p.path }));
  });
  info = await invoke("init");
  await loadLocales();
  useLanguage(info.language);
  $("#name").textContent = info.name;
  $("#foot-ver").textContent = `OpenThor Patcher v${info.version}`;
  document.title = info.name + " Patcher";
  await loadTheme();
  if (info.error) { state("bad", t("state.unconfigured"), I18N.err(info.error), "error"); idle(); buttons(); return; }
  const c = await check(false);
  if (c && c.update && await selfUpdate()) return;
  if (c && c.open && !upToDate(c.summary) && info.auto_update) patch(false);
})();
