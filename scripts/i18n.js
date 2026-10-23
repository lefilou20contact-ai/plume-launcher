#!/usr/bin/env node
// Outil de traduction de Plume.
//   node scripts/i18n.js status              couverture de chaque langue
//   node scripts/i18n.js check               erreurs (clés inconnues, variables, JSON) + clés utilisées dans le code
//   node scripts/i18n.js missing <code>      clés pas encore traduites pour une langue
//   node scripts/i18n.js new <code> "<Nom>"  crée une nouvelle langue (fichier vide + ajout à languages.json)
//   node scripts/i18n.js merge-into <dossier>  copie les langues de ce projet vers <dossier> SANS écraser les traductions déjà présentes (utilisé par update.sh)
const fs = require("fs"), path = require("path");
const root = path.join(__dirname, "..");
const dir = path.join(root, "src", "i18n");
const read = f => JSON.parse(fs.readFileSync(path.join(dir, f), "utf8"));
const keysOf = o => Object.keys(o).filter(k => !k.startsWith("_"));
const ref = read("fr.json"), refKeys = keysOf(ref);
const langs = read("languages.json");
const done = (d, k) => typeof d[k] === "string" && d[k].trim() !== "";
const vars = s => (String(s).match(/\{\w+\}/g) || []).sort().join(",");
const load = code => { try { return read(code + ".json"); } catch (e) { return null; } };
const [cmd, a, b] = process.argv.slice(2);

if (cmd === "status" || !cmd) {
  console.log("Langue".padEnd(14) + "Traduit".padEnd(12) + "%");
  for (const l of langs) {
    const d = load(l.code); if (!d) { console.log(`${l.name.padEnd(14)}fichier manquant`); continue; }
    const n = refKeys.filter(k => done(d, k)).length, p = Math.floor(n / refKeys.length * 100);
    console.log(`${(l.name + " (" + l.code + ")").padEnd(14)}${(n + "/" + refKeys.length).padEnd(12)}${p} %  ${"#".repeat(Math.round(p / 5)).padEnd(20, ".")}`);
  }
} else if (cmd === "missing") {
  const d = load(a); if (!d) { console.error("Langue inconnue : " + a); process.exit(1); }
  refKeys.filter(k => !done(d, k)).forEach(k => console.log(k + "  →  " + ref[k]));
} else if (cmd === "new") {
  if (!/^[a-z]{2,3}(-[A-Za-z]{2})?$/.test(a || "") || !b) { console.error('Usage : node scripts/i18n.js new <code> "<Nom>"   (ex : new de "Deutsch")'); process.exit(1); }
  const f = path.join(dir, a + ".json");
  if (fs.existsSync(f)) { console.error("Ce fichier existe déjà."); process.exit(1); }
  const o = {}; refKeys.forEach(k => { o[k] = ""; });
  fs.writeFileSync(f, JSON.stringify(o, null, 1) + "\n");
  if (!langs.find(l => l.code === a)) { langs.push({ code: a, name: b }); fs.writeFileSync(path.join(dir, "languages.json"), JSON.stringify(langs, null, 1) + "\n"); }
  console.log(`Créé : src/i18n/${a}.json (${refKeys.length} clés vides). Remplis les valeurs ; le % se calcule tout seul.`);
} else if (cmd === "merge-into") {
  const dest = a ? path.resolve(a) : "";
  if (!dest || !fs.existsSync(dest)) { console.error("Usage : node scripts/i18n.js merge-into <dossier-destination>"); process.exit(1); }
  const rd = f => { try { return JSON.parse(fs.readFileSync(f, "utf8")); } catch (e) { return null; } };
  const wr = (f, o) => fs.writeFileSync(f, JSON.stringify(o, null, 1) + "\n");
  for (const f of fs.readdirSync(dir)) {
    const src = path.join(dir, f), dst = path.join(dest, f);
    if (f === "README.md" || !fs.existsSync(dst)) { fs.copyFileSync(src, dst); continue; }
    if (f === "languages.json") { const s = rd(src) || [], d = rd(dst) || []; s.forEach(l => { if (!d.find(x => x.code === l.code)) d.push(l); }); wr(dst, d); continue; }
    if (!f.endsWith(".json")) continue;
    const s = rd(src) || {}, d = rd(dst);
    if (!d) { fs.copyFileSync(src, dst); continue; }
    let n = 0;
    for (const k of Object.keys(s)) {
      // On n'ajoute que les clés absentes ; seuls les messages du programme (rs.*, pg.*) du français sont toujours remis à jour.
      if (!(k in d) || (f === "fr.json" && /^(rs|pg)\./.test(k) && d[k] !== s[k])) { d[k] = s[k]; n++; }
    }
    wr(dst, d);
    if (n) console.log(`${f} : ${n} clé(s) ajoutée(s) ou mise(s) à jour`);
  }
} else if (cmd === "check") {
  let errors = 0; const err = m => { console.error("ERREUR  " + m); errors++; };
  for (const l of langs) {
    const d = load(l.code); if (!d) { err(`${l.code}.json illisible ou manquant`); continue; }
    for (const k of Object.keys(d)) {
      if (k.startsWith("_")) continue;
      if (!(k in ref)) { err(`${l.code}: clé inconnue « ${k} »`); continue; }
      if (done(d, k) && vars(d[k]) !== vars(ref[k])) err(`${l.code}: variables différentes pour « ${k} » (attendu ${vars(ref[k]) || "aucune"})`);
    }
  }
  const src = ["app.js", "index.html"].map(f => fs.readFileSync(path.join(root, "src", f), "utf8")).join("\n");
  const used = new Set(); const prefixes = new Set(refKeys.map(k => k.split(".")[0]));
  for (const m of src.matchAll(/(?:\bt|\btn)\(\s*"([\w.]+)"|data-i18n(?:-aria)?="([\w.]+)"|"([a-z]+\.[a-z_0-9]+)"/g)) {
    const k = m[1] || m[2] || m[3]; if (!k) continue;
    if (m[3] && !prefixes.has(k.split(".")[0])) continue;
    used.add(k); if (!(k in ref)) err(`clé utilisée dans le code mais absente de fr.json : « ${k} »`);
  }
  const unused = refKeys.filter(k => !used.has(k) && !/^(rs|pg)\./.test(k));
  if (unused.length) console.log("Avertissement : clés jamais utilisées : " + unused.join(", "));
  console.log(errors ? `${errors} erreur(s)` : "Traductions OK");
  process.exit(errors ? 1 : 0);
} else { console.error("Commande inconnue. Voir l'en-tête du fichier."); process.exit(1); }
