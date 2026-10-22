"use strict";
const T = window.__TAURI__;
const inv = (c, a) => T.core.invoke(c, a);
const $ = s => document.querySelector(s), $$ = s => [...document.querySelectorAll(s)];

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
const row = (t, d, ctl) => h("div", { class: "item" }, h("div", {}, h("b", {}, t), d && h("small", {}, d)), h("div", { class: "ctl" }, ctl));
const card = (t, ...k) => h("section", { class: "card" }, t && h("h3", {}, t), ...k);
const empty = t => h("div", { class: "empty" }, t);
const fmt = n => n >= 1e6 ? (n / 1e6).toFixed(1) + " M" : n >= 1e3 ? Math.round(n / 1e3) + " k" : String(n || 0);
const errText = e => String(e && e.message || e);

function toast(msg, kind) {
  const t = h("div", { class: "toast" + (kind === "bad" ? " bad" : "") }, msg);
  $("#toasts").append(t);
  setTimeout(() => t.remove(), kind === "bad" ? 7000 : 3500);
}
function modal(title, body, actions) {
  $("#modal").replaceChildren(h("div", { class: "mcard", role: "dialog", "aria-modal": "true" }, h("h2", {}, title), body, h("div", { class: "macts" }, actions)));
  $("#modal").classList.add("show");
}
const closeModal = () => $("#modal").classList.remove("show");
const mbtn = (label, fn, cls = "") => h("button", { class: "btn " + cls, type: "button", onclick: fn }, label);
function confirmBox(title, text, label, fn) {
  modal(title, h("p", {}, text), [mbtn("Annuler", closeModal), mbtn(label, async () => { closeModal(); await fn(); }, "p")]);
}
$("#modal").addEventListener("click", e => { if (e.target.id === "modal") closeModal(); });
document.addEventListener("keydown", e => { if (e.key === "Escape") closeModal(); });

function showPrep(title) {
  $("#ptitle").textContent = title; $("#prog").textContent = "Démarrage…";
  $("#pb").parentElement.classList.add("ind"); $("#pb").style.width = "";
  $("#ovPrep").classList.add("show");
}
const hidePrep = () => $("#ovPrep").classList.remove("show");
T.event.listen("progress", e => {
  const p = e.payload, bar = $("#pb").parentElement, mb = p.n > 1e6;
  $("#prog").textContent = p.t + (p.n ? " : " + (mb ? (p.d / 1e6).toFixed(1) + " / " + (p.n / 1e6).toFixed(1) + " Mo" : p.d + " / " + p.n) : "…");
  if (p.n) { bar.classList.remove("ind"); $("#pb").style.width = Math.min(100, p.d / p.n * 100) + "%"; }
  else { bar.classList.add("ind"); $("#pb").style.width = ""; }
});

// ---------- Réglages (personnalisation) ----------
const P = [{ n: "Éco", ram: 2, nice: 10 }, { n: "Équilibré", ram: 4, nice: 0 }, { n: "Performance", ram: 8, nice: 0 }];
const COLORS = ["#2f55f0", "#1f9d63", "#d6812b", "#b74bd6", "#d64545", "#2b9fd6"];
const DEF = { theme: "system", accent: "#2f55f0", compact: false, hide: true, width: 0, height: 0, fullscreen: false, profile: 1, ram: 4, jvm: "", snapshots: false, pack: true };
let S = { ...DEF };
try { S = { ...DEF, ...JSON.parse(localStorage.getItem("plume.s") || "{}") }; } catch (_) {}
function ink(hex) { const n = parseInt(hex.slice(1), 16); return ((n >> 16) * 299 + ((n >> 8) & 255) * 587 + (n & 255) * 114) / 1000 > 150 ? "#101820" : "#fff"; }
function applyS() {
  const r = document.documentElement;
  if (S.theme === "system") r.removeAttribute("data-theme"); else r.dataset.theme = S.theme;
  const acc = /^#[0-9a-f]{6}$/i.test(S.accent) ? S.accent : DEF.accent;
  r.style.setProperty("--acc", acc); r.style.setProperty("--accink", ink(acc));
  document.body.classList.toggle("compact", !!S.compact);
}
const saveS = () => { localStorage.setItem("plume.s", JSON.stringify(S)); applyS(); };
const launchOpts = i => ({ ramGb: i.ram_gb || S.ram, nice: P[S.profile].nice, jvmArgs: S.jvm, width: S.width || null, height: S.height || null, fullscreen: S.fullscreen, hide: S.hide });

