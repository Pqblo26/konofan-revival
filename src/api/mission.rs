use std::sync::Arc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_repr::{Deserialize_repr, Serialize_repr};
use tracing::{info, warn};

use crate::api::mission_progress::{self, MissionState};

use crate::api::master_all::{get_master_manager, get_masters};
use crate::api::{battle, ApiRequest, NotificationData};
use crate::user::id::UserId;
use crate::api::battle::{
  AutoProgressionResultResponse, BattleResultResponse,
};
use crate::call::{CallCustom, CallResponse};
use crate::extractor::Params;
use crate::handler::{IntoHandlerResponse, Signed, Unsigned};
use crate::user::session::Session;
use crate::AppState;
use crate::user::grant::grant_items;
use crate::user::wallet::Wallet;
use anyhow::Context;

#[derive(Debug, Serialize)]
pub struct MissionList {
  #[serde(rename = "list")]
  pub missions: Vec<Mission>,
}

impl CallCustom for MissionList {}

// See [Wonder_Api_MissionListResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct Mission {
  pub mission_id: i32,
  #[serde(rename = "type")]
  pub kind: i32,
  pub progress: i32,
  pub received: i32,
  pub newmisson: i32,
  /// Enables the Play button
  pub is_challenge: i32,
}

#[derive(Debug, Clone, Copy, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum MissionKind {
  Beginner = 1,
  Daily = 2,
  Event = 3,
  Normal = 4,
}

#[derive(Debug, Deserialize)]
pub struct MissionListRequest {
  /// "all"
  #[serde(rename = "type")]
  pub kind: String,
}

/// The Missions screen list: Normal, Daily and Beginner missions with their progress, then
/// event missions (no progress yet).
fn mission_entries(progress: &MissionState, missions: &[Value], event_missions: &[&Value]) -> Vec<Mission> {
  let kind_of = |mission: &Value| match mission["mission_type"].as_str().unwrap_or("") {
    "DAILY" => MissionKind::Daily as i32,
    "BEGINNER" => MissionKind::Beginner as i32,
    "EVENT" => MissionKind::Event as i32,
    _ => MissionKind::Normal as i32,
  };
  let mut list = missions
    .iter()
    .map(|mission| Mission {
      mission_id: mission_progress::int(mission, "mission_id"),
      kind: kind_of(mission),
      progress: progress.progress(mission),
      received: progress.is_received(mission) as i32,
      newmisson: 0,
      is_challenge: 1,
    })
    .collect::<Vec<_>>();
  // Missions of the running event, shown in its own list
  list.extend(event_missions.iter().map(|mission| Mission {
    mission_id: mission_progress::int(mission, "mission_id"),
    kind: MissionKind::Event as i32,
    progress: progress.progress(mission),
    received: progress.is_received(mission) as i32,
    newmisson: 0,
    is_challenge: 1,
  }));
  list
}

pub async fn mission_list(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<MissionListRequest>,
) -> impl IntoHandlerResponse {
  let client = state.get_database_client().await?;
  let progress = MissionState::load(&client, session.user_id).await?;

  let masters = get_masters().await;
  let missions: Vec<Value> = serde_json::from_str(&masters["mission"].master_decompressed).unwrap();
  let event_missions = mission_progress::active_event_missions();

  let list = mission_entries(&progress, &missions, &event_missions);
  let receivable = missions
    .iter()
    .filter(|mission| !progress.is_received(mission))
    .filter(|mission| mission_progress::target(mission).is_some_and(|target| progress.progress(mission) >= target))
    .count();
  info!(kind = %params.kind, receivable, "mission list");

  Ok(Signed(MissionList { missions: list }, session))
}

#[derive(Debug, Serialize)]
pub struct BattleQuestInfo {
  pub ticket: i32,
  pub opflag: i32,
  pub symbol: i32,
  pub boss: Boss,
}

#[derive(Debug, Serialize)]
pub struct Boss {
  pub quest_id: i32,
  pub status: i32,
  pub kill: i32,
}

impl CallCustom for BattleQuestInfo {}

#[derive(Debug, Deserialize)]
pub struct BattleQuestInfoRequest {
  pub event_id: i32,
}

pub async fn battle_quest_info(
  session: Arc<Session>,
  Params(params): Params<BattleQuestInfoRequest>,
) -> impl IntoHandlerResponse {
  warn!(?params, "encountered stub: battle_quest_info");

  Ok(Signed(
    BattleQuestInfo {
      ticket: 0,
      opflag: 0,
      symbol: 0,
      boss: Boss {
        quest_id: 0,
        status: 0,
        kill: 0,
      },
    },
    session,
  ))
}

