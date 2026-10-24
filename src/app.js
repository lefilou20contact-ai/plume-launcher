"use strict";
const T = window.__TAURI__;
const inv = (c, a) => T.core.invoke(c, a);
const $ = s => document.querySelector(s), $$ = s => [...document.querySelectorAll(s)];
const SITE = "https://lefilou20contact-ai.github.io/plume-launcher/", REPO = "https://github.com/lefilou20contact-ai/plume-launcher";

// ---------- Traductions : le français (fr.json) est la référence ----------
let FR = {}, CUR = {}, lang = "fr", LANGS = [], RS = [];
const interp = (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => (v && k in v ? v[k] : m));
function plural(s, v) {
  if (!s.includes("|") || !v || typeof v.n !== "number") return s;
  const p = s.split("|"), one = v.n === 1 || (v.n === 0 && lang === "fr");
  return p[one ? 0 : 1] ?? p[0];
}
function t(k, v) { const s = typeof CUR[k] === "string" && CUR[k].trim() ? CUR[k] : (FR[k] ?? k); return interp(plural(s, v), v); }
// Comme t(), mais certaines variables sont des éléments HTML (ex : un lien en gras).
const tn = (k, nodes) => t(k).split(/(\{\w+\})/).map(x => { const m = x.match(/^\{(\w+)\}$/); return m && nodes[m[1]] ? nodes[m[1]] : x; });
const refKeys = () => Object.keys(FR).filter(k => !k.startsWith("_"));
const coverage = d => { const ks = refKeys(); return ks.length ? Math.floor(ks.filter(k => typeof d[k] === "string" && d[k].trim()).length / ks.length * 100) : 0; };
async function jget(p) { const r = await fetch(p); if (!r.ok) throw new Error(p); return r.json(); }
// Les messages du programme (Rust) sont en français : on les retrouve grâce aux clés rs.* et pg.* du français.
function buildRs() {
  RS = Object.keys(FR).filter(k => /^(rs|pg)\./.test(k)).map(k => {
    const names = [];
    const src = FR[k].replace(/[.*+?^$()|[\]\\]/g, "\\$&").replace(/\{(\w+)\}/g, (m, n) => { names.push(n); return "(.+?)"; });
    return { k, names, re: new RegExp("^" + src + "$") };
  });
}
function trRs(m) {
  if (lang === "fr" || typeof m !== "string") return m;
  for (const r of RS) { const x = m.match(r.re); if (x) return t(r.k, Object.fromEntries(r.names.map((n, i) => [n, x[i + 1]]))); }
  return m;
}
const errText = e => trRs(String((e && e.message) || e));
function resolveLang() {
  if (S.lang !== "auto" && LANGS.find(l => l.code === S.lang)) return S.lang;
  for (const c of (navigator.languages || [navigator.language || "fr"])) {
    const base = String(c).toLowerCase().split("-")[0], l = LANGS.find(x => x.code === base && x.pct > 0);
    if (l) return l.code;
  }
  return "fr";
}
function applyStatic() { $$("[data-i18n]").forEach(el => { el.textContent = t(el.dataset.i18n); }); }
function pickLang() {
  lang = resolveLang(); CUR = (LANGS.find(l => l.code === lang) || {}).data || FR;
  document.documentElement.lang = lang; applyStatic(); renderSide(); if (R[page]) R[page]();
}
const LANGOK = c => typeof c === "string" && /^[a-z]{2,3}(-[A-Za-z]{2})?$/.test(c);
const vars = x => (String(x).match(/\{\w+\}/g) || []).sort().join(",");
// On ignore toute traduction dont les variables {x} diffèrent du français (évite les textes cassés).
const sanitize = d => { const o = {}; for (const k of Object.keys(d || {})) { const v = d[k]; if (typeof v === "string" && v.trim() && v.length <= 1000 && k in FR && vars(v) === vars(FR[k])) o[k] = v; } return o; };
let BUNDLED = { list: [], files: {} };
async function initI18n() {
  let fr = {}, list = [{ code: "fr", name: "Français" }];
  try { fr = await jget("i18n/fr.json"); } catch (_) {}
  try { list = await jget("i18n/languages.json"); } catch (_) {}
  const files = { fr };
  await Promise.all(list.filter(l => l.code !== "fr").map(async l => { try { files[l.code] = await jget("i18n/" + l.code + ".json"); } catch (_) {} }));
  BUNDLED = { list, files };
  await mergeI18n();
}
// Fusionne les langues intégrées au launcher avec celles récupérées depuis GitHub (les plus récentes gagnent).
async function mergeI18n() {
  let c = null; try { c = await inv("i18n_load"); } catch (_) {}
  const files = { ...BUNDLED.files, ...((c && c.files) || {}) };
  const list = (c && Array.isArray(c.languages) ? c.languages : BUNDLED.list).filter(l => l && LANGOK(l.code) && typeof l.name === "string");
  FR = files.fr || {}; buildRs();
  LANGS = list.map(l => ({ code: l.code, name: l.name, data: sanitize(files[l.code]) }));
  if (!LANGS.find(l => l.code === "fr")) LANGS.unshift({ code: "fr", name: "Français", data: FR });
  LANGS.forEach(l => { l.pct = l.code === "fr" ? 100 : coverage(l.data); });
}
async function syncI18n(manual) {
  if (!manual && !S.langSync) return;
  try {
    const changed = await inv("i18n_sync");
    if (changed) { await mergeI18n(); pickLang(); toast(t("toast.lang_updated")); }
    else if (manual) toast(t("toast.lang_uptodate"));
  } catch (_) { if (manual) toast(t("toast.lang_fail"), "bad"); }
}
const MC_LANG = { fr: "fr_fr", en: "en_us", es: "es_es", de: "de_de", it: "it_it", pt: "pt_br", nl: "nl_nl", pl: "pl_pl", ru: "ru_ru", tr: "tr_tr", ja: "ja_jp", ko: "ko_kr", zh: "zh_cn" };
const siteUrl = hash => SITE + (lang === "fr" ? "" : "en/") + (hash || "");

// ---------- Outils d'interface (aucun innerHTML : tout est du texte, donc sûr) ----------
function h(tag, attrs = {}, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null || v === false) continue;
    if (k === "class") el.className = v;
    else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
    else if (k === "value") el.value = v;
    else if (v === true) el.setAttribute(k, "");
    else el.setAttribute(k, v);
  }
  for (const x of kids.flat(2)) { if (x == null || x === false) continue; el.append(x instanceof Node ? x : String(x)); }
  return el;
}
const toggle = (on, fn) => {
  const b = h("button", { class: "tg", type: "button", "aria-pressed": String(!!on) });
  b.addEventListener("click", () => { const v = b.getAttribute("aria-pressed") !== "true"; b.setAttribute("aria-pressed", String(v)); fn(v); });
  return b;
};
const seg = (opts, cur, fn) => {
  const w = h("div", { class: "seg" });
  opts.forEach(([v, l]) => {
    const b = h("button", { type: "button", class: v === cur ? "on" : "" }, l);
    b.addEventListener("click", () => { [...w.children].forEach(x => x.classList.toggle("on", x === b)); fn(v); });
    w.append(b);
  });
  return w;
};
const row = (title, hint, ctl) => h("div", { class: "item" }, h("div", {}, h("b", {}, title), hint && h("small", {}, hint)), h("div", { class: "ctl" }, ctl));
const card = (title, ...k) => h("section", { class: "card" }, title && h("h3", {}, title), ...k);
const empty = m => h("div", { class: "empty" }, m);
const skel = n => h("div", {}, Array.from({ length: n }, () => h("div", { class: "skel" })));
const fmt = n => n >= 1e6 ? (n / 1e6).toFixed(1) + " M" : n >= 1e3 ? Math.round(n / 1e3) + " k" : String(n || 0);
const fmtBytes = b => b >= 1073741824 ? (b / 1073741824).toFixed(1) + " " + t("u.gb") : b >= 1048576 ? Math.round(b / 1048576) + " " + t("u.mb") : Math.max(1, Math.round(b / 1024)) + " " + t("u.kb");
const openUrl = u => inv("open_url", { url: u }).catch(x => toast(errText(x), "bad"));

