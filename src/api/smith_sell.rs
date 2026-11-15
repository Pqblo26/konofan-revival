use anyhow::Context;
use jwt_simple::prelude::Serialize;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::api::master_all::get_master_manager;
use crate::api::ApiRequest;
use crate::call::CallCustom;
use crate::extractor::Params;
use crate::handler::{IntoHandlerResponse, Signed, Unsigned};
use crate::api::{smith_cost, RemoteDataItemType};
use crate::blob::IntoRemoteData;
use crate::call::CallResponse;
use crate::item::UpdateItemCountBy;
use crate::user::wallet::Wallet;
use tracing::{info, warn};
use crate::user::session::Session;
use crate::AppState;

// See [Wonder_Api_SaleListResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SaleList {
  pub items: Vec<SaleItem>,
}

impl CallCustom for SaleList {}

// See [Wonder_Api_SaleListItemsResponseDto_Fields]
#[derive(Debug, Serialize)]
pub struct SaleItem {
  pub item_type: i32,
  pub item_id: i64,
  pub target_item_id: i64,
  pub item_num: i32,
  #[serde(with = "crate::bool_as_int")]
  pub islock: bool,
  #[serde(with = "crate::bool_as_int")]
  pub isuse: bool,
  pub trial: bool,
}

#[derive(Debug, Deserialize)]
pub enum SaleListKind {
  /// "Equipment"
  #[serde(rename = "equip")]
  Equipment,

  /// "Crafting Materials"
  #[serde(rename = "material")]
  Material,
}

#[derive(Debug, Deserialize)]
pub struct SaleListRequest {
  #[serde(rename = "type")]
  pub kind: SaleListKind,
}

pub async fn sale_list(
  state: Arc<AppState>,
  session: Arc<Session>,
  Params(params): Params<SaleListRequest>,
) -> impl IntoHandlerResponse {
  let items = get_master_manager().get_master("item");
  let equip_weapons = get_master_manager().get_master("equip_weapon");
  let equip_weapon_details = get_master_manager().get_master("equip_weapon_details");

  let client = state.get_database_client().await?;

  let items = match params.kind {
    SaleListKind::Equipment => {
      // (item_id, level) -> item_id_details
      let item_to_item_details: HashMap<(i64, i32), i64> = equip_weapon_details
        .iter()
        .map(|data| {
          let item_id: i64 = data["item_id"].as_str().unwrap().parse::<i64>().unwrap();
          let level: i32 = data["lv"].as_str().unwrap().parse::<i32>().unwrap();
          let item_id_details: i64 = data["item_id_details"].as_str().unwrap().parse::<i64>().unwrap();
          ((item_id, level), item_id_details)
        })
        .collect::<HashMap<_, _>>();

      #[rustfmt::skip]
      let statement = client
        .prepare(/* language=postgresql */ r#"
          select
            id,
            item_type,
            item_id,
            level,
            is_locked
          from user_items_equipment
          where user_id = $1
        "#)
        .await
        .context("failed to prepare statement")?;
      let rows = client
        .query(&statement, &[&session.user_id])
        .await
        .context("failed to execute query")?;

      rows
        .iter()
        .map(|item| {
          let item_id = item.get::<_, i64>("item_id");
          let level = item.get::<_, i32>("level");
          let item_details_id = *item_to_item_details
            .get(&(item_id, level))
            .expect(&format!("missing item details for item_id={} level={}", item_id, level));

          SaleItem {
            item_type: item.get::<_, i64>("item_type") as i32,
            item_id: item_details_id,
            target_item_id: item.get::<_, i64>("id"),
            item_num: 1,
            islock: item.get::<_, bool>("is_locked"),
            isuse: false,
            trial: false,
          }
        })
        .collect()
    }
    SaleListKind::Material => {
      #[rustfmt::skip]
      let statement = client
        .prepare(/* language=postgresql */ r#"
          select
            item_type,
            item_id,
            quantity
          from user_items
          where user_id = $1
        "#)
        .await
        .context("failed to prepare statement")?;
      let rows = client
        .query(&statement, &[&session.user_id])
        .await
        .context("failed to execute query")?;

      rows
        .iter()
        .map(|item| SaleItem {
          item_type: item.get::<_, i64>("item_type") as i32,
          item_id: item.get::<_, i64>("item_id"),
          target_item_id: item.get::<_, i64>("item_id"),
          item_num: item.get::<_, i32>("quantity"),
          islock: false,
          isuse: false,
          trial: false,
        })
        .collect()
    }
  };

  Ok(Unsigned(SaleList { items }))
}

// type=material
// items=[{"item_type":15,"target_item_id":0,"use_num":1},{"item_type":15,"target_item_id":0,"use_num":1}]
#[derive(Debug, Deserialize)]
pub struct SaleRequestItem {
  pub item_type: i32,
  /// Equipment: its unique id; materials: the item id (as sent by [sale_list])
  pub target_item_id: i64,
  pub use_num: i32,
}

/// "Sell": removes the equipment or materials and pays their sale price in Eris.
pub async fn sale(state: Arc<AppState>, request: ApiRequest, session: Arc<Session>) -> impl IntoHandlerResponse {
  let items: Vec<SaleRequestItem> =
    serde_json::from_str(request.body.get("items").map(String::as_str).unwrap_or("[]")).context("bad items")?;
  let kind = request.body.get("type").cloned().unwrap_or_default();

  let mut client = state.get_database_client().await?;
  let transaction = client.transaction().await.context("failed to start transaction")?;
  let mut wallet = Wallet::load(&transaction, session.user_id).await?;
  let update = UpdateItemCountBy::new(&transaction).await?;
  let mut response = CallResponse::new_success_empty();
  let mut earned = 0i64;
  for item in &items {
    if matches!(item.item_type, 4 | 5) {
      #[rustfmt::skip]
      let row = transaction.query_opt(/* language=postgresql */ r#"
        delete from user_items_equipment where id = $1 and user_id = $2 and not is_locked
        returning item_type, item_id, level
      "#, &[&item.target_item_id, &session.user_id]).await.context("failed to sell equipment")?;
      let Some(row) = row else {
        warn!(?item, "equipment to sell not found or locked");
        continue;
      };
      let (item_type, item_id, level): (i64, i64, i32) = (row.get(0), row.get(1), row.get(2));
      earned += smith_cost::equipment_price(item_type as i32, item_id, level);
      let details = smith_cost::details(item_type as i32, item_id, level)
        .and_then(|row| row["item_id_details"].as_str()?.parse().ok())
        .unwrap_or(item_id);
      response.remote.push(smith_cost::delete_equipment(item_type as i32, details, item.target_item_id));
    } else {
      let num = item.use_num.max(1);
      let Ok(counted) = update
        .run(session.user_id, (RemoteDataItemType::from(item.item_type), item.target_item_id), -num)
        .await
      else {
        warn!(?item, "not enough items to sell");
        continue;
      };
      earned += smith_cost::item_price(item.item_type, item.target_item_id) * num as i64;
      response.remote.extend(counted.into_remote_data());
    }
  }
  let _ = wallet.add(RemoteDataItemType::Money, earned);
  wallet.save(&transaction, session.user_id).await?;
  transaction.commit().await.context("failed to commit transaction")?;
  response.remote.extend(wallet.to_remote_data());
  info!(%kind, sold = items.len(), earned, "sold items");

  // See [Wonder_Api_SaleResponseDto_Fields]
  Ok(Signed(response, session))
}
