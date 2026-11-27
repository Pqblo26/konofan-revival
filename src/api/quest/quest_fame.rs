//! Hierarchy is Rank (I) -> Area (Near the Axel Village) -> Stage (Dire Bunny Raid)

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use anyhow::Context;
use tracing::{debug, warn};

use crate::api::battle_multi::{BattleCharacterLove, BattleMemberExp};
use crate::api::master_all::{get_master_manager, get_masters};
use crate::api::quest::quest_hunting::{BattleReward};
use crate::api::{battle, ApiRequest, SkillPaFameAddStatus};
use crate::api::battle::{
  apply_reward_multiplier, grant_rewards, make_battle_member_exp_and_character_love, pay_quest_money,
};
use crate::api::quest::stage::stage_info;
use crate::api::quest::parse_reward_items;
use crate::call::{CallCustom, CallResponse};
use crate::extractor::Params;
use crate::handler::{IntoHandlerResponse, Unsigned};
use crate::user::session::Session;
use crate::AppState;
use crate::member::FetchUserParty;

// See [Wonder_Api_FameQuestRankListResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct QuestFameRankListResponse {
  quest_rank_id_list: Vec<i32>,
  has_emergency_quest: bool,
}

impl CallCustom for QuestFameRankListResponse {}

pub async fn fame_quest_rank_list() -> impl IntoHandlerResponse {
  let masters = get_masters().await;
  let ranks: Vec<Value> = serde_json::from_str(&masters["fame_quest_rank"].master_decompressed).unwrap();

  Ok(Unsigned(QuestFameRankListResponse {
    quest_rank_id_list: ranks
      .iter()
      .map(|rank| rank.get("id").unwrap().as_str().unwrap().parse::<i32>().unwrap())
      .collect::<Vec<_>>(),
    has_emergency_quest: false,
  }))
}

// See [Wonder_Api_FameQuestAreaListResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct QuestFameAreaListResponse {
  area_info_list: Vec<FameQuestAreaInfo>,
  has_emergency_quest: bool,
}

impl CallCustom for QuestFameAreaListResponse {}

// See [Wonder_Api_FameQuestAreaInfoResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct FameQuestAreaInfo {
  pub area_id: i32,
}

#[derive(Debug, Deserialize)]
pub struct FameQuestAreaListRequest {
  pub quest_rank_id: i32,
}

pub async fn fame_quest_area_list(Params(params): Params<FameQuestAreaListRequest>) -> impl IntoHandlerResponse {
  let masters = get_masters().await;
  let areas: Vec<Value> = serde_json::from_str(&masters["fame_quest_area"].master_decompressed).unwrap();

  Ok(Unsigned(QuestFameAreaListResponse {
    area_info_list: areas
      .iter()
      .filter(|area| {
        area
          .get("quest_rank_id")
          .unwrap()
          .as_str()
          .unwrap()
          .parse::<i32>()
          .unwrap()
          == params.quest_rank_id
      })
      .map(|rank| FameQuestAreaInfo {
        area_id: rank.get("id").unwrap().as_str().unwrap().parse::<i32>().unwrap(),
      })
      .collect::<Vec<_>>(),
    has_emergency_quest: false,
  }))
}

// See [Wonder_Api_FameQuestStageListResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct QuestFameStageListResponse {
  quest_list: Vec<FameQuestStageInfo>,
  unlock_area_id_list: Vec<i32>,
  has_emergency_quest: bool,
  remaining_count: i32,
  transition_fame_quest_id: i32,
  can_skip: bool,
}

impl CallCustom for QuestFameStageListResponse {}

// See [Wonder_Api_FameQuestStageInfoResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct FameQuestStageInfo {
  pub stage_id: i32,
  pub task1: i32,
  pub task2: i32,
  pub task3: i32,
  pub expired_at: i64,
  /// See [Wonder.Api.FameQuestStageInfoResponseDto$$get_IsAllClearReleaseCondition]
  pub release_condition: FameQuestReleaseConditionInfo,
  pub bonus_skill_pa_fame_rate: i32,
}

// See [Wonder_Api_FameQuestReleaseConditionInfoResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct FameQuestReleaseConditionInfo {
  pub key_quest: i32,
  pub story: i32,
  pub event_story: i32,
}

#[derive(Debug, Deserialize)]
pub struct FameQuestStageListRequest {
  pub area_id: i32,
  pub mode: i32,
}