function toast(msg, kind) {
  const el = h("div", { class: "toast" + (kind === "bad" ? " bad" : "") }, msg);
  $("#toasts").append(el);
  setTimeout(() => el.remove(), kind === "bad" ? 7000 : 3500);
}
function modal(title, body, actions) {
  $("#modal").replaceChildren(h("div", { class: "mcard", role: "dialog", "aria-modal": "true" }, h("h2", {}, title), body, h("div", { class: "macts" }, actions)));
  $("#modal").classList.add("show");
  const f = $("#modal input[type=text],#modal input[type=search]"); if (f) f.focus();
}
const closeModal = () => $("#modal").classList.remove("show");
const mbtn = (label, fn, cls = "") => h("button", { class: "btn " + cls, type: "button", onclick: fn }, label);
function confirmBox(title, text, label, fn) {
  modal(title, h("p", {}, text), [mbtn(t("c.cancel"), closeModal), mbtn(label, async () => { closeModal(); await fn(); }, "p")]);
}
$("#modal").addEventListener("click", e => { if (e.target.id === "modal") closeModal(); });
document.addEventListener("keydown", e => {
  if (e.key === "Escape") closeModal();
  if (e.key === "Enter" && $("#modal").classList.contains("show") && e.target.tagName !== "SELECT" && e.target.tagName !== "BUTTON") { const p = $("#modal .macts .btn.p"); if (p) p.click(); }
});

function showPrep(title) {
  $("#ptitle").textContent = title; $("#prog").textContent = t("prep.start");
  $("#pb").parentElement.classList.add("ind"); $("#pb").style.width = "";
  $("#ovPrep").classList.add("show");
}
const hidePrep = () => $("#ovPrep").classList.remove("show");
T.event.listen("progress", e => {
  const p = e.payload, bar = $("#pb").parentElement, mb = p.n > 1e6, label = trRs(p.t);
  $("#prog").textContent = label + (p.n ? " : " + (mb ? (p.d / 1e6).toFixed(1) + " / " + (p.n / 1e6).toFixed(1) + " " + t("u.mb") : p.d + " / " + p.n) : "…");
  if (p.n) { bar.classList.remove("ind"); $("#pb").style.width = Math.min(100, p.d / p.n * 100) + "%"; }
  else { bar.classList.add("ind"); $("#pb").style.width = ""; }
});

// ---------- Réglages (personnalisation) ----------
const PROFILES = [{ k: "prof.eco", ram: 2, nice: 10 }, { k: "prof.balanced", ram: 4, nice: 0 }, { k: "prof.perf", ram: 8, nice: 0 }];
const COLORS = ["#2f55f0", "#1f9d63", "#d6812b", "#b74bd6", "#d64545", "#2b9fd6"];
const DEF = { lang: "auto", langSync: true, theme: "system", accent: "#2f55f0", compact: false, hide: true, width: 0, height: 0, fullscreen: false, profile: 1, ram: 4, jvm: "", java: "", snapshots: false, pack: true };
let S = { ...DEF };
try { S = { ...DEF, ...JSON.parse(localStorage.getItem("plume.s") || "{}") }; } catch (_) {}
function ink(hex) { const n = parseInt(hex.slice(1), 16); return ((n >> 16) * 299 + ((n >> 8) & 255) * 587 + (n & 255) * 114) / 1000 > 150 ? "#101820" : "#fff"; }
function applyS() {
  const r = document.documentElement;
  if (S.theme === "system") r.removeAttribute("data-theme"); else r.dataset.theme = S.theme;
  const acc = /^#[0-9a-f]{6}$/i.test(S.accent) ? S.accent : DEF.accent;
  r.style.setProperty("--acc", acc); r.style.setProperty("--accink", ink(acc));
  const n = parseInt(acc.slice(1), 16); r.style.setProperty("--glow", "rgba(" + (n >> 16) + "," + ((n >> 8) & 255) + "," + (n & 255) + ",.38)");
  document.body.classList.toggle("compact", !!S.compact);
}
const saveS = () => { localStorage.setItem("plume.s", JSON.stringify(S)); applyS(); };
const launchOpts = i => ({ ramGb: clampRam(i.ram_gb || S.ram), nice: PROFILES[S.profile].nice, jvmArgs: S.jvm, javaPath: S.java, profile: S.profile, width: S.width || null, height: S.height || null, fullscreen: S.fullscreen, hide: S.hide });

// ---------- État ----------
let insts = [], curId = localStorage.getItem("plume.cur"), acct = null, dev = null, page = "home", modsTab = "inst", instQuery = "", sysRam = null;
const running = new Set(), stopping = new Set();
const curInst = () => insts.find(i => i.id === curId);
const R = { home: renderHome, inst: renderInsts, mods: renderMods, logs: renderLogs, lab: renderLab, set: renderSettings };
function go(p) {
  page = p;
  $$(".v").forEach(v => v.classList.toggle("on", v.id === "p-" + p));
  $$("#nav button[data-p]").forEach(b => b.classList.toggle("on", b.dataset.p === p));
  stopLogs();
  R[p]();
}
async function loadInsts() {
  try { insts = await inv("list_instances"); } catch (_) { insts = []; }
  if (!insts.find(i => i.id === curId)) { const last = [...insts].sort((a, b) => (b.last_played || 0) - (a.last_played || 0))[0]; curId = last ? last.id : null; }
  if (curId) localStorage.setItem("plume.cur", curId);
}
const selectInst = id => { curId = id; localStorage.setItem("plume.cur", id); };
function ago(ts) {
  if (!ts) return t("d.never");
  const d = Math.floor((Date.now() / 1000 - ts) / 86400);
  return d <= 0 ? t("d.today") : d === 1 ? t("d.yesterday") : t("d.days", { n: d });
}
const dur = s => s < 3600 ? t("dur.min", { n: Math.floor(s / 60) }) : t("dur.h", { h: Math.floor(s / 3600), m: String(Math.floor(s % 3600 / 60)).padStart(2, "0") });
const LOADER_KEY = { vanilla: "d.vanilla", fabric: "d.fabric", quilt: "d.quilt", forge: "d.forge", neoforge: "d.neoforge" };
const loaderName = i => t(LOADER_KEY[i.loader] || "d.vanilla");
function desc(i) {
  const p = [loaderName(i), t("d.mc", { v: i.version })];
  if (i.loader !== "vanilla") p.push(t("d.mods", { n: i.mods || 0 }));
  p.push(ago(i.last_played));
  if (i.playtime >= 60) p.push(t("d.playtime", { t: dur(i.playtime) }));
  return p.join(" · ");
}
function avatar(i) {
  const a = h("div", { class: "av" }, (i.name.trim()[0] || "?").toUpperCase());
  a.style.background = /^#[0-9a-f]{6}$/i.test(i.color) ? i.color : DEF.accent;
  return a;
}

