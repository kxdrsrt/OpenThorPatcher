// OpenThor Patcher window. All work happens in the Tauri backend (otp-core); this file only shows state.
"use strict";
const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const icon = (n) => `<svg class="ic" aria-hidden="true"><use href="#i-${n}"/></svg>`;
const human = (n) => { const u = ["B", "KB", "MB", "GB"]; let i = 0, v = Number(n) || 0; while (v >= 1024 && i < 3) { v /= 1024; i++; } return (i ? v.toFixed(1) : Math.round(v)) + " " + u[i]; };

let info = null, checked = null, patching = false, toastTimer = 0;

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
  if (total > 0) { b.classList.remove("indeterminate"); b.querySelector("i").style.width = Math.min(100, (done / total) * 100) + "%"; }
  else { b.classList.add("indeterminate"); b.querySelector("i").style.width = ""; }
}

function idle() {
  $("#bar").classList.remove("indeterminate");
  $("#bar").querySelector("i").style.width = "0";
  $("#p-name").textContent = "";
  $("#p-size").textContent = "";
}

function log(line) {
  const l = $("#log");
  l.textContent += (l.textContent ? "\n" : "") + line;
  l.scrollTop = l.scrollHeight;
}

function need(s) {
  const parts = [];
  if (s.patches_applied.length) parts.push(`${s.patches_applied.length} patch${s.patches_applied.length > 1 ? "es" : ""}`);
  if (s.files_updated.length) parts.push(`${s.files_updated.length} file${s.files_updated.length > 1 ? "s" : ""}`);
  if (s.repaired.length) parts.push(`${s.repaired.length} to repair`);
  if (s.removed.length) parts.push(`${s.removed.length} to remove`);
  return parts.join(", ");
}

function buttons() {
  const has = checked && !checked.summary.up_to_date && checked.open;
  $("#btn-play").disabled = patching || !checked || !checked.can_play;
  $("#btn-play").hidden = false;
  $("#btn-update").hidden = patching || !has;
  $("#btn-cancel").hidden = !patching;
  $("#btn-check").disabled = patching;
  $("#btn-setup").hidden = !(checked && checked.can_setup);
  $("#btn-setup").disabled = patching;
  $("#btn-update").classList.toggle("primary", !!has);
  $("#btn-play").classList.toggle("primary", !has);
}

function news() {
  const url = checked?.news_url;
  const f = $("#news-frame");
  if (url && /^https:\/\//.test(url)) { if (f.src !== url) f.src = url; f.hidden = false; $("#news-fallback").hidden = true; }
  else { f.hidden = true; $("#news-fallback").hidden = false; $("#notice").textContent = checked?.notice || `Welcome to ${info?.name || "the server"}.`; }
}

async function check(force = false) {
  state("busy", "Checking for updates…", "", "update");
  bar(0, 0);
  try {
    checked = await invoke("check", { force });
  } catch (e) {
    checked = null;
    idle();
    state("bad", "Cannot reach the server", String(e), "error");
    buttons();
    return null;
  }
  idle();
  news();
  if (!checked.open) state("warn", "Patching is closed", checked.message || "The server is in maintenance.", "warn");
  else if (checked.summary.up_to_date) state("ok", "Up to date", checked.server, "check");
  else state("warn", "Updates available", need(checked.summary), "update");
  buttons();
  renderPacks();
  return checked;
}

async function patch(force = false) {
  patching = true;
  $("#log").textContent = "";
  state("busy", "Updating…", "Do not close the patcher.", "update");
  bar(0, 0);
  buttons();
  try {
    const s = await invoke("patch", { force });
    toast(s.up_to_date ? "Everything is up to date." : "Update finished.");
  } catch (e) {
    const m = String(e);
    if (m === "cancelled") toast("Update cancelled. Nothing was damaged; the next start continues.", "warn");
    else { toast(m, "bad"); log("error: " + m); }
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
    l.innerHTML = `<span>${esc(p.title)}${p.locked ? "<small>required by the server</small>" : ""}</span><span class="switch"><input type="checkbox" ${p.on ? "checked" : ""} ${p.locked ? "disabled" : ""}><i></i></span>`;
    l.querySelector("input").addEventListener("change", async (e) => {
      try { await invoke("set_pack", { id: p.id, on: e.target.checked }); toast(`${p.title} ${e.target.checked ? "on" : "off"}. Restart the game to apply it.`); }
      catch (err) { e.target.checked = !e.target.checked; toast(String(err), "bad"); }
    });
    root.appendChild(l);
  });
}

async function openSettings() {
  $("#s-dir").value = info.client_dir;
  $("#s-auto").checked = info.auto_update;
  $("#s-bak").checked = info.keep_backup;
  $("#dlg").showModal();
}

async function saveSettings() {
  try {
    await invoke("set_settings", { clientDir: $("#s-dir").value, autoUpdate: $("#s-auto").checked, keepBackup: $("#s-bak").checked });
    const changed = $("#s-dir").value !== info.client_dir;
    info.client_dir = $("#s-dir").value; info.auto_update = $("#s-auto").checked; info.keep_backup = $("#s-bak").checked;
    if (changed) check(false);
  } catch (e) { toast(String(e), "bad"); }
}

$("#btn-settings").onclick = openSettings;
$("#s-pick").onclick = async () => { const d = await T.dialog.open({ directory: true, title: "Client folder" }); if (d) $("#s-dir").value = d; };
$("#dlg").addEventListener("close", saveSettings);
$("#btn-update").onclick = () => patch(false);
$("#btn-check").onclick = async () => { const c = await check(true); if (c && !c.summary.up_to_date) patch(true); else if (c) toast("All files match the server."); };
$("#btn-cancel").onclick = () => { invoke("cancel"); $("#btn-cancel").disabled = true; };
$("#btn-play").onclick = () => invoke("play").then(() => toast("Starting the game…")).catch((e) => toast(String(e), "bad"));
$("#btn-setup").onclick = () => invoke("setup").catch((e) => toast(String(e), "bad"));

(async () => {
  await T.event.listen("progress", (ev) => {
    const p = ev.payload;
    if (p.kind === "info") log(p.text);
    else if (p.kind === "download") {
      $("#p-name").textContent = p.name;
      $("#p-size").textContent = p.total ? `${human(p.done)} / ${human(p.total)}` : human(p.done);
      bar(p.done, p.total);
    } else if (p.kind === "applied") { log(`${p.target}: patch ${p.id} applied`); bar(0, 0); $("#p-name").textContent = "Applying…"; $("#p-size").textContent = ""; }
    else if (p.kind === "removed") log("removed " + p.path);
  });
  info = await invoke("init");
  $("#name").textContent = info.name;
  document.title = info.name + " Patcher";
  if (info.error) { state("bad", "Not configured", info.error, "error"); idle(); buttons(); return; }
  const c = await check(false);
  if (c && c.open && !c.summary.up_to_date && info.auto_update) patch(false);
})();
