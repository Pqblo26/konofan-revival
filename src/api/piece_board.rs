//! Potential Blessing (masters `character_piece_board*`): each character has up to four boards of
//! twelve stages, paid with pieces (the character's own, `usable_piece_id1`, or generic ones,
//! `usable_piece_id2`). Rewards (`character_piece_board_reward`) come with the number of stages
//! released, and some stages need a reward first (`unlock_reward_id`).

use crate::api::character_enhance::int;
use crate::api::master_all::get_master_manager;
use crate::api::quest::finish::pack_items;
use crate::api::{ApiRequest, RemoteDataItemType};
use crate::blob::{AddCharacter, IntoRemoteData};
use crate::call::{CallCustom, CallResponse};
use crate::database::QueryExecutor;
use crate::extractor::Params;
use crate::handler::{IntoHandlerResponse, Unsigned};
use crate::item::UpdateItemCountBy;
use crate::user::id::UserId;
use crate::user::session::Session;
use crate::user::wallet::Wallet;
use crate::AppState;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

fn boards_of(character_id: i64) -> Vec<i64> {
  get_master_manager()
    .get_master("character_piece_board")
    .iter()
    .filter(|row| int(row, "chara_id") == character_id)
    .map(|row| int(row, "board_id"))
    .collect()
}

fn board_stage(stage_id: i64) -> Option<&'static Value> {
  get_master_manager()
    .get_master("character_piece_board_stage")
    .iter()
    .find(|row| int(row, "stage_id") == stage_id)
}

fn rewards_of(character_id: i64) -> Vec<&'static Value> {
  get_master_manager()
    .get_master("character_piece_board_reward")
    .iter()
    .filter(|row| int(row, "character_id") == character_id)
    .collect()
}

/// Rewards reached with this many released stages.
fn reached_rewards(character_id: i64, released_count: usize) -> Vec<i32> {
  rewards_of(character_id)
    .iter()
    .filter(|row| int(row, "require_release_count") as usize <= released_count)
    .map(|row| int(row, "reward_id") as i32)
    .collect()
}

/// Released stages of each character of a user (character id -> stage ids).
pub async fn released_board_stages<'a>(
  executor: impl Into<QueryExecutor<'a>>,
  user_id: UserId,
) -> anyhow::Result<HashMap<i64, Vec<i64>>> {
  let executor = executor.into();
  #[rustfmt::skip]
  let rows = executor.client().query(/* language=postgresql */ r#"
    select character_id, stage_id from user_character_piece_board where user_id = $1
  "#, &[&user_id]).await.context("failed to read released board stages")?;
  let mut released: HashMap<i64, Vec<i64>> = HashMap::new();
  for row in &rows {
    released.entry(row.get(0)).or_default().push(row.get(1));
  }
  Ok(released)
}

/// Last released stage of each board, in board order (0 when none), for the board list.
pub fn last_stage_per_board(character_id: i64, released: &[i64]) -> Vec<(i64, i64)> {
  boards_of(character_id)
    .into_iter()
    .map(|board_id| {
      let last = released
        .iter()
        .filter_map(|&stage_id| board_stage(stage_id).filter(|row| int(row, "board_id") == board_id))
        .max_by_key(|row| int(row, "board_stage_id"))
        .map(|row| int(row, "stage_id"))
        .unwrap_or(0);
      (board_id, last)
    })
    .collect()
}

/// `character_piece_board_stage_id_list`: every released stage, empty when none (as the original
/// server sent). Zeros here (one per board) left every battle loading forever.
pub fn stage_id_list(_character_id: i64, released: &[i64]) -> Vec<i32> {
  let mut list: Vec<i32> = released.iter().map(|&stage_id| stage_id as i32).collect();
  list.sort();
  list
}

// See [Wonder_Api_CharacterPieceBoardInfoResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct CharacterPieceBoardInfo {
  pub board_info: Vec<PieceBoardInfo>,
  pub reward_ids: Vec<i32>,
}

impl CallCustom for CharacterPieceBoardInfo {}

// See [Wonder_Api_PieceBoardInfoResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct PieceBoardInfo {
  pub board_id: i32,
  /// Last released stage of the board, 0 if none
  pub stage_id: i32,
}

#[derive(Debug, Deserialize)]
pub struct CharacterPieceBoardInfoRequest {
  pub character_id: i64,
}

// Thanks to https://youtu.be/2NG0ZLuhNNg.
pub async fn character_piece_board_info(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<CharacterPieceBoardInfoRequest>,
) -> impl IntoHandlerResponse {
  let client = state.get_database_client().await?;
  let released = released_board_stages(&client, session.user_id)
    .await?
    .remove(&params.character_id)
    .unwrap_or_default();
  Ok(Unsigned(CallResponse::new_success(Box::new(CharacterPieceBoardInfo {
    board_info: last_stage_per_board(params.character_id, &released)
      .into_iter()
      .map(|(board_id, stage_id)| PieceBoardInfo { board_id: board_id as i32, stage_id: stage_id as i32 })
      .collect(),
    reward_ids: reached_rewards(params.character_id, released.len()),
  }))))
}

// See [Wonder_Api_CharacterPieceBoardReleaseResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct CharacterPieceBoardRelease {
  pub stage_ids: Vec<i64>,
  pub reward_ids: Vec<i32>,
}

impl CallCustom for CharacterPieceBoardRelease {}