// ---------- Comptes ----------
let accts = [];
async function refreshAcct() {
  try { accts = await inv("account_list"); } catch (_) { accts = []; }
  acct = accts.find(a => a.active) || null;
  try { dev = await inv("dev_offline"); } catch (_) { dev = null; }
  renderSide();
}
const canPlay = () => !!curInst() && !!(acct || dev);
// Tête du joueur (peau enregistrée) ; une pastille avec l'initiale sinon.
function head(a, size) {
  if (!a || typeof a.skin !== "string" || !a.skin.startsWith("data:image/png;base64,")) {
    const d = h("div", { class: "av" }, ((a && a.name && a.name[0]) || "?").toUpperCase());
    d.style.width = d.style.height = size + "px"; d.style.fontSize = Math.round(size * .45) + "px"; d.style.borderRadius = Math.round(size * .25) + "px";
    return d;
  }
  const sc = size / 8, w = h("div", { class: "skin" });
  const layer = (x, y) => { const l = h("div"); l.style.backgroundImage = 'url("' + a.skin + '")'; l.style.backgroundSize = (64 * sc) + "px auto"; l.style.backgroundPosition = (-x * sc) + "px " + (-y * sc) + "px"; return l; };
  w.style.width = w.style.height = size + "px"; w.append(layer(8, 8), layer(40, 8));
  return w;
}
function renderSide() {
  closePop();
  const label = acct ? acct.name : dev ? t("acc.test", { name: dev }) : t("side.signin");
  $("#side-acc").replaceChildren(h("button", { class: "accbtn", "aria-haspopup": "menu", onclick: e => { e.stopPropagation(); accts.length ? openPop() : login(); } },
    head(acct, 34), h("span", { class: "grow" }, h("b", {}, label), h("small", {}, acct ? t("side.account") : t("side.noaccount"))), accts.length ? h("span", { class: "chev" }) : null));
}
function closePop() { const p = $("#pop"); if (p) p.remove(); }
function openPop() {
  if ($("#pop")) { closePop(); return; }
  const items = accts.map(a => h("button", { class: "pitem", onclick: async () => { closePop(); if (!a.active) await switchAcct(a.uuid); } },
    head(a, 28), h("span", { class: "grow" }, a.name), a.active && h("span", { class: "chip" }, t("acc.active"))));
  $("#side-acc").append(h("div", { id: "pop", class: "pop", role: "menu" }, items,
    h("button", { class: "pitem", onclick: () => { closePop(); login(); } }, h("span", { class: "plus" }, "+"), t("acc.add")),
    acct && h("button", { class: "pitem danger", onclick: async () => { closePop(); await removeAcct(acct.uuid); } }, t("acc.logout"))));
}
document.addEventListener("click", e => { if (!e.target.closest("#side-acc")) closePop(); });
async function switchAcct(uuid) { try { await inv("account_switch", { uuid }); await refreshAcct(); R[page](); toast(t("toast.account_changed")); } catch (err) { toast(errText(err), "bad"); } }
async function removeAcct(uuid) { try { await inv("account_remove", { uuid }); await refreshAcct(); R[page](); } catch (err) { toast(errText(err), "bad"); } }
function accountCard() {
  const c = card(t("set.accounts"));
  accts.forEach(a => c.append(h("div", { class: "item" },
    h("div", { class: "ctl" }, head(a, 36), h("div", {}, h("b", {}, a.name, a.active && h("span", { class: "chip" }, t("acc.active"))), h("small", {}, t("acc.edition")))),
    h("div", { class: "ctl" }, !a.active && h("button", { class: "btn sm", onclick: () => switchAcct(a.uuid) }, t("c.use")),
      h("button", { class: "btn sm danger", onclick: () => confirmBox(t("acc.remove_title", { name: a.name }), t("acc.remove_text"), t("c.remove"), () => removeAcct(a.uuid)) }, t("c.remove"))))));
  if (!accts.length) c.append(row(dev ? t("acc.test", { name: dev }) : t("acc.none"), t("acc.hint"), null));
  c.append(h("div", { class: "bar2" }, h("button", { class: "btn p", onclick: login }, accts.length ? t("acc.add") : t("acc.signin"))));
  return c;
}
const loginHint = () => (acct || dev) ? null : card(null, h("p", {}, t("home.signin_hint")), h("br"), h("button", { class: "btn p", onclick: login }, t("acc.signin")));
async function login() {
  $("#lerr").textContent = t("login.waiting"); $("#lerr").classList.remove("bad");
  try {
    const d = await inv("login_start");
    $("#lcode").textContent = d.user_code; $("#lhelp").replaceChildren(...tn("login.help", { url: h("b", {}, d.verification_uri) }));
    $("#ovLogin").classList.add("show");
    inv("open_url", { url: d.verification_uri }).catch(() => {});
    await inv("login_finish", { deviceCode: d.device_code, interval: d.interval, expiresIn: d.expires_in });
    $("#ovLogin").classList.remove("show");
    await refreshAcct(); R[page](); toast(t("toast.account_added"));
  } catch (err) { $("#lerr").textContent = errText(err); $("#lerr").classList.add("bad"); $("#ovLogin").classList.add("show"); }
}
$("#lclose").addEventListener("click", () => $("#ovLogin").classList.remove("show"));

// ---------- Jeu ----------
async function crash(msg, id, launchFail) {
  $("#ctitle").textContent = t(launchFail ? "crash.launch_title" : "crash.title");
  $("#cmsg").textContent = msg; $("#clog").textContent = "";
  try { $("#clog").textContent = await inv("read_log", { instanceId: id || curId }); } catch (_) {}
  $("#ovCrash").classList.add("show");
}
async function play(inst) {
  const i = inst || curInst(); if (!i) return;
  selectInst(i.id);
  showPrep(t("prep.game"));
  try { await inv("launch", { instanceId: i.id, opts: launchOpts(i) }); running.add(i.id); hidePrep(); await loadInsts(); R[page](); }
  catch (err) { hidePrep(); await crash(errText(err), i.id, true); }
}
async function stopGame(inst) {
  const i = inst || curInst(); if (!i) return;
  stopping.add(i.id);
  try { await inv("stop_game", { instanceId: i.id }); } catch (err) { stopping.delete(i.id); toast(errText(err), "bad"); }
}
T.event.listen("game-exit", async e => {
  const { id, code } = e.payload;
  running.delete(id);
  const intended = stopping.delete(id);
  await loadInsts(); R[page]();
  if (code !== 0 && !intended) await crash(t("crash.code", { code }), id);
});
$("#again").addEventListener("click", () => { $("#ovCrash").classList.remove("show"); play(); });
$("#logs").addEventListener("click", () => { $("#ovCrash").classList.remove("show"); go("logs"); });
$("#back").addEventListener("click", () => $("#ovCrash").classList.remove("show"));

