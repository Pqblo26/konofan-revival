-- Fame Bonus: fame ranks per member, Fame Traits per member, potions waiting for a choice and the
-- trait each party form has set.

create table if not exists user_member_fame (
  user_id bigint not null references users (id) on delete cascade,
  member_id bigint not null,
  hp integer not null default 0,
  attack integer not null default 0,
  magicattack integer not null default 0,
  defense integer not null default 0,
  magicdefence integer not null default 0,
  updated_at timestamptz not null default now(),
  primary key (user_id, member_id)
);

create table if not exists user_skill_pa_fame (
  id bigserial primary key,
  user_id bigint not null references users (id) on delete cascade,
  member_id bigint not null,
  skill_pa_fame_id bigint not null,
  -- JSON list of [type, value]
  add_status text not null default '[]',
  is_lock boolean not null default false,
  created_at timestamptz not null default now()
);
create index if not exists user_skill_pa_fame_user on user_skill_pa_fame (user_id, member_id);

create table if not exists user_fame_potions (
  id bigserial primary key,
  user_id bigint not null references users (id) on delete cascade,
  member_id bigint not null,
  quest_rank integer not null default 1,
  -- JSON list of [potion_choice_id, potion_type, skill_pa_fame_id, add_status]
  choices text not null,
  created_at timestamptz not null default now()
);
create index if not exists user_fame_potions_user on user_fame_potions (user_id);

create table if not exists user_party_form_fame (
  user_id bigint not null references users (id) on delete cascade,
  party_id bigint not null,
  form_id bigint not null,
  user_skill_pa_fame_id bigint not null,
  primary key (user_id, party_id, form_id)
);