#[derive(Debug, Serialize)]
pub struct BattleMarathonInfo {
  pub opflag: i32,
  pub boss: Boss,
  pub total_boss_info: TotalBossInfo,
}

// See [Wonder_Api_TotalBossInfoResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct TotalBossInfo {
  pub total_defeat_count: i32,
  pub my_defeat_count: i32,
  pub boss_count_rewards: Vec<BossCountRewards>,
  pub ranking: i32,
  pub in_ranking_period: bool,
}

// See [Wonder_Api_BattlemarathoninfoBossCountRewardsResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct BossCountRewards {
  pub count_id: i32,
  pub is_received: bool,
}

impl CallCustom for BattleMarathonInfo {}

#[derive(Debug, Deserialize)]
pub struct BattleMarathonInfoRequest {
  pub event_id: i32,
}

/// Boss defeats of an event (single and multi boss stages). This server has one player, so the
/// total is the player's own count. Boss count rewards (`event_boss_count`) are not sent yet.
async fn total_boss_info(state: &AppState, session: &Session, event_id: i32) -> anyhow::Result<TotalBossInfo> {
  let client = state.get_database_client().await?;
  let progress = crate::api::quest::progress::load_progress(&client, session.user_id).await?;
  let event_id = event_id.to_string();
  let defeats: i32 = ["event_marathon_quest_stage_boss_single", "event_marathon_quest_stage_boss_multi"]
    .into_iter()
    .flat_map(|master| get_master_manager().get_master(master).iter())
    .filter(|boss| boss["event_id"].as_str() == Some(event_id.as_str()))
    .filter_map(|boss| boss["id"].as_str().and_then(|id| id.parse::<i32>().ok()))
    .map(|quest_id| progress.get(&quest_id).map_or(0, |progress| progress.clear_count))
    .sum();
  Ok(TotalBossInfo {
    total_defeat_count: defeats,
    my_defeat_count: defeats,
    boss_count_rewards: vec![],
    ranking: 0,
    in_ranking_period: false,
  })
}

pub async fn battle_marathon_info(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<BattleMarathonInfoRequest>,
) -> impl IntoHandlerResponse {
  warn!(?params, "encountered stub: battle_marathon_info");
  let total_boss_info = total_boss_info(&state, &session, params.event_id).await?;

  Ok(Signed(
    BattleMarathonInfo {
      opflag: 0,
      boss: Boss {
        quest_id: 0,
        status: 0,
        kill: 0,
      },
      total_boss_info,
    },
    session,
  ))
}

// See [Wonder_Api_MarathonInfoResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct MarathonInfo {
  pub opflag: i32,
  /// 0 - not available, 1 - available
  pub boss: i32,
  pub open_scorechallenge: bool,
  pub multi_battle_invitation: Option<MultiBattleInvitationRoom>,
  pub total_boss_info: TotalBossInfo,
  pub emergency_boss_info: EmergencyBossInfo,
}

// See [Wonder_Api_MultiBattleInvitationRoomResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct MultiBattleInvitationRoom {
  pub room_no: i32,
  pub quest_id: i32,
  pub user_icon: i64,
  #[serde(with = "crate::string_as_base64")]
  pub user_name: String,
}

// See [Wonder_Api_EmergencyBossInfoResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct EmergencyBossInfo {
  pub emergency_boss_id: i32,
  pub status: i32,
  pub total_defeat_count: i32,
  pub my_defeat_count: i32,
  pub ranking: i32,
}

impl CallCustom for MarathonInfo {}

#[derive(Debug, Deserialize)]
pub struct MarathonInfoRequest {
  pub event_id: i32,
  #[serde(with = "crate::bool_as_int")]
  pub display_multi_battle_invitation: bool,
}

pub async fn marathon_info(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<MarathonInfoRequest>,
) -> impl IntoHandlerResponse {
  info!(?params, "marathon_info");
  let total_boss_info = total_boss_info(&state, &session, params.event_id).await?;
  Ok(Signed(
    MarathonInfo {
      opflag: 0,
      boss: 1,
      open_scorechallenge: true,
      multi_battle_invitation: None,
      total_boss_info,
      emergency_boss_info: EmergencyBossInfo {
        emergency_boss_id: 0,
        status: 0,
        total_defeat_count: 0,
        my_defeat_count: 0,
        ranking: 0,
      },
    },
    session,
  ))
}