// ---------- Accueil ----------
function renderHome() {
  const el = $("#p-home"), i = curInst();
  el.replaceChildren(h("h1", {}, t("home.title")), h("p", { class: "sub" }, t("home.sub")));
  if (!i) {
    el.append(card(null, h("p", {}, t("home.empty")), h("br"), h("button", { class: "btn p", onclick: () => newInstModal() }, t("home.create_first"))), loginHint());
    return;
  }
  const sel = h("select", { class: "inp", "aria-label": t("home.active"), onchange: e => { selectInst(e.target.value); renderHome(); } }, insts.map(x => h("option", { value: x.id }, x.name)));
  sel.value = i.id; sel.style.maxWidth = "260px";
  const run = running.has(i.id);
  el.append(
    h("div", { class: "hero" }, avatar(i), h("div", { class: "grow" }, h("h2", {}, i.name, run && h("span", { class: "chip live" }, t("home.running"))), h("p", { class: "mute" }, desc(i))),
      run ? h("button", { class: "btn danger big", onclick: () => stopGame() }, t("home.stop")) : h("button", { class: "play", disabled: !canPlay(), onclick: () => play() }, t("c.play"))),
    insts.length > 1 ? card(t("home.active"), sel) : null,
    loginHint());
}

// ---------- Instances ----------
let instTab = "mine";
function renderInsts() {
  const el = $("#p-inst");
  const tabs = seg([["mine", t("inst.tab_mine")], ["modpacks", t("inst.tab_modpacks")]], instTab, v => { instTab = v; renderInsts(); });
  el.replaceChildren(h("div", { class: "head" }, h("div", {}, h("h1", {}, t("inst.title")), h("p", { class: "sub" }, t("inst.sub"))), h("button", { class: "btn p", onclick: () => newInstModal() }, t("inst.new"))), tabs);
  if (instTab === "modpacks") { el.append(modpackBrowser()); return; }
  const grid = h("div", { class: "grid" });
  const fill = () => {
    const q = instQuery.trim().toLowerCase();
    const list = insts.filter(i => !q || i.name.toLowerCase().includes(q)).sort((a, b) => (b.last_played || 0) - (a.last_played || 0) || a.name.localeCompare(b.name));
    grid.replaceChildren(...list.map(instCard));
  };
  const search = h("input", { class: "inp", type: "search", placeholder: t("inst.search"), "aria-label": t("inst.search"), value: instQuery });
  search.style.maxWidth = "320px"; search.style.marginBottom = "14px";
  search.addEventListener("input", () => { instQuery = search.value; fill(); });
  el.append(insts.length > 4 ? search : null, insts.length ? grid : empty(t("inst.none")));
  fill();
}
function modpackBrowser() {
  const wrap = h("div"); let offset = 0, timer;
  const inp = h("input", { class: "inp", type: "search", placeholder: t("mp.search_ph") });
  const res = h("div", { class: "list" }), more = h("button", { class: "btn", hidden: true }, t("mods.more"));
  async function run(reset) {
    if (reset) { offset = 0; res.replaceChildren(skel(4)); }
    try {
      const r = await inv("mod_search_modpacks", { query: inp.value.trim(), offset });
      if (reset) res.replaceChildren();
      if (!r.hits.length && reset) res.append(empty(t("mp.none")));
      r.hits.forEach(x => res.append(hitRow(x)));
      offset += r.hits.length; more.hidden = offset >= r.total;
    } catch (err) { res.replaceChildren(h("p", { class: "bad" }, errText(err))); }
  }
  function hitRow(x) {
    const b = h("button", { class: "btn p sm" }, t("c.install"));
    b.addEventListener("click", async () => {
      b.disabled = true; showPrep(t("prep.modpack", { name: x.title }));
      try { const inst = await inv("mod_install_modpack", { project: x.id }); selectInst(inst.id); hidePrep(); await loadInsts(); instTab = "mine"; go("inst"); toast(t("toast.mp_done", { name: inst.name })); }
      catch (err) { hidePrep(); b.disabled = false; toast(errText(err), "bad"); }
    });
    return h("div", { class: "item mod" }, icon(x.icon_url), h("div", { class: "grow" }, h("b", {}, x.title), h("small", {}, x.description), h("small", {}, t("mods.by", { author: x.author, n: fmt(x.downloads) }))), b);
  }
  inp.addEventListener("input", () => { clearTimeout(timer); timer = setTimeout(() => run(true), 350); });
  more.addEventListener("click", () => run(false));
  wrap.append(h("small", {}, t("mp.hint")), inp, res, more); run(true);
  return wrap;
}
async function importPack(path) {
  showPrep(t("prep.import"));
  try { const inst = await inv("import_mrpack", { path }); selectInst(inst.id); hidePrep(); await loadInsts(); instTab = "mine"; go("inst"); toast(t("toast.mp_done", { name: inst.name })); }
  catch (err) { hidePrep(); toast(errText(err), "bad"); }
}
// Fichiers glissés dans la fenêtre : .mrpack = nouvelle instance ; .jar = mods ; .zip = packs de ressources.
T.event.listen("tauri://drag-drop", async e => {
  const paths = (e.payload && e.payload.paths) || [];
  const packs = paths.filter(p => /\.mrpack$/i.test(p));
  if (packs.length) { for (const p of packs) await importPack(p); return; }
  const i = curInst(); if (!i) return;
  try { const n = await inv("add_files", { instanceId: i.id, paths }); if (n) { await loadInsts(); R[page](); toast(t("toast.dropped", { n })); } else toast(t("toast.drop_none"), "bad"); }
  catch (err) { toast(errText(err), "bad"); }
});
function instCard(i) {
  const on = i.id === curId, run = running.has(i.id);
  return h("div", { class: "icard" + (on ? " on" : ""), onclick: () => { selectInst(i.id); renderInsts(); } },
    avatar(i), h("div", { class: "grow" }, h("b", {}, i.name, on && h("span", { class: "chip" }, t("inst.active")), run && h("span", { class: "chip live" }, t("home.running"))), h("small", {}, desc(i))),
    h("div", { class: "acts" },
      run ? h("button", { class: "btn sm danger", onclick: e => { e.stopPropagation(); stopGame(i); } }, t("home.stop"))
          : h("button", { class: "btn sm p", disabled: !(acct || dev), onclick: e => { e.stopPropagation(); play(i); } }, t("c.play")),
      h("button", { class: "btn sm", onclick: e => { e.stopPropagation(); editInstModal(i); } }, t("c.edit")),
      h("button", { class: "btn sm", onclick: e => { e.stopPropagation(); inv("open_folder", { id: i.id }).catch(x => toast(errText(x), "bad")); } }, t("c.open")),
      h("button", { class: "btn sm danger", onclick: e => { e.stopPropagation(); confirmBox(t("inst.delete_title", { name: i.name }), t("inst.delete_text"), t("inst.delete_confirm"), async () => {
        try { await inv("delete_instance", { id: i.id }); await loadInsts(); R[page](); toast(t("toast.deleted")); } catch (x) { toast(errText(x), "bad"); } }); } }, t("c.delete"))));
}
async function newInstModal(preset) {
  const name = h("input", { class: "inp", type: "text", maxlength: "40", placeholder: t("new.name_ph") });
  const ver = h("select", { class: "inp" });
  const snap = h("input", { type: "checkbox" }); snap.checked = S.snapshots;
  let loader = preset || "vanilla", versionsOk = false;
  const pack = h("input", { type: "checkbox" }); pack.checked = S.pack;
  const packRow = h("label", { class: "chk" }, pack, t("new.pack"));
  packRow.hidden = loader !== "fabric";
  const note = h("small", {}, t("new.forge_note")); note.hidden = !(loader === "forge" || loader === "neoforge");
  async function fill() {
    versionsOk = false; ver.replaceChildren(h("option", {}, t("c.loading")));
    try { const l = await inv("list_versions", { snapshots: snap.checked }); ver.replaceChildren(...l.map(v => h("option", { value: v }, v))); versionsOk = l.length > 0; }
    catch (err) { ver.replaceChildren(h("option", {}, t("new.unavailable"))); toast(errText(err), "bad"); }
  }
  snap.addEventListener("change", () => { S.snapshots = snap.checked; saveS(); fill(); });
  const body = h("div", {}, h("label", {}, t("new.name")), name, h("label", {}, t("new.version")), ver,
    h("label", { class: "chk" }, snap, t("new.snapshots")),
    h("label", {}, t("new.type")), seg([["vanilla", t("new.vanilla")], ["fabric", "Fabric"], ["quilt", "Quilt"], ["forge", "Forge"], ["neoforge", "NeoForge"]], loader, v => { loader = v; packRow.hidden = v !== "fabric"; note.hidden = !(v === "forge" || v === "neoforge"); }), packRow, note);
  modal(t("new.title"), body, [mbtn(t("c.cancel"), closeModal), mbtn(t("c.create"), async () => {
    if (!name.value.trim()) { toast(t("toast.need_name"), "bad"); return; }
    if (!versionsOk) { toast(t("toast.need_version"), "bad"); return; }
    const n = name.value.trim(), v = ver.value, l = loader, withPack = pack.checked;
    closeModal(); showPrep(t("prep.create"));
    try {
      const inst = await inv("create_instance", { name: n, version: v, loader: l, mcLang: MC_LANG[lang] || null, profile: S.profile });
      selectInst(inst.id);
      if (l === "fabric" && withPack) {
        S.pack = true; saveS();
        for (const slug of ["sodium", "lithium", "ferrite-core"]) {
          try { await inv("mod_install", { instanceId: inst.id, project: slug }); } catch (err) { toast(slug + " : " + errText(err), "bad"); }
        }
      }
      hidePrep(); await loadInsts(); go(page === "mods" ? "mods" : "inst"); toast(t("toast.created"));
    } catch (err) { hidePrep(); toast(errText(err), "bad"); }
  }, "p")]);
  fill();
}
function editInstModal(i) {
  const name = h("input", { class: "inp", type: "text", maxlength: "40", value: i.name });
  let color = i.color;
  const sw = h("div", { class: "swatches" }, COLORS.map(c => { const b = h("button", { class: "sw" + (c === color ? " on" : ""), type: "button", "aria-label": c }); b.style.background = c; b.addEventListener("click", () => { color = c; [...sw.children].forEach(x => x.classList.toggle("on", x === b)); }); return b; }));
  let custom = !!i.ram_gb, ramV = i.ram_gb || S.ram;
  const lab = h("b", {}, ramV + " " + t("u.gb")), rng = h("input", { type: "range", min: "1", max: String(ramMax()), value: String(ramV) });
  rng.disabled = !custom;
  rng.addEventListener("input", () => { ramV = +rng.value; lab.textContent = ramV + " " + t("u.gb"); });
  const use = toggle(custom, v => { custom = v; rng.disabled = !v; });
  const expW = h("input", { type: "checkbox" });
  const sz = h("small", {}, t("edit.disk_calc"));
  inv("instance_size", { id: i.id }).then(b => { sz.textContent = t("edit.disk", { size: fmtBytes(b) }); }).catch(() => { sz.textContent = ""; });
  const body = h("div", {}, h("label", {}, t("new.name")), name, h("label", {}, t("edit.color")), sw,
    h("label", {}, t("edit.ram")), h("div", { class: "item" }, h("div", {}, t("edit.ram_own"), h("small", {}, t("edit.ram_hint"))), use),
    h("div", { class: "item" }, rng, lab),
    h("div", { class: "item" }, h("div", {}, t("edit.verify"), h("small", {}, t("edit.verify_hint"))), mbtn(t("c.verify"), async () => { try { await inv("repair_instance", { id: i.id }); toast(t("toast.verify")); } catch (err) { toast(errText(err), "bad"); } })),
    h("div", { class: "item" }, h("div", {}, t("edit.dup"), h("small", {}, t("edit.dup_hint"))), mbtn(t("c.duplicate"), async () => {
      closeModal(); showPrep(t("prep.copy"));
      try { await inv("duplicate_instance", { id: i.id }); await loadInsts(); hidePrep(); R[page](); toast(t("toast.duplicated")); } catch (err) { hidePrep(); toast(errText(err), "bad"); } })),
    h("div", { class: "item" }, h("div", {}, t("edit.export"), h("small", {}, t("edit.export_hint"))), h("div", { class: "ctl" }, h("label", { class: "chk" }, expW, t("edit.export_worlds")), mbtn(t("edit.export"), async () => {
      closeModal(); showPrep(t("prep.export"));
      try { await inv("export_instance", { id: i.id, worlds: expW.checked }); hidePrep(); toast(t("toast.exported")); inv("open_exports"); } catch (err) { hidePrep(); toast(errText(err), "bad"); } }))),
    sz,
    h("small", {}, t("edit.version_info", { v: i.version, loader: i.loader === "vanilla" ? t("d.vanilla") : loaderName(i) + " " + (i.loader_version || "") })));
  modal(t("edit.title"), body, [mbtn(t("c.cancel"), closeModal), mbtn(t("c.save"), async () => {
    try { await inv("update_instance", { id: i.id, patch: { name: name.value, color, ram_gb: custom ? ramV : null } }); closeModal(); await loadInsts(); R[page](); toast(t("toast.edited")); }
    catch (err) { toast(errText(err), "bad"); }
  }, "p")]);
}

