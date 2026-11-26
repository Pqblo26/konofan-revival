//! Fame Bonus: the potions Fame Quests drop, the fame ranks they raise and the Fame Traits.
//!
//! - After a Fame Quest each front member has a [fame_bonus_incidence]% chance (or a sure one per
//!   Fame Supplement used) of a potion with three choices: +1 rank in a stat (HP, P. Atk, M. Atk,
//!   P. Def, M. Def) or a Fame Trait ([skill_pa_fame]).
//! - The player picks one; the client sends the member's new ranks and the server keeps them.
//! - Fame ranks only count in Fame Quests (the game shows them in brackets).
//! - Traits go to an inventory of up to [pa_skill_fame_possession_limit] and are set in the party
//!   form's trait slot.
//!
//! Needs the `user_member_fame`, `user_skill_pa_fame`, `user_fame_potions` and
//! `user_party_form_fame` tables. Without them (migration not applied yet) everything here does
//! nothing, so the rest of the game keeps working.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Context;
use rand::Rng;
use serde::Serialize;
use serde_json::Value;
use tracing::{info, warn};

use crate::api::master_all::get_master_manager;
use crate::api::quest::quest_fame::{FameQuestLotteryPotion, FameQuestLotteryPotionChoice};
use crate::api::{ApiRequest, RemoteData, RemoteDataItemType, SkillPaFame, SkillPaFameAddStatus};
use crate::blob::{IntoRemoteData, UpdateMember};
use crate::call::{CallCustom, CallResponse};
use crate::handler::{IntoHandlerResponse, Unsigned};
use crate::item::UpdateItemCountBy;
use crate::member::{FetchUserMemberSkillsIn, FetchUserMembersIn, Member, MemberPrototype};
use crate::user::id::UserId;
use crate::user::session::Session;
use crate::AppState;

/// Potion type of a Fame Trait choice (the stats are 1-5).
const TRAIT: i32 = 99;
/// Fame Supplement and Fame Scroll item ids (`fame_supplement_id`, `fame_scroll_id`).
const SUPPLEMENT_ID: i64 = 1;
const SCROLL_ID: i64 = 1;

fn system_value(key: &str, default: i64) -> i64 {
  get_master_manager()
    .get_master("system")
    .iter()
    .find(|row| row["key"].as_str() == Some(key))
    .and_then(|row| row["value"].as_str()?.parse().ok())
    .unwrap_or(default)
}

fn field(row: &Value, key: &str) -> i64 {
  row[key].as_str().and_then(|value| value.parse().ok()).unwrap_or(0)
}

// 0 = not checked, 1 = tables there, 2 = missing
static TABLES: AtomicU8 = AtomicU8::new(0);

/// Whether the Fame migration has been applied. Safe inside a transaction.
pub async fn tables_ready(client: &tokio_postgres::Client) -> bool {
  match TABLES.load(Ordering::Relaxed) {
    1 => return true,
    2 => return false,
    _ => {}
  }
  let ready = match client
    .query_one("select to_regclass('user_party_form_fame') is not null", &[])
    .await
  {
    Ok(row) => row.get::<_, bool>(0),
    Err(error) => {
      warn!(?error, "could not check the fame tables");
      return false;
    }
  };
  if !ready {
    warn!("fame tables missing (migration 20261126000001 not applied): fame bonus disabled");
  }
  TABLES.store(if ready { 1 } else { 2 }, Ordering::Relaxed);
  ready
}

fn parse_status(text: &str) -> Vec<SkillPaFameAddStatus> {
  serde_json::from_str::<Vec<(i32, i32)>>(text)
    .unwrap_or_default()
    .into_iter()
    .map(|(kind, value)| SkillPaFameAddStatus { kind, value })
    .collect()
}

fn status_text(status: &[SkillPaFameAddStatus]) -> String {
  serde_json::to_string(&status.iter().map(|s| (s.kind, s.value)).collect::<Vec<_>>()).unwrap()
}

/// Fills the fame ranks and the traits each member found (members fetched from the database).
pub async fn fill_member_fame(client: &tokio_postgres::Client, user_id: UserId, members: &mut [Member]) {
  if members.is_empty() || !tables_ready(client).await {
    return;
  }
  if let Err(error) = fill_member_fame_impl(client, user_id, members).await {
    warn!(?error, "failed to load member fame");
  }
}

