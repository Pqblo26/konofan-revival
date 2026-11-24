-- Accessories (item type 6) are stored with the weapons (5): presents, exchanges and crafting can give them.

alter table user_items_equipment
  drop constraint if exists chk_user_items_equipment_type;
alter table user_items_equipment
  add constraint chk_user_items_equipment_type check (item_type in (4, 5, 6));
