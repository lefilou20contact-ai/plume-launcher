#!/usr/bin/env bash
# Applique le dernier zip de Plume sans perdre ce que tu as modifié sur GitHub (langues, liens de don...).
# Usage :  bash update.sh "message du commit" [tag]      ex :  bash update.sh "Loaders" v0.3.0
# Pour mettre aussi à jour le site (en gardant tes liens de don) :  PLUME_SITE=1 bash update.sh "message" [tag]
main() {
  set -euo pipefail
  cd "$(dirname "$0")"
  local MSG="${1:-Mise à jour}" TAG="${2:-}"
  local ZIP="${PLUME_ZIP:-$(ls -t ~/Téléchargements/plume-launcher*.zip ~/Downloads/plume-launcher*.zip 2>/dev/null | head -1 || true)}"
  [ -n "$ZIP" ] || { echo "Aucun zip plume-launcher*.zip trouvé dans Téléchargements."; exit 1; }
  if unzip -l "$ZIP" | grep -q " plume-launcher/"; then echo "Ce zip contient un dossier plume-launcher/ : retélécharge le dernier zip."; exit 1; fi
  echo "Zip utilisé : $ZIP"
  # 1) récupère d'abord ce qui a été ajouté sur GitHub (nouvelles langues...) : plus de push refusé
  git pull --rebase --autostash origin main
  local TMP; TMP="$(mktemp -d)"; trap "rm -rf '$TMP'" EXIT
  unzip -q -o "$ZIP" -d "$TMP"
  # 2) tout le projet est mis à jour, sauf les langues (fusionnées) et le site (gardé sauf si PLUME_SITE=1)
  (cd "$TMP" && tar cf - --exclude=./src/i18n --exclude=./docs .) | tar xf -
  mkdir -p src/i18n
  node "$TMP/scripts/i18n.js" merge-into "$PWD/src/i18n"
  if [ ! -d docs ]; then
    cp -a "$TMP/docs" docs
  elif [ "${PLUME_SITE:-}" = "1" ]; then
    local DON; DON="$(grep -h -m1 '^var DONATE' docs/index.html || true)"
    cp -a "$TMP/docs/." docs/
    if [ -n "$DON" ]; then
      for f in docs/index.html docs/en/index.html; do
        python3 - "$f" "$DON" <<'PY'
import sys, re
p, don = sys.argv[1], sys.argv[2]
s = open(p, encoding="utf-8").read()
s = re.sub(r"(?m)^var DONATE.*$", lambda m: don, s, count=1)
open(p, "w", encoding="utf-8").write(s)
PY
      done
    fi
    echo "Site (docs/) mis à jour, tes liens de don sont conservés."
  else
    echo "Site (docs/) conservé : tes liens de don ne sont pas écrasés. (PLUME_SITE=1 pour le mettre à jour.)"
  fi
  node scripts/i18n.js check || echo "(vérifie les traductions ci-dessus)"
  # 3) envoi
  git add -A
  git commit -m "$MSG" || echo "Rien de nouveau à enregistrer."
  git push origin main
  if [ -n "$TAG" ]; then git tag "$TAG" && git push origin "$TAG"; fi
  echo "Terminé."
}
main "$@"; exit $?