// ---------- Mods (Modrinth) ----------
let modsKind = "mod";
async function renderMods() {
  const el = $("#p-mods"), i = curInst();
  el.replaceChildren(h("h1", {}, t("mods.title")));
  if (!i) { el.append(empty(t("c.need_instance"))); return; }
  el.append(h("p", { class: "sub" }, t("mods.instance", { name: i.name, v: i.version })));
  const body = h("div");
  const draw = () => modsTab === "inst" ? drawInstalled(body, i) : drawBrowse(body, i);
  el.append(seg([["inst", t("mods.tab_installed")], ["browse", t("mods.tab_browse")]], modsTab, v => { modsTab = v; draw(); }), body);
  draw();
}
const icon = url => typeof url === "string" && url.startsWith("https://cdn.modrinth.com/") ? h("img", { class: "ico", src: url, alt: "", loading: "lazy" }) : h("div", { class: "ico" });
async function drawInstalled(body, i) {
  body.replaceChildren(skel(3));
  let list; try { list = await inv("mod_list", { instanceId: i.id }); } catch (err) { body.replaceChildren(h("p", { class: "bad" }, errText(err))); return; }
  body.replaceChildren(
    h("div", { class: "bar2" }, h("button", { class: "btn", onclick: () => checkUpdates(i, body) }, t("mods.check_updates")), h("button", { class: "btn", onclick: () => inv("open_folder", { id: i.id }) }, t("c.open_folder"))),
    list.length ? h("div", { class: "list" }, list.map(m => modItem(m, i, body))) : empty(t("mods.none")));
}
function modItem(m, i, body) {
  const man = !!m.manual;
  const call = op => man ? inv(op + "_file", { instanceId: i.id, filename: m.filename }) : inv(op, { instanceId: i.id, project: m.project_id });
  return h("div", { class: "item mod" }, man ? h("div", { class: "ico" }) : icon(m.icon_url),
    h("div", { class: "grow" }, h("b", {}, m.title, m.auto && h("span", { class: "chip" }, t("mods.dep")), man && h("span", { class: "chip" }, t("mods.manual")), m.kind === "shader" && h("span", { class: "chip" }, t("mods.chip_shader")), m.kind === "resourcepack" && h("span", { class: "chip" }, t("mods.chip_pack"))), h("small", {}, man ? m.filename : t("mods.version", { v: m.version_number }))),
    h("div", { class: "ctl" },
      toggle(m.enabled, async () => { try { await call("mod_toggle"); } catch (err) { toast(errText(err), "bad"); drawInstalled(body, i); } }),
      h("button", { class: "btn sm danger", onclick: () => confirmBox(t("mods.remove_title", { name: m.title }), t("mods.remove_text"), t("c.remove"), async () => {
        try { await call("mod_remove"); await loadInsts(); drawInstalled(body, i); } catch (err) { toast(errText(err), "bad"); } }) }, t("c.remove"))));
}
async function checkUpdates(i, body) {
  showPrep(t("prep.checking"));
  try {
    const l = await inv("mod_updates", { instanceId: i.id, apply: false }); hidePrep();
    if (!l.length) { toast(t("toast.mods_ok")); return; }
    modal(t("mods.updates_title", { n: l.length }), h("ul", { class: "ul" }, l.map(u => h("li", {}, u.title + " : " + u.from + " → " + u.to))),
      [mbtn(t("c.later"), closeModal), mbtn(t("mods.update_all"), async () => {
        closeModal(); showPrep(t("prep.mods_update"));
        try { await inv("mod_updates", { instanceId: i.id, apply: true }); toast(t("toast.mods_updated")); } catch (err) { toast(errText(err), "bad"); }
        hidePrep(); drawInstalled(body, i);
      }, "p")]);
  } catch (err) { hidePrep(); toast(errText(err), "bad"); }
}
function drawBrowse(body, i) {
  let offset = 0, timer; const installed = new Set();
  const kinds = seg([["mod", t("mods.kind_mod")], ["shader", t("mods.kind_shader")], ["resourcepack", t("mods.kind_pack")]], modsKind, v => { modsKind = v; drawBrowse(body, i); });
  if (modsKind === "mod" && i.loader === "vanilla") {
    body.replaceChildren(kinds, card(null, h("p", {}, t("mods.vanilla_mods")), h("br"), h("button", { class: "btn p", onclick: () => newInstModal("fabric") }, t("mods.create_fabric"))));
    return;
  }
  const kind = modsKind;
  const inp = h("input", { class: "inp", type: "search", placeholder: kind === "shader" ? t("mods.search_ph_shader") : kind === "resourcepack" ? t("mods.search_ph_pack") : t("mods.search_ph") });
  const res = h("div", { class: "list" }), more = h("button", { class: "btn", hidden: true }, t("mods.more"));
  async function run(reset) {
    if (reset) { offset = 0; res.replaceChildren(skel(4)); }
    try {
      const [r, l] = await Promise.all([inv("mod_search", { instanceId: i.id, query: inp.value.trim(), offset, kind }), inv("mod_list", { instanceId: i.id })]);
      l.forEach(m => { if (m.project_id) installed.add(m.project_id); });
      if (reset) res.replaceChildren();
      if (!r.hits.length && reset) res.append(empty(t("mods.no_results2", { v: i.version })));
      r.hits.forEach(x => res.append(hit(x)));
      offset += r.hits.length; more.hidden = offset >= r.total;
    } catch (err) { res.replaceChildren(h("p", { class: "bad" }, errText(err))); }
  }
  function hit(x) {
    const b = h("button", { class: "btn p sm" }, installed.has(x.id) ? t("c.installed") : t("c.install"));
    b.disabled = installed.has(x.id);
    b.addEventListener("click", async () => {
      b.disabled = true; showPrep(t("prep.install_mod", { name: x.title }));
      try {
        const a = await inv("mod_install", { instanceId: i.id, project: x.id, kind }); hidePrep();
        installed.add(x.id); b.textContent = t("c.installed"); await loadInsts();
        toast(a.length > 1 ? t("toast.mod_installed_deps", { name: x.title, n: a.length - 1 }) : t("toast.mod_installed", { name: x.title }));
      } catch (err) { hidePrep(); b.disabled = false; toast(errText(err), "bad"); }
    });
    return h("div", { class: "item mod" }, icon(x.icon_url),
      h("div", { class: "grow" }, h("b", {}, x.title), h("small", {}, x.description), h("small", {}, t("mods.by", { author: x.author, n: fmt(x.downloads) }))), b);
  }
  inp.addEventListener("input", () => { clearTimeout(timer); timer = setTimeout(() => run(true), 350); });
  more.addEventListener("click", () => run(false));
  body.replaceChildren(kinds, inp, res, more); run(true);
}

