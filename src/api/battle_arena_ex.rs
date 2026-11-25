//! Arena EX (scorechallenge_ex): one boss per season, 3 attempts a day. Each battle gives random
//! rewards by the rank of its score (S, A...), and every score adds up to the season's
//! "achievement score", which unlocks the milestone rewards. Once a battle is done, the remaining
//! attempts of the day can be skipped with the best score.
//!
//! The client has no request classes for these calls, so the parameters are read by name with
//! fallbacks, and logged.

use crate::api::battle::{BattleMember, BattleParty};
use crate::api::exchange::{get_pack, pack_contents};
use crate::api::master_all::get_master_manager;
use crate::api::quest::progress::{load_progress, parse_master_date, record_quest};
use crate::api::ApiRequest;
use crate::call::{CallCustom, CallResponse};
use crate::database::QueryExecutor;
use crate::handler::{IntoHandlerResponse, Unsigned};
use crate::user::grant::grant_items;
use crate::user::id::UserId;
use crate::user::session::Session;
use crate::user::wallet::Wallet;
use crate::AppState;
use anyhow::Context;
use chrono::Utc;
use rand::Rng;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{info, warn};

const DAILY_ATTEMPTS: i32 = 3;

fn int(row: &Value, key: &str) -> i64 {
  row[key].as_str().and_then(|v| v.parse().ok()).unwrap_or(0)
}

fn clamp_score(score: i64) -> i32 {
  score.clamp(0, i32::MAX as i64) as i32
}

/// Integer parameter by the first of [keys] that is present.
fn param(body: &HashMap<String, String>, keys: &[&str]) -> Option<i64> {
  keys.iter().find_map(|key| body.get(*key).and_then(|v| v.trim().parse().ok()))
}

/// The season open now: (scorechallenge_ex_id, score_type_id). Only one is opened by the master patch.
fn current_season() -> Option<(i32, i64)> {
  let now = Utc::now();
  get_master_manager()
    .get_master("scorechallenge_ex")
    .iter()
    .filter_map(|row| {
      let start = parse_master_date(row["start_at"].as_str()?)?;
      let end = parse_master_date(row["end_at"].as_str()?)?;
      (start <= now && now <= end).then(|| (start, int(row, "scorechallenge_ex_id") as i32, int(row, "score_type_id")))
    })
    .max()
    .map(|(_, id, score_type)| (id, score_type))
}

fn season_stages(season: i32) -> Vec<i32> {
  get_master_manager()
    .get_master("scorechallenge_ex_stage")
    .iter()
    .filter(|row| int(row, "scorechallenge_ex_id") == season as i64)
    .map(|row| int(row, "id") as i32)
    .collect()
}

/// The season's total score is kept with the stage bests, under the season id made negative
/// (stage ids are positive), so no new table is needed.
fn achievement_key(season: i32) -> i32 {
  -season
}

struct SeasonState {
  best: HashMap<i32, i64>,
  achievement: i64,
  used_today: i32,
}

impl SeasonState {
  fn attempts_left(&self) -> i32 {
    (DAILY_ATTEMPTS - self.used_today).max(0)
  }

  fn best_of(&self, quest_id: i32) -> i64 {
    self.best.get(&quest_id).copied().unwrap_or(0)
  }
}

