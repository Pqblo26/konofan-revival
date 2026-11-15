-- Branch (surprise events, masters `surprise_*`): plays per day, best score and the pending story result.
create table if not exists user_surprise_events
(
  user_id           bigint not null references users (id) on delete cascade,
  surprise_event_id int    not null,
  best_score        int    not null default 0,
  play_date         date,
  play_count        int    not null default 0,
  pending_result_id int    not null default 0,
  primary key (user_id, surprise_event_id)
);
