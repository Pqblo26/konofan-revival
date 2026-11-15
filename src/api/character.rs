use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

use crate::api::master_all::get_masters;
use crate::call::{CallCustom, CallResponse};
use crate::extractor::Params;
use crate::handler::{IntoHandlerResponse, Unsigned};

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
  /// 0 if none
  pub stage_id: i32,
}

#[derive(Debug, Deserialize)]
pub struct CharacterPieceBoardInfoRequest {
  pub character_id: i32,
}

// Thanks to https://youtu.be/2NG0ZLuhNNg.
// There are multiple blessing paths called 'boards' internally (Fire & Light, Water & Wind, etc.),
// each of which contains 8+4 stages (Elemental ATK boost, Stats up, etc.).
pub async fn character_piece_board_info(
  Params(params): Params<CharacterPieceBoardInfoRequest>,
) -> impl IntoHandlerResponse {
  warn!(?params, "encountered stub: character_piece_board_info");

  let masters = get_masters().await;
  let piece_boards: Vec<Value> = serde_json::from_str(&masters["character_piece_board"].master_decompressed).unwrap();
  let piece_boards = piece_boards
    .iter()
    .filter(|board| board["chara_id"].as_str().unwrap().parse::<i32>().unwrap() == params.character_id)
    .collect::<Vec<_>>();
  let piece_board_stages: Vec<Value> =
    serde_json::from_str(&masters["character_piece_board_stage"].master_decompressed).unwrap();

  Ok(Unsigned(CallResponse::new_success(Box::new(CharacterPieceBoardInfo {
    board_info: piece_boards
      .iter()
      .map(|board| {
        let board_id: i32 = board["board_id"].as_str().unwrap().parse().unwrap();
        let stage = piece_board_stages
          .iter()
          .find(|stage| stage["board_id"].as_str().unwrap().parse::<i32>().unwrap() == board_id)
          .unwrap();
        let stage_id: i32 = stage["stage_id"].as_str().unwrap().parse().unwrap();
        PieceBoardInfo { board_id, stage_id: 0 }
      })
      .collect(),
    reward_ids: vec![],
  }))))
}

// See [Wonder_Api_CharacterEnhanceInfoResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct CharacterEnhanceInfo {
  pub progress: Vec<CharacterEnhanceInfoProgress>,
  pub trial_timestamp: i64,
}

impl CallCustom for CharacterEnhanceInfo {}

// See [System_Collections_Generic_List_CharacterEnhanceInfoProgressResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct CharacterEnhanceInfoProgress {
  pub root_id: i32,
  pub root_stage_id: i32,
  pub stage_id: i32,
  pub parameter: EnhanceParameter,
  pub unique_weapon_id: i64,
  pub specialskill: Vec<EnhanceSpecialSkill>,
  pub unique_stone: EnhanceUniqueStone,
  pub material_items: Vec<CharacterEnhanceMaterial>,
  pub money: i32,
}

// See [Wonder_Api_EnhanceSpecialskillResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct EnhanceSpecialSkill {
  pub sp_id: i64,
  pub sp_group_id: i64,
  pub sp_lv: i32,
}

// See [Wonder_Api_EnhanceUniqueStoneResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct EnhanceUniqueStone {
  pub unique_stone_id: i64,
  pub unique_stone_lv: i32,
}

// See [Wonder_Api_EnhanceParameterResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct EnhanceParameter {
  pub hp: i32,
  pub attack: i32,
  pub magicattack: i32,
  pub defense: i32,
  pub magicdefence: i32,
  pub agility: i32,
  pub dexterity: i32,
  pub luck: i32,
}

// See [Wonder_Api_CharacterEnhanceMaterialResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct CharacterEnhanceMaterial {
  pub item_type: i32,
  pub item_id: i64,
  pub item_num: i32,
}

#[derive(Debug, Deserialize)]
pub struct CharacterEnhanceInfoRequest {
  pub character_id: i32,
}

// Thanks to https://youtu.be/o5UUz2kHhto for unbricking this endpoint
/// Every stage of the character with what it gives and costs (master `character_enhance`).
pub async fn character_enhance_info(Params(params): Params<CharacterEnhanceInfoRequest>) -> impl IntoHandlerResponse {
  use crate::api::character_enhance::{int, stage_cost, stages_of};
  let stages = stages_of(params.character_id as i64);
  Ok(Unsigned(CharacterEnhanceInfo {
    progress: stages
      .iter()
      .map(|row| {
        let (materials, money) = stage_cost(row);
        let stat = |key: &str| int(row, key) as i32;
        CharacterEnhanceInfoProgress {
          root_id: stat("root_id"),
          root_stage_id: stat("root_stage_id"),
          stage_id: stat("stage_id"),
          parameter: EnhanceParameter {
            hp: stat("hp"),
            attack: stat("attack"),
            magicattack: stat("magicattack"),
            defense: stat("defense"),
            magicdefence: stat("magicdefence"),
            agility: stat("agility"),
            dexterity: stat("dexterity"),
            luck: stat("luck"),
          },
          unique_weapon_id: int(row, "unique_weapon_id"),
          specialskill: if int(row, "sp_id") != 0 {
            vec![EnhanceSpecialSkill { sp_id: int(row, "sp_id"), sp_group_id: int(row, "sp_group_id"), sp_lv: stat("sp_lv") }]
          } else {
            vec![]
          },
          unique_stone: EnhanceUniqueStone { unique_stone_id: int(row, "unique_stone_id"), unique_stone_lv: 0 },
          material_items: materials
            .into_iter()
            .map(|(item_type, item_id, item_num)| CharacterEnhanceMaterial { item_type, item_id, item_num })
            .collect(),
          money: money as i32,
        }
      })
      .collect(),
    trial_timestamp: chrono::Utc::now().timestamp(),
  }))
}