async fn load_season<'a>(executor: impl Into<QueryExecutor<'a>>, user_id: UserId, season: i32) -> anyhow::Result<SeasonState> {
  let executor = executor.into();
  let stages = season_stages(season);
  let mut ids = stages.clone();
  ids.push(achievement_key(season));
  #[rustfmt::skip]
  let rows = executor.client().query(/* language=postgresql */ r#"
    select quest_id, best_score from user_score_challenge_best where user_id = $1 and quest_id = any($2)
  "#, &[&user_id, &ids]).await.context("failed to read arena ex scores")?;
  let mut best = rows.iter().map(|row| (row.get::<_, i32>(0), row.get::<_, i64>(1))).collect::<HashMap<_, _>>();
  let achievement = best.remove(&achievement_key(season)).unwrap_or(0);
  let progress = load_progress(executor.client(), user_id).await?;
  let used_today = stages.iter().filter_map(|id| progress.get(id)).map(|entry| entry.daily_count).sum();
  Ok(SeasonState { best, achievement, used_today })
}

/// Score rank of a score: (score_id, img_type), from the best rank reached.
fn score_rank(score_type: i64, score: i64) -> Option<(i64, i32)> {
  get_master_manager()
    .get_master("scorechallenge_ex_score")
    .iter()
    .filter(|row| int(row, "score_type_id") == score_type && int(row, "score") <= score)
    .max_by_key(|row| int(row, "score"))
    .map(|row| (int(row, "score_id"), int(row, "img_type") as i32))
}

// See [Wonder_Api_ScorechallengeExResultRewardResponseDto_Fields]
#[derive(Debug, Serialize, Clone)]
pub struct ExReward {
  pub item_type: i32,
  pub item_id: i64,
  pub item_num: i32,
  pub is_rare: i32,
}

/// The battle rewards of a score rank: one of its packs, drawn by weight.
fn draw_rewards(score_id: i64) -> Vec<ExReward> {
  let master = get_master_manager();
  let packs = master
    .get_master("scorechallenge_ex_score_reward")
    .iter()
    .filter(|row| int(row, "score_id") == score_id)
    .map(|row| (int(row, "pack_id"), int(row, "weight").max(0)))
    .collect::<Vec<_>>();
  let total = packs.iter().map(|(_, weight)| weight).sum::<i64>();
  if total <= 0 {
    return vec![];
  }
  let mut roll = rand::rng().random_range(0..total);
  let pack_id = packs
    .iter()
    .find(|(_, weight)| {
      roll -= weight;
      roll < 0
    })
    .map(|(id, _)| *id)
    .unwrap_or(packs[0].0);
  let rare = master
    .get_master("scorechallenge_ex_score_reward_item")
    .iter()
    .filter(|row| int(row, "score_id") == score_id && int(row, "rare") == 1)
    .map(|row| (int(row, "item_type") as i32, int(row, "item_id")))
    .collect::<HashSet<_>>();
  get_pack(&pack_id.to_string())
    .map(pack_contents)
    .unwrap_or_default()
    .into_iter()
    .map(|(item_type, item_id, item_num)| ExReward {
      item_type,
      item_id,
      item_num,
      is_rare: rare.contains(&(item_type, item_id)) as i32,
    })
    .collect()
}

/// Milestones of the season reached when its total goes from [before] to [after]: (ids, rewards).
fn achievement_rewards(season: i32, before: i64, after: i64) -> (Vec<i32>, Vec<(i32, i64, i32)>) {
  let mut ids = Vec::new();
  let mut items = Vec::new();
  for row in get_master_manager().get_master("scorechallenge_ex_reward_achievement") {
    if int(row, "scorechallenge_ex_id") != season as i64 {
      continue;
    }
    let needed = int(row, "score");
    if before < needed && needed <= after {
      ids.push(int(row, "id") as i32);
      items.extend(row["pack_id"].as_str().and_then(get_pack).map(pack_contents).unwrap_or_default());
    }
  }
  (ids, items)
}

fn to_items(rewards: &[ExReward]) -> Vec<(i32, i64, i32)> {
  rewards.iter().map(|r| (r.item_type, r.item_id, r.item_num)).collect()
}

// See [Wonder_Api_ScorechallengeExInfoResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct ScoreChallengeExInfoResponse {
  pub challengenum: i32,
  pub scores: Vec<ScoreChallengeExScore>,
  pub achievement_score: i64,
}

impl CallCustom for ScoreChallengeExInfoResponse {}

#[derive(Debug, Serialize)]
pub struct ScoreChallengeExScore {
  pub quest_id: i32,
  pub score: i64,
}

