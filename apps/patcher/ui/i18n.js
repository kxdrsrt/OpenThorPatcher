// Languages of the player window. Every language is a flat dictionary (key -> text) registered with I18N.add; English is
// the fallback for missing keys. Built in: ui/locales/*.js. More (or corrections) come from the server's theme folder
// (patcher-theme/locales/<lang>.json, see get_locales), so a server can add a language without a new patcher.
"use strict";
const I18N = (() => {
  const dicts = {};
  let lang = "en";

  const add = (l, d) => { l = String(l).toLowerCase(); dicts[l] = Object.assign(dicts[l] || {}, d); };
  const has = (l) => !!dicts[String(l).toLowerCase()];
  // languages with their own name ("Deutsch"), sorted by that name
  const available = () => Object.keys(dicts).map((l) => ({ id: l, name: dicts[l]._name || l })).sort((a, b) => a.name.localeCompare(b.name));

  /* The player's choice, or for "auto" the first system language we have (de-AT -> de), else English. */
  function resolve(pref, system) {
    const want = pref && pref !== "auto" ? [pref] : (system && system.length ? system : ["en"]);
    for (const w of want) {
      const l = String(w).toLowerCase();
      if (dicts[l]) return l;
      const base = l.split("-")[0];
      if (dicts[base]) return base;
    }
    return "en";
  }

  /* t("need.files", {n: 3}) -> "3 files"; with n, "<key>_one" is used for 1 when it exists. */
  function t(key, vars) {
    let k = key;
    if (vars && vars.n !== undefined && Number(vars.n) === 1) {
      const one = key + "_one";
      if ((dicts[lang] && one in dicts[lang]) || (dicts.en && one in dicts.en)) k = one;
    }
    const s = (dicts[lang] && dicts[lang][k]) ?? (dicts.en && dicts.en[k]) ?? key;
    return String(s).replace(/\{(\w+)\}/g, (m, v) => (vars && v in vars ? String(vars[v]) : m));
  }

  /* Static texts of the page: data-i18n (text), data-i18n-title, data-i18n-aria (aria-label). */
  function apply(root = document) {
    root.querySelectorAll("[data-i18n]").forEach((e) => { e.textContent = t(e.dataset.i18n); });
    root.querySelectorAll("[data-i18n-title]").forEach((e) => { e.title = t(e.dataset.i18nTitle); });
    root.querySelectorAll("[data-i18n-aria]").forEach((e) => { e.setAttribute("aria-label", t(e.dataset.i18nAria)); });
  }

  function set(l) {
    lang = has(l) ? String(l).toLowerCase() : "en";
    if (typeof document !== "undefined") { document.documentElement.lang = lang; apply(); }
    return lang;
  }

  /* Messages of the patcher program (English) in the player's language; unknown ones stay as they are. */
  const ERRORS = [
    [/^The game is starting/, "err.starting"],
    [/^The game is already running\. Only one/, "err.limit1"],
    [/^The game is already running (\d+) times/, "err.limitN", (m) => ({ n: m[1] })],
    [/^(.+) not found in the client folder$/, "err.notFound", (m) => ({ file: m[1] })],
    [/^patcher\.json not found next to the program \((.*)\)$/, "err.noConfig", (m) => ({ path: m[1] })],
    [/^patcher\.json is invalid: (.*)$/, "err.badConfig", (m) => ({ detail: m[1] })],
    [/^patcher\.json has no patch_url/, "err.noUrl"],
    [/^patching is closed by the server/, "err.closed"],
    [/^this pack is required by the server/, "err.packRequired"],
    [/^the server did not say which program to start/, "err.noGame"],
    [/^self-update is off/, "err.selfUpdateOff"],
    [/^busy$/, "err.busy"],
    [/^the patch list has no signature/, "err.noSig"],
  ];
  function err(msg) {
    const s = String(msg ?? "");
    for (const [re, key, vars] of ERRORS) {
      const m = s.match(re);
      if (m) return t(key, vars ? vars(m) : undefined);
    }
    return s;
  }

  /* News and changelog entries may carry a language. Shown: entries without one, plus the player's language; if the
     server wrote none in it, English; else the first language there is. */
  function pick(items) {
    const list = items || [];
    const langs = [...new Set(list.map((i) => String(i.lang || "").toLowerCase()).filter(Boolean))];
    if (!langs.length) return list;
    const use = langs.includes(lang) ? lang : langs.includes("en") ? "en" : langs[0];
    return list.filter((i) => !i.lang || String(i.lang).toLowerCase() === use);
  }

  /* A text with translations ({"de": "..."}) and a fallback. */
  const local = (map, fallback) => (map && (map[lang] || map[lang.split("-")[0]])) || fallback;

  return { add, has, available, resolve, set, t, apply, err, pick, local, get lang() { return lang; } };
})();
const t = (k, v) => I18N.t(k, v);
// also as window properties, so separately evaluated scripts (tests) find them
if (typeof window !== "undefined") { window.I18N = I18N; window.t = t; }
if (typeof module !== "undefined") module.exports = I18N;
