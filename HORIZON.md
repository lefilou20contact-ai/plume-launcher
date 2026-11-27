# Plume et Horizon (Minecraft en Rust)

**Horizon** est un projet indépendant et open source (licence MIT) qui réécrit Minecraft Java Edition 26.3 en Rust. Ce n'est pas un launcher : c'est un autre moteur de jeu. Il n'embarque aucun fichier de Mojang et utilise ta propre copie du jeu.

## Ce que Plume sait faire avec
- **Moteur externe** : dans « Modifier l'instance », choisis « Moteur externe », indique le programme Horizon et ses arguments. Plume télécharge et prépare les fichiers officiels du jeu (versions, bibliothèques, ressources dans son dossier partagé), puis lance ce programme à la place de Java. Variables disponibles dans les arguments : `{minecraft_dir}` `{game_dir}` `{version}` `{username}` `{width}` `{height}` `{world}`.
- **Compatibilité (Plume Lab)** : dépose un `.jar` ; Plume dit s'il ne contient aucun code (contenu seul, donc chargeable comme pack) ou du code Java (donc impossible dans Horizon).
- **Extraction en pack** : pour un mod qui contient des ressources et des données, Plume en tire un pack `.zip` (assets, data, `pack.mcmeta`). C'est partiel : les blocs, objets et comportements ajoutés par le code Java ne sont pas recréés.

## Ce qui n'est pas possible
- **Faire tourner un mod Java dans Horizon, ou le « convertir » automatiquement.** Un mod Java est du code qui modifie les classes Java de Minecraft ; ces classes n'existent plus dans Horizon. Horizon a son propre système de mods (WebAssembly). Un mod précis peut être réécrit à la main pour cette API, mais aucun outil ne le fait automatiquement.
- **Se connecter avec un compte Microsoft** : Horizon ne le gère pas encore, donc pas de serveurs qui vérifient les comptes.

## À vérifier toi-même
Cette intégration n'a pas été testée avec Horizon : la ligne de commande exacte (programme, arguments) doit être celle que la documentation d'Horizon indique pour la version installée. Les options connues de son client incluent `--world`, `--seed`, `--creative`, `--render-distance` et `--minecraft-dir`.
