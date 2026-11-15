// Called "Branch" in-game

//
// Each event can be played `daily_limit` times a day (Tokyo date); the stock id sent to the client
// is the event id. Rewards come from the masters: score tiers for quests (`surprise_quest_score*`),
// the event's result patterns for short events (`surprise_short_result` through
// `surprise_short_reward_view`) and weighted choices for stories (`surprise_story_*`).

use std::sync::Arc;

use anyhow::Context;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{info, warn};

use crate::api::battle::{BattleMember, BattleParty};
use crate::api::character_enhance::int;
use crate::api::master_all::get_master_manager;
use crate::api::quest::finish::pack_items;
use crate::api::{ApiRequest, NotificationData};
use crate::call::{CallCustom, CallResponse};
use crate::extractor::Params;
use crate::handler::{IntoHandlerResponse, Unsigned};
use crate::user::id::UserId;
use crate::user::session::Session;
use crate::user::wallet::Wallet;
use crate::AppState;

fn master(name: &str) -> &'static Vec<Value> {
  get_master_manager().get_master(name)
}

/// Picks one of the rows by its `weight` column (rows with no weight count as 1).
fn pick_weighted<'a>(rows: &[&'a Value]) -> Option<&'a Value> {
  let weight = |row: &Value| int(row, "weight").max(if row.get("weight").is_some() { 0 } else { 1 });
  let total: i64 = rows.iter().map(|row| weight(row)).sum();
  if total <= 0 {
    return rows.first().copied();
  }
  let mut roll = rand::rng().random_range(0..total);
  for row in rows {
    roll -= weight(row);
    if roll < 0 {
      return Some(row);
    }
  }
  rows.last().copied()
}

/// Plays left today of each event, and the best score (event id -> (plays today, best score)).
async fn load_plays(client: &tokio_postgres::Client, user_id: UserId) -> anyhow::Result<std::collections::HashMap<i32, (i32, i32)>> {
  #[rustfmt::skip]
  let rows = client.query(/* language=postgresql */ r#"
    select surprise_event_id,
           case when play_date = (now() at time zone 'Asia/Tokyo')::date then play_count else 0 end,
           best_score
    from user_surprise_events where user_id = $1
  "#, &[&user_id]).await.context("failed to read surprise events")?;
  Ok(rows.iter().map(|row| (row.get(0), (row.get(1), row.get(2)))).collect())
}

/// Counts one play of an event today, keeping the best score.
async fn record_play(
  client: &tokio_postgres::Client,
  user_id: UserId,
  event_id: i32,
  score: i32,
) -> anyhow::Result<()> {
  #[rustfmt::skip]
  client.execute(/* language=postgresql */ r#"
    insert into user_surprise_events (user_id, surprise_event_id, best_score, play_date, play_count)
    values ($1, $2, $3, (now() at time zone 'Asia/Tokyo')::date, 1)
    on conflict (user_id, surprise_event_id) do update set
      best_score = greatest(user_surprise_events.best_score, $3),
      play_count = case when user_surprise_events.play_date = (now() at time zone 'Asia/Tokyo')::date
                        then user_surprise_events.play_count + 1 else 1 end,
      play_date = (now() at time zone 'Asia/Tokyo')::date,
      pending_result_id = 0
  "#, &[&user_id, &event_id, &score]).await.context("failed to record surprise event")?;
  Ok(())
}

/// Gives the packs and returns the items given (type, id, num) and the remote data.
async fn grant_packs(
  state: &AppState,
  user_id: UserId,
  packs: &[i64],
) -> anyhow::Result<(Vec<(i32, i64, i32)>, Vec<crate::api::RemoteData>)> {
  let items: Vec<(i32, i64, i32)> = packs.iter().flat_map(|&pack_id| pack_items(pack_id)).collect();
  let mut client = state.get_database_client().await?;
  let transaction = client.transaction().await.context("failed to start transaction")?;
  let mut wallet = Wallet::load(&transaction, user_id).await?;
  let mut remote = crate::user::grant::grant_items(&transaction, user_id, &mut wallet, &items).await?;
  wallet.save(&transaction, user_id).await?;
  remote.extend(wallet.to_remote_data());
  transaction.commit().await.context("failed to commit transaction")?;
  Ok((items, remote))
}

// See [Wonder_Api_SurpriseMiniEventSelectResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SurpriseMiniEventSelectResponse {
  pub select_list: Vec<SurpriseEvent>,
}

impl CallCustom for SurpriseMiniEventSelectResponse {}

// See [Wonder_Api_SurpriseEventStockListResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SurpriseEvent {
  pub user_stock_id: i32,
  pub surprise_event_id: i32,
  pub expired_date: i64,
}