pub async fn score_challenge_ex_info(state: Arc<AppState>, request: ApiRequest, session: Arc<Session>) -> impl IntoHandlerResponse {
  info!(body = ?request.body, "arena ex info");
  let client = state.get_database_client().await?;
  let Some((season, _)) = current_season() else {
    warn!("no arena ex season open");
    return Ok(Unsigned(ScoreChallengeExInfoResponse { challengenum: 0, scores: vec![], achievement_score: 0 }));
  };
  let state = load_season(&client, session.user_id, season).await?;
  Ok(Unsigned(ScoreChallengeExInfoResponse {
    challengenum: state.attempts_left(),
    scores: season_stages(season)
      .into_iter()
      .map(|quest_id| ScoreChallengeExScore { quest_id, score: state.best_of(quest_id) })
      .collect(),
    achievement_score: state.achievement,
  }))
}

// See [Wonder_Api_ScorechallengeExStartResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct ScoreChallengeExStartResponse {
  pub party: BattleParty,
  pub members: Vec<BattleMember>,
}

impl CallCustom for ScoreChallengeExStartResponse {}

pub async fn score_challenge_ex_start(state: Arc<AppState>, request: ApiRequest, session: Arc<Session>) -> impl IntoHandlerResponse {
  info!(body = ?request.body, "arena ex start");
  let (season, _) = current_season().context("no arena ex season open")?;
  let quest_id = param(&request.body, &["quest_id", "stage_id"])
    .map(|v| v as i32)
    .or_else(|| season_stages(season).first().copied())
    .context("arena ex season without stages")?;
  let party_id = param(&request.body, &["party_no", "party_id"]).unwrap_or(1);

  let client = state.pool.get().await.context("failed to get database connection")?;
  let season_state = load_season(&client, session.user_id, season).await?;
  if season_state.attempts_left() <= 0 {
    warn!(quest_id, "no arena ex attempts left today, the battle will give nothing");
  }
  // The attempt is counted at the result: a battle left halfway does not use it
  #[rustfmt::skip]
  client.execute(/* language=postgresql */ r#"
    insert into user_score_challenge_runs (user_id, quest_id, is_practice) values ($1, $2, $3)
    on conflict (user_id) do update set quest_id = excluded.quest_id, is_practice = excluded.is_practice, started_at = now()
  "#, &[&session.user_id, &quest_id, &(season_state.attempts_left() <= 0)]).await.context("failed to save arena ex run")?;

  let (party, members) = crate::api::battle::battle_party_members(&client, session.user_id, party_id).await?;
  Ok(Unsigned(ScoreChallengeExStartResponse { party, members }))
}

// See [Wonder_Api_ScorechallengeExResultResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct ScoreChallengeExResultResponse {
  pub previous_bestscore: i32,
  pub bestscore: i32,
  pub modescore: i32,
  pub total_score: i32,
  pub basic_score: i32,
  pub battletime_coefficient: String,
  pub bonus_score: i32,
  pub survival_coefficient: String,
  pub character_bonus_coefficient: String,
  pub rarity_bonus_coefficient: String,
  pub reward: Vec<ExReward>,
  pub score_img_type: i32,
  pub achievement_score: i64,
  pub achievement_reward_ids: Vec<i32>,
  pub challengenum: i32,
}

impl CallCustom for ScoreChallengeExResultResponse {}