// See [Wonder_Api_MarathonStageListResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct MarathonStageList {
  pub quests: Vec<MarathonStageQuest>,
}

// See [Wonder_Api_MarathonStageListQuestsResponseDto_Fields]
// See [Wonder_Api_BattlemarathonstagelistQuestsResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct MarathonStageQuest {
  pub quest_id: i32,
  /// 0 - new, 1 - played, 2 - completed, 3 - 100% completed (locked stages are not listed)
  pub status: i32,
  pub task1: i32,
  pub task2: i32,
  pub task3: i32,
  pub hardnum: i32,
}

impl CallCustom for MarathonStageList {}

#[derive(Debug, Deserialize)]
pub struct MarathonStageListRequest {
  pub event_id: i32,
}

pub async fn marathon_stage_list(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<MarathonStageListRequest>,
) -> impl IntoHandlerResponse {
  use crate::api::quest::progress::{load_progress, stage_unlocked};
  use crate::api::quest::stage::stage_info;
  let client = state.get_database_client().await?;
  let progress = load_progress(&client, session.user_id).await?;
  let now = chrono::Utc::now();
  let event_id = params.event_id.to_string();
  let quests = get_master_manager()
    .get_master("event_marathon_quest_stage")
    .iter()
    .filter(|quest| quest["event_id"].as_str() == Some(event_id.as_str()))
    // Locked stages are left out, as in the main quest list
    .filter(|quest| {
      stage_info(mission_progress::int(quest, "id")).is_none_or(|stage| stage_unlocked(stage, &progress, now))
    })
    .map(|quest| {
      let quest_id = mission_progress::int(quest, "id");
      let entry = progress.get(&quest_id);
      let tasks = entry.map(|entry| entry.tasks).unwrap_or_default();
      MarathonStageQuest {
        quest_id,
        // 0 new, 1 played, 2 cleared, 3 cleared with all crowns
        status: crate::api::quest::progress::quest_status(entry),
        task1: tasks[0] as i32,
        task2: tasks[1] as i32,
        task3: tasks[2] as i32,
        hardnum: entry.map(|entry| entry.daily_count).unwrap_or(0),
      }
    })
    .collect::<Vec<_>>();

  Ok(Signed(MarathonStageList { quests }, session))
}

// quest_id=514012
// party_no=1
// auto_progression_info={"is_start":false,"stop_setting":0,"incomplete_setting":0}
// event_id=24013
pub async fn marathon_quest_start(
  state: Arc<AppState>,
  session: Arc<Session>,
  request: ApiRequest,
) -> impl IntoHandlerResponse {
  let quest_id = request.body["quest_id"].parse::<i32>().unwrap();
  let party_no = request.body["party_no"].parse::<i32>().unwrap();
  let auto_progression_info: Value = serde_json::from_str(&request.body["auto_progression_info"])?;
  let event_id: Value = serde_json::from_str(&request.body["event_id"])?;

  Ok(battle::make_battle_start(&state, &session, party_no, Some((quest_id, 1))).await)
}

// See [Wonder_Api_MarathonQuestResultRequest_Fields]
#[derive(Debug, Deserialize)]
pub struct MarathonQuestResultRequest {
  pub quest_id: i32,
  #[serde(rename = "party_no")]
  pub party_id: i32,
  pub win: i32,
  pub wave: i32,
  pub clearquestmission: Vec<i32>,
  pub auto_progression_stop: bool,
  pub memcheckcount: i32,
  pub event_id: i32,
}

pub async fn marathon_quest_result(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<MarathonQuestResultRequest>,
)  -> impl IntoHandlerResponse {
  let outcome = crate::api::quest::finish::finish_quest_battle(
    &state,
    &session,
    params.quest_id,
    params.party_id,
    params.win != 0,
    &params.clearquestmission,
  )
  .await?;
  let mut response = CallResponse::new_success(Box::new(BattleResultResponse {
    limit: outcome.limit,
    exp: outcome.exp,
    lvup: outcome.lvup,
    money: outcome.money,
    storyunlock: vec![],
    love: outcome.love,
    member_exp: outcome.member_exp,
    mission: params.clearquestmission,
    reward: outcome.reward,
    clearreward: outcome.clearreward,
    auto_progression_result: AutoProgressionResultResponse {
      auto_count: 0,
      is_continue: false,
      stop_reason: 0,
      stamina_all: 0,
      reward_all: vec![],
      clearreward_all: vec![],
    },
    firstclear: outcome.firstclear,
  }));
  response.remote.extend(outcome.remote);
  Ok(Unsigned(response))
}

