//! What the Smithy charges and pays: crafting and upgrade costs, sale prices (masters
//! `equip_weapon`, `equip_accessory`, their `_details` and `item`).

use crate::api::master_all::get_master_manager;
use crate::api::{RemoteData, RemoteDataCommand, RemoteDataItemType};
use crate::blob::IntoRemoteData;
use crate::item::UpdateItemCountBy;
use crate::user::id::UserId;
use crate::user::wallet::Wallet;
use anyhow::Context;
use serde_json::Value;
use std::collections::BTreeMap;

pub type Cost = BTreeMap<(i32, i64), i32>;

fn int(row: &Value, key: &str) -> i64 {
  row[key].as_str().and_then(|value| value.parse().ok()).unwrap_or(0)
}

/// (base master, details master) for weapons (5) and accessories (4).
fn masters(item_type: i32) -> (&'static str, &'static str) {
  if item_type == 4 {
    ("equip_accessory", "equip_accessory_details")
  } else {
    ("equip_weapon", "equip_weapon_details")
  }
}

/// Row of the details master for an equipment at a level.
pub fn details(item_type: i32, item_id: i64, level: i32) -> Option<&'static Value> {
  get_master_manager()
    .get_master(masters(item_type).1)
    .iter()
    .find(|row| int(row, "item_id") == item_id && int(row, "lv") == level as i64)
}

/// Materials (item_typeN, materialN, numN) and money of a master row, times `times`.
fn row_cost(row: &Value, times: i32, cost: &mut Cost, money: &mut i64) {
  for n in 1..=4 {
    let (item_type, item_id, num) = (
      int(row, &format!("item_type{n}")) as i32,
      int(row, &format!("material{n}")),
      int(row, &format!("num{n}")) as i32,
    );
    if item_type != 0 && num > 0 {
      *cost.entry((item_type, item_id)).or_default() += num * times;
    }
  }
  *money += int(row, "money") * times as i64;
}

/// Cost of crafting `num` of an equipment.
pub fn craft_cost(item_type: i32, item_id: i64, num: i32) -> Option<(Cost, i64)> {
  let row = get_master_manager()
    .get_master(masters(item_type).0)
    .iter()
    .find(|row| int(row, "item_id") == item_id)?;
  let (mut cost, mut money) = (Cost::new(), 0);
  row_cost(row, num, &mut cost, &mut money);
  Some((cost, money))
}

/// Cost of upgrading an equipment from `level` by `count` levels (each level's row holds the cost
/// of reaching it).
pub fn upgrade_cost(item_type: i32, item_id: i64, level: i32, count: i32) -> Option<(Cost, i64)> {
  let (mut cost, mut money) = (Cost::new(), 0);
  for next in level + 1..=level + count {
    row_cost(details(item_type, item_id, next)?, 1, &mut cost, &mut money);
  }
  Some((cost, money))
}

/// Sale price of one equipment at a level.
pub fn equipment_price(item_type: i32, item_id: i64, level: i32) -> i64 {
  details(item_type, item_id, level).map(|row| int(row, "sell")).unwrap_or(0)
}

/// Sale price of one item (materials and such).
pub fn item_price(item_type: i32, item_id: i64) -> i64 {
  get_master_manager()
    .get_master("item")
    .iter()
    .find(|row| int(row, "type") == item_type as i64 && int(row, "id") == item_id)
    .map(|row| int(row, "sell"))
    .unwrap_or(0)
}

/// Takes the materials and money. Returns None (and changes nothing) when something is missing.
pub async fn pay(
  transaction: &deadpool_postgres::Transaction<'_>,
  user_id: UserId,
  wallet: &mut Wallet,
  cost: &Cost,
  money: i64,
) -> anyhow::Result<Option<Vec<RemoteData>>> {
  for (&(item_type, item_id), &num) in cost {
    #[rustfmt::skip]
    let owned: i32 = transaction.query_opt(/* language=postgresql */ r#"
      select quantity from user_items where user_id = $1 and item_type = $2 and item_id = $3
    "#, &[&user_id, &(item_type as i64), &item_id]).await.context("failed to read item")?
      .map(|row| row.get(0))
      .unwrap_or(0);
    if owned < num {
      return Ok(None);
    }
  }
  if wallet.add(RemoteDataItemType::Money, -money).is_err() {
    return Ok(None);
  }
  let update = UpdateItemCountBy::new(transaction).await?;
  let mut remote = Vec::new();
  for (&(item_type, item_id), &num) in cost {
    remote.extend(
      update
        .run(user_id, (RemoteDataItemType::from(item_type), item_id), -num)
        .await?
        .into_remote_data(),
    );
  }
  Ok(Some(remote))
}

/// Remote data that takes `money` Eris off what the client shows (delete lowers by item_num).
pub fn spend_money(money: i64) -> RemoteData {
  RemoteData {
    cmd: RemoteDataCommand::UserParamDelete as i32,
    uid: None,
    item_type: RemoteDataItemType::Money.into(),
    item_id: 0,
    item_num: money.min(i32::MAX as i64) as i32,
    uniqid: 0,
    lv: 0,
    tag: String::from("-"),
    member_parameter: None,
    character_parameter: None,
    is_trial: None,
  }
}

/// Remote data that removes one equipment from the client.
pub fn delete_equipment(item_type: i32, item_id_details: i64, unique_id: i64) -> RemoteData {
  RemoteData {
    cmd: RemoteDataCommand::UserParamDelete as i32,
    uid: None,
    item_type,
    item_id: item_id_details,
    item_num: 1,
    uniqid: unique_id as i32,
    lv: 0,
    tag: String::from("-"),
    member_parameter: None,
    character_parameter: None,
    is_trial: None,
  }
}
