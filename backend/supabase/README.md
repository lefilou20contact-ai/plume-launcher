# Serveur des cosmétiques Plume

Une base Supabase (`schema.sql`) et une fonction (`functions/plume/index.ts`).

## État du projet créé
- Projet Supabase **plume-cosmetics** (région eu-west-3), schéma appliqué, fonction `plume` déployée (sans vérification JWT : les routes protégées vérifient notre jeton signé).
- Adresse à donner au launcher et au mod : `https://uslvogwfhcbugumgooco.supabase.co/functions/v1`

## Ce qu'il reste à faire à la main
1. **Secret de signature des jetons** (obligatoire, sinon la connexion renvoie une erreur) :
   génère une valeur avec `openssl rand -hex 32`, puis dans le tableau de bord Supabase :
   *Edge Functions → Secrets → Add new secret* : nom `PLUME_JWT_SECRET`, valeur = ta valeur.
2. **Secret GitHub** (utilisé à la compilation du launcher et du mod) :
       gh secret set PLUME_BACKEND_URL --body "https://uslvogwfhcbugumgooco.supabase.co/functions/v1"
3. **Une cape de test** : dans *Storage → cosmetics*, dépose `capes/cape_aurore.png` et `previews/cape_aurore_preview.png`
   (fichiers dans `backend/samples/`), puis dans l'éditeur SQL :
       insert into public.cosmetics (id, kind, name, price_cents, asset_path, preview_path, published)
       values ('cape_aurore', 'cape', 'Aurore', 0, 'capes/cape_aurore.png', 'previews/cape_aurore_preview.png', true);

## Sécurité
- Personne n'écrit directement dans les tables : tout passe par la fonction (clé de service côté serveur).
- Les tables `players`, `ownership` et `auth_challenges` ont la sécurité par lignes activée et aucune règle publique : le contrôle Supabase les signale en « information », c'est voulu.
- L'identité est prouvée avec `join` / `hasJoined` de Mojang ; le jeton signé dure 1 heure.
- Le mod ne lit que les capes des joueurs demandés (50 maximum par requête).
- À ajouter si le service grossit : limitation du nombre de requêtes par adresse.