/// Events that can still be played today, the stock id being the event id.
pub async fn surprise_mini_event_select(state: Arc<AppState>, session: Arc<Session>) -> impl IntoHandlerResponse {
  let client = state.get_database_client().await?;
  let plays = load_plays(&client, session.user_id).await?;
  let select_list: Vec<SurpriseEvent> = master("surprise_event")
    .iter()
    .filter(|event| {
      let id = int(event, "id") as i32;
      plays.get(&id).map_or(0, |&(today, _)| today) < int(event, "daily_limit").max(1) as i32
    })
    .map(|event| SurpriseEvent {
      user_stock_id: int(event, "id") as i32,
      surprise_event_id: int(event, "id") as i32,
      expired_date: 0,
    })
    .collect();
  info!(available = select_list.len(), "surprise events");
  Ok(Unsigned(SurpriseMiniEventSelectResponse { select_list }))
}

// See [Wonder_Api_SurpriseMiniEventTopResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SurpriseMiniEventTop {
  pub best_score: i32,
  pub expired_date: i64,
}

impl CallCustom for SurpriseMiniEventTop {}

#[derive(Debug, Deserialize)]
pub struct SurpriseMiniEventTopRequest {
  pub surprise_event_id: i32,
  pub user_stock_id: i32,
}

// user_stock_id=42
// surprise_event_id=10001
pub async fn surprise_mini_event_top(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<SurpriseMiniEventTopRequest>,
) -> impl IntoHandlerResponse {
  let client = state.get_database_client().await?;
  let plays = load_plays(&client, session.user_id).await?;
  Ok(Unsigned(SurpriseMiniEventTop {
    best_score: plays.get(&params.surprise_event_id).map_or(0, |&(_, best)| best),
    expired_date: 0,
  }))
}

// See [Wonder_Api_SurpriseQuestStartResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SurpriseQuestStart {
  pub party: BattleParty,
  pub members: Vec<BattleMember>,
}

impl CallCustom for SurpriseQuestStart {}

// See [Wonder_Api_BasicBattlePartyFormResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct BasicBattlePartyForm {
  pub id: i32,
  pub party_no: i32,
  pub form_no: i32,
  pub main: i32,
  pub sub1: i32,
  pub sub2: i32,
  pub weapon: i32,
  pub acc: i32,
  pub skill_pa_fame: i64,
}

#[derive(Debug, Deserialize)]
pub struct SurpriseQuestStartRequest {
  pub surprise_quest_id: i32,
  pub party_no: i32,
  pub user_stock_id: i32,
  pub surprise_event_id: i32,
}

// surprise_quest_id=10001
// party_no=1
// user_stock_id=42
// surprise_event_id=10001
/// Cabbage fights, with the player's own party (the same one a quest battle sends).
pub async fn surprise_quest_start(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<SurpriseQuestStartRequest>,
) -> impl IntoHandlerResponse {
  info!(?params, "surprise quest start");
  let client = state.get_database_client().await?;
  let (party, members) = crate::api::battle::battle_party_members(&client, session.user_id, params.party_no as i64).await?;
  let mut response = CallResponse::new_success(Box::new(SurpriseQuestStart { party, members }));
  response.add_notifications(vec![NotificationData::new(1, 7, 6, 0, "".to_string(), "".to_string())]);
  Ok(Unsigned(response))
}

// See [Wonder_Api_SurpriseQuestResultResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SurpriseQuestResult {
  pub reward: Vec<SurpriseQuestResultReward>,
  pub previous_best_score: i32,
  pub best_score: i32,
}

impl CallCustom for SurpriseQuestResult {}

// See [Wonder_Api_SurpriseQuestResultRewardResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SurpriseQuestResultReward {
  pub item_type: i32,
  pub item_id: i64,
  pub item_num: i32,
  pub is_rare: i32,
}

