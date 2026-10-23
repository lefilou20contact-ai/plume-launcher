# Plume

Launcher Minecraft léger (Tauri + Rust).

- Instances multiples (Vanilla ou Fabric), chacune avec ses mods, mondes et réglages
- Mods Modrinth : recherche, installation avec dépendances, activation, mises à jour (sommes SHA-512 vérifiées)
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
    bash update.sh "message du commit" v0.2.5
Récupère d'abord GitHub (nouvelles langues, liens de don), fusionne les traductions sans les écraser, puis envoie et crée le tag.

## Publier
    git tag v0.2.3 && git push origin v0.2.3   # trois nombres obligatoires
