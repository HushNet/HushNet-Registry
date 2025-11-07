create table if not exists nodes (
  id uuid primary key default gen_random_uuid(),
  name text not null,
  host text not null unique,
  ip inet,
  api_base_url text not null,
  pubkey bytea not null,
  protocol_version text not null,
  features jsonb not null default '{}',
  contact_email text,
  registered_at timestamptz not null default now(),

  country_code text,
  country_name text,
  last_seen_at timestamptz,
  last_latency_ms integer,
  status text not null default 'unknown', -- online|offline|unknown
  uptime_ratio real default 0
);

-- challenges (nonces)
create table if not exists challenges (
  nonce text primary key,
  pubkey_b64 text not null,
  expires_at timestamptz not null
);

create index if not exists idx_nodes_status on nodes(status);
ALTER TABLE nodes ADD CONSTRAINT unique_pubkey UNIQUE (pubkey);