async fn fill_member_fame_impl(
  client: &tokio_postgres::Client,
  user_id: UserId,
  members: &mut [Member],
) -> anyhow::Result<()> {
  #[rustfmt::skip]
  let ranks = client.query(/* language=postgresql */ r#"
    select member_id, hp, attack, magicattack, defense, magicdefence
    from user_member_fame where user_id = $1
  "#, &[&user_id]).await?;
  let ranks: HashMap<i64, [i32; 5]> = ranks
    .into_iter()
    .map(|row| (row.get(0), [row.get(1), row.get(2), row.get(3), row.get(4), row.get(5)]))
    .collect();
  #[rustfmt::skip]
  let traits = client.query(/* language=postgresql */ r#"
    select id, member_id, skill_pa_fame_id, add_status
    from user_skill_pa_fame where user_id = $1 order by id
  "#, &[&user_id]).await?;
  let mut by_member: HashMap<i64, Vec<SkillPaFame>> = HashMap::new();
  for row in traits {
    let id: i64 = row.get(0);
    let status: String = row.get(3);
    by_member.entry(row.get(1)).or_default().push(SkillPaFame {
      skill_pa_fame_id: row.get(2),
      user_skill_pa_fame_id: id as i32,
      add_status_list: parse_status(&status),
    });
  }
  for member in members.iter_mut() {
    if let Some([hp, attack, magicattack, defense, magicdefence]) = ranks.get(&member.prototype.id) {
      member.fame_stats.fame_hp = *hp;
      member.fame_stats.fame_attack = *attack;
      member.fame_stats.fame_magicattack = *magicattack;
      member.fame_stats.fame_defense = *defense;
      member.fame_stats.fame_magicdefence = *magicdefence;
    }
    if let Some(list) = by_member.remove(&member.prototype.id) {
      member.skill_pa_fame_list = list;
    }
  }
  Ok(())
}

/// Stats a member's fame ranks add in a Fame Quest: hp, attack, magicattack, defense, magicdefence.
pub fn fame_quest_bonus(member: &Member) -> [i32; 5] {
  let stats = &member.fame_stats;
  let hp = system_value("fame_quest_status_hp", 100) as i32;
  let attack = system_value("fame_quest_status_attack", 20) as i32;
  let magicattack = system_value("fame_quest_status_magicattack", 20) as i32;
  let defense = system_value("fame_quest_status_defense", 20) as i32;
  let magicdefense = system_value("fame_quest_status_magicdefense", 20) as i32;
  [
    stats.fame_hp * hp,
    stats.fame_attack * attack,
    stats.fame_magicattack * magicattack,
    stats.fame_defense * defense,
    stats.fame_magicdefence * magicdefense,
  ]
}

/// The trait each form of these parties has set (party id, form id) -> user trait id.
pub async fn party_traits(client: &tokio_postgres::Client, user_id: UserId) -> HashMap<(i64, i64), i64> {
  if !tables_ready(client).await {
    return HashMap::new();
  }
  #[rustfmt::skip]
  let rows = client.query(/* language=postgresql */ r#"
    select party_id, form_id, user_skill_pa_fame_id from user_party_form_fame where user_id = $1
  "#, &[&user_id]).await;
  match rows {
    Ok(rows) => rows.into_iter().map(|row| ((row.get(0), row.get(1)), row.get(2))).collect(),
    Err(error) => {
      warn!(?error, "failed to load party fame traits");
      HashMap::new()
    }
  }
}

/// Sets the trait of a party form (0 takes it off).
pub async fn set_party_trait(
  client: &tokio_postgres::Client,
  user_id: UserId,
  party_id: i64,
  form_id: i64,
  user_skill_pa_fame_id: i64,
) -> anyhow::Result<()> {
  if !tables_ready(client).await {
    return Ok(());
  }
  if user_skill_pa_fame_id <= 0 {
    #[rustfmt::skip]
    client.execute(/* language=postgresql */ r#"
      delete from user_party_form_fame where user_id = $1 and party_id = $2 and form_id = $3
    "#, &[&user_id, &party_id, &form_id]).await?;
  } else {
    #[rustfmt::skip]
    client.execute(/* language=postgresql */ r#"
      insert into user_party_form_fame (user_id, party_id, form_id, user_skill_pa_fame_id)
      values ($1, $2, $3, $4)
      on conflict (user_id, party_id, form_id) do update set user_skill_pa_fame_id = excluded.user_skill_pa_fame_id
    "#, &[&user_id, &party_id, &form_id, &user_skill_pa_fame_id]).await?;
  }
  info!(party_id, form_id, user_skill_pa_fame_id, "fame trait set");
  Ok(())
}

// --- Potions ---

/// Fame Supplements used and runs (cost ratio) of the Fame Quest each player is fighting.
fn pending_runs() -> &'static Mutex<HashMap<UserId, (i32, i32)>> {
  static RUNS: OnceLock<Mutex<HashMap<UserId, (i32, i32)>>> = OnceLock::new();
  RUNS.get_or_init(Default::default)
}

/// Takes the Fame Supplements used to start a Fame Quest and remembers them for the result.
pub async fn start_fame_quest(
  client: &tokio_postgres::Client,
  user_id: UserId,
  supplements: i32,
  runs: i32,
) -> anyhow::Result<Vec<RemoteData>> {
  let mut remote = Vec::new();
  let mut used = 0;
  if supplements > 0 && tables_ready(client).await {
    #[rustfmt::skip]
    let owned: i32 = client.query_opt(/* language=postgresql */ r#"
      select quantity from user_items where user_id = $1 and item_type = $2 and item_id = $3
    "#, &[&user_id, &(i32::from(RemoteDataItemType::FamePotion) as i64), &SUPPLEMENT_ID]).await?
      .map(|row| row.get(0))
      .unwrap_or(0);
    used = supplements.min(owned.max(0));
    if used > 0 {
      let update = UpdateItemCountBy::new(client).await?;
      remote.extend(
        update
          .run(user_id, (RemoteDataItemType::FamePotion, SUPPLEMENT_ID), -used)
          .await?
          .into_remote_data(),
      );
    }
    if used != supplements {
      warn!(supplements, owned, "not enough fame supplements");
    }
  }
  pending_runs().lock().unwrap().insert(user_id, (used, runs.max(1)));
  info!(supplements = used, runs, "fame quest started");
  Ok(remote)
}

fn quest_rank(stage_id: i32) -> i64 {
  let masters = get_master_manager();
  let area_id = masters
    .get_master("fame_quest_stage")
    .iter()
    .find(|row| field(row, "id") == stage_id as i64)
    .map(|row| field(row, "area_id"))
    .unwrap_or(0);
  masters
    .get_master("fame_quest_area")
    .iter()
    .find(|row| field(row, "id") == area_id)
    .map(|row| field(row, "quest_rank_id"))
    .unwrap_or(1)
    .max(1)
}

fn weighted<T: Copy>(items: &[(T, i64)]) -> Option<T> {
  let total: i64 = items.iter().map(|(_, weight)| weight.max(&0)).sum();
  if total <= 0 {
    return None;
  }
  let mut roll = rand::rng().random_range(0..total);
  for (item, weight) in items {
    if roll < *weight {
      return Some(*item);
    }
    roll -= weight.max(&0);
  }
  None
}

/// A random trait for a member, with the rarity odds of the quest rank.
fn draw_trait(rank: i64, character_id: i64) -> Option<i64> {
  let masters = get_master_manager();
  let rare_odds: Vec<(i64, i64)> = masters
    .get_master("skill_pa_fame_lot_rare")
    .iter()
    .filter(|row| field(row, "quest_rank_id") == rank)
    .map(|row| (field(row, "rare"), field(row, "weight")))
    .collect();
  let rare = weighted(&rare_odds).unwrap_or(1);
  let skills: Vec<(i64, i64)> = masters
    .get_master("skill_pa_fame")
    .iter()
    .filter(|row| field(row, "rare") == rare)
    .filter(|row| matches!(field(row, "chara_id"), 0) || field(row, "chara_id") == character_id)
    .map(|row| (field(row, "skill_id"), 1))
    .collect();
  weighted(&skills)
}

/// Three different choices for a member's potion.
fn draw_choices(rank: i64, character_id: i64) -> Vec<FameQuestLotteryPotionChoice> {
  let mut lots: Vec<(i32, i64)> = get_master_manager()
    .get_master("fame_random_potion_lot")
    .iter()
    .map(|row| (field(row, "status") as i32, field(row, "weight")))
    .collect();
  let mut choices = Vec::new();
  while choices.len() < 3 {
    let Some(potion_type) = weighted(&lots) else { break };
    lots.retain(|(kind, _)| *kind != potion_type);
    let choice = if potion_type == TRAIT {
      let Some(skill_id) = draw_trait(rank, character_id) else { continue };
      FameQuestLotteryPotionChoice {
        potion_choice_id: 0,
        potion_type,
        skill_pa_fame_id: skill_id,
        skill_pa_fame_status: vec![],
      }
    } else {
      FameQuestLotteryPotionChoice {
        potion_choice_id: 0,
        potion_type,
        skill_pa_fame_id: 0,
        skill_pa_fame_status: vec![SkillPaFameAddStatus { kind: potion_type, value: 1 }],
      }
    };
    choices.push(choice);
  }
  for (index, choice) in choices.iter_mut().enumerate() {
    choice.potion_choice_id = index as i32 + 1;
  }
  choices
}

/// Draws the potions of a finished Fame Quest for its front members and keeps them until the
/// player picks. Returns what the result screen shows.
pub async fn draw_potions(
  client: &tokio_postgres::Client,
  user_id: UserId,
  stage_id: i32,
  front_members: &[i64],
  win: bool,
) -> Vec<FameQuestLotteryPotion> {
  let (supplements, runs) = pending_runs().lock().unwrap().remove(&user_id).unwrap_or((0, 1));
  if !win || !tables_ready(client).await {
    return vec![];
  }
  let chance = system_value("fame_bonus_incidence", 20);
  let rank = quest_rank(stage_id);
  let mut potions = Vec::new();
  for &member_id in front_members {
    let character_id = MemberPrototype::load_from_id(member_id).character_id;
    for run in 0..runs {
      if run >= supplements && rand::rng().random_range(0..100) >= chance {
        continue;
      }
      let choices = draw_choices(rank, character_id);
      if choices.is_empty() {
        continue;
      }
      let saved = save_potion(client, user_id, member_id, rank, &choices).await;
      match saved {
        Ok(potion_id) => potions.push(FameQuestLotteryPotion {
          potion_id,
          user_member_id: member_id,
          potion_choice_list: choices,
        }),
        Err(error) => warn!(?error, "failed to save fame potion"),
      }
    }
  }
  info!(stage_id, rank, supplements, runs, potions = potions.len(), "fame potions drawn");
  potions
}

async fn save_potion(
  client: &tokio_postgres::Client,
  user_id: UserId,
  member_id: i64,
  rank: i64,
  choices: &[FameQuestLotteryPotionChoice],
) -> anyhow::Result<i32> {
  let choices = serde_json::to_string(
    &choices
      .iter()
      .map(|c| (c.potion_choice_id, c.potion_type, c.skill_pa_fame_id, status_text(&c.skill_pa_fame_status)))
      .collect::<Vec<_>>(),
  )?;
  #[rustfmt::skip]
  let row = client.query_one(/* language=postgresql */ r#"
    insert into user_fame_potions (user_id, member_id, quest_rank, choices)
    values ($1, $2, $3, $4) returning id
  "#, &[&user_id, &member_id, &(rank as i32), &choices]).await?;
  Ok(row.get::<_, i64>(0) as i32)
}

struct PendingPotion {
  id: i32,
  member_id: i64,
  rank: i32,
  choices: Vec<FameQuestLotteryPotionChoice>,
}

fn parse_choices(text: &str) -> Vec<FameQuestLotteryPotionChoice> {
  serde_json::from_str::<Vec<(i32, i32, i64, String)>>(text)
    .unwrap_or_default()
    .into_iter()
    .map(|(potion_choice_id, potion_type, skill_pa_fame_id, status)| FameQuestLotteryPotionChoice {
      potion_choice_id,
      potion_type,
      skill_pa_fame_id,
      skill_pa_fame_status: parse_status(&status),
    })
    .collect()
}

async fn pending_potions(client: &tokio_postgres::Client, user_id: UserId) -> anyhow::Result<Vec<PendingPotion>> {
  #[rustfmt::skip]
  let rows = client.query(/* language=postgresql */ r#"
    select id, member_id, quest_rank, choices from user_fame_potions where user_id = $1 order by id
  "#, &[&user_id]).await?;
  Ok(
    rows
      .into_iter()
      .map(|row| {
        let id: i64 = row.get(0);
        let choices: String = row.get(3);
        PendingPotion {
          id: id as i32,
          member_id: row.get(1),
          rank: row.get(2),
          choices: parse_choices(&choices),
        }
      })
      .collect(),
  )
}

// See [Wonder_Api_FamePotionListResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct FamePotionListResponse {
  pub fame_potion_list: Vec<FameQuestLotteryPotion>,
  pub quest_rank: i32,
}

impl CallCustom for FamePotionListResponse {}

/// Potions found but not chosen yet (the game asks for them if it was closed before picking).
pub async fn fame_potion_list(state: Arc<AppState>, session: Arc<Session>, request: ApiRequest) -> impl IntoHandlerResponse {
  info!(body = ?request.body, "fame_potion_list");
  let client = state.get_database_client().await?;
  let potions = if tables_ready(&client).await {
    pending_potions(&client, session.user_id).await?
  } else {
    vec![]
  };
  let quest_rank = potions.iter().map(|p| p.rank).max().unwrap_or(1);
  let fame_potion_list = potions
    .into_iter()
    .map(|potion| FameQuestLotteryPotion {
      potion_id: potion.id,
      user_member_id: potion.member_id,
      potion_choice_list: potion.choices,
    })
    .collect();
  Ok(Unsigned(CallResponse::new_success(Box::new(FamePotionListResponse {
    fame_potion_list,
    quest_rank,
  }))))
}

/// A list sent either as JSON (`[1,2]`) or as a single number.
fn id_list(value: Option<&String>) -> Vec<i64> {
  let Some(value) = value else { return vec![] };
  if let Ok(list) = serde_json::from_str::<Vec<i64>>(value) {
    return list;
  }
  value.parse::<i64>().ok().filter(|id| *id > 0).into_iter().collect()
}

fn number(body: &HashMap<String, String>, key: &str) -> Option<i64> {
  body.get(key)?.parse().ok()
}

/// What the client confirms for one member: chosen potions, new ranks and traits to throw away.
#[derive(Debug, Default)]
struct ConfirmEntry {
  member_id: Option<i64>,
  selected: Vec<(i64, i64)>,
  ranks: [Option<i64>; 5],
  discard: Vec<i64>,
}

const RANK_KEYS: [&str; 5] = ["hp", "attack", "magicattack", "defense", "magicdefence"];

fn json_number(value: &Value) -> Option<i64> {
  value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

fn json_ids(value: &Value) -> Vec<i64> {
  match value {
    Value::Array(list) => list.iter().filter_map(json_number).collect(),
    other => json_number(other).filter(|id| *id > 0).into_iter().collect(),
  }
}

fn entry_from_json(object: &Value) -> ConfirmEntry {
  let mut entry = ConfirmEntry {
    member_id: json_number(&object["user_member_id"]),
    ..Default::default()
  };
  if let Some(list) = object["selected_potion_info"].as_array() {
    for info in list {
      entry.selected.push((
        json_number(&info["potion_id"]).unwrap_or(0),
        json_number(&info["selected_potion_choice_id"]).unwrap_or(0),
      ));
    }
  }
  if let Some(potion_id) = json_number(&object["potion_id"]) {
    entry.selected.push((potion_id, json_number(&object["selected_potion_choice_id"]).unwrap_or(0)));
  }
  for (index, stat) in RANK_KEYS.iter().enumerate() {
    entry.ranks[index] = json_number(&object[format!("fame_{stat}_rank")]);
  }
  entry.discard = json_ids(&object["discard_user_skill_pa_fame_id"]);
  entry
}

/// The request as a JSON object (every parameter, with lists decoded).
fn body_json(body: &HashMap<String, String>) -> Value {
  Value::Object(
    body
      .iter()
      .map(|(key, value)| {
        let decoded = serde_json::from_str::<Value>(value)
          .ok()
          .filter(|v| v.is_array() || v.is_object())
          .unwrap_or_else(|| Value::String(value.clone()));
        (key.clone(), decoded)
      })
      .collect(),
  )
}

fn confirm_entries(body: &HashMap<String, String>) -> Vec<ConfirmEntry> {
  let json = body_json(body);
  let mut entries = Vec::new();
  // A list of FameQuestPotionConfirmInfoRequestDto, whatever the parameter is called
  for value in json.as_object().unwrap().values() {
    for item in value.as_array().into_iter().flatten() {
      if item.get("selected_potion_info").is_some() || item.get("user_member_id").is_some() {
        entries.push(entry_from_json(item));
      }
    }
  }
  // Or the fields straight in the request (FameQuestConfirmPotionRequestDto)
  if body.contains_key("potion_id") || body.contains_key("selected_potion_info") || body.contains_key("user_member_id") {
    entries.push(entry_from_json(&json));
  }
  entries
}

/// `fame_quest_potion_confirm` and `confirm_potion_list`: applies the chosen potions, keeps the
/// ranks the client worked out and throws away the traits it says.
pub async fn fame_quest_potion_confirm(
  state: Arc<AppState>,
  session: Arc<Session>,
  request: ApiRequest,
) -> impl IntoHandlerResponse {
  info!(body = ?request.body, "fame potion confirm");
  let entries = confirm_entries(&request.body);
  if entries.is_empty() {
    warn!("fame potion confirm without anything recognisable");
  }

  let mut client = state.get_database_client().await?;
  if !tables_ready(&client).await {
    warn!("fame potion confirmed without the fame tables");
    return Ok(Unsigned(CallResponse::new_success(Box::new(()))));
  }
  let pending = pending_potions(&client, session.user_id).await?;
  let transaction = client.transaction().await.context("failed to start transaction")?;
  let limit = system_value("pa_skill_fame_possession_limit", 30);
  let cap = system_value("fame_status_bonus_individual_limit", 25) as i32;
  let mut members_touched = Vec::new();

  for entry in entries {
    for user_skill_pa_fame_id in &entry.discard {
      #[rustfmt::skip]
      transaction.execute(/* language=postgresql */ r#"
        delete from user_skill_pa_fame where user_id = $1 and id = $2 and not is_lock
      "#, &[&session.user_id, user_skill_pa_fame_id]).await?;
    }
    let mut member_id = entry.member_id;
    for (potion_id, choice_id) in &entry.selected {
      let Some(potion) = pending.iter().find(|p| p.id as i64 == *potion_id) else {
        warn!(potion_id, "unknown fame potion");
        continue;
      };
      member_id.get_or_insert(potion.member_id);
      match potion.choices.iter().find(|c| c.potion_choice_id as i64 == *choice_id) {
        Some(choice) if choice.potion_type == TRAIT => {
          #[rustfmt::skip]
          let owned: i64 = transaction.query_one(/* language=postgresql */ r#"
            select count(*) from user_skill_pa_fame where user_id = $1 and member_id = $2
          "#, &[&session.user_id, &potion.member_id]).await?.get(0);
          if owned >= limit {
            warn!(owned, limit, "member's fame traits full, trait not kept");
          } else {
            #[rustfmt::skip]
            transaction.execute(/* language=postgresql */ r#"
              insert into user_skill_pa_fame (user_id, member_id, skill_pa_fame_id, add_status)
              values ($1, $2, $3, $4)
            "#, &[&session.user_id, &potion.member_id, &choice.skill_pa_fame_id, &status_text(&choice.skill_pa_fame_status)]).await?;
            info!(skill_pa_fame_id = choice.skill_pa_fame_id, member_id = potion.member_id, "fame trait kept");
          }
        }
        Some(choice) => info!(potion_type = choice.potion_type, member_id = potion.member_id, "fame stat chosen"),
        None => warn!(potion_id, choice_id, "fame potion left without a choice"),
      }
      #[rustfmt::skip]
      transaction.execute(/* language=postgresql */ r#"
        delete from user_fame_potions where user_id = $1 and id = $2
      "#, &[&session.user_id, potion_id]).await?;
    }

    let Some(member_id) = member_id else { continue };
    members_touched.push(member_id);
    if entry.ranks.iter().any(Option::is_some) {
      let ranks: Vec<i32> = entry.ranks.iter().map(|r| (r.unwrap_or(0) as i32).clamp(0, cap)).collect();
      #[rustfmt::skip]
      transaction.execute(/* language=postgresql */ r#"
        insert into user_member_fame (user_id, member_id, hp, attack, magicattack, defense, magicdefence)
        values ($1, $2, $3, $4, $5, $6, $7)
        on conflict (user_id, member_id) do update set
          hp = excluded.hp, attack = excluded.attack, magicattack = excluded.magicattack,
          defense = excluded.defense, magicdefence = excluded.magicdefence, updated_at = now()
      "#, &[&session.user_id, &member_id, &ranks[0], &ranks[1], &ranks[2], &ranks[3], &ranks[4]]).await?;
      info!(member_id, ?ranks, "fame ranks saved");
    }
  }
  #[rustfmt::skip]
  transaction.execute(/* language=postgresql */ r#"
    delete from user_party_form_fame f where f.user_id = $1
      and not exists(select 1 from user_skill_pa_fame s where s.user_id = f.user_id and s.id = f.user_skill_pa_fame_id)
  "#, &[&session.user_id]).await?;
  transaction.commit().await.context("failed to commit transaction")?;

  members_touched.sort();
  members_touched.dedup();
  let mut response = CallResponse::new_success(Box::new(()));
  let mut members = FetchUserMembersIn::new(&client).await?.run(session.user_id, &members_touched).await?;
  FetchUserMemberSkillsIn::new(&client)
    .await?
    .run(session.user_id, &mut members.iter_mut().collect::<Vec<_>>())
    .await?;
  for member in members {
    response.remote.extend(UpdateMember::new(member.to_member_parameter_wire()).into_remote_data());
  }
  Ok(Unsigned(response))
}

// --- Traits ---

// See [Wonder_Api_SkillPaFameListResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SkillPaFameListResponse {
  pub skill_pa_fame_list: Vec<SkillPaFameOwned>,
}

impl CallCustom for SkillPaFameListResponse {}

#[derive(Debug, Serialize)]
pub struct SkillPaFameOwned {
  pub skill_pa_fame_id: i64,
  pub user_skill_pa_fame_id: i64,
  pub is_lock: bool,
  pub is_use: bool,
}

/// The traits of a member (the request carries its id), or all of them.
pub async fn skill_pa_fame_list(state: Arc<AppState>, session: Arc<Session>, request: ApiRequest) -> impl IntoHandlerResponse {
  info!(body = ?request.body, "skill_pa_fame_list");
  let member_id = number(&request.body, "user_member_id")
    .or_else(|| number(&request.body, "member_id"))
    .unwrap_or(0);
  let client = state.get_database_client().await?;
  let mut list = Vec::new();
  if tables_ready(&client).await {
    #[rustfmt::skip]
    let rows = client.query(/* language=postgresql */ r#"
      select s.id, s.skill_pa_fame_id, s.is_lock,
        exists(select 1 from user_party_form_fame f where f.user_id = s.user_id and f.user_skill_pa_fame_id = s.id)
      from user_skill_pa_fame s where s.user_id = $1 and ($2::int8 = 0 or s.member_id = $2::int8) order by s.id
    "#, &[&session.user_id, &member_id]).await?;
    for row in rows {
      list.push(SkillPaFameOwned {
        user_skill_pa_fame_id: row.get(0),
        skill_pa_fame_id: row.get(1),
        is_lock: row.get(2),
        is_use: row.get(3),
      });
    }
  }
  Ok(Unsigned(CallResponse::new_success(Box::new(SkillPaFameListResponse {
    skill_pa_fame_list: list,
  }))))
}

/// Trait ids of a request: `user_skill_pa_fame_id(_list)` or, failing that, any list of numbers.
fn trait_ids(body: &HashMap<String, String>) -> Vec<i64> {
  let mut ids = id_list(body.get("user_skill_pa_fame_id_list"));
  ids.extend(id_list(body.get("user_skill_pa_fame_id")));
  if ids.is_empty() {
    for value in body.values() {
      if let Ok(list) = serde_json::from_str::<Vec<i64>>(value) {
        ids.extend(list);
      }
    }
  }
  ids
}

/// Throws traits away (locked ones stay) and takes them off any party.
pub async fn discard_skill_pa_fame(state: Arc<AppState>, session: Arc<Session>, request: ApiRequest) -> impl IntoHandlerResponse {
  info!(body = ?request.body, "discard_skill_pa_fame");
  let ids = trait_ids(&request.body);
  let client = state.get_database_client().await?;
  if tables_ready(&client).await && !ids.is_empty() {
    #[rustfmt::skip]
    let removed = client.execute(/* language=postgresql */ r#"
      delete from user_skill_pa_fame where user_id = $1 and id = any($2) and not is_lock
    "#, &[&session.user_id, &ids]).await?;
    #[rustfmt::skip]
    client.execute(/* language=postgresql */ r#"
      delete from user_party_form_fame f where f.user_id = $1
        and not exists(select 1 from user_skill_pa_fame s where s.user_id = f.user_id and s.id = f.user_skill_pa_fame_id)
    "#, &[&session.user_id]).await?;
    info!(?ids, removed, "fame traits discarded");
  }
  Ok(Unsigned(CallResponse::new_success(Box::new(()))))
}

/// Locks or unlocks traits (`is_lock` if sent, otherwise it switches).
pub async fn lock_skill_pa_fame(state: Arc<AppState>, session: Arc<Session>, request: ApiRequest) -> impl IntoHandlerResponse {
  info!(body = ?request.body, "lock_skill_pa_fame");
  let ids = trait_ids(&request.body);
  let lock = request.body.get("is_lock").map(|value| value == "1" || value == "true");
  let client = state.get_database_client().await?;
  if tables_ready(&client).await && !ids.is_empty() {
    #[rustfmt::skip]
    client.execute(/* language=postgresql */ r#"
      update user_skill_pa_fame set is_lock = coalesce($3, not is_lock)
      where user_id = $1 and id = any($2)
    "#, &[&session.user_id, &ids, &lock]).await?;
  }
  Ok(Unsigned(CallResponse::new_success(Box::new(()))))
}

// See [Wonder_Api_UseFameScrollItemResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct UseFameScrollItemResponse {
  pub skill_pa_fame_list: SkillPaFameItem,
}

impl CallCustom for UseFameScrollItemResponse {}

#[derive(Debug, Serialize)]
pub struct SkillPaFameItem {
  pub skill_pa_fame_id: i64,
  pub skill_pa_fame_status: Vec<SkillPaFameAddStatus>,
}

/// A Fame Scroll gives a random trait (odds of the highest quest rank).
pub async fn use_fame_scroll_item(state: Arc<AppState>, session: Arc<Session>, request: ApiRequest) -> impl IntoHandlerResponse {
  info!(body = ?request.body, "use_fame_scroll_item");
  let mut client = state.get_database_client().await?;
  let member_id = number(&request.body, "user_member_id").unwrap_or(0);
  let character_id = if member_id > 0 { MemberPrototype::load_from_id(member_id).character_id } else { 0 };
  let max_rank = get_master_manager()
    .get_master("skill_pa_fame_lot_rare")
    .iter()
    .map(|row| field(row, "quest_rank_id"))
    .max()
    .unwrap_or(1);
  let skill_id = draw_trait(max_rank, character_id).unwrap_or(0);
  let mut response = CallResponse::new_success(Box::new(UseFameScrollItemResponse {
    skill_pa_fame_list: SkillPaFameItem {
      skill_pa_fame_id: skill_id,
      skill_pa_fame_status: vec![],
    },
  }));
  if tables_ready(&client).await && skill_id > 0 {
    let transaction = client.transaction().await.context("failed to start transaction")?;
    let update = UpdateItemCountBy::new(&transaction).await?;
    let scroll = update.run(session.user_id, (RemoteDataItemType::FameScroll, SCROLL_ID), -1).await;
    match scroll {
      Ok(item) => {
        #[rustfmt::skip]
        transaction.execute(/* language=postgresql */ r#"
          insert into user_skill_pa_fame (user_id, member_id, skill_pa_fame_id, add_status) values ($1, $2, $3, '[]')
        "#, &[&session.user_id, &member_id, &skill_id]).await?;
        transaction.commit().await?;
        response.remote.extend(item.into_remote_data());
        info!(skill_id, "fame scroll used");
      }
      Err(error) => warn!(?error, "no fame scroll to use"),
    }
  }
  Ok(Unsigned(response))
}