/// End of a cabbage fight. The request has not been seen yet: its body is logged, and the score
/// and event are taken from `score` and `surprise_event_id` (or any key holding them). The reward
/// is one pack of the best score tier reached, by weight.
pub async fn surprise_quest_result(
  state: Arc<AppState>,
  session: Arc<Session>,
  request: ApiRequest,
) -> impl IntoHandlerResponse {
  info!(body = ?request.body, "surprise_quest_result request");
  let number = |key: &str| -> i64 {
    request.body.get(key).and_then(|value| value.parse().ok()).or_else(|| {
      request.body.iter().find(|(name, _)| name.contains(key)).and_then(|(_, value)| value.parse().ok())
    }).unwrap_or(0)
  };
  let score = number("score").clamp(0, i32::MAX as i64) as i32;
  let event_id = match number("surprise_event_id") {
    0 => 10001,
    id => id as i32,
  };

  let client = state.get_database_client().await?;
  let previous_best_score = load_plays(&client, session.user_id).await?.get(&event_id).map_or(0, |&(_, best)| best);
  record_play(&client, session.user_id, event_id, score).await?;
  drop(client);

  let score_type = master("surprise_quest")
    .iter()
    .find(|quest| int(quest, "surprise_event_id") == event_id as i64)
    .map(|quest| int(quest, "score_type_id"))
    .unwrap_or(101);
  // Best tier reached (the lowest tier needs 1 point)
  let tier = master("surprise_quest_score")
    .iter()
    .filter(|tier| int(tier, "score_type_id") == score_type && int(tier, "score") <= score.max(1) as i64)
    .max_by_key(|tier| int(tier, "score"));
  let pack = tier.and_then(|tier| {
    let rows: Vec<&Value> = master("surprise_quest_score_reward")
      .iter()
      .filter(|reward| int(reward, "score_id") == int(tier, "score_id"))
      .collect();
    pick_weighted(&rows).map(|reward| int(reward, "pack_id"))
  });
  let (items, remote) = grant_packs(&state, session.user_id, pack.as_slice()).await?;
  let rare = |item_type: i32, item_id: i64| {
    tier.is_some_and(|tier| {
      master("surprise_quest_score_reward_item").iter().any(|item| {
        int(item, "score_id") == int(tier, "score_id")
          && int(item, "item_type") == item_type as i64
          && int(item, "item_id") == item_id
          && int(item, "rare") != 0
      })
    })
  };
  info!(event_id, score, ?pack, "surprise quest result");

  let mut response = CallResponse::new_success(Box::new(SurpriseQuestResult {
    reward: items
      .iter()
      .map(|&(item_type, item_id, item_num)| SurpriseQuestResultReward {
        item_type,
        item_id,
        item_num,
        is_rare: rare(item_type, item_id) as i32,
      })
      .collect(),
    previous_best_score,
    best_score: previous_best_score.max(score),
  }));
  response.remote.extend(remote);
  Ok(Unsigned(response))
}

// See [Wonder_Api_SurpriseShortEventResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SurpriseShortEvent {
  pub surprise_short_id: i32,
  pub result: SurpriseShortEventResult,
}

impl CallCustom for SurpriseShortEvent {}

#[derive(Debug, Deserialize)]
pub struct SurpriseShortEventRequest {
  pub surprise_event_id: i32,
  pub user_stock_id: i32,
}

// surprise_event_id=20002
// user_stock_id=42
/// Explosions
/// One result pattern of the event (those whose image is in its `surprise_short_reward_view`), and
/// its pack.
pub async fn surprise_short_event(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<SurpriseShortEventRequest>,
) -> impl IntoHandlerResponse {
  let Some(short) = master("surprise_short")
    .iter()
    .find(|short| int(short, "surprise_event_id") == params.surprise_event_id as i64)
  else {
    warn!(?params, "unknown short event");
    return Ok(Unsigned(CallResponse::new_success(Box::new(SurpriseShortEvent {
      surprise_short_id: 0,
      result: SurpriseShortEventResult { result_pattern_id: 0 },
    }))));
  };
  let short_id = int(short, "surprise_short_id");
  let images: Vec<&str> = master("surprise_short_reward_view")
    .iter()
    .filter(|view| int(view, "surprise_short_id") == short_id)
    .filter_map(|view| view["result_image_id"].as_str())
    .collect();
  let patterns: Vec<&Value> = master("surprise_short_result")
    .iter()
    .filter(|result| result["result_image_id"].as_str().is_some_and(|image| images.contains(&image)))
    .collect();
  let Some(pattern) = pick_weighted(&patterns) else {
    warn!(?params, short_id, "short event without results");
    return Ok(Unsigned(CallResponse::new_success(Box::new(SurpriseShortEvent {
      surprise_short_id: 0,
      result: SurpriseShortEventResult { result_pattern_id: 0 },
    }))));
  };

  let client = state.get_database_client().await?;
  record_play(&client, session.user_id, params.surprise_event_id, 0).await?;
  drop(client);
  let (_, remote) = grant_packs(&state, session.user_id, &[int(pattern, "pack_id")]).await?;
  info!(?params, pattern = int(pattern, "result_pattern_id"), "surprise short event");

  let mut response = CallResponse::new_success(Box::new(SurpriseShortEvent {
    surprise_short_id: short_id as i32,
    result: SurpriseShortEventResult {
      result_pattern_id: int(pattern, "result_pattern_id") as i32,
    },
  }));
  response.remote.extend(remote);
  Ok(Unsigned(response))
}

// See [Wonder_Api_SurpriseShortEventResultResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SurpriseShortEventResult {
  pub result_pattern_id: i32,
}

// See [Wonder_Api_SurpriseStoryStartResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SurpriseStoryStart {
  pub result_id_list: Vec<i32>,
}

