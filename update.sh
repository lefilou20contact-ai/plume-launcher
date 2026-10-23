#!/usr/bin/env bash
# Applique le dernier zip de Plume sans perdre ce que tu as modifié sur GitHub (langues, liens de don...).
# Usage :  bash update.sh "message du commit" [tag]      ex :  bash update.sh "Loaders" v0.2.5
set -euo pipefail
cd "$(dirname "$0")"
MSG="${1:-Mise à jour}"; TAG="${2:-}"
ZIP="${PLUME_ZIP:-$(ls -t ~/Téléchargements/plume-launcher*.zip ~/Downloads/plume-launcher*.zip 2>/dev/null | head -1 || true)}"
[ -n "$ZIP" ] || { echo "Aucun zip plume-launcher*.zip trouvé dans Téléchargements."; exit 1; }
if unzip -l "$ZIP" | grep -q " plume-launcher/"; then echo "Ce zip contient un dossier plume-launcher/ : retélécharge le dernier zip."; exit 1; fi
echo "Zip utilisé : $ZIP"
# 1) récupère d'abord ce qui a été ajouté sur GitHub (nouvelles langues...) : plus de push refusé
git pull --rebase --autostash origin main
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
unzip -q -o "$ZIP" -d "$TMP"
# 2) tout le projet est mis à jour, sauf les langues (fusionnées) et le site (gardé tel quel s'il existe)
(cd "$TMP" && tar cf - --exclude=./src/i18n --exclude=./docs .) | tar xf -
mkdir -p src/i18n
node "$TMP/scripts/i18n.js" merge-into "$PWD/src/i18n"
if [ ! -d docs ]; then cp -a "$TMP/docs" docs; else echo "Site (docs/) conservé : tes liens de don ne sont pas écrasés."; fi
node scripts/i18n.js check || echo "(vérifie les traductions ci-dessus)"
# 3) envoi
git add -A
git commit -m "$MSG" || echo "Rien de nouveau à enregistrer."
git push origin main
if [ -n "$TAG" ]; then git tag "$TAG" && git push origin "$TAG"; fi
echo "Terminé."