// ---------- État ----------
let insts = [], curId = localStorage.getItem("plume.cur"), acct = null, dev = null, page = "home", modsTab = "inst";
const curInst = () => insts.find(i => i.id === curId);
const R = { home: renderHome, inst: renderInsts, mods: renderMods, set: renderSettings };
function go(p) {
  page = p;
  $$(".v").forEach(v => v.classList.toggle("on", v.id === "p-" + p));
  $$("#nav button[data-p]").forEach(b => b.classList.toggle("on", b.dataset.p === p));
  R[p]();
}
async function loadInsts() {
  try { insts = await inv("list_instances"); } catch (_) { insts = []; }
  if (!insts.find(i => i.id === curId)) curId = insts[0] ? insts[0].id : null;
  if (curId) localStorage.setItem("plume.cur", curId);
}
const selectInst = id => { curId = id; localStorage.setItem("plume.cur", id); };
function ago(ts) {
  if (!ts) return "jamais jouée";
  const d = Math.floor((Date.now() / 1000 - ts) / 86400);
  return d <= 0 ? "jouée aujourd’hui" : d === 1 ? "jouée hier" : "jouée il y a " + d + " jours";
}
const desc = i => (i.loader === "fabric" ? "Fabric" : "Vanilla") + " · Minecraft " + i.version + (i.loader === "fabric" ? " · " + i.mods + " mod" + (i.mods > 1 ? "s" : "") : "") + " · " + ago(i.last_played);
function avatar(i) {
  const a = h("div", { class: "av" }, (i.name.trim()[0] || "?").toUpperCase());
  a.style.background = /^#[0-9a-f]{6}$/i.test(i.color) ? i.color : DEF.accent;
  return a;
}

// ---------- Compte ----------
async function refreshAcct() {
  try { acct = await inv("account_info"); } catch (_) { acct = null; }
  try { dev = await inv("dev_offline"); } catch (_) { dev = null; }
}
const canPlay = () => !!curInst() && !!(acct || dev);
function accountCard() {
  return card("Compte Microsoft", acct
    ? row(acct.name, "Compte vérifié, Minecraft Java Edition", h("button", { class: "btn", onclick: async () => { await inv("logout"); await refreshAcct(); R[page](); } }, "Se déconnecter"))
    : row(dev ? dev + " (test)" : "Non connecté", "Ton compte doit posséder Minecraft Java Edition.", h("button", { class: "btn p", onclick: login }, "Se connecter avec Microsoft")));
}
async function login() {
  $("#lerr").textContent = "En attente de la connexion…"; $("#lerr").classList.remove("bad");
  try {
    const d = await inv("login_start");
    $("#lcode").textContent = d.user_code; $("#lurl").textContent = d.verification_uri;
    $("#ovLogin").classList.add("show");
    inv("open_url", { url: d.verification_uri }).catch(() => {});
    await inv("login_finish", { deviceCode: d.device_code, interval: d.interval, expiresIn: d.expires_in });
    $("#ovLogin").classList.remove("show");
    await refreshAcct(); R[page]();
  } catch (err) { $("#lerr").textContent = errText(err); $("#lerr").classList.add("bad"); $("#ovLogin").classList.add("show"); }
}
$("#lclose").addEventListener("click", () => $("#ovLogin").classList.remove("show"));

// ---------- Jeu ----------
async function crash(msg) {
  $("#cmsg").textContent = msg; $("#clog").textContent = "";
  try { $("#clog").textContent = await inv("read_log", { instanceId: curId }); } catch (_) {}
  $("#ovCrash").classList.add("show");
}
async function play() {
  const i = curInst(); if (!i) return;
  showPrep("Préparation du jeu");
  try {
    const code = await inv("launch", { instanceId: i.id, opts: launchOpts(i) });
    hidePrep(); await loadInsts(); R[page]();
    if (code !== 0) await crash("Minecraft s’est arrêté avec le code " + code + ".");
  } catch (err) { hidePrep(); await crash(errText(err)); }
}
$("#again").addEventListener("click", () => { $("#ovCrash").classList.remove("show"); play(); });
$("#back").addEventListener("click", () => $("#ovCrash").classList.remove("show"));