// ---------- Logs ----------
let logTimer = null;
const stopLogs = () => { if (logTimer) { clearInterval(logTimer); logTimer = null; } };
const logLabel = f => f.rel === "launcher.log" ? t("logs.launcher") : f.rel === "logs/latest.log" ? t("logs.latest") : t("logs.crash", { date: new Date(f.modified * 1000).toLocaleString(lang) });
const lvl = x => /\bERROR\b|\bFATAL\b|Exception|^\s+at |^Caused by/.test(x) ? "err" : /\bWARN\b/.test(x) ? "warn" : "info";
async function renderLogs() {
  stopLogs();
  const el = $("#p-logs"), i = curInst();
  el.replaceChildren(h("h1", {}, t("logs.title")), h("p", { class: "sub" }, t("logs.sub")));
  if (!i) { el.append(empty(t("c.need_instance"))); return; }
  let files = [];
  try { files = await inv("log_files", { instanceId: i.id }); } catch (err) { el.append(h("p", { class: "bad" }, errText(err))); return; }
  if (!files.length) { el.append(empty(t("logs.none"))); return; }
  const sel = h("select", { class: "inp" }, files.map(f => h("option", { value: f.rel }, logLabel(f)))); sel.style.maxWidth = "300px";
  const view = h("div", { class: "logview", tabindex: "0", role: "log" });
  let off = 0, carry = "", auto = true, count = 0;
  const addLine = x => { view.append(h("div", { class: "ln " + lvl(x) }, x || " ")); if (++count > 3000) { view.firstChild.remove(); count--; } };
  async function pull(reset) {
    if (reset) { view.replaceChildren(); off = 0; carry = ""; count = 0; }
    let r; try { r = await inv("log_read", { instanceId: i.id, rel: sel.value, offset: off }); } catch (_) { return; }
    if (r.reset) { view.replaceChildren(); carry = ""; count = 0; }
    off = r.offset;
    if (r.text) { const parts = (carry + r.text).split("\n"); carry = parts.pop(); parts.forEach(x => addLine(x.replace(/\r$/, ""))); }
    else if (carry) { addLine(carry); carry = ""; }
    if (auto) view.scrollTop = view.scrollHeight;
  }
  sel.addEventListener("change", () => pull(true));
  const mode = seg([["all", t("logs.all")], ["warn", t("logs.warn")], ["err", t("logs.err")]], "all", v => { view.className = "logview f-" + v; });
  el.append(h("div", { class: "bar2 wrap" }, sel, mode, h("div", { class: "ctl" }, t("logs.scroll"), toggle(true, v => { auto = v; })),
    h("button", { class: "btn", onclick: async () => { try { await navigator.clipboard.writeText([...view.children].map(x => x.textContent).join("\n")); toast(t("toast.copied")); } catch (_) { toast(t("toast.copy_fail"), "bad"); } } }, t("c.copy")),
    h("button", { class: "btn", onclick: () => inv("open_folder", { id: i.id }) }, t("c.open_folder"))), view);
  await pull(true);
  logTimer = setInterval(() => { if (page === "logs") pull(false); }, 1000);
}

