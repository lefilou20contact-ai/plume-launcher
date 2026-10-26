# Plume Cosmetics (mod Fabric, côté client)

Affiche les capes choisies dans le launcher **Plume** sur tous les joueurs qui ont le mod.

- Minecraft **1.21.4**, Fabric Loader ≥ 0.16 (fonctionne aussi sur Quilt). Pas besoin de Fabric API.
- Le rendu est celui du jeu : le mod ne fait que remplacer la texture de cape d'un joueur.
- Premières fonctions : **capes**. Les chapeaux et ailes (modèles 3D) viendront ensuite.

## Confidentialité
Toutes les 20 secondes, le mod envoie au serveur de cosmétiques **la liste des identifiants (UUID) des joueurs visibles** et reçoit en retour leurs capes. Rien d'autre n'est envoyé (ni pseudo, ni jeton, ni adresse de serveur de jeu). Pour tout couper : `"enabled": false` dans `config/plume-cosmetics.json`, ou supprimer le mod.

## Réglages
`config/plume-cosmetics.json` : `enabled` (vrai/faux) et `backendUrl` (adresse du serveur ; vide = adresse intégrée au mod à la compilation).

## Compiler
    gradle build -Pmod_version=0.1.0            # nécessite Java 21 et Gradle 8.10+
    # adresse du serveur : variable PLUME_BACKEND_URL ou  -PbackendUrl=https://xxxx.supabase.co/functions/v1
Le fichier est dans `build/libs/plume-cosmetics-<version>+mc1.21.4.jar`.

## Publier
Un tag `mod-v0.1.0` lance le workflow `.github/workflows/mod.yml` : compilation, fichier `.jar` + somme SHA-512 dans les Releases (le launcher les télécharge), et publication Modrinth si les secrets `MODRINTH_TOKEN` et la variable `MODRINTH_ID` existent.

## À faire avant de publier sur Modrinth
- Choisir une licence (`"license"` dans `fabric.mod.json` est « All-Rights-Reserved » par défaut).
- Utiliser uniquement des textures créées par toi, ou sous licence claire. Ne rien copier d'un autre client.