// ---------- Accueil ----------
function renderHome() {
  const el = $("#p-home"), i = curInst();
  el.replaceChildren(h("h1", {}, "Prêt à jouer"), h("p", { class: "sub" }, "Le launcher se cache pendant la partie et revient à l’arrêt du jeu."));
  if (!i) {
    el.append(card(null, h("p", {}, "Tu n’as pas encore d’instance. Une instance, c’est une installation de Minecraft avec sa propre version et ses propres mods."), h("br"), h("button", { class: "btn p", onclick: () => newInstModal() }, "Créer ma première instance")), accountCard());
    return;
  }
  const sel = h("select", { class: "inp", "aria-label": "Instance active", onchange: e => { selectInst(e.target.value); renderHome(); } }, insts.map(x => h("option", { value: x.id }, x.name)));
  sel.value = i.id; sel.style.maxWidth = "260px";
  el.append(
    h("div", { class: "hero" }, avatar(i), h("div", { class: "grow" }, h("h2", {}, i.name), h("p", { class: "mute" }, desc(i))),
      h("button", { class: "play", disabled: !canPlay(), onclick: play }, "Jouer")),
    insts.length > 1 ? card("Instance active", sel) : null,
    accountCard());
}

// ---------- Instances ----------
function renderInsts() {
  $("#p-inst").replaceChildren(
    h("div", { class: "head" }, h("div", {}, h("h1", {}, "Instances"), h("p", { class: "sub" }, "Chaque instance a ses propres mods, mondes et version de Minecraft.")),
      h("button", { class: "btn p", onclick: () => newInstModal() }, "Nouvelle instance")),
    insts.length ? h("div", { class: "grid" }, insts.map(instCard)) : empty("Aucune instance. Crée la première pour commencer."));
}
function instCard(i) {
  const on = i.id === curId;
  return h("div", { class: "icard" + (on ? " on" : ""), onclick: () => { selectInst(i.id); renderInsts(); } },
    avatar(i), h("div", { class: "grow" }, h("b", {}, i.name, on && h("span", { class: "chip" }, "Active")), h("small", {}, desc(i))),
    h("div", { class: "acts" },
      h("button", { class: "btn sm", onclick: e => { e.stopPropagation(); editInstModal(i); } }, "Modifier"),
      h("button", { class: "btn sm", onclick: e => { e.stopPropagation(); inv("open_folder", { id: i.id }).catch(x => toast(errText(x), "bad")); } }, "Dossier"),
      h("button", { class: "btn sm danger", onclick: e => { e.stopPropagation(); confirmBox("Supprimer « " + i.name + " » ?", "Les mods, les mondes et les options de cette instance seront supprimés définitivement.", "Supprimer définitivement", async () => {
        try { await inv("delete_instance", { id: i.id }); await loadInsts(); renderInsts(); toast("Instance supprimée"); } catch (x) { toast(errText(x), "bad"); } }); } }, "Supprimer")));
}
async function newInstModal(preset) {
  const name = h("input", { class: "inp", type: "text", maxlength: "40", placeholder: "Ex : Survie 1.21" });
  const ver = h("select", { class: "inp" });
  const snap = h("input", { type: "checkbox" }); snap.checked = S.snapshots;
  let loader = preset || "vanilla", versionsOk = false;
  const pack = h("input", { type: "checkbox" }); pack.checked = S.pack;
  const packRow = h("label", { class: "chk" }, pack, "Installer les mods de performance (Sodium, Lithium, FerriteCore)");
  packRow.hidden = loader !== "fabric";
  async function fill() {
    versionsOk = false; ver.replaceChildren(h("option", {}, "Chargement…"));
    try { const l = await inv("list_versions", { snapshots: snap.checked }); ver.replaceChildren(...l.map(v => h("option", { value: v }, v))); versionsOk = l.length > 0; }
    catch (err) { ver.replaceChildren(h("option", {}, "Liste indisponible")); toast(errText(err), "bad"); }
  }
  snap.addEventListener("change", () => { S.snapshots = snap.checked; saveS(); fill(); });
  const body = h("div", {}, h("label", {}, "Nom"), name, h("label", {}, "Version de Minecraft"), ver,
    h("label", { class: "chk" }, snap, "Afficher les snapshots"),
    h("label", {}, "Type"), seg([["vanilla", "Vanilla"], ["fabric", "Fabric (mods)"]], loader, v => { loader = v; packRow.hidden = v !== "fabric"; }), packRow);
  modal("Nouvelle instance", body, [mbtn("Annuler", closeModal), mbtn("Créer", async () => {
    if (!name.value.trim()) { toast("Donne un nom à l’instance", "bad"); return; }
    if (!versionsOk) { toast("Choisis une version", "bad"); return; }
    const n = name.value.trim(), v = ver.value, l = loader, withPack = pack.checked;
    closeModal(); showPrep("Création de l’instance");
    try {
      const inst = await inv("create_instance", { name: n, version: v, loader: l });
      selectInst(inst.id);
      if (l === "fabric" && withPack) {
        S.pack = true; saveS();
        for (const slug of ["sodium", "lithium", "ferrite-core"]) {
          try { await inv("mod_install", { instanceId: inst.id, project: slug }); } catch (err) { toast(slug + " : " + errText(err), "bad"); }
        }
      }
      hidePrep(); await loadInsts(); go(page === "mods" ? "mods" : "inst"); toast("Instance créée");
    } catch (err) { hidePrep(); toast(errText(err), "bad"); }
  }, "p")]);
  fill();
}
function editInstModal(i) {
  const name = h("input", { class: "inp", type: "text", maxlength: "40", value: i.name });
  let color = i.color;
  const sw = h("div", { class: "swatches" }, COLORS.map(c => { const b = h("button", { class: "sw" + (c === color ? " on" : ""), type: "button", "aria-label": c }); b.style.background = c; b.addEventListener("click", () => { color = c; [...sw.children].forEach(x => x.classList.toggle("on", x === b)); }); return b; }));
  let custom = !!i.ram_gb, ramV = i.ram_gb || S.ram;
  const lab = h("b", {}, ramV + " Go"), rng = h("input", { type: "range", min: "1", max: "16", value: String(ramV) });
  rng.disabled = !custom;
  rng.addEventListener("input", () => { ramV = +rng.value; lab.textContent = ramV + " Go"; });
  const use = toggle(custom, v => { custom = v; rng.disabled = !v; });
  const body = h("div", {}, h("label", {}, "Nom"), name, h("label", {}, "Couleur"), sw,
    h("label", {}, "Mémoire (RAM)"), h("div", { class: "item" }, h("div", {}, "RAM propre à cette instance", h("small", {}, "Sinon, le réglage global est utilisé")), use),
    h("div", { class: "item" }, rng, lab),
    h("small", {}, "Version : Minecraft " + i.version + (i.loader === "fabric" ? " (Fabric " + (i.loader_version || "") + ")" : " (Vanilla)") + ". Pour changer de version, crée une nouvelle instance."));
  modal("Modifier l’instance", body, [mbtn("Annuler", closeModal), mbtn("Enregistrer", async () => {
    try { await inv("update_instance", { id: i.id, patch: { name: name.value, color, ram_gb: custom ? ramV : null } }); closeModal(); await loadInsts(); R[page](); toast("Instance modifiée"); }
    catch (err) { toast(errText(err), "bad"); }
  }, "p")]);
}