// ---------- Plume Lab ----------
const labHistory = id => { try { return JSON.parse(localStorage.getItem("plume.lab." + id) || "[]"); } catch (_) { return []; } };
const saveLab = (id, r) => localStorage.setItem("plume.lab." + id, JSON.stringify([...labHistory(id), r].slice(-8)));
let labBusy = false;
T.event.listen("lab-progress", e => {
  const p = e.payload, el = $("#labstatus"); if (!el || !curInst() || p.id !== curInst().id) return;
  const s = Math.round(p.elapsed);
  el.textContent = p.ready != null ? t("lab.live_ready", { s, r: Math.round(p.ready), m: p.rss_mb }) : t("lab.live_wait", { s, m: p.rss_mb });
});
function labReport(i, r, prev) {
  const V = { ok: ["lab.verdict_ok", "ok"], crash: ["lab.verdict_crash", "bad"], timeout: ["lab.verdict_timeout", "bad"], unknown: ["lab.verdict_unknown", "warn"] };
  const [vk, vc] = V[r.verdict] || V.unknown;
  const na = t("lab.na"), sec = n => t("lab.secs", { n: Math.round(n * 10) / 10 }), mb = n => t("lab.mb", { n });
  const delta = (a, b, f) => (a != null && b != null) ? ((a - b > 0 ? "+" : "") + f(a - b)) : "";
  const list = (title, arr) => arr && arr.length ? h("div", {}, h("b", {}, title), h("ul", { class: "issues" }, arr.map(x => h("li", {}, x)))) : null;
  const clean = !r.crash_report && !(r.mixin || []).length && !(r.deps || []).length && !r.errors;
  return h("div", {},
    h("div", { class: "verdict " + vc }, t(vk)),
    card(null,
      row(t("lab.m_ready"), prev ? t("lab.compare") + " : " + delta(r.ready_secs, prev.ready_secs, n => sec(n)) : null, r.ready_secs != null ? sec(r.ready_secs) : na),
      row(t("lab.m_ram"), prev ? delta(r.peak_rss_mb, prev.peak_rss_mb, n => mb(n)) : null, r.peak_rss_mb ? mb(r.peak_rss_mb) : na),
      row(t("lab.m_cpu"), null, r.cpu_cores != null ? t("lab.m_cpu_val", { n: r.cpu_cores }) : na),
      row(t("lab.m_mods"), null, String(r.mods)),
      row(t("lab.m_gc"), null, r.gc ? t("lab.m_gc_val", { n: r.gc.count, max: r.gc.max_ms, total: r.gc.total_ms }) : na),
      row(t("lab.m_errors"), null, r.errors + " / " + r.warns)),
    card(t("lab.issues"),
      clean ? h("p", { class: "mute" }, t("lab.no_issue")) : null,
      r.crash_report ? h("p", { class: "bad" }, t("lab.i_crash", { file: r.crash_report })) : null,
      list(t("lab.i_mixin"), r.mixin), list(t("lab.i_deps"), r.deps), list(t("lab.i_errors"), r.samples)),
    h("div", { class: "bar2" }, h("button", { class: "btn", onclick: async () => {
      const md = ["# Plume Lab · " + i.name + " (" + i.version + ", " + loaderName(i) + ")", "", t(vk), "",
        "- " + t("lab.m_ready") + " : " + (r.ready_secs != null ? sec(r.ready_secs) : na), "- " + t("lab.m_ram") + " : " + (r.peak_rss_mb ? mb(r.peak_rss_mb) : na),
        "- " + t("lab.m_mods") + " : " + r.mods, "- " + t("lab.m_errors") + " : " + r.errors + " / " + r.warns,
        ...(r.mixin || []).map(x => "- Mixin : " + x), ...(r.deps || []).map(x => "- " + x)].join("\n");
      try { await navigator.clipboard.writeText(md); toast(t("lab.copied")); } catch (_) { toast(t("toast.copy_fail"), "bad"); } } }, t("lab.copy"))));
}
async function renderLab() {
  const el = $("#p-lab"), i = curInst();
  el.replaceChildren(h("h1", {}, t("lab.title")), h("p", { class: "sub" }, t("lab.sub")));
  if (!i) { el.append(empty(t("c.need_instance"))); return; }
  const secs = h("input", { class: "inp", type: "number", min: "5", max: "300", value: String(S.labSeconds || 20), "aria-label": t("lab.settle") }); secs.style.width = "100px";
  const status = h("p", { class: "mute", id: "labstatus" }), out = h("div");
  const run = h("button", { class: "btn p", disabled: labBusy || running.has(i.id) || !(acct || dev) }, t("lab.run"));
  run.addEventListener("click", async () => {
    labBusy = true; run.disabled = true; out.replaceChildren(); status.textContent = t("lab.starting");
    S.labSeconds = Math.max(5, Math.min(300, +secs.value || 20)); saveS();
    stopping.add(i.id);
    try {
      const r = await inv("lab_run", { instanceId: i.id, seconds: S.labSeconds });
      const prev = labHistory(i.id).slice(-1)[0]; saveLab(i.id, r);
      status.textContent = ""; out.replaceChildren(labReport(i, r, prev));
    } catch (err) { stopping.delete(i.id); status.textContent = ""; toast(errText(err), "bad"); }
    labBusy = false; run.disabled = false; await loadInsts();
  });
  el.append(card(t("lab.run_title"), row(t("lab.settle"), t("lab.settle_hint"), secs), h("div", { class: "bar2" }, run), status), out);
  const hist = labHistory(i.id);
  if (hist.length) out.append(labReport(i, hist[hist.length - 1], hist[hist.length - 2]));
}

