-- Plume : cosmétiques. Schéma Supabase (PostgreSQL).
-- Principe : tout le monde peut LIRE le catalogue et les tenues ; personne ne peut ÉCRIRE directement.
-- Les écritures passent par la fonction serveur « plume » (clé de service) qui vérifie l'identité du joueur auprès de Mojang.

create table if not exists public.cosmetics (
  id            text primary key,                                  -- ex : cape_aurore
  kind          text not null check (kind in ('cape','hat','wings','aura','pet')),
  name          text not null,
  description   text,
  price_cents   integer not null default 0 check (price_cents >= 0),
  asset_path    text not null,                                     -- texture dans le stockage (ex : capes/cape_aurore.png)
  preview_path  text,                                              -- aperçu affiché dans le launcher
  published     boolean not null default false,
  created_at    timestamptz not null default now()
);

create table if not exists public.players (
  uuid       uuid primary key,                                     -- UUID Minecraft vérifié
  name       text not null,
  last_seen  timestamptz not null default now()
);

create table if not exists public.ownership (
  player       uuid not null references public.players(uuid) on delete cascade,
  cosmetic     text not null references public.cosmetics(id) on delete cascade,
  source       text not null default 'grant' check (source in ('grant','purchase','gift')),
  acquired_at  timestamptz not null default now(),
  primary key (player, cosmetic)
);

create table if not exists public.equipped (
  player      uuid primary key references public.players(uuid) on delete cascade,
  cape        text references public.cosmetics(id),
  hat         text references public.cosmetics(id),
  wings       text references public.cosmetics(id),
  updated_at  timestamptz not null default now()
);

-- Défis d'identité à usage unique : le joueur les « rejoint » auprès de Mojang pour prouver son compte.
create table if not exists public.auth_challenges (
  server_id   text primary key,
  created_at  timestamptz not null default now()
);

alter table public.cosmetics       enable row level security;
alter table public.players         enable row level security;
alter table public.ownership       enable row level security;
alter table public.equipped        enable row level security;
alter table public.auth_challenges enable row level security;

create policy "catalogue public" on public.cosmetics for select using (published);
create policy "tenues visibles par tous" on public.equipped for select using (true);
-- players, ownership et auth_challenges : aucune policy = inaccessibles côté public ; seule la fonction serveur y accède.

-- Stockage public des textures (capes 64x32) et des aperçus (dossiers capes/ et previews/).
insert into storage.buckets (id, name, public) values ('cosmetics', 'cosmetics', true) on conflict (id) do nothing;

-- Exemple de cape gratuite, une fois capes/cape_aurore.png (64x32) et previews/cape_aurore.png déposés dans le bucket « cosmetics » :
-- insert into public.cosmetics (id, kind, name, price_cents, asset_path, preview_path, published)
-- values ('cape_aurore', 'cape', 'Aurore', 0, 'capes/cape_aurore.png', 'previews/cape_aurore.png', true);
-- Les cosmétiques à 0 centime sont donnés automatiquement à chaque joueur à sa première connexion.