// ---------- Mods (Modrinth) ----------
async function renderMods() {
  const el = $("#p-mods"), i = curInst();
  el.replaceChildren(h("h1", {}, "Mods"));
  if (!i) { el.append(empty("Crée d’abord une instance.")); return; }
  el.append(h("p", { class: "sub" }, "Instance : " + i.name + " · Minecraft " + i.version));
  if (i.loader !== "fabric") {
    el.append(card(null, h("p", {}, "Cette instance est Vanilla : elle ne peut pas utiliser de mods. Crée une instance Fabric pour installer des mods depuis Modrinth."), h("br"), h("button", { class: "btn p", onclick: () => newInstModal("fabric") }, "Créer une instance Fabric")));
    return;
  }
  const body = h("div");
  const draw = () => modsTab === "inst" ? drawInstalled(body, i) : drawBrowse(body, i);
  el.append(seg([["inst", "Installés"], ["browse", "Parcourir Modrinth"]], modsTab, v => { modsTab = v; draw(); }), body);
  draw();
}
const icon = url => typeof url === "string" && url.startsWith("https://cdn.modrinth.com/") ? h("img", { class: "ico", src: url, alt: "", loading: "lazy" }) : h("div", { class: "ico" });
async function drawInstalled(body, i) {
  body.replaceChildren(h("p", { class: "mute" }, "Chargement…"));
  let list; try { list = await inv("mod_list", { instanceId: i.id }); } catch (err) { body.replaceChildren(h("p", { class: "bad" }, errText(err))); return; }
  body.replaceChildren(
    h("div", { class: "bar2" }, h("button", { class: "btn", onclick: () => checkUpdates(i, body) }, "Rechercher des mises à jour"), h("button", { class: "btn", onclick: () => inv("open_folder", { id: i.id }) }, "Ouvrir le dossier")),
    list.length ? h("div", { class: "list" }, list.map(m => modItem(m, i, body))) : empty("Aucun mod installé. Va dans « Parcourir Modrinth »."));
}
function modItem(m, i, body) {
  return h("div", { class: "item mod" }, icon(m.icon_url),
    h("div", { class: "grow" }, h("b", {}, m.title, m.auto && h("span", { class: "chip" }, "dépendance")), h("small", {}, "Version " + m.version_number)),
    h("div", { class: "ctl" },
      toggle(m.enabled, async () => { try { await inv("mod_toggle", { instanceId: i.id, project: m.project_id }); } catch (err) { toast(errText(err), "bad"); drawInstalled(body, i); } }),
      h("button", { class: "btn sm danger", onclick: () => confirmBox("Retirer " + m.title + " ?", "Le fichier du mod sera supprimé de cette instance.", "Retirer", async () => {
        try { await inv("mod_remove", { instanceId: i.id, project: m.project_id }); await loadInsts(); drawInstalled(body, i); } catch (err) { toast(errText(err), "bad"); } }) }, "Retirer")));
}
async function checkUpdates(i, body) {
  showPrep("Recherche de mises à jour");
  try {
    const l = await inv("mod_updates", { instanceId: i.id, apply: false }); hidePrep();
    if (!l.length) { toast("Tous les mods sont à jour"); return; }
    modal(l.length + " mise" + (l.length > 1 ? "s" : "") + " à jour", h("ul", { class: "ul" }, l.map(u => h("li", {}, u.title + " : " + u.from + " → " + u.to))),
      [mbtn("Plus tard", closeModal), mbtn("Tout mettre à jour", async () => {
        closeModal(); showPrep("Mise à jour des mods");
        try { await inv("mod_updates", { instanceId: i.id, apply: true }); toast("Mods mis à jour"); } catch (err) { toast(errText(err), "bad"); }
        hidePrep(); drawInstalled(body, i);
      }, "p")]);
  } catch (err) { hidePrep(); toast(errText(err), "bad"); }
}
function drawBrowse(body, i) {
  let offset = 0, timer; const installed = new Set();
  const inp = h("input", { class: "inp", type: "search", placeholder: "Rechercher un mod (Sodium, Iris, Mod Menu…)" });
  const res = h("div", { class: "list" }), more = h("button", { class: "btn", hidden: true }, "Afficher plus");
  async function run(reset) {
    if (reset) { offset = 0; res.replaceChildren(h("p", { class: "mute" }, "Recherche…")); }
    try {
      const [r, l] = await Promise.all([inv("mod_search", { instanceId: i.id, query: inp.value.trim(), offset }), inv("mod_list", { instanceId: i.id })]);
      l.forEach(m => installed.add(m.project_id));
      if (reset) res.replaceChildren();
      if (!r.hits.length && reset) res.append(empty("Aucun résultat compatible avec Minecraft " + i.version + " et Fabric."));
      r.hits.forEach(x => res.append(hit(x)));
      offset += r.hits.length; more.hidden = offset >= r.total;
    } catch (err) { res.replaceChildren(h("p", { class: "bad" }, errText(err))); }
  }
  function hit(x) {
    const b = h("button", { class: "btn p sm" }, installed.has(x.id) ? "Installé" : "Installer");
    b.disabled = installed.has(x.id);
    b.addEventListener("click", async () => {
      b.disabled = true; showPrep("Installation de " + x.title);
      try {
        const a = await inv("mod_install", { instanceId: i.id, project: x.id }); hidePrep();
        installed.add(x.id); b.textContent = "Installé"; await loadInsts();
        toast(x.title + " installé" + (a.length > 1 ? " avec " + (a.length - 1) + " dépendance(s)" : ""));
      } catch (err) { hidePrep(); b.disabled = false; toast(errText(err), "bad"); }
    });
    return h("div", { class: "item mod" }, icon(x.icon_url),
      h("div", { class: "grow" }, h("b", {}, x.title), h("small", {}, x.description), h("small", {}, "par " + x.author + " · " + fmt(x.downloads) + " téléchargements")), b);
  }
  inp.addEventListener("input", () => { clearTimeout(timer); timer = setTimeout(() => run(true), 350); });
  more.addEventListener("click", () => run(false));
  body.replaceChildren(inp, res, more); run(true);
}

