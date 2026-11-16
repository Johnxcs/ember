-- Ember schema. The server runs this automatically on startup (idempotent),
-- or paste it into Supabase > SQL Editor.
create table if not exists channels (
  id          text primary key,
  name        text not null,
  ttl         bigint,
  created_at  timestamptz not null default now()
);

create table if not exists messages (
  id          text primary key,
  channel_id  text not null references channels(id) on delete cascade,
  author      text not null,
  body        text not null,
  ts          bigint not null,
  expires     bigint,
  sparks      integer not null default 0,
  poll        jsonb,
  voters      text[] not null default '{}'
);

create index if not exists messages_channel_ts on messages (channel_id, ts desc);
create index if not exists messages_expires on messages (expires) where expires is not null;

-- Lock the tables away from Supabase's public REST API (anon/authenticated keys).
-- The Ember server connects directly as the database owner, which bypasses RLS.
alter table channels enable row level security;
alter table messages enable row level security;

-- Accounts: one unique username per Supabase Auth user.
create table if not exists profiles (
  user_id     text primary key,
  username    text not null,
  created_at  timestamptz not null default now()
);
create unique index if not exists profiles_username_lower on profiles (lower(username));
alter table profiles enable row level security;