// See [Wonder_Api_MarathonBossListResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct MarathonBossList {
  #[serde(rename = "boss")]
  pub bosses: Vec<MarathonBoss>,
}

// See [Wonder_Api_MarathonBossListBossResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct MarathonBoss {
  pub quest_id: i32,
  pub status: i32,
  pub kill: i32,
}

// See [Wonder_Api_BattlemarathonbosslistBossResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct BattleMarathonBoss {
  pub quest_id: i32,
  pub hp1: i32,
  pub hp2: i32,
  pub hp3: i32,
  /// 0 - locked, 1 - unlocked, 2 - defeated
  pub status: i32,
  /// Wins
  pub kill: i32,
  pub limit_num: i32,
  pub display: i32,
  pub ticket_ratio: i32,
}

impl CallCustom for MarathonBossList {}

#[derive(Debug, Deserialize)]
pub struct MarathonBossListRequest {
  pub event_id: i32,
  #[serde(with = "crate::bool_as_int")]
  pub is_multi: bool,
}

pub async fn marathon_boss_list(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<MarathonBossListRequest>,
) -> impl IntoHandlerResponse {
  let client = state.get_database_client().await?;
  let progress = crate::api::quest::progress::load_progress(&client, session.user_id).await?;

  let bosses = if params.is_multi {
    get_master_manager().get_master("event_marathon_quest_stage_boss_multi")
  } else {
    get_master_manager().get_master("event_marathon_quest_stage_boss_single")
  };
  let bosses = bosses
    .into_iter()
    .filter(|boss| boss.get("event_id").unwrap().as_str().unwrap().parse::<i32>().unwrap() == params.event_id)
    .map(|boss| {
      let quest_id = boss.get("id").unwrap().as_str().unwrap().parse::<i32>().unwrap();
      MarathonBoss {
        quest_id,
        status: 1,
        // Defeated at least once
        kill: progress.get(&quest_id).is_some_and(|progress| progress.clear_count > 0) as i32,
      }
    })
    .collect::<Vec<_>>();

  Ok(Signed(MarathonBossList { bosses }, session))
}

#[derive(Debug, Deserialize)]
pub struct MissionGetRequest {
  #[serde(rename = "missionid")]
  pub mission_id: i32,
}

// See [Wonder_Api_MissiongetResponseDto_Fields] (guessed: same goods lists as panel missions)
#[derive(Debug, Serialize)]
pub struct MissionGetResponse {
  #[serde(rename = "missionid")]
  pub mission_id: i32,
  #[serde(rename = "goodsid")]
  pub goods_id: Vec<crate::api::panel_mission::MissionGood>,
  #[serde(rename = "unreceived_goodsid")]
  pub unreceived_goods_id: Vec<crate::api::panel_mission::MissionGood>,
}

impl CallCustom for MissionGetResponse {}

impl MissionGetResponse {
  fn empty() -> Self {
    MissionGetResponse { mission_id: 0, goods_id: vec![], unreceived_goods_id: vec![] }
  }
}