// ---------- Paramètres ----------
function renderSettings() {
  const el = $("#p-set");
  const sw = h("div", { class: "swatches" }, COLORS.map(c => { const b = h("button", { class: "sw" + (c === S.accent ? " on" : ""), type: "button", "aria-label": "Couleur " + c }); b.style.background = c; b.addEventListener("click", () => { S.accent = c; saveS(); [...sw.children].forEach(x => x.classList.toggle("on", x === b)); }); return b; }));
  const num = (k, ph) => { const n = h("input", { class: "inp", type: "number", min: "0", max: "10000", placeholder: ph, value: S[k] || "" }); n.addEventListener("change", () => { S[k] = Math.max(0, parseInt(n.value) || 0); saveS(); }); return n; };
  const lab = h("b", {}, S.ram + " Go"), rng = h("input", { type: "range", min: "1", max: "16", value: String(S.ram), "aria-label": "RAM par défaut" });
  rng.addEventListener("input", () => { lab.textContent = rng.value + " Go"; });
  rng.addEventListener("change", () => { S.ram = +rng.value; saveS(); });
  const jvm = h("input", { class: "inp", type: "text", placeholder: "-XX:+UseZGC", value: S.jvm }); jvm.style.width = "260px";
  jvm.addEventListener("change", () => { S.jvm = jvm.value.trim(); saveS(); });
  const ver = h("span", {}, "…");
  el.replaceChildren(h("h1", {}, "Paramètres"), h("p", { class: "sub" }, "Tout est enregistré automatiquement."),
    card("Apparence",
      row("Thème", null, seg([["system", "Système"], ["light", "Clair"], ["dark", "Sombre"]], S.theme, v => { S.theme = v; saveS(); })),
      row("Couleur d’accentuation", null, sw),
      row("Interface compacte", "Réduit les marges", toggle(S.compact, v => { S.compact = v; saveS(); }))),
    card("Lancement",
      row("Cacher le launcher pendant la partie", "Il revient à la fermeture du jeu ou en cas de crash", toggle(S.hide, v => { S.hide = v; saveS(); })),
      row("Plein écran", null, toggle(S.fullscreen, v => { S.fullscreen = v; saveS(); })),
      row("Taille de la fenêtre du jeu", "Largeur × hauteur, vide = automatique", h("div", { class: "ctl" }, num("width", "Largeur"), "×", num("height", "Hauteur")))),
    card("Performance",
      row("Profil", "Éco : priorité basse. Performance : plus de mémoire.", seg(P.map((p, k) => [k, p.n]), S.profile, v => { S.profile = v; S.ram = P[v].ram; saveS(); renderSettings(); })),
      row("Mémoire par défaut (RAM)", "Chaque instance peut avoir la sienne", h("div", { class: "ctl" }, rng, lab)),
      row("Arguments Java supplémentaires", "Pour utilisateurs avancés", jvm)),
    accountCard(),
    card("À propos",
      row("Plume", null, ver),
      row("Dossier de données", "Instances, bibliothèques et ressources", h("button", { class: "btn", onclick: () => inv("open_folder", { id: null }) }, "Ouvrir")),
      row("Réinitialiser les réglages", "Remet l’apparence et les options par défaut", h("button", { class: "btn danger", onclick: () => confirmBox("Réinitialiser ?", "Tes instances et mods ne sont pas touchés.", "Réinitialiser", () => { localStorage.removeItem("plume.s"); S = { ...DEF }; saveS(); renderSettings(); }) }, "Réinitialiser"))));
  T.app.getVersion().then(v => { ver.textContent = "Version " + v; }).catch(() => { ver.textContent = ""; });
}

// ---------- Mises à jour du launcher ----------
$("#updgo").addEventListener("click", async () => {
  showPrep("Mise à jour de Plume");
  try { await inv("install_update"); } catch (err) { hidePrep(); $("#updt").textContent = "Échec de la mise à jour : " + errText(err); }
});
async function checkSelfUpdate() {
  try { const n = await inv("check_update"); if (n) { $("#updt").textContent = "Mise à jour " + n + " disponible"; $("#upd").hidden = false; } } catch (_) {}
}

// ---------- Démarrage ----------
$$("#nav button[data-p]").forEach(b => b.addEventListener("click", () => go(b.dataset.p)));
applyS();
(async () => {
  T.app.getVersion().then(v => { $("#appv").textContent = v; }).catch(() => {});
  await refreshAcct(); await loadInsts(); go("home"); checkSelfUpdate();
})();