pub async fn score_challenge_ex_result(
  state: Arc<AppState>,
  request: ApiRequest,
  session: Arc<Session>,
) -> anyhow::Result<impl IntoHandlerResponse> {
  let body = &request.body;
  info!(?body, "arena ex result");
  let (season, score_type) = current_season().context("no arena ex season open")?;
  // Same scrambling as the normal arena when it sends a seed
  let score = match (param(body, &["score"]), param(body, &["seed"])) {
    (Some(score), Some(seed)) => (score ^ seed) as i32 as i64,
    (Some(score), None) => score,
    _ => param(body, &["original_score", "total_score"]).unwrap_or(0),
  };
  if let Some(original) = param(body, &["original_score"]) {
    if original != score {
      warn!(score, original, "arena ex score check failed, using the decrypted score");
    }
  }
  let score = score.max(0);

  let mut client = state.get_database_client().await?;
  let transaction = client.transaction().await.context("failed to start transaction")?;
  #[rustfmt::skip]
  let run = transaction.query_opt(/* language=postgresql */ r#"
    delete from user_score_challenge_runs where user_id = $1 returning quest_id, is_practice
  "#, &[&session.user_id]).await.context("failed to read arena ex run")?
    .map(|row| (row.get::<_, i32>(0), row.get::<_, bool>(1)));
  let quest_id = param(body, &["quest_id", "stage_id"])
    .map(|v| v as i32)
    .or(run.map(|(id, _)| id))
    .or_else(|| season_stages(season).first().copied())
    .context("arena ex season without stages")?;

  let before = load_season(&transaction, session.user_id, season).await?;
  let previous_best = before.best_of(quest_id);
  // No attempts left (or a battle started without them): the score is shown but gives nothing
  let counts = before.attempts_left() > 0 && !run.map(|(_, no_attempt)| no_attempt).unwrap_or(false);
  let rank = score_rank(score_type, score);
  let mut reward = Vec::new();
  let mut achievement_ids = Vec::new();
  let mut achievement = before.achievement;
  let mut remote = Vec::new();
  let mut wallet = Wallet::load(&transaction, session.user_id).await?;
  if counts {
    record_quest(&transaction, session.user_id, quest_id, 1, [false; 3]).await?;
    #[rustfmt::skip]
    transaction.execute(/* language=postgresql */ r#"
      insert into user_score_challenge_best (user_id, quest_id, best_score) values ($1, $2, $3)
      on conflict (user_id, quest_id) do update
        set best_score = greatest(user_score_challenge_best.best_score, excluded.best_score), updated_at = now()
    "#, &[&session.user_id, &quest_id, &score]).await.context("failed to save arena ex score")?;
    #[rustfmt::skip]
    transaction.execute(/* language=postgresql */ r#"
      insert into user_score_challenge_best (user_id, quest_id, best_score) values ($1, $2, $3)
      on conflict (user_id, quest_id) do update
        set best_score = user_score_challenge_best.best_score + excluded.best_score, updated_at = now()
    "#, &[&session.user_id, &achievement_key(season), &score]).await.context("failed to save arena ex total")?;
    achievement += score;

    if let Some((score_id, _)) = rank {
      reward = draw_rewards(score_id);
    }
    let (ids, mut items) = achievement_rewards(season, before.achievement, achievement);
    achievement_ids = ids;
    items.extend(to_items(&reward));
    remote.extend(grant_items(&transaction, session.user_id, &mut wallet, &items).await?);
  } else {
    warn!(quest_id, score, "arena ex battle without attempts left, nothing saved");
  }
  wallet.save(&transaction, session.user_id).await?;
  transaction.commit().await.context("failed to commit transaction")?;
  info!(quest_id, score, counts, achievement, "arena ex battle finished");

  let coefficient = |key: &str| body.get(key).cloned().unwrap_or_else(|| "1".to_owned());
  let basic_score = param(body, &["basic_score"]).unwrap_or(score);
  let mut response = CallResponse::new_success(Box::new(ScoreChallengeExResultResponse {
    previous_bestscore: clamp_score(previous_best),
    bestscore: clamp_score(if counts { previous_best.max(score) } else { previous_best }),
    modescore: clamp_score(score),
    total_score: clamp_score(score),
    basic_score: clamp_score(basic_score),
    battletime_coefficient: coefficient("battletime_coefficient"),
    bonus_score: clamp_score(param(body, &["bonus_score"]).unwrap_or(0)),
    survival_coefficient: coefficient("survival_coefficient"),
    character_bonus_coefficient: coefficient("character_bonus_coefficient"),
    rarity_bonus_coefficient: coefficient("rarity_bonus_coefficient"),
    reward,
    score_img_type: rank.map(|(_, img)| img).unwrap_or(1),
    achievement_score: achievement,
    achievement_reward_ids: achievement_ids,
    // Like the normal arena, taken as the attempts used
    challengenum: counts as i32,
  }));
  response.remote.extend(remote);
  response.remote.extend(wallet.to_remote_data());
  Ok(Unsigned(response))
}

// See [Wonder_Api_ScorechallengeExSkipResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct ScoreChallengeExSkipResponse {
  pub reward_list: Vec<ScoreChallengeExSkipReward>,
  pub achievement_reward_ids: Vec<i32>,
  pub achievement_score: i64,
  pub best_score: i64,
  pub remain_challenge_num: i32,
}

impl CallCustom for ScoreChallengeExSkipResponse {}

#[derive(Debug, Serialize)]
pub struct ScoreChallengeExSkipReward {
  pub skip_num: i32,
  pub reward_item_list: Vec<ExReward>,
}

pub async fn score_challenge_ex_skip(
  state: Arc<AppState>,
  request: ApiRequest,
  session: Arc<Session>,
) -> anyhow::Result<impl IntoHandlerResponse> {
  let body = &request.body;
  info!(?body, "arena ex skip");
  let (season, score_type) = current_season().context("no arena ex season open")?;
  let stages = season_stages(season);
  // Two integers, names unknown: the stage and how many attempts to skip
  let quest_id = param(body, &["quest_id", "stage_id"])
    .map(|v| v as i32)
    .or_else(|| body.values().filter_map(|v| v.parse::<i32>().ok()).find(|v| stages.contains(v)))
    .or_else(|| stages.first().copied())
    .context("arena ex season without stages")?;
  let wanted = param(body, &["skip_num", "num", "count", "skip_count", "challenge_num", "challengenum"])
    .map(|v| v as i32)
    .or_else(|| body.values().filter_map(|v| v.parse::<i32>().ok()).find(|v| (1..=DAILY_ATTEMPTS).contains(v)))
    .unwrap_or(1);

  let mut client = state.get_database_client().await?;
  let transaction = client.transaction().await.context("failed to start transaction")?;
  let before = load_season(&transaction, session.user_id, season).await?;
  let best = before.best_of(quest_id);
  let skips = if best > 0 { wanted.clamp(0, before.attempts_left()) } else { 0 };
  if skips != wanted {
    warn!(wanted, skips, best, "arena ex skip limited by attempts left or missing best score");
  }

  let mut reward_list = Vec::new();
  let mut items = Vec::new();
  let rank = score_rank(score_type, best);
  for skip_num in 1..=skips {
    let rewards = rank.map(|(score_id, _)| draw_rewards(score_id)).unwrap_or_default();
    items.extend(to_items(&rewards));
    reward_list.push(ScoreChallengeExSkipReward { skip_num, reward_item_list: rewards });
  }
  let achievement = before.achievement + best * skips as i64;
  let (achievement_ids, achievement_items) = achievement_rewards(season, before.achievement, achievement);
  items.extend(achievement_items);

  let mut wallet = Wallet::load(&transaction, session.user_id).await?;
  let mut remote = Vec::new();
  if skips > 0 {
    record_quest(&transaction, session.user_id, quest_id, skips, [false; 3]).await?;
    #[rustfmt::skip]
    transaction.execute(/* language=postgresql */ r#"
      insert into user_score_challenge_best (user_id, quest_id, best_score) values ($1, $2, $3)
      on conflict (user_id, quest_id) do update
        set best_score = user_score_challenge_best.best_score + excluded.best_score, updated_at = now()
    "#, &[&session.user_id, &achievement_key(season), &(best * skips as i64)]).await.context("failed to save arena ex total")?;
    remote.extend(grant_items(&transaction, session.user_id, &mut wallet, &items).await?);
  }
  wallet.save(&transaction, session.user_id).await?;
  transaction.commit().await.context("failed to commit transaction")?;
  info!(quest_id, skips, achievement, "arena ex skipped");

  let mut response = CallResponse::new_success(Box::new(ScoreChallengeExSkipResponse {
    reward_list,
    achievement_reward_ids: achievement_ids,
    achievement_score: achievement,
    best_score: best,
    remain_challenge_num: before.attempts_left() - skips,
  }));
  response.remote.extend(remote);
  response.remote.extend(wallet.to_remote_data());
  Ok(Unsigned(response))
}
