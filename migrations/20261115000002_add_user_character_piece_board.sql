-- Potential Blessing (masters `character_piece_board*`): board stages released per character.
create table if not exists user_character_piece_board
(
  user_id      bigint      not null references users (id) on delete cascade,
  character_id bigint      not null,
  stage_id     bigint      not null,
  released_at  timestamptz not null default now(),
  primary key (user_id, stage_id)
);