// missionid=31220001
/// "Accept" on a completed mission: gives its pack and marks it received.
pub async fn mission_get(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<MissionGetRequest>,
) -> impl IntoHandlerResponse {
  let mut client = state.get_database_client().await?;
  let Some(mission) = mission_progress::mission_row(params.mission_id) else {
    warn!(?params, "unknown mission");
    return Ok(Signed(CallResponse::new_success(Box::new(MissionGetResponse::empty())), session));
  };
  let progress = MissionState::load(&client, session.user_id).await?;
  let value = progress.progress(mission);
  let target = mission_progress::target(mission);
  if progress.is_received(mission) || !progress.is_unlocked(mission) || target.is_some_and(|target| value < target) {
    warn!(?params, value, ?target, received = progress.is_received(mission), "mission not receivable");
    return Ok(Signed(CallResponse::new_success(Box::new(MissionGetResponse::empty())), session));
  }

  let pack_id = mission["pack_id"].as_str().unwrap_or("0");
  let items = crate::api::exchange::get_pack(pack_id)
    .map(crate::api::exchange::pack_contents)
    .unwrap_or_default();
  let transaction = client.transaction().await.context("failed to start transaction")?;
  let mut wallet = Wallet::load(&transaction, session.user_id).await?;
  let mut remote = grant_items(&transaction, session.user_id, &mut wallet, &items).await?;
  #[rustfmt::skip]
  transaction.execute(/* language=postgresql */ r#"
    insert into user_missions (user_id, mission_id) values ($1, $2)
    on conflict (user_id, mission_id) do update set received_at = now()
  "#, &[&session.user_id, &params.mission_id]).await.context("failed to save mission")?;
  wallet.save(&transaction, session.user_id).await?;
  transaction.commit().await.context("failed to commit transaction")?;
  remote.extend(wallet.to_remote_data());
  info!(mission_id = params.mission_id, pack_id, ?items, "mission received");

  let mut response = CallResponse::new_success(Box::new(MissionGetResponse {
    mission_id: params.mission_id,
    goods_id: items
      .iter()
      .map(|&(item_type, item_id, item_num)| crate::api::panel_mission::MissionGood { item_type, item_id, item_num })
      .collect(),
    unreceived_goods_id: vec![],
  }));
  response.remote.extend(remote);
  Ok(Signed(response, session))
}

// See [Wonder_Api_MissiongetallResponseDto_Fields] (field names guessed from the metadata strings)
#[derive(Debug, Serialize)]
pub struct MissionGetAllResponse {
  pub list: Vec<Mission>,
  #[serde(rename = "goodsid")]
  pub goods_id: Vec<crate::api::panel_mission::MissionGood>,
  #[serde(rename = "unreceived_goodsid")]
  pub unreceived_goods_id: Vec<crate::api::panel_mission::MissionGood>,
  #[serde(rename = "expired_goodsid")]
  pub expired_goods_id: Vec<crate::api::panel_mission::MissionGood>,
}

impl CallCustom for MissionGetAllResponse {}

/// "Accept All": receives every completed mission (of the tab's type, when the request says
/// which). The request fields are not known yet, so they are read loosely and logged.
pub async fn mission_get_all(
  state: Arc<AppState>,
  request: ApiRequest,
  session: Arc<Session>,
) -> impl IntoHandlerResponse {
  let body = &request.body;
  info!(?body, "missiongetall");
  let kind = body.get("type").and_then(|value| value.parse::<i32>().ok()).filter(|kind| *kind > 0);
  // The game sends the missions it shows as receivable, from every tab
  let requested = body
    .get("mission_id_list")
    .and_then(|list| serde_json::from_str::<std::collections::HashSet<i32>>(list).ok());

  let mut client = state.get_database_client().await?;
  let mut progress = MissionState::load(&client, session.user_id).await?;
  let masters = get_masters().await;
  let missions: Vec<Value> = serde_json::from_str(&masters["mission"].master_decompressed).unwrap();
  let kind_of = |mission: &Value| match mission["mission_type"].as_str().unwrap_or("") {
    "DAILY" => MissionKind::Daily as i32,
    "BEGINNER" => MissionKind::Beginner as i32,
    _ => MissionKind::Normal as i32,
  };
  // Receiving a mission can unlock the next one of its chain (enabled_id) or complete an "all"
  // mission: repeat until nothing new is ready, like pressing the button again. Chains can have
  // hundreds of steps, so after the first pass only the next steps and "all" missions are checked.
  let wanted = |mission: &&Value| {
    kind.is_none_or(|kind| kind_of(mission) == kind)
      && requested
        .as_ref()
        .is_none_or(|requested| requested.contains(&mission_progress::int(mission, "mission_id")))
  };
  let is_all = |mission: &Value| mission["rule"].as_str().is_some_and(|rule| rule.starts_with("all_"));
  let pool = missions.iter().filter(wanted).collect::<Vec<_>>();
  let mut next_steps: std::collections::HashMap<i32, Vec<&Value>> = std::collections::HashMap::new();
  for mission in &pool {
    next_steps.entry(mission_progress::int(mission, "enabled_id")).or_default().push(mission);
  }
  let all_missions = pool.iter().copied().filter(|mission| is_all(mission)).collect::<Vec<_>>();
  let mut items = Vec::new();
  let mut ids = Vec::new();
  let mut candidates = pool.clone();
  while !candidates.is_empty() {
    let ready = candidates
      .into_iter()
      .filter(|mission| !progress.is_received(mission) && progress.is_unlocked(mission))
      .filter(|mission| {
        mission_progress::target(mission).is_some_and(|target| progress.progress(mission) >= target)
      })
      .collect::<Vec<_>>();
    let mut received_now = std::collections::HashSet::new();
    for mission in ready {
      let mission_id = mission_progress::int(mission, "mission_id");
      progress.mark_received(mission_id);
      received_now.insert(mission_id);
      ids.push(mission_id);
      let pack_id = mission["pack_id"].as_str().unwrap_or("0");
      items.extend(
        crate::api::exchange::get_pack(pack_id)
          .map(crate::api::exchange::pack_contents)
          .unwrap_or_default(),
      );
    }
    if received_now.is_empty() {
      break;
    }
    candidates = received_now
      .iter()
      .flat_map(|id| next_steps.get(id).into_iter().flatten().copied())
      .chain(all_missions.iter().copied())
      .collect();
  }

  let transaction = client.transaction().await.context("failed to start transaction")?;
  let mut wallet = Wallet::load(&transaction, session.user_id).await?;
  let mut remote = grant_items(&transaction, session.user_id, &mut wallet, &items).await?;
  #[rustfmt::skip]
  transaction.execute(/* language=postgresql */ r#"
    insert into user_missions (user_id, mission_id) select $1, unnest($2::int4[])
    on conflict (user_id, mission_id) do update set received_at = now()
  "#, &[&session.user_id, &ids]).await.context("failed to save missions")?;
  wallet.save(&transaction, session.user_id).await?;
  transaction.commit().await.context("failed to commit transaction")?;
  remote.extend(wallet.to_remote_data());
  info!(?kind, missions = ?ids, ?items, "missions received (all)");

  let progress = MissionState::load(&client, session.user_id).await?;
  let event_missions = mission_progress::active_event_missions();
  let mut response = CallResponse::new_success(Box::new(MissionGetAllResponse {
    list: mission_entries(&progress, &missions, &event_missions),
    goods_id: items
      .iter()
      .map(|&(item_type, item_id, item_num)| crate::api::panel_mission::MissionGood { item_type, item_id, item_num })
      .collect(),
    unreceived_goods_id: vec![],
    expired_goods_id: vec![],
  }));
  response.remote.extend(remote);
  Ok(Signed(response, session))
}

/// The recommended mission cards: one per active event and the home screen's one. For events
/// ("event_mission_recommend") the client shows the first row, by priority, that has a notification
/// of type 10 (key = recommend_id, value = mission_id), and "Done" when none has. So each event gets one for its highest-priority
/// mission not received yet, and none once all are received.
pub async fn recommend_event_notifications(
  client: &deadpool_postgres::Client,
  user_id: UserId,
) -> anyhow::Result<Vec<NotificationData>> {
  #[rustfmt::skip]
  let received: std::collections::HashSet<i32> = client.query(/* language=postgresql */ r#"
    select mission_id from user_missions where user_id = $1
  "#, &[&user_id]).await.context("failed to read received missions")?
    .iter().map(|row| row.get(0)).collect();
  let int = |row: &serde_json::Value, key: &str| row[key].as_str().and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
  let rows = get_master_manager().get_master("event_mission_recommend");
  let mut notifications = Vec::new();
  for event_id in crate::api::master_all::active_event_ids() {
    let mut candidates = rows
      .iter()
      .filter(|row| row["event_id"].as_str() == Some(event_id.as_str()))
      .filter(|row| !received.contains(&int(row, "mission_id")))
      .collect::<Vec<_>>();
    candidates.sort_by_key(|row| std::cmp::Reverse(int(row, "priority")));
    if let Some(row) = candidates.first() {
      notifications.push(NotificationData::new(
        1,
        10,
        int(row, "recommend_id"),
        int(row, "mission_id"),
        String::new(),
        String::new(),
      ));
    }
  }
  // The home screen's recommended mission ("mission_recommend"), type 6, works the same way
  let mut home = get_master_manager()
    .get_master("mission_recommend")
    .iter()
    .filter(|row| !received.contains(&int(row, "mission_id")))
    .collect::<Vec<_>>();
  home.sort_by_key(|row| std::cmp::Reverse(int(row, "priority")));
  if let Some(row) = home.first() {
    notifications.push(NotificationData::new(1, 6, int(row, "recommend_id"), int(row, "mission_id"), String::new(), String::new()));
  }
  Ok(notifications)
}
