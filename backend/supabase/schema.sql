-- Plume : cosmétiques. Schéma Supabase (PostgreSQL). À appliquer depuis l'éditeur SQL de Supabase.
-- Principe : tout le monde peut LIRE le catalogue et les tenues ; personne ne peut ÉCRIRE directement.
-- Les écritures passent par une fonction serveur (Edge Function) qui vérifie l'identité du joueur auprès de Mojang.

create table if not exists public.cosmetics (
  id           text primary key,                                   -- ex : cape_aurore
  kind         text not null check (kind in ('cape','hat','wings','aura','pet')),
  name         text not null,
  description  text,
  price_cents  integer not null default 0 check (price_cents >= 0),
  asset_path   text not null,                                      -- modèle et texture dans le stockage
  published    boolean not null default false,
  created_at   timestamptz not null default now()
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

alter table public.cosmetics enable row level security;
alter table public.players   enable row level security;
alter table public.ownership enable row level security;
alter table public.equipped  enable row level security;

create policy "catalogue public" on public.cosmetics for select using (published);
create policy "tenues visibles par tous" on public.equipped for select using (true);
-- players et ownership : aucune policy = inaccessibles côté public ; seule la clé de service (Edge Function) y accède.
