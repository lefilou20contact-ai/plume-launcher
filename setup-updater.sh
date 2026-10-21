#!/usr/bin/env bash
# À lancer UNE fois : crée la clé de signature des mises à jour et configure GitHub.
set -e
cd "$(dirname "$0")"
command -v gh >/dev/null || { echo "Installe d'abord la CLI gh (gh auth login)"; exit 1; }
REPO=$(gh repo view --json nameWithOwner -q .nameWithOwner)
[ -d node_modules ] || npm install
mkdir -p ~/.tauri
if [ ! -f ~/.tauri/plume.key ]; then
  openssl rand -hex 16 > ~/.tauri/plume.pw
  npx tauri signer generate --ci -w ~/.tauri/plume.key -p "$(cat ~/.tauri/plume.pw)"
fi
python3 - "$REPO" "$(cat ~/.tauri/plume.key.pub)" <<'PY'
import sys
p='src-tauri/tauri.conf.json'
s=open(p).read().replace('OWNER/REPO',sys.argv[1]).replace('PUBKEY_A_REMPLACER',sys.argv[2])
open(p,'w').write(s)
PY
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/plume.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --body "$(cat ~/.tauri/plume.pw)"
echo "OK. Sauvegarde le dossier ~/.tauri : sans cette clé, impossible de publier des mises à jour."
