//! Trial of the Ancients (master `character_enhance`): stages each character has released, what
//! they cost and what they give. Stages are grouped in up to four roots (1 stats and unique
//! equipment, 2 new special skill, 3 special skill levels, 4 other skills); each one needs the
//! previous one (`unlock_clearstage`) and an affinity level (`intimacy_lv`).

use crate::api::master_all::get_master_manager;
use crate::api::smith_cost::Cost;
use crate::api::{CharacterParameter, SpSkill};
use crate::blob::{AddCharacter, IntoRemoteData};
use crate::call::CallResponse;
use crate::database::QueryExecutor;
use crate::extractor::Params;
use crate::handler::{IntoHandlerResponse, Unsigned};
use crate::level::get_intimacy_level_calculator;
use crate::user::id::UserId;
use crate::user::session::Session;
use crate::user::wallet::Wallet;
use crate::AppState;
use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

pub fn int(row: &Value, key: &str) -> i64 {
  row[key].as_str().and_then(|value| value.parse().ok()).unwrap_or(0)
}

/// Stages of a character, in master order.
pub fn stages_of(character_id: i64) -> Vec<&'static Value> {
  get_master_manager()
    .get_master("character_enhance")
    .iter()
    .filter(|row| int(row, "character_id") == character_id)
    .collect()
}

fn stage(character_id: i64, stage_id: i64) -> Option<&'static Value> {
  get_master_manager()
    .get_master("character_enhance")
    .iter()
    .find(|row| int(row, "stage_id") == stage_id && int(row, "character_id") == character_id)
}

/// Materials (item_typeN, item_idN, item_numN) and Eris of a stage.
pub fn stage_cost(row: &Value) -> (Vec<(i32, i64, i32)>, i64) {
  let materials = (1..=5)
    .map(|n| {
      (
        int(row, &format!("item_type{n}")) as i32,
        int(row, &format!("item_id{n}")),
        int(row, &format!("item_num{n}")) as i32,
      )
    })
    .filter(|&(item_type, _, num)| item_type != 0 && num > 0)
    .collect();
  (materials, int(row, "money"))
}

