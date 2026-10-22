# Plume

Launcher Minecraft léger (Tauri + Rust).

- Instances multiples (Vanilla ou Fabric), chacune avec ses mods, mondes et réglages
- Mods Modrinth : recherche, installation avec dépendances, activation, mises à jour (sommes SHA-512 vérifiées)
- Plusieurs comptes Microsoft (tête du joueur dans la barre latérale), jeu hors ligne possible 30 jours après la dernière vérification
- Personnalisation : thème, couleur d'accent, mémoire, arguments Java, taille de fenêtre
- Mises à jour automatiques du launcher (AppImage sous Linux)

## Développer (Linux)
    npm install
    npx tauri icon app-icon.png
    npm run dev          # le mode test PLUME_DEV_OFFLINE=Pseudo n'existe qu'ici

## Publier
    git tag v0.2.0 && git push origin v0.2.0   # trois nombres obligatoires
