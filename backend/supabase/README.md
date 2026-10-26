# Serveur des cosmétiques Plume

Une base Supabase (`schema.sql`) et une fonction (`functions/plume/index.ts`).

## Mise en place
1. Crée un projet Supabase, puis colle `schema.sql` dans l'éditeur SQL.
2. Dépose les textures (cape 64x32 PNG, 64 Ko max) et les aperçus dans le bucket **cosmetics**, puis ajoute les lignes dans la table `cosmetics` (voir l'exemple en bas de `schema.sql`).
3. Déploie la fonction :
       supabase functions deploy plume --no-verify-jwt
       supabase secrets set PLUME_JWT_SECRET="$(openssl rand -hex 32)"
4. L'adresse à donner au launcher et au mod est `https://<ton-projet>.supabase.co/functions/v1`
   (secret GitHub `PLUME_BACKEND_URL`, utilisé à la compilation du launcher et du mod).

## Sécurité
- Personne n'écrit directement dans les tables : tout passe par la fonction (clé de service côté serveur).
- L'identité est prouvée avec le mécanisme `join` / `hasJoined` de Mojang ; le jeton signé dure 1 heure.
- Le mod ne lit que les capes des joueurs demandés (50 maximum par requête).
- À ajouter si le service grossit : limitation du nombre de requêtes par adresse.
