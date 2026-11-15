use serde::{Deserialize, Serialize};

use crate::call::CallCustom;
use crate::extractor::Params;
use crate::handler::{IntoHandlerResponse, Unsigned};

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