pub async fn fame_quest_stage_list(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<FameQuestStageListRequest>,
) -> impl IntoHandlerResponse {
  // Crowns achieved in each stage
  let progress = match state.get_database_client().await {
    Ok(client) => crate::api::quest::progress::load_progress(&client, session.user_id)
      .await
      .unwrap_or_else(|error| {
        warn!(?error, "failed to read fame quest progress");
        Default::default()
      }),
    Err(error) => {
      warn!(?error, "no database connection for fame quest progress");
      Default::default()
    }
  };
  let masters = get_masters().await;
  let areas: Vec<Value> = serde_json::from_str(&masters["fame_quest_area"].master_decompressed).unwrap();
  let stages: Vec<Value> = serde_json::from_str(&masters["fame_quest_stage"].master_decompressed).unwrap();

  let area = areas
    .iter()
    .find(|area| area.get("id").unwrap().as_str().unwrap().parse::<i32>().unwrap() == params.area_id)
    .unwrap();
  let current_rank = area
    .get("quest_rank_id")
    .unwrap()
    .as_str()
    .unwrap()
    .parse::<i32>()
    .unwrap();
  debug!("current rank: {}", current_rank);

  Ok(Unsigned(QuestFameStageListResponse {
    quest_list: stages
      .iter()
      .filter(|stage| {
        stage.get("area_id").unwrap().as_str().unwrap().parse::<i32>().unwrap() == params.area_id
          && stage.get("mode").unwrap().as_str().unwrap().parse::<i32>().unwrap() == params.mode
      })
      .map(|stage| {
        let stage_id = stage.get("id").unwrap().as_str().unwrap().parse::<i32>().unwrap();
        let tasks = progress.get(&stage_id).map(|progress| progress.tasks).unwrap_or_default();
        FameQuestStageInfo {
        stage_id,
        task1: tasks[0] as i32,
        task2: tasks[1] as i32,
        task3: tasks[2] as i32,
        expired_at: 0,
        release_condition: FameQuestReleaseConditionInfo {
          key_quest: 2,
          story: 2,
          event_story: 2,
        },
        bonus_skill_pa_fame_rate: 0,
        }
      })
      .collect::<Vec<_>>(),
    // All areas for the given rank, thanks https://www.youtube.com/watch?v=Muk190J7LFo
    unlock_area_id_list: areas
      .iter()
      .filter(|area| {
        let area_rank = area
          .get("quest_rank_id")
          .unwrap()
          .as_str()
          .unwrap()
          .parse::<i32>()
          .unwrap();
        area_rank == current_rank
      })
      .map(|area| area.get("id").unwrap().as_str().unwrap().parse::<i32>().unwrap())
      .collect::<Vec<_>>(),
    has_emergency_quest: false,
    remaining_count: 1,
    transition_fame_quest_id: 0,
    can_skip: false,
  }))
}

// See [Wonder_Api_FameQuestStartResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct FameQuestStart {
  // pub party: FameQuestParty,
  // pub members: Vec<FameQuestMember>,
}

// use_supplement_num=0
// party_no=1
// stage_id=710011
// cost_ratio=1
pub async fn fame_quest_start(
  state: Arc<AppState>,
  session: Arc<Session>,
  request: ApiRequest,
) -> impl IntoHandlerResponse {
  let use_supplement_num: i32 = request.body["use_supplement_num"].parse().unwrap();
  let party_no: i32 = request.body["party_no"].parse().unwrap();
  let stage_id: i32 = request.body["stage_id"].parse().unwrap();
  let cost_ratio: i32 = request.body["cost_ratio"].parse().unwrap();

  // Fame Supplements are taken here; failing to take them must not stop the battle
  let supplements_remote = match state.get_database_client().await {
    Ok(client) => crate::api::fame::start_fame_quest(&client, session.user_id, use_supplement_num, cost_ratio)
      .await
      .unwrap_or_else(|error| {
        warn!(?error, "failed to take fame supplements");
        Vec::new()
      }),
    Err(error) => {
      warn!(?error, "no database connection for fame supplements");
      Vec::new()
    }
  };
  battle::make_battle_start_with(&state, &session, party_no, Some((stage_id, cost_ratio)), true, supplements_remote).await
}

