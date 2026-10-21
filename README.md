# Plume

Launcher Minecraft léger (Tauri). Version de test : Minecraft vanilla 1.19+ en mode hors-ligne.

## Développer en local (Linux)
Prérequis : Node 20+, Rust, Java 21, et les paquets Tauri
(`libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf build-essential`).

    npm install
    npx tauri icon app-icon.png   # une seule fois, génère les icônes
    npm run dev

## Publier une version
    git tag v0.1.0 && git push origin v0.1.0
GitHub Actions compile Windows, macOS et Linux, puis ajoute les installateurs dans l'onglet Releases.
