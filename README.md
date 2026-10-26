# Plume

Launcher Minecraft léger (Tauri + Rust).

- Instances multiples (Vanilla, Fabric, Quilt, Forge, NeoForge), chacune avec ses mods, mondes et réglages
- Mods, shaders et packs de ressources Modrinth : recherche, dépendances, activation, mises à jour (SHA-512 vérifiées)
- Modpacks Modrinth (.mrpack) : installation en un clic ou glisser-déposer
- Plume Lab : test de lancement automatique pour les créateurs de mods (démarrage, mémoire, CPU, erreurs, compatibilité)
- Export d'instance en .zip
- Plusieurs comptes Microsoft, jeu hors ligne possible 30 jours après la dernière vérification
- Téléchargements vérifiés (SHA-1), relancés en cas d'échec, Java téléchargé automatiquement
- Logs en direct, temps de jeu, duplication d'instance
- Personnalisation : thème, couleur d'accent, mémoire, arguments Java, taille de fenêtre
- Interface traduisible, langues synchronisées automatiquement depuis GitHub (voir `src/i18n/README.md`)
- Mises à jour automatiques du launcher (AppImage sous Linux)

## Développer (Linux)
    npm install
    npx tauri icon app-icon.png
    npm run dev          # le mode test PLUME_DEV_OFFLINE=Pseudo n'existe qu'ici

## Traductions
    node scripts/i18n.js status      # pourcentage de chaque langue
    node scripts/i18n.js new de "Deutsch"

## Site web
Le dossier `docs/` contient le site (français à la racine, anglais dans `en/`), publié avec GitHub Pages.

## Mettre à jour le projet depuis un zip
    bash update.sh "message du commit" v0.3.0      # PLUME_SITE=1 pour aussi mettre à jour le site
Récupère d'abord GitHub (nouvelles langues, liens de don), fusionne les traductions sans les écraser, puis envoie et crée le tag.

## Publier
    git tag v0.3.0 && git push origin v0.3.0   # trois nombres obligatoires

## Cosmétiques
Mod Fabric `mod/` (tag `mod-vX.Y.Z`), serveur `backend/supabase/` et page « Cosmétiques » du launcher. Voir `backend/COSMETICS.md`.