impl CallCustom for SurpriseStoryStart {}

#[derive(Debug, Deserialize)]
pub struct SurpriseStoryStartRequest {
  pub surprise_event_id: i32,
  pub user_stock_id: i32,
  pub surprise_story_id: i32,
}

// surprise_event_id=40001
// user_stock_id=42
// surprise_story_id=1001
/// Vanir box gambling: one weighted pattern of two choices, and the result already drawn for
/// each choice (`surprise_story_result` ids, one per choice).
pub async fn surprise_story_start(Params(params): Params<SurpriseStoryStartRequest>) -> impl IntoHandlerResponse {
  let patterns: Vec<&Value> = master("surprise_story_select_pattern")
    .iter()
    .filter(|pattern| int(pattern, "surprise_story_id") == params.surprise_story_id as i64)
    .collect();
  let result_id_list: Vec<i32> = pick_weighted(&patterns)
    .map(|pattern| {
      ["select_id_1", "select_id_2"]
        .iter()
        .map(|key| int(pattern, key))
        .filter(|&select_id| select_id != 0)
        .filter_map(|select_id| {
          let results: Vec<&Value> = master("surprise_story_result")
            .iter()
            .filter(|result| int(result, "select_id") == select_id)
            .collect();
          pick_weighted(&results).map(|result| int(result, "id") as i32)
        })
        .collect()
    })
    .unwrap_or_default();
  info!(?params, ?result_id_list, "surprise story start");
  Ok(Unsigned(SurpriseStoryStart { result_id_list }))
}

#[derive(Debug, Deserialize)]
pub struct SurpriseStorySelectRequest {
  pub surprise_story_id: i32,
  pub surprise_event_id: i32,
  pub user_stock_id: i32,
  pub result_id: i32,
}

// surprise_story_id=1001
// surprise_event_id=40001
// user_stock_id=42
// result_id=2
/// Keeps the chosen result until the story ends.
pub async fn surprise_story_select(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<SurpriseStorySelectRequest>,
) -> impl IntoHandlerResponse {
  info!(?params, "surprise story select");
  let client = state.get_database_client().await?;
  #[rustfmt::skip]
  client.execute(/* language=postgresql */ r#"
    insert into user_surprise_events (user_id, surprise_event_id, pending_result_id) values ($1, $2, $3)
    on conflict (user_id, surprise_event_id) do update set pending_result_id = $3
  "#, &[&session.user_id, &params.surprise_event_id, &params.result_id]).await.context("failed to save story choice")?;

  // See [Wonder_Api_SurpriseStorySelectResponseDto_Fields]
  Ok(Unsigned(()))
}

// See [Wonder_Api_SurpriseStoryResultResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SurpriseStoryResult {
  pub reward: Vec<SurpriseStoryReward>,
}

impl CallCustom for SurpriseStoryResult {}

// See [Wonder_Api_StoryrewardRewardResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SurpriseStoryReward {
  pub item_type: i32,
  pub item_id: i64,
  pub num: i32,
}

#[derive(Debug, Deserialize)]
pub struct SurpriseStoryResultRequest {
  pub surprise_story_id: i32,
  pub surprise_event_id: i32,
  pub user_stock_id: i32,
}

// surprise_story_id=1001
// surprise_event_id=40001
// user_stock_id=42
/// Gives the pack of the result chosen in `surprise_story_select`.
pub async fn surprise_story_result(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<SurpriseStoryResultRequest>,
) -> impl IntoHandlerResponse {
  let client = state.get_database_client().await?;
  #[rustfmt::skip]
  let result_id: i32 = client.query_opt(/* language=postgresql */ r#"
    select pending_result_id from user_surprise_events where user_id = $1 and surprise_event_id = $2
  "#, &[&session.user_id, &params.surprise_event_id]).await.context("failed to read story choice")?
    .map(|row| row.get(0)).unwrap_or(0);
  record_play(&client, session.user_id, params.surprise_event_id, 0).await?;
  drop(client);

  let pack = master("surprise_story_result")
    .iter()
    .find(|result| int(result, "id") == result_id as i64)
    .map(|result| int(result, "pack_id"));
  if pack.is_none() {
    warn!(?params, result_id, "story ended without a chosen result");
  }
  let (items, remote) = grant_packs(&state, session.user_id, pack.as_slice()).await?;
  info!(?params, result_id, ?pack, "surprise story result");

  let mut response = CallResponse::new_success(Box::new(SurpriseStoryResult {
    reward: items
      .into_iter()
      .map(|(item_type, item_id, num)| SurpriseStoryReward { item_type, item_id, num })
      .collect(),
  }));
  response.remote.extend(remote);
  Ok(Unsigned(response))
}