// See [Wonder_Api_FameQuestResultResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct FameQuestResultResponse {
  pub fame_rank_up: i32,
  pub money: i32,
  pub exp: i32,
  pub lvup: i32,
  pub love: Vec<BattleCharacterLove>,
  pub member_exp: Vec<BattleMemberExp>,
  // [mission_reward] and [clear_reward] names seem to be swapped
  pub mission_reward: Vec<BattleClearReward>,
  pub clear_reward: Vec<BattleReward>,
  pub lottery_potion_list: Vec<FameQuestLotteryPotion>,
}

impl CallCustom for FameQuestResultResponse {}

// See [Wonder_Api_FameQuestResultClearRewardResponseDto_Fields]
// extends [Wonder_Api_ResultClearrewardResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct BattleClearReward {
  pub itemtype: i32,
  pub itemid: i64,
  pub itemnum: i32,
  pub mission: i32,
  #[serde(with = "crate::bool_as_int")]
  pub is_rare: bool,
}

// See [Wonder_Api_FameQuestLotteryPotionResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct FameQuestLotteryPotion {
  pub potion_id: i32,
  pub user_member_id: i64,
  pub potion_choice_list: Vec<FameQuestLotteryPotionChoice>,
}

// See [Wonder_Api_FameQuestLotteryPotionChoiceResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct FameQuestLotteryPotionChoice {
  pub potion_choice_id: i32,
  pub potion_type: i32,
  pub skill_pa_fame_id: i64,
  pub skill_pa_fame_status: Vec<SkillPaFameAddStatus>,
}

// Interestingly it does not have [memcheckcount].
// See [Wonder_Api_FameQuestResultRequest_Fields]
#[derive(Debug, Deserialize)]
pub struct FameQuestResultRequest {
  #[serde(rename = "party_no")]
  pub party_id: i32,
  pub stage_id: i32,
  pub win: i32,
  pub clear_mission_list: Vec<i32>,
}

pub async fn fame_quest_result(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<FameQuestResultRequest>,
) -> impl IntoHandlerResponse {
  let mut client = state.get_database_client().await?;
  let transaction = client.transaction().await.context("failed to start transaction")?;

  let stage = stage_info(params.stage_id).context("unknown fame quest stage")?;
  let mut rewards = stage.rewards.clone();
  apply_reward_multiplier(&transaction, &session, &mut rewards).await?;
  let update_items = grant_rewards(&transaction, &session, &rewards).await?;
  let fame_record = crate::api::fame::record_fame_quest(
    &transaction,
    session.user_id,
    params.stage_id,
    params.win != 0,
    &params.clear_mission_list,
  )
  .await?;
  transaction.commit().await.context("failed to commit transaction")?;
  let wallet_remote = pay_quest_money(&client, &session, stage.money as i64).await?;

  let party = FetchUserParty::new(&client)
    .await?
    .run(session.user_id, params.party_id as i64)
    .await?;

  let (member_exp, love) =
    make_battle_member_exp_and_character_love(&party, &client, &session, stage.intimacy_exp).await?;
  // The main member of each form; forms 1-3 are the front ones
  let members: Vec<(i64, bool)> = party
    .party_forms
    .iter()
    .enumerate()
    .filter(|(_, form)| form.main > 0)
    .map(|(index, form)| (form.main as i64, index < 3))
    .collect();
  let lottery_potion_list =
    crate::api::fame::draw_potions(&client, session.user_id, params.stage_id, &members, params.win != 0).await;
  let mut response = CallResponse::new_success(Box::new(FameQuestResultResponse {
    // 1 shows "Fame Rank UP!"
    fame_rank_up: fame_record.fame_rank_up as i32,
    exp: stage.player_exp,
    lvup: 0,
    money: stage.money,
    love,
    // TODO: We must send only members that are used in the party, otherwise hardlock occurs??
    member_exp,
    clear_reward: rewards
      .iter()
      .map(|item| BattleReward {
        itemtype: item.item_type,
        itemid: item.item_id,
        itemnum: item.item_num,
        is_rare: item.item_rare,
      })
      .collect(),
    mission_reward: fame_record
      .crown_rewards
      .iter()
      .map(|&(mission, itemtype, itemid, itemnum)| BattleClearReward {
        itemtype,
        itemid,
        itemnum,
        mission,
        is_rare: false,
      })
      .collect(),
    lottery_potion_list,
  }));
  response.remote.extend(update_items);
  response.remote.extend(fame_record.remote);
  response.remote.extend(wallet_remote);
  Ok(Unsigned(response))
}
