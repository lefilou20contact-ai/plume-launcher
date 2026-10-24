# Cosmétiques Plume : architecture prévue

Rien n'est vendu ni activé pour l'instant. Ce document fixe le plan pour que chaque pièce puisse être construite séparément.

## Les 4 pièces
1. **Le mod (Fabric, puis les autres loaders)** : affiche les capes et accessoires sur les joueurs. Il demande au serveur « quelles tenues portent ces UUID ? » et garde la réponse en cache quelques minutes. Les modèles et textures sont créés par vous (Blockbench) : ne rien copier d'un autre client.
2. **Le serveur** (Supabase) : `backend/supabase/schema.sql`. Lecture publique du catalogue et des tenues ; écritures uniquement par une Edge Function.
3. **La preuve d'identité** : le joueur prouve qu'il possède son UUID avec le mécanisme `joinServer` / `hasJoined` de Mojang. L'Edge Function vérifie auprès de Mojang puis délivre un jeton court. Aucun mot de passe, aucun jeton Microsoft ne quitte le jeu.
4. **Le launcher** : réglage « Cosmétiques Plume » (déjà présent, marqué « Bientôt »), qui installera le mod depuis Modrinth et ouvrira une page de choix des tenues.

## Ordre conseillé
1. Un mod avec **une seule cape** affichée localement (pas de serveur) + workflow GitHub de publication Modrinth.
2. Le serveur en lecture seule et l'affichage pour les autres joueurs.
3. La preuve d'identité et le choix des tenues depuis le launcher.
4. Seulement ensuite, éventuellement, des achats.

## À vérifier avant tout paiement
- Les règles d'usage commercial de Mojang concernant les contenus liés à Minecraft (ne jamais conditionner le jeu lui-même à un paiement).
- La fiscalité de la vente en ligne dans ton pays, et le choix d'un prestataire de paiement externe (Plume ne doit jamais manipuler de carte bancaire).
- Les licences des modèles et textures (les tiens uniquement, ou sous licence claire).