/// Released stages of each character of a user (character id -> stage ids).
pub async fn released_stages<'a>(
  executor: impl Into<QueryExecutor<'a>>,
  user_id: UserId,
) -> anyhow::Result<HashMap<i64, Vec<i64>>> {
  let executor = executor.into();
  #[rustfmt::skip]
  let rows = executor.client().query(/* language=postgresql */ r#"
    select character_id, stage_id from user_character_enhance where user_id = $1
  "#, &[&user_id]).await.context("failed to read released trial stages")?;
  let mut released: HashMap<i64, Vec<i64>> = HashMap::new();
  for row in &rows {
    released.entry(row.get(0)).or_default().push(row.get(1));
  }
  Ok(released)
}

/// Last released stage of each of the four roots (0 when none): `character_enhance_stage_id_list`.
pub fn stage_id_list(character_id: i64, released: &[i64]) -> Vec<i32> {
  let mut list = vec![0; 4];
  let mut best = [0i64; 4];
  for row in stages_of(character_id) {
    let stage_id = int(row, "stage_id");
    let (root, root_stage) = (int(row, "root_id"), int(row, "root_stage_id"));
    if (1..=4).contains(&root) && released.contains(&stage_id) && root_stage > best[root as usize - 1] {
      best[root as usize - 1] = root_stage;
      list[root as usize - 1] = stage_id as i32;
    }
  }
  list
}

/// Stat bonuses of each character of a user (character id -> [hp, attack, magicattack, defense,
/// magicdefence, agility, dexterity, luck]): the affinity level's row of
/// `character_intimacy_details` (not cumulative) plus every released trial stage. The game adds
/// them on the character sheet only; in battle it uses the member stats as sent.
pub async fn character_bonuses<'a>(
  executor: impl Into<QueryExecutor<'a>>,
  user_id: UserId,
) -> anyhow::Result<HashMap<i64, [i32; 8]>> {
  const KEYS: [&str; 8] = ["hp", "attack", "magicattack", "defense", "magicdefence", "agility", "dexterity", "luck"];
  let executor = executor.into();
  let client = executor.client();
  let mut bonuses: HashMap<i64, [i32; 8]> = HashMap::new();

  #[rustfmt::skip]
  let characters = client.query(/* language=postgresql */ r#"
    select character_id, intimacy from user_characters where user_id = $1
  "#, &[&user_id]).await.context("failed to read characters")?;
  let intimacy_details = get_master_manager().get_master("character_intimacy_details");
  for row in &characters {
    let (character_id, intimacy): (i64, i32) = (row.get(0), row.get(1));
    let level = get_intimacy_level_calculator().get_level(intimacy) as i64;
    if let Some(details) = intimacy_details
      .iter()
      .find(|details| int(details, "chara_id") == character_id && int(details, "character_level") == level)
    {
      let bonus = bonuses.entry(character_id).or_default();
      for (index, key) in KEYS.iter().enumerate() {
        // The master spells magic attack "magicattak"
        let key = if *key == "magicattack" { "magicattak" } else { key };
        bonus[index] += int(details, key) as i32;
      }
    }
  }

  for (character_id, stages) in released_stages(client, user_id).await? {
    let bonus = bonuses.entry(character_id).or_default();
    for row in stages_of(character_id).into_iter().filter(|row| stages.contains(&int(row, "stage_id"))) {
      for (index, key) in KEYS.iter().enumerate() {
        bonus[index] += int(row, key) as i32;
      }
    }
  }
  Ok(bonuses)
}

/// Everything the client keeps about one character, as sent at login.
pub async fn character_parameter<'a>(
  executor: impl Into<QueryExecutor<'a>>,
  user_id: UserId,
  character_id: i64,
) -> anyhow::Result<CharacterParameter> {
  let executor = executor.into();
  let client = executor.client();
  #[rustfmt::skip]
  let intimacy: i32 = client.query_opt(/* language=postgresql */ r#"
    select intimacy from user_characters where user_id = $1 and character_id = $2
  "#, &[&user_id, &character_id]).await.context("failed to read character")?.map(|row| row.get(0)).unwrap_or(0);
  #[rustfmt::skip]
  let skills = client.query(/* language=postgresql */ r#"
    select skill_id, level from user_character_special_skills where user_id = $1 and character_id = $2
  "#, &[&user_id, &character_id]).await.context("failed to read special skills")?;
  let skill_sp = get_master_manager().get_master("skill_sp");
  let sp_skill = skills
    .iter()
    .map(|row| {
      let (id, lv): (i64, i32) = (row.get(0), row.get(1));
      let group_id = skill_sp
        .iter()
        .find(|skill| int(skill, "skill_id") == id)
        .map(|skill| int(skill, "skill_group_id") as i32)
        .unwrap_or(0);
      SpSkill { group_id, id, lv, is_trial: false }
    })
    .collect();
  #[rustfmt::skip]
  let released: Vec<i64> = client.query(/* language=postgresql */ r#"
    select stage_id from user_character_enhance where user_id = $1 and character_id = $2
  "#, &[&user_id, &character_id]).await.context("failed to read released trial stages")?
    .iter().map(|row| row.get(0)).collect();
  #[rustfmt::skip]
  let board_released: Vec<i64> = client.query(/* language=postgresql */ r#"
    select stage_id from user_character_piece_board where user_id = $1 and character_id = $2
  "#, &[&user_id, &character_id]).await.context("failed to read released board stages")?
    .iter().map(|row| row.get(0)).collect();
  Ok(CharacterParameter {
    id: character_id,
    character_id,
    rank: get_intimacy_level_calculator().get_level(intimacy),
    rank_progress: intimacy,
    sp_skill,
    character_enhance_stage_id_list: stage_id_list(character_id, &released),
    character_piece_board_stage_id_list: crate::api::piece_board::stage_id_list(character_id, &board_released),
    is_trial: false,
  })
}

#[derive(Debug, Deserialize)]
pub struct CharacterEnhanceMultiReleaseRequest {
  pub character_id: i64,
  /// JSON list of stage ids, e.g. "[1000101]"
  pub stage_ids: String,
}

/// Releases one or more stages: checks order and affinity, takes materials and Eris, gives the
/// special skills, and sends the character back with its new stage list.
pub async fn character_enhance_multi_release(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<CharacterEnhanceMultiReleaseRequest>,
) -> impl IntoHandlerResponse {
  let stage_ids: Vec<i64> = serde_json::from_str(&params.stage_ids).context("invalid stage_ids")?;
  let character_id = params.character_id;

  let mut client = state.get_database_client().await?;
  let transaction = client.transaction().await.context("failed to start transaction")?;

  let mut released = released_stages(&transaction, session.user_id).await?.remove(&character_id).unwrap_or_default();
  #[rustfmt::skip]
  let intimacy: i32 = transaction.query_opt(/* language=postgresql */ r#"
    select intimacy from user_characters where user_id = $1 and character_id = $2
  "#, &[&session.user_id, &character_id]).await.context("failed to read character")?.map(|row| row.get(0)).unwrap_or(0);
  let affinity = get_intimacy_level_calculator().get_level(intimacy) as i64;

  let refuse = |reason: &str| {
    warn!(character_id, ?stage_ids, affinity, reason, "trial stage not released");
    Ok(Unsigned(CallResponse::new_success(Box::new(()))))
  };

  let (mut cost, mut money) = (Cost::new(), 0i64);
  let mut rows = Vec::new();
  for &stage_id in &stage_ids {
    let Some(row) = stage(character_id, stage_id) else {
      return refuse("unknown stage");
    };
    let previous = int(row, "unlock_clearstage");
    if released.contains(&stage_id) {
      return refuse("already released");
    }
    if previous != 0 && !released.contains(&previous) {
      return refuse("previous stage not released");
    }
    if int(row, "intimacy_lv") > affinity {
      return refuse("affinity too low");
    }
    let (materials, stage_money) = stage_cost(row);
    for (item_type, item_id, num) in materials {
      *cost.entry((item_type, item_id)).or_default() += num;
    }
    money += stage_money;
    released.push(stage_id);
    rows.push(row);
  }

  let mut wallet = Wallet::load(&transaction, session.user_id).await?;
  let Some(mut remote) = crate::api::smith_cost::pay(&transaction, session.user_id, &mut wallet, &cost, money).await? else {
    return refuse("not enough materials or Eris");
  };
  wallet.save(&transaction, session.user_id).await?;
  if money > 0 {
    remote.push(crate::api::smith_cost::spend_money(money));
  }

  for row in &rows {
    let stage_id = int(row, "stage_id");
    #[rustfmt::skip]
    transaction.execute(/* language=postgresql */ r#"
      insert into user_character_enhance (user_id, character_id, stage_id) values ($1, $2, $3)
      on conflict do nothing
    "#, &[&session.user_id, &character_id, &stage_id]).await.context("failed to release trial stage")?;

    // Special skills: a new one replaces the others of its group, a level up raises it
    let (sp_id, sp_group_id, sp_lv) = (int(row, "sp_id"), int(row, "sp_group_id"), int(row, "sp_lv") as i32);
    if sp_id != 0 {
      let group: Vec<i64> = get_master_manager()
        .get_master("skill_sp")
        .iter()
        .filter(|skill| int(skill, "skill_group_id") == sp_group_id && int(skill, "skill_id") != sp_id)
        .map(|skill| int(skill, "skill_id"))
        .collect();
      #[rustfmt::skip]
      transaction.execute(/* language=postgresql */ r#"
        delete from user_character_special_skills where user_id = $1 and character_id = $2 and skill_id = any($3)
      "#, &[&session.user_id, &character_id, &group]).await.context("failed to replace special skill")?;
      #[rustfmt::skip]
      transaction.execute(/* language=postgresql */ r#"
        insert into user_character_special_skills (user_id, character_id, skill_id, level) values ($1, $2, $3, $4)
        on conflict (user_id, character_id, skill_id) do update set level = greatest(user_character_special_skills.level, $4)
      "#, &[&session.user_id, &character_id, &sp_id, &sp_lv.max(1)]).await.context("failed to give special skill")?;
    }
  }

  let character = character_parameter(&transaction, session.user_id, character_id).await?;
  remote.extend(crate::blob::UpdateCharacter(AddCharacter::new(character_id as i32, character)).into_remote_data());
  transaction.commit().await.context("failed to commit transaction")?;
  info!(character_id, ?stage_ids, ?cost, money, "released trial stages");

  let mut response = CallResponse::new_success(Box::new(()));
  response.remote.extend(remote);
  Ok(Unsigned(response))
}