// ---------- Paramètres ----------
const clampRam = r => sysRam ? Math.min(r, ramMax()) : r;
const ramMax = () => sysRam ? Math.max(2, Math.min(32, sysRam - 1)) : 16;
function langCard() {
  const bar = p => { const u = h("u"); u.style.width = p + "%"; return h("div", { class: "pb" }, u); };
  const opt = (code, name, pct) => {
    const b = h("button", { class: "lang" + (S.lang === code ? " on" : ""), type: "button" }, h("div", { class: "grow" }, h("b", {}, name), pct != null && h("small", {}, t("set.lang_pct", { p: pct }))), pct != null && bar(pct));
    b.addEventListener("click", () => { S.lang = code; saveS(); pickLang(); });
    return b;
  };
  return card(t("set.language"), h("div", { class: "langs" }, opt("auto", t("set.lang_auto"), null), LANGS.map(l => opt(l.code, l.name, l.pct))), row(t("set.lang_sync"), t("set.lang_sync_hint"), h("div", { class: "ctl" }, h("button", { class: "btn sm", onclick: () => syncI18n(true) }, t("set.lang_refresh")), toggle(S.langSync, v => { S.langSync = v; saveS(); }))), h("small", {}, t("set.lang_help")));
}
function renderSettings() {
  const el = $("#p-set");
  const sw = h("div", { class: "swatches" }, COLORS.map(c => { const b = h("button", { class: "sw" + (c === S.accent ? " on" : ""), type: "button", "aria-label": c }); b.style.background = c; b.addEventListener("click", () => { S.accent = c; saveS(); [...sw.children].forEach(x => x.classList.toggle("on", x === b)); }); return b; }));
  const num = (k, ph) => { const n = h("input", { class: "inp", type: "number", min: "0", max: "10000", placeholder: ph, "aria-label": ph, value: S[k] || "" }); n.addEventListener("change", () => { S[k] = Math.max(0, parseInt(n.value) || 0); saveS(); }); return n; };
  const lab = h("b", {}, S.ram + " " + t("u.gb")), warn = h("div", { class: "warn" }), rng = h("input", { type: "range", min: "1", max: String(ramMax()), value: String(Math.min(S.ram, ramMax())), "aria-label": t("set.ram") });
  const chk = () => { warn.textContent = sysRam && +rng.value > sysRam * 0.75 ? t("set.ram_warn", { total: sysRam }) : ""; };
  rng.addEventListener("input", () => { lab.textContent = rng.value + " " + t("u.gb"); chk(); });
  rng.addEventListener("change", () => { S.ram = +rng.value; saveS(); });
  chk();
  const jvm = h("input", { class: "inp", type: "text", placeholder: "-XX:+UseZGC", value: S.jvm }); jvm.style.width = "260px";
  jvm.addEventListener("change", () => { S.jvm = jvm.value.trim(); saveS(); });
  const jp = h("input", { class: "inp", type: "text", placeholder: t("set.java_ph"), value: S.java }); jp.style.width = "260px";
  jp.addEventListener("change", () => { S.java = jp.value.trim(); saveS(); });
  const ver = h("span", {}, "…"), dhint = h("small", {}, t("set.data_hint", { size: "…" }));
  el.replaceChildren(h("h1", {}, t("set.title")), h("p", { class: "sub" }, t("set.sub")),
    card(t("set.appearance"),
      row(t("set.theme"), null, seg([["system", t("set.theme_system")], ["light", t("set.theme_light")], ["dark", t("set.theme_dark")]], S.theme, v => { S.theme = v; saveS(); })),
      row(t("set.accent"), null, sw),
      row(t("set.compact"), t("set.compact_hint"), toggle(S.compact, v => { S.compact = v; saveS(); }))),
    langCard(),
    card(t("set.launch"),
      row(t("set.hide"), t("set.hide_hint"), toggle(S.hide, v => { S.hide = v; saveS(); })),
      row(t("set.fullscreen"), null, toggle(S.fullscreen, v => { S.fullscreen = v; saveS(); })),
      row(t("set.window"), t("set.window_hint"), h("div", { class: "ctl" }, num("width", t("set.width")), "×", num("height", t("set.height"))))),
    card(t("set.perf"),
      row(t("set.profile"), t("set.profile_hint"), seg(PROFILES.map((p, k) => [k, t(p.k)]), S.profile, v => { S.profile = v; S.ram = Math.min(PROFILES[v].ram, ramMax()); saveS(); renderSettings(); })),
      row(t("set.ram"), t("set.ram_hint"), h("div", {}, h("div", { class: "ctl" }, rng, lab), warn)),
      row(t("set.jvm"), t("set.jvm_hint"), jvm),
      row(t("set.java"), t("set.java_hint"), jp)),
    card(t("set.cosmetics"), row(t("set.cosmetics"), t("set.cosmetics_hint"), h("span", { class: "chip" }, t("c.soon")))),
    accountCard(),
    card(t("set.about"),
      row("Plume", null, ver),
      h("div", { class: "item" }, h("div", {}, h("b", {}, t("set.data")), dhint), h("div", { class: "ctl" }, h("button", { class: "btn", onclick: () => inv("open_folder", { id: null }) }, t("c.open")))),
      row(t("set.diag"), t("set.diag_hint"), h("button", { class: "btn", onclick: async () => {
        try { const d = await inv("diagnostics"); await navigator.clipboard.writeText(d + "\nLangue : " + lang + "\nThème : " + S.theme); toast(t("toast.diag_copied")); } catch (_) { toast(t("toast.copy_fail"), "bad"); } } }, t("c.copy"))),
      row(t("set.report"), null, h("button", { class: "btn", onclick: () => openUrl(REPO + "/issues") }, t("c.open"))),
      row(t("set.source"), null, h("button", { class: "btn", onclick: () => openUrl(REPO) }, t("c.open"))),
      row(t("set.website"), null, h("button", { class: "btn", onclick: () => openUrl(siteUrl()) }, t("c.open"))),
      row(t("set.donate"), t("set.donate_hint"), h("button", { class: "btn p", onclick: donate }, t("nav.donate"))),
      row(t("set.reset"), t("set.reset_hint"), h("button", { class: "btn danger", onclick: () => confirmBox(t("set.reset_title"), t("set.reset_text"), t("c.reset"), () => { localStorage.removeItem("plume.s"); S = { ...DEF }; saveS(); pickLang(); }) }, t("c.reset")))));
  T.app.getVersion().then(v => { ver.textContent = t("set.version", { v }); }).catch(() => { ver.textContent = ""; });
  inv("data_size").then(b => { dhint.textContent = t("set.data_hint", { size: fmtBytes(b) }); }).catch(() => {});
}
const donate = () => openUrl(siteUrl(lang === "fr" ? "#soutenir" : "#support"));
$("#donate").addEventListener("click", donate);

// ---------- Mises à jour du launcher ----------
$("#updgo").addEventListener("click", async () => {
  showPrep(t("prep.self"));
  try { await inv("install_update"); } catch (err) { hidePrep(); $("#updt").textContent = t("upd.fail", { e: errText(err) }); }
});
async function checkSelfUpdate() {
  try { const n = await inv("check_update"); if (n) { $("#updt").textContent = t("upd.available", { v: n }); $("#upd").hidden = false; } } catch (_) {}
}

// ---------- Démarrage ----------
$$("#nav button[data-p]").forEach(b => b.addEventListener("click", () => go(b.dataset.p)));
window.addEventListener("offline", () => toast(t("toast.offline"), "bad"));
window.addEventListener("online", () => toast(t("toast.online")));
applyS();
(async () => {
  T.app.getVersion().then(v => { $("#appv").textContent = v; }).catch(() => {});
  await initI18n(); lang = resolveLang(); CUR = (LANGS.find(l => l.code === lang) || {}).data || FR; document.documentElement.lang = lang; applyStatic();
  try { sysRam = await inv("system_ram"); } catch (_) {}
  if (sysRam && S.ram > ramMax()) { S.ram = ramMax(); saveS(); }
  try { (await inv("running")).forEach(x => running.add(x)); } catch (_) {}
  await refreshAcct(); await loadInsts(); go("home"); checkSelfUpdate(); syncI18n(false);
})();
