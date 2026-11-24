-- Banners listed in Recruit (gachainfo), editable without touching the code.
--
-- With no enabled rows, Recruit lists every banner of the `gacha` master except the disabled
-- rows. With some enabled rows, it lists only those, ordered by sort_order. A banner is listed
-- only inside its optional start_at/end_at window. The draw rates are in gacha.rates.

create table if not exists gacha.banners
(
  gacha_id   bigint primary key,
  enabled    boolean     not null default true,
  sort_order integer     not null default 0,
  -- Whether the banner has the once-a-day draw
  daily      boolean     not null default true,
  start_at   timestamptz null,
  end_at     timestamptz null,
  note       text        null
);