/// Releases board stages. The request has not been seen yet: its body is logged, and the stages
/// are taken from `stage_ids` (JSON list) or `stage_id`.
pub async fn character_piece_board_release(
  state: Arc<AppState>,
  request: ApiRequest,
  session: Arc<Session>,
) -> impl IntoHandlerResponse {
  info!(body = ?request.body, "character_piece_board_release request");
  let body = &request.body;
  let character_id: i64 = body.get("character_id").and_then(|value| value.parse().ok()).unwrap_or(0);
  let stage_ids: Vec<i64> = body
    .get("stage_ids")
    .and_then(|value| serde_json::from_str(value).ok())
    .or_else(|| body.get("stage_id").and_then(|value| value.parse().ok()).map(|id| vec![id]))
    .unwrap_or_default();
  // Some flag about using generic pieces (IsPieceBoardUsePiece2 in the client)
  let generic_only = body
    .iter()
    .any(|(key, value)| key.contains("piece2") && value != "0" && value != "false" && !value.is_empty());

  let empty = || {
    Ok(Unsigned(CallResponse::new_success(Box::new(CharacterPieceBoardRelease {
      stage_ids: vec![],
      reward_ids: vec![],
    }))))
  };
  if character_id == 0 || stage_ids.is_empty() {
    warn!(?body, "board release without character or stages");
    return empty();
  }

  let mut client = state.get_database_client().await?;
  let transaction = client.transaction().await.context("failed to start transaction")?;
  let mut released = released_board_stages(&transaction, session.user_id)
    .await?
    .remove(&character_id)
    .unwrap_or_default();
  let rewards_before = reached_rewards(character_id, released.len());

  // Pieces needed of each kind
  let mut own_needed = 0;
  let mut generic_needed = 0;
  let (mut own_id, mut generic_id) = (0i64, 0i64);
  for &stage_id in &stage_ids {
    let Some(row) = board_stage(stage_id).filter(|row| int(row, "character_id") == character_id) else {
      warn!(stage_id, "unknown board stage");
      return empty();
    };
    let unlock_reward = int(row, "unlock_reward_id") as i32;
    if released.contains(&stage_id) || (unlock_reward != 0 && !reached_rewards(character_id, released.len()).contains(&unlock_reward)) {
      warn!(stage_id, "board stage already released or still locked");
      return empty();
    }
    own_id = int(row, "usable_piece_id1");
    generic_id = int(row, "usable_piece_id2");
    let num = int(row, "piece_num") as i32;
    if generic_only {
      generic_needed += num;
    } else {
      own_needed += num;
    }
    released.push(stage_id);
  }

  let owned = |item_id: i64| {
    let transaction = &transaction;
    let user_id = session.user_id;
    async move {
      #[rustfmt::skip]
      let row = transaction.query_opt(/* language=postgresql */ r#"
        select quantity from user_items where user_id = $1 and item_type = $2 and item_id = $3
      "#, &[&user_id, &(i32::from(RemoteDataItemType::CharacterPiece) as i64), &item_id]).await;
      row.map(|row| row.map(|row| row.get::<_, i32>(0)).unwrap_or(0))
    }
  };
  // Own pieces first, generic ones for what is missing
  let own_owned = if own_needed > 0 { owned(own_id).await.context("failed to read pieces")? } else { 0 };
  if own_needed > own_owned {
    generic_needed += own_needed - own_owned;
    own_needed = own_owned;
  }
  let generic_owned = if generic_needed > 0 { owned(generic_id).await.context("failed to read pieces")? } else { 0 };
  if generic_needed > generic_owned {
    warn!(character_id, ?stage_ids, own_needed, generic_needed, generic_owned, "not enough pieces");
    return empty();
  }

  let update = UpdateItemCountBy::new(&transaction).await?;
  let mut remote = Vec::new();
  for (item_id, num) in [(own_id, own_needed), (generic_id, generic_needed)] {
    if num > 0 {
      remote.extend(update.run(session.user_id, (RemoteDataItemType::CharacterPiece, item_id), -num).await?.into_remote_data());
    }
  }
  for &stage_id in &stage_ids {
    #[rustfmt::skip]
    transaction.execute(/* language=postgresql */ r#"
      insert into user_character_piece_board (user_id, character_id, stage_id) values ($1, $2, $3)
      on conflict do nothing
    "#, &[&session.user_id, &character_id, &stage_id]).await.context("failed to release board stage")?;
  }

  // Rewards newly reached
  let new_rewards: Vec<i32> = reached_rewards(character_id, released.len())
    .into_iter()
    .filter(|reward| !rewards_before.contains(reward))
    .collect();
  let mut wallet = Wallet::load(&transaction, session.user_id).await?;
  let money_before = wallet.get(RemoteDataItemType::Money);
  for row in rewards_of(character_id) {
    if new_rewards.contains(&(int(row, "reward_id") as i32)) && int(row, "pack_id") != 0 {
      let items = pack_items(int(row, "pack_id"));
      remote.extend(crate::user::grant::grant_items(&transaction, session.user_id, &mut wallet, &items).await?);
    }
  }
  wallet.save(&transaction, session.user_id).await?;
  if wallet.get(RemoteDataItemType::Money) != money_before {
    remote.extend(wallet.to_remote_data());
  }

  let character = crate::api::character_enhance::character_parameter(&transaction, session.user_id, character_id).await?;
  remote.extend(AddCharacter::new(character_id as i32, character).into_remote_data());
  transaction.commit().await.context("failed to commit transaction")?;
  info!(character_id, ?stage_ids, own_needed, generic_needed, ?new_rewards, "released board stages");

  let mut response = CallResponse::new_success(Box::new(CharacterPieceBoardRelease { stage_ids, reward_ids: new_rewards }));
  response.remote.extend(remote);
  Ok(Unsigned(response))
}
