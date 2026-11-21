use serde::Serialize;

use std::collections::HashMap;
use std::sync::Arc;

use crate::call::CallCustom;
use crate::handler::{IntoHandlerResponse, Unsigned};
use crate::user::session::Session;
use crate::AppState;

// See [Wonder_Api_WeaponlistResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct WeaponList {
  pub items: Vec<WeaponListItem>,
}

impl CallCustom for WeaponList {}

// See [Wonder_Api_WeaponlistItemsResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct WeaponListItem {
  /// Opaque
  pub id: i64,
  /// `equip_weapon_details.item_id_details`
  pub weapon_id: i64,
  #[serde(with = "crate::bool_as_int")]
  pub islock: bool,
  pub trial: bool,
}

/// Weapons the player owns: `id` is the unique id a team stores, `weapon_id` the item at its level.
pub async fn weapon_list(state: Arc<AppState>, session: Arc<Session>) -> impl IntoHandlerResponse {
  let client = state.get_database_client().await?;
  let equipment = crate::api::party::load_equipment(&client, session.user_id).await?;
  let locked = equipment.iter().map(|item| (item.unique_id, item.is_locked)).collect::<HashMap<_, _>>();
  let (weapons, _) = crate::api::party::equipment_wire(&equipment);

  Ok(Unsigned(WeaponList {
    items: weapons
      .into_iter()
      .map(|weapon| WeaponListItem {
        id: weapon.id,
        weapon_id: weapon.weapon_id,
        islock: locked.get(&weapon.id).copied().unwrap_or(false),
        trial: false,
      })
      .collect(),
  }))
}

// See [Wonder_Api_AccessorylistResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct AccessoryList {
  pub items: Vec<AccessoryListItem>,
}

impl CallCustom for AccessoryList {}

// See [Wonder_Api_AccessorylistItemsResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct AccessoryListItem {
  /// Opaque
  pub id: i64,
  /// `equip_accessory_details.item_id_details`
  pub accessory_id: i64,
  #[serde(with = "crate::bool_as_int")]
  pub islock: bool,
}

/// Accessories the player owns, like [weapon_list].
pub async fn accessory_list(state: Arc<AppState>, session: Arc<Session>) -> impl IntoHandlerResponse {
  let client = state.get_database_client().await?;
  let equipment = crate::api::party::load_equipment(&client, session.user_id).await?;
  let locked = equipment.iter().map(|item| (item.unique_id, item.is_locked)).collect::<HashMap<_, _>>();
  let (_, accessories) = crate::api::party::equipment_wire(&equipment);

  Ok(Unsigned(AccessoryList {
    items: accessories
      .into_iter()
      .map(|accessory| AccessoryListItem {
        id: accessory.id,
        accessory_id: accessory.accessory_id,
        islock: locked.get(&accessory.id).copied().unwrap_or(false),
      })
      .collect(),
  }))
}
