// AUTO-GENERATED FILE. DO NOT EDIT.
#![allow(dead_code)]

use super::{DecodeError, Value, arg, decode};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Event {
    AdHidden { ad: String, by: String },
    Connection { status: String },
    Debug { message: String },
    ExtensionResponse { packet: String },
    Loaded,
    Progress { percent: i32 },
    PacketFromClient { packet: String },
    PacketFromServer { packet: String },
}

impl Event {
    pub(crate) fn parse(name: &str, args: &[Value]) -> Result<Option<Self>, DecodeError> {
        Ok(Some(match name {
            "onAdHidden" => Self::AdHidden {
                ad: decode("onAdHidden", arg(args, 0))?,
                by: decode("onAdHidden", arg(args, 1))?,
            },
            "onConnection" => Self::Connection {
                status: decode("onConnection", arg(args, 0))?,
            },
            "onDebug" => Self::Debug {
                message: decode("onDebug", arg(args, 0))?,
            },
            "onExtensionResponse" => Self::ExtensionResponse {
                packet: decode("onExtensionResponse", arg(args, 0))?,
            },
            "onLoaded" => Self::Loaded,
            "onProgress" => Self::Progress {
                percent: decode("onProgress", arg(args, 0))?,
            },
            "packetFromClient" => Self::PacketFromClient {
                packet: decode("packetFromClient", arg(args, 0))?,
            },
            "packetFromServer" => Self::PacketFromServer {
                packet: decode("packetFromServer", arg(args, 0))?,
            },
            _ => return Ok(None),
        }))
    }
}

pub(crate) mod auth {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn connect_to(loader: &impl Loader, server: &str) -> Result<Value, CallError> {
        let call_args = vec![Value::String(server.to_owned())];
        let reply = loader.call("auth.connectTo", call_args)?;
        Ok(decode("auth.connectTo", reply)?)
    }

    pub(crate) fn get_servers(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("auth.getServers", call_args)?;
        Ok(decode("auth.getServers", reply)?)
    }

    pub(crate) fn is_logged_in(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("auth.isLoggedIn", call_args)?;
        Ok(decode("auth.isLoggedIn", reply)?)
    }

    pub(crate) fn is_temporarily_kicked(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("auth.isTemporarilyKicked", call_args)?;
        Ok(decode("auth.isTemporarilyKicked", reply)?)
    }

    pub(crate) fn login(
        loader: &impl Loader,
        username: &str,
        password: &str,
    ) -> Result<(), CallError> {
        let call_args = vec![
            Value::String(username.to_owned()),
            Value::String(password.to_owned()),
        ];
        let reply = loader.call("auth.login", call_args)?;
        Ok(decode("auth.login", reply)?)
    }

    pub(crate) fn logout(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("auth.logout", call_args)?;
        Ok(decode("auth.logout", reply)?)
    }
}

pub(crate) mod bank {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn deposit(loader: &impl Loader, selector: Value) -> Result<bool, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("bank.deposit", call_args)?;
        Ok(decode("bank.deposit", reply)?)
    }

    pub(crate) fn get_item(loader: &impl Loader, selector: Value) -> Result<Value, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("bank.getItem", call_args)?;
        Ok(decode("bank.getItem", reply)?)
    }

    pub(crate) fn get_items(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("bank.getItems", call_args)?;
        Ok(decode("bank.getItems", reply)?)
    }

    pub(crate) fn get_slots(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("bank.getSlots", call_args)?;
        Ok(decode("bank.getSlots", reply)?)
    }

    pub(crate) fn get_used_slots(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("bank.getUsedSlots", call_args)?;
        Ok(decode("bank.getUsedSlots", reply)?)
    }

    pub(crate) fn is_loaded(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("bank.isLoaded", call_args)?;
        Ok(decode("bank.isLoaded", reply)?)
    }

    pub(crate) fn is_open(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("bank.isOpen", call_args)?;
        Ok(decode("bank.isOpen", reply)?)
    }

    pub(crate) fn load_items(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("bank.loadItems", call_args)?;
        Ok(decode("bank.loadItems", reply)?)
    }

    pub(crate) fn open(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("bank.open", call_args)?;
        Ok(decode("bank.open", reply)?)
    }

    pub(crate) fn swap(
        loader: &impl Loader,
        inventory_selector: Value,
        bank_selector: Value,
    ) -> Result<bool, CallError> {
        let call_args = vec![inventory_selector, bank_selector];
        let reply = loader.call("bank.swap", call_args)?;
        Ok(decode("bank.swap", reply)?)
    }

    pub(crate) fn withdraw(loader: &impl Loader, selector: Value) -> Result<bool, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("bank.withdraw", call_args)?;
        Ok(decode("bank.withdraw", reply)?)
    }
}

pub(crate) mod combat {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn attack_monster(loader: &impl Loader, selector: Value) -> Result<bool, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("combat.attackMonster", call_args)?;
        Ok(decode("combat.attackMonster", reply)?)
    }

    pub(crate) fn cancel_auto_attack(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("combat.cancelAutoAttack", call_args)?;
        Ok(decode("combat.cancelAutoAttack", reply)?)
    }

    pub(crate) fn cancel_target(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("combat.cancelTarget", call_args)?;
        Ok(decode("combat.cancelTarget", reply)?)
    }

    pub(crate) fn can_use_skill(loader: &impl Loader, index: i32) -> Result<bool, CallError> {
        let call_args = vec![Value::Number(f64::from(index))];
        let reply = loader.call("combat.canUseSkill", call_args)?;
        Ok(decode("combat.canUseSkill", reply)?)
    }

    pub(crate) fn cast_consumable_on_monster(
        loader: &impl Loader,
        selector: Value,
        expected_item_id: i32,
    ) -> Result<Value, CallError> {
        let call_args = vec![selector, Value::Number(f64::from(expected_item_id))];
        let reply = loader.call("combat.castConsumableOnMonster", call_args)?;
        Ok(decode("combat.castConsumableOnMonster", reply)?)
    }

    pub(crate) fn get_consumable_skill_item(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("combat.getConsumableSkillItem", call_args)?;
        Ok(decode("combat.getConsumableSkillItem", reply)?)
    }

    pub(crate) fn get_skill_cooldown_remaining(
        loader: &impl Loader,
        index: i32,
    ) -> Result<i32, CallError> {
        let call_args = vec![Value::Number(f64::from(index))];
        let reply = loader.call("combat.getSkillCooldownRemaining", call_args)?;
        Ok(decode("combat.getSkillCooldownRemaining", reply)?)
    }

    pub(crate) fn get_target(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("combat.getTarget", call_args)?;
        Ok(decode("combat.getTarget", reply)?)
    }

    pub(crate) fn has_target(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("combat.hasTarget", call_args)?;
        Ok(decode("combat.hasTarget", reply)?)
    }

    pub(crate) fn use_skill(
        loader: &impl Loader,
        index: &str,
        selector: Option<Value>,
        force: Option<bool>,
    ) -> Result<bool, CallError> {
        let call_args = vec![
            Value::String(index.to_owned()),
            selector.unwrap_or(Value::Null),
            Value::Bool(force.unwrap_or(false)),
        ];
        let reply = loader.call("combat.useSkill", call_args)?;
        Ok(decode("combat.useSkill", reply)?)
    }
}

pub(crate) mod drops {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn accept(loader: &impl Loader, selector: Value) -> Result<i32, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("drops.accept", call_args)?;
        Ok(decode("drops.accept", reply)?)
    }

    pub(crate) fn get_item(loader: &impl Loader, selector: Value) -> Result<Value, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("drops.getItem", call_args)?;
        Ok(decode("drops.getItem", reply)?)
    }

    pub(crate) fn get_items(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("drops.getItems", call_args)?;
        Ok(decode("drops.getItems", reply)?)
    }

    pub(crate) fn is_using_custom_drops(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("drops.isUsingCustomDrops", call_args)?;
        Ok(decode("drops.isUsingCustomDrops", reply)?)
    }

    pub(crate) fn reject(loader: &impl Loader, selector: Value) -> Result<bool, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("drops.reject", call_args)?;
        Ok(decode("drops.reject", reply)?)
    }

    pub(crate) fn toggle_ui(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("drops.toggleUi", call_args)?;
        Ok(decode("drops.toggleUi", reply)?)
    }
}

pub(crate) mod flash {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn call_game_function(
        loader: &impl Loader,
        path: &str,
        args: Vec<Value>,
    ) -> Result<Option<String>, CallError> {
        let mut call_args = vec![Value::String(path.to_owned())];
        call_args.extend(args);
        let reply = loader.call("flash.callGameFunction", call_args)?;
        Ok(decode("flash.callGameFunction", reply)?)
    }

    pub(crate) fn call_game_function0(
        loader: &impl Loader,
        path: &str,
    ) -> Result<Option<String>, CallError> {
        let call_args = vec![Value::String(path.to_owned())];
        let reply = loader.call("flash.callGameFunction0", call_args)?;
        Ok(decode("flash.callGameFunction0", reply)?)
    }

    pub(crate) fn get_array_object(
        loader: &impl Loader,
        path: &str,
        index: i32,
    ) -> Result<Option<String>, CallError> {
        let call_args = vec![
            Value::String(path.to_owned()),
            Value::Number(f64::from(index)),
        ];
        let reply = loader.call("flash.getArrayObject", call_args)?;
        Ok(decode("flash.getArrayObject", reply)?)
    }

    pub(crate) fn get_conn_mc_text(loader: &impl Loader) -> Result<Option<String>, CallError> {
        let call_args = vec![];
        let reply = loader.call("flash.getConnMcText", call_args)?;
        Ok(decode("flash.getConnMcText", reply)?)
    }

    pub(crate) fn get_game_object(
        loader: &impl Loader,
        path: &str,
    ) -> Result<Option<String>, CallError> {
        let call_args = vec![Value::String(path.to_owned())];
        let reply = loader.call("flash.getGameObject", call_args)?;
        Ok(decode("flash.getGameObject", reply)?)
    }

    pub(crate) fn get_game_object_key(
        loader: &impl Loader,
        path: &str,
        key: &str,
    ) -> Result<Option<String>, CallError> {
        let call_args = vec![
            Value::String(path.to_owned()),
            Value::String(key.to_owned()),
        ];
        let reply = loader.call("flash.getGameObjectKey", call_args)?;
        Ok(decode("flash.getGameObjectKey", reply)?)
    }

    pub(crate) fn get_game_object_s(
        loader: &impl Loader,
        path: &str,
    ) -> Result<Option<String>, CallError> {
        let call_args = vec![Value::String(path.to_owned())];
        let reply = loader.call("flash.getGameObjectS", call_args)?;
        Ok(decode("flash.getGameObjectS", reply)?)
    }

    pub(crate) fn hide_conn_mc(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("flash.hideConnMc", call_args)?;
        Ok(decode("flash.hideConnMc", reply)?)
    }

    pub(crate) fn is_conn_mc_back_button_visible(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("flash.isConnMcBackButtonVisible", call_args)?;
        Ok(decode("flash.isConnMcBackButtonVisible", reply)?)
    }

    pub(crate) fn is_null(loader: &impl Loader, path: &str) -> Result<bool, CallError> {
        let call_args = vec![Value::String(path.to_owned())];
        let reply = loader.call("flash.isNull", call_args)?;
        Ok(decode("flash.isNull", reply)?)
    }

    pub(crate) fn is_text_field_focused(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("flash.isTextFieldFocused", call_args)?;
        Ok(decode("flash.isTextFieldFocused", reply)?)
    }

    pub(crate) fn select_array_objects(
        loader: &impl Loader,
        path: &str,
        selector: &str,
    ) -> Result<Option<String>, CallError> {
        let call_args = vec![
            Value::String(path.to_owned()),
            Value::String(selector.to_owned()),
        ];
        let reply = loader.call("flash.selectArrayObjects", call_args)?;
        Ok(decode("flash.selectArrayObjects", reply)?)
    }

    pub(crate) fn send_client_packet(
        loader: &impl Loader,
        packet: &str,
        r#type: &str,
    ) -> Result<(), CallError> {
        let call_args = vec![
            Value::String(packet.to_owned()),
            Value::String(r#type.to_owned()),
        ];
        let reply = loader.call("flash.sendClientPacket", call_args)?;
        Ok(decode("flash.sendClientPacket", reply)?)
    }

    pub(crate) fn set_array_object(
        loader: &impl Loader,
        path: &str,
        index: i32,
        value: Value,
    ) -> Result<(), CallError> {
        let call_args = vec![
            Value::String(path.to_owned()),
            Value::Number(f64::from(index)),
            value,
        ];
        let reply = loader.call("flash.setArrayObject", call_args)?;
        Ok(decode("flash.setArrayObject", reply)?)
    }

    pub(crate) fn set_game_object(
        loader: &impl Loader,
        path: &str,
        value: Value,
    ) -> Result<(), CallError> {
        let call_args = vec![Value::String(path.to_owned()), value];
        let reply = loader.call("flash.setGameObject", call_args)?;
        Ok(decode("flash.setGameObject", reply)?)
    }

    pub(crate) fn set_game_object_key(
        loader: &impl Loader,
        path: &str,
        key: &str,
        value: Value,
    ) -> Result<(), CallError> {
        let call_args = vec![
            Value::String(path.to_owned()),
            Value::String(key.to_owned()),
            value,
        ];
        let reply = loader.call("flash.setGameObjectKey", call_args)?;
        Ok(decode("flash.setGameObjectKey", reply)?)
    }
}

pub(crate) mod house {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn get_item(loader: &impl Loader, selector: Value) -> Result<Value, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("house.getItem", call_args)?;
        Ok(decode("house.getItem", reply)?)
    }

    pub(crate) fn get_items(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("house.getItems", call_args)?;
        Ok(decode("house.getItems", reply)?)
    }

    pub(crate) fn get_slots(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("house.getSlots", call_args)?;
        Ok(decode("house.getSlots", reply)?)
    }

    pub(crate) fn get_used_slots(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("house.getUsedSlots", call_args)?;
        Ok(decode("house.getUsedSlots", reply)?)
    }
}

pub(crate) mod inventory {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn equip(loader: &impl Loader, selector: Value) -> Result<bool, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("inventory.equip", call_args)?;
        Ok(decode("inventory.equip", reply)?)
    }

    pub(crate) fn get_item(loader: &impl Loader, selector: Value) -> Result<Value, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("inventory.getItem", call_args)?;
        Ok(decode("inventory.getItem", reply)?)
    }

    pub(crate) fn get_items(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("inventory.getItems", call_args)?;
        Ok(decode("inventory.getItems", reply)?)
    }

    pub(crate) fn get_slots(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("inventory.getSlots", call_args)?;
        Ok(decode("inventory.getSlots", reply)?)
    }

    pub(crate) fn get_used_slots(loader: &impl Loader, pool: &str) -> Result<i32, CallError> {
        let call_args = vec![Value::String(pool.to_owned())];
        let reply = loader.call("inventory.getUsedSlots", call_args)?;
        Ok(decode("inventory.getUsedSlots", reply)?)
    }

    pub(crate) fn unequip(loader: &impl Loader, selector: Value) -> Result<bool, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("inventory.unequip", call_args)?;
        Ok(decode("inventory.unequip", reply)?)
    }

    pub(crate) fn r#use(loader: &impl Loader, selector: Value) -> Result<bool, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("inventory.use", call_args)?;
        Ok(decode("inventory.use", reply)?)
    }

    pub(crate) fn wear(loader: &impl Loader, selector: Value) -> Result<bool, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("inventory.wear", call_args)?;
        Ok(decode("inventory.wear", reply)?)
    }
}

pub(crate) mod outfits {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn equip(
        loader: &impl Loader,
        name: &str,
        keep_colors: Option<bool>,
    ) -> Result<bool, CallError> {
        let call_args = vec![
            Value::String(name.to_owned()),
            Value::Bool(keep_colors.unwrap_or(false)),
        ];
        let reply = loader.call("outfits.equip", call_args)?;
        Ok(decode("outfits.equip", reply)?)
    }

    pub(crate) fn get(loader: &impl Loader, name: &str) -> Result<Value, CallError> {
        let call_args = vec![Value::String(name.to_owned())];
        let reply = loader.call("outfits.get", call_args)?;
        Ok(decode("outfits.get", reply)?)
    }

    pub(crate) fn get_all(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("outfits.getAll", call_args)?;
        Ok(decode("outfits.getAll", reply)?)
    }

    pub(crate) fn wear(
        loader: &impl Loader,
        name: &str,
        keep_colors: Option<bool>,
    ) -> Result<bool, CallError> {
        let call_args = vec![
            Value::String(name.to_owned()),
            Value::Bool(keep_colors.unwrap_or(false)),
        ];
        let reply = loader.call("outfits.wear", call_args)?;
        Ok(decode("outfits.wear", reply)?)
    }
}

pub(crate) mod player {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn get_auras(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getAuras", call_args)?;
        Ok(decode("player.getAuras", reply)?)
    }

    pub(crate) fn get_cell(loader: &impl Loader) -> Result<Option<String>, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getCell", call_args)?;
        Ok(decode("player.getCell", reply)?)
    }

    pub(crate) fn get_char_id(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getCharId", call_args)?;
        Ok(decode("player.getCharId", reply)?)
    }

    pub(crate) fn get_class_name(loader: &impl Loader) -> Result<Option<String>, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getClassName", call_args)?;
        Ok(decode("player.getClassName", reply)?)
    }

    pub(crate) fn get_data(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getData", call_args)?;
        Ok(decode("player.getData", reply)?)
    }

    pub(crate) fn get_factions(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getFactions", call_args)?;
        Ok(decode("player.getFactions", reply)?)
    }

    pub(crate) fn get_gender(loader: &impl Loader) -> Result<Option<String>, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getGender", call_args)?;
        Ok(decode("player.getGender", reply)?)
    }

    pub(crate) fn get_gold(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getGold", call_args)?;
        Ok(decode("player.getGold", reply)?)
    }

    pub(crate) fn get_hp(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getHp", call_args)?;
        Ok(decode("player.getHp", reply)?)
    }

    pub(crate) fn get_level(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getLevel", call_args)?;
        Ok(decode("player.getLevel", reply)?)
    }

    pub(crate) fn get_map(loader: &impl Loader) -> Result<Option<String>, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getMap", call_args)?;
        Ok(decode("player.getMap", reply)?)
    }

    pub(crate) fn get_max_hp(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getMaxHp", call_args)?;
        Ok(decode("player.getMaxHp", reply)?)
    }

    pub(crate) fn get_max_mp(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getMaxMp", call_args)?;
        Ok(decode("player.getMaxMp", reply)?)
    }

    pub(crate) fn get_mp(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getMp", call_args)?;
        Ok(decode("player.getMp", reply)?)
    }

    pub(crate) fn get_pad(loader: &impl Loader) -> Result<Option<String>, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getPad", call_args)?;
        Ok(decode("player.getPad", reply)?)
    }

    pub(crate) fn get_position(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getPosition", call_args)?;
        Ok(decode("player.getPosition", reply)?)
    }

    pub(crate) fn get_state(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getState", call_args)?;
        Ok(decode("player.getState", reply)?)
    }

    pub(crate) fn get_user_id(loader: &impl Loader) -> Result<i32, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.getUserId", call_args)?;
        Ok(decode("player.getUserId", reply)?)
    }

    pub(crate) fn go_to_player(loader: &impl Loader, name: &str) -> Result<(), CallError> {
        let call_args = vec![Value::String(name.to_owned())];
        let reply = loader.call("player.goToPlayer", call_args)?;
        Ok(decode("player.goToPlayer", reply)?)
    }

    pub(crate) fn has_active_boost(
        loader: &impl Loader,
        boost_type: &str,
    ) -> Result<bool, CallError> {
        let call_args = vec![Value::String(boost_type.to_owned())];
        let reply = loader.call("player.hasActiveBoost", call_args)?;
        Ok(decode("player.hasActiveBoost", reply)?)
    }

    pub(crate) fn is_afk(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.isAfk", call_args)?;
        Ok(decode("player.isAfk", reply)?)
    }

    pub(crate) fn is_loaded(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.isLoaded", call_args)?;
        Ok(decode("player.isLoaded", reply)?)
    }

    pub(crate) fn is_member(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("player.isMember", call_args)?;
        Ok(decode("player.isMember", reply)?)
    }

    pub(crate) fn join_map(
        loader: &impl Loader,
        map: &str,
        cell: Option<&str>,
        pad: Option<&str>,
    ) -> Result<(), CallError> {
        let call_args = vec![
            Value::String(map.to_owned()),
            Value::String(cell.unwrap_or("Enter").to_owned()),
            Value::String(pad.unwrap_or("Spawn").to_owned()),
        ];
        let reply = loader.call("player.joinMap", call_args)?;
        Ok(decode("player.joinMap", reply)?)
    }

    pub(crate) fn jump(
        loader: &impl Loader,
        cell: &str,
        pad: Option<&str>,
    ) -> Result<(), CallError> {
        let call_args = vec![
            Value::String(cell.to_owned()),
            Value::String(pad.unwrap_or("Spawn").to_owned()),
        ];
        let reply = loader.call("player.jump", call_args)?;
        Ok(decode("player.jump", reply)?)
    }

    pub(crate) fn rest(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("player.rest", call_args)?;
        Ok(decode("player.rest", reply)?)
    }

    pub(crate) fn walk_to(
        loader: &impl Loader,
        x: i32,
        y: i32,
        walk_speed: Option<f64>,
    ) -> Result<bool, CallError> {
        let call_args = vec![
            Value::Number(f64::from(x)),
            Value::Number(f64::from(y)),
            Value::Number(walk_speed.unwrap_or(f64::NAN)),
        ];
        let reply = loader.call("player.walkTo", call_args)?;
        Ok(decode("player.walkTo", reply)?)
    }
}

pub(crate) mod players {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn get(loader: &impl Loader, username: &str) -> Result<Value, CallError> {
        let call_args = vec![Value::String(username.to_owned())];
        let reply = loader.call("players.get", call_args)?;
        Ok(decode("players.get", reply)?)
    }

    pub(crate) fn get_all(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("players.getAll", call_args)?;
        Ok(decode("players.getAll", reply)?)
    }
}

pub(crate) mod quests {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn abandon(loader: &impl Loader, quest_id: i32) -> Result<bool, CallError> {
        let call_args = vec![Value::Number(f64::from(quest_id))];
        let reply = loader.call("quests.abandon", call_args)?;
        Ok(decode("quests.abandon", reply)?)
    }

    pub(crate) fn accept(loader: &impl Loader, quest_id: i32) -> Result<bool, CallError> {
        let call_args = vec![Value::Number(f64::from(quest_id))];
        let reply = loader.call("quests.accept", call_args)?;
        Ok(decode("quests.accept", reply)?)
    }

    pub(crate) fn can_complete(loader: &impl Loader, quest_id: i32) -> Result<bool, CallError> {
        let call_args = vec![Value::Number(f64::from(quest_id))];
        let reply = loader.call("quests.canComplete", call_args)?;
        Ok(decode("quests.canComplete", reply)?)
    }

    pub(crate) fn complete(
        loader: &impl Loader,
        quest_id: i32,
        turn_ins: i32,
        reward_item_id: i32,
    ) -> Result<bool, CallError> {
        let call_args = vec![
            Value::Number(f64::from(quest_id)),
            Value::Number(f64::from(turn_ins)),
            Value::Number(f64::from(reward_item_id)),
        ];
        let reply = loader.call("quests.complete", call_args)?;
        Ok(decode("quests.complete", reply)?)
    }

    pub(crate) fn get(loader: &impl Loader, quest_id: i32) -> Result<Value, CallError> {
        let call_args = vec![Value::Number(f64::from(quest_id))];
        let reply = loader.call("quests.get", call_args)?;
        Ok(decode("quests.get", reply)?)
    }

    pub(crate) fn get_accepted(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("quests.getAccepted", call_args)?;
        Ok(decode("quests.getAccepted", reply)?)
    }

    pub(crate) fn get_max_turn_ins(loader: &impl Loader, quest_id: i32) -> Result<i32, CallError> {
        let call_args = vec![Value::Number(f64::from(quest_id))];
        let reply = loader.call("quests.getMaxTurnIns", call_args)?;
        Ok(decode("quests.getMaxTurnIns", reply)?)
    }

    pub(crate) fn get_tree(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("quests.getTree", call_args)?;
        Ok(decode("quests.getTree", reply)?)
    }

    pub(crate) fn is_available(loader: &impl Loader, quest_id: i32) -> Result<bool, CallError> {
        let call_args = vec![Value::Number(f64::from(quest_id))];
        let reply = loader.call("quests.isAvailable", call_args)?;
        Ok(decode("quests.isAvailable", reply)?)
    }

    pub(crate) fn is_in_progress(loader: &impl Loader, quest_id: i32) -> Result<bool, CallError> {
        let call_args = vec![Value::Number(f64::from(quest_id))];
        let reply = loader.call("quests.isInProgress", call_args)?;
        Ok(decode("quests.isInProgress", reply)?)
    }

    pub(crate) fn load(
        loader: &impl Loader,
        quest_ids: &str,
        silent: bool,
    ) -> Result<Option<String>, CallError> {
        let call_args = vec![Value::String(quest_ids.to_owned()), Value::Bool(silent)];
        let reply = loader.call("quests.load", call_args)?;
        Ok(decode("quests.load", reply)?)
    }
}

pub(crate) mod settings {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn enemy_magnet(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("settings.enemyMagnet", call_args)?;
        Ok(decode("settings.enemyMagnet", reply)?)
    }

    pub(crate) fn infinite_range(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("settings.infiniteRange", call_args)?;
        Ok(decode("settings.infiniteRange", reply)?)
    }

    pub(crate) fn provoke_cell(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("settings.provokeCell", call_args)?;
        Ok(decode("settings.provokeCell", reply)?)
    }

    pub(crate) fn reset_custom_guild(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("settings.resetCustomGuild", call_args)?;
        Ok(decode("settings.resetCustomGuild", reply)?)
    }

    pub(crate) fn reset_custom_name(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("settings.resetCustomName", call_args)?;
        Ok(decode("settings.resetCustomName", reply)?)
    }

    pub(crate) fn set_ads_visible(loader: &impl Loader, visible: bool) -> Result<(), CallError> {
        let call_args = vec![Value::Bool(visible)];
        let reply = loader.call("settings.setAdsVisible", call_args)?;
        Ok(decode("settings.setAdsVisible", reply)?)
    }

    pub(crate) fn set_animations_enabled(
        loader: &impl Loader,
        enabled: bool,
    ) -> Result<(), CallError> {
        let call_args = vec![Value::Bool(enabled)];
        let reply = loader.call("settings.setAnimationsEnabled", call_args)?;
        Ok(decode("settings.setAnimationsEnabled", reply)?)
    }

    pub(crate) fn set_collisions_enabled(
        loader: &impl Loader,
        enabled: bool,
    ) -> Result<(), CallError> {
        let call_args = vec![Value::Bool(enabled)];
        let reply = loader.call("settings.setCollisionsEnabled", call_args)?;
        Ok(decode("settings.setCollisionsEnabled", reply)?)
    }

    pub(crate) fn set_custom_guild(loader: &impl Loader, name: &str) -> Result<(), CallError> {
        let call_args = vec![Value::String(name.to_owned())];
        let reply = loader.call("settings.setCustomGuild", call_args)?;
        Ok(decode("settings.setCustomGuild", reply)?)
    }

    pub(crate) fn set_custom_name(loader: &impl Loader, name: &str) -> Result<(), CallError> {
        let call_args = vec![Value::String(name.to_owned())];
        let reply = loader.call("settings.setCustomName", call_args)?;
        Ok(decode("settings.setCustomName", reply)?)
    }

    pub(crate) fn set_frame_rate(loader: &impl Loader, fps: i32) -> Result<(), CallError> {
        let call_args = vec![Value::Number(f64::from(fps))];
        let reply = loader.call("settings.setFrameRate", call_args)?;
        Ok(decode("settings.setFrameRate", reply)?)
    }

    pub(crate) fn set_other_players_visible(
        loader: &impl Loader,
        visible: bool,
    ) -> Result<(), CallError> {
        let call_args = vec![Value::Bool(visible)];
        let reply = loader.call("settings.setOtherPlayersVisible", call_args)?;
        Ok(decode("settings.setOtherPlayersVisible", reply)?)
    }

    pub(crate) fn set_walk_speed(loader: &impl Loader, speed: i32) -> Result<(), CallError> {
        let call_args = vec![Value::Number(f64::from(speed))];
        let reply = loader.call("settings.setWalkSpeed", call_args)?;
        Ok(decode("settings.setWalkSpeed", reply)?)
    }

    pub(crate) fn set_world_visible(loader: &impl Loader, visible: bool) -> Result<(), CallError> {
        let call_args = vec![Value::Bool(visible)];
        let reply = loader.call("settings.setWorldVisible", call_args)?;
        Ok(decode("settings.setWorldVisible", reply)?)
    }

    pub(crate) fn skip_cutscenes(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("settings.skipCutscenes", call_args)?;
        Ok(decode("settings.skipCutscenes", reply)?)
    }
}

pub(crate) mod shops {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn buy(
        loader: &impl Loader,
        selector: Value,
        quantity: Option<i32>,
    ) -> Result<bool, CallError> {
        let call_args = vec![selector, Value::Number(f64::from(quantity.unwrap_or(1)))];
        let reply = loader.call("shops.buy", call_args)?;
        Ok(decode("shops.buy", reply)?)
    }

    pub(crate) fn can_buy_item(
        loader: &impl Loader,
        selector: Value,
        quantity: Option<i32>,
    ) -> Result<bool, CallError> {
        let call_args = vec![selector, Value::Number(f64::from(quantity.unwrap_or(1)))];
        let reply = loader.call("shops.canBuyItem", call_args)?;
        Ok(decode("shops.canBuyItem", reply)?)
    }

    pub(crate) fn can_enhance(loader: &impl Loader, selector: Value) -> Result<bool, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("shops.canEnhance", call_args)?;
        Ok(decode("shops.canEnhance", reply)?)
    }

    pub(crate) fn close(loader: &impl Loader, shop_id: Option<i32>) -> Result<bool, CallError> {
        let call_args = vec![Value::Number(f64::from(shop_id.unwrap_or(0)))];
        let reply = loader.call("shops.close", call_args)?;
        Ok(decode("shops.close", reply)?)
    }

    pub(crate) fn enhance(
        loader: &impl Loader,
        selector: Value,
        item_id: i32,
    ) -> Result<bool, CallError> {
        let call_args = vec![selector, Value::Number(f64::from(item_id))];
        let reply = loader.call("shops.enhance", call_args)?;
        Ok(decode("shops.enhance", reply)?)
    }

    pub(crate) fn get_item(loader: &impl Loader, selector: Value) -> Result<Value, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("shops.getItem", call_args)?;
        Ok(decode("shops.getItem", reply)?)
    }

    pub(crate) fn get_max_buy_quantity(
        loader: &impl Loader,
        selector: Value,
    ) -> Result<i32, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("shops.getMaxBuyQuantity", call_args)?;
        Ok(decode("shops.getMaxBuyQuantity", reply)?)
    }

    pub(crate) fn get_shop(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("shops.getShop", call_args)?;
        Ok(decode("shops.getShop", reply)?)
    }

    pub(crate) fn is_open(loader: &impl Loader, shop_id: Option<i32>) -> Result<bool, CallError> {
        let call_args = vec![Value::Number(f64::from(shop_id.unwrap_or(0)))];
        let reply = loader.call("shops.isOpen", call_args)?;
        Ok(decode("shops.isOpen", reply)?)
    }

    pub(crate) fn load(loader: &impl Loader, shop_id: i32) -> Result<(), CallError> {
        let call_args = vec![Value::Number(f64::from(shop_id))];
        let reply = loader.call("shops.load", call_args)?;
        Ok(decode("shops.load", reply)?)
    }

    pub(crate) fn sell(
        loader: &impl Loader,
        selector: Value,
        quantity: Option<i32>,
    ) -> Result<bool, CallError> {
        let call_args = vec![selector, Value::Number(f64::from(quantity.unwrap_or(-1)))];
        let reply = loader.call("shops.sell", call_args)?;
        Ok(decode("shops.sell", reply)?)
    }
}

pub(crate) mod temp_inventory {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn get_item(loader: &impl Loader, selector: Value) -> Result<Value, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("tempInventory.getItem", call_args)?;
        Ok(decode("tempInventory.getItem", reply)?)
    }

    pub(crate) fn get_items(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("tempInventory.getItems", call_args)?;
        Ok(decode("tempInventory.getItems", reply)?)
    }
}

pub(crate) mod world {
    use crate::bridge::{CallError, Loader, Value, decode};

    pub(crate) fn get_available_monster_map_ids(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("world.getAvailableMonsterMapIds", call_args)?;
        Ok(decode("world.getAvailableMonsterMapIds", reply)?)
    }

    pub(crate) fn get_cell_pads(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("world.getCellPads", call_args)?;
        Ok(decode("world.getCellPads", reply)?)
    }

    pub(crate) fn get_cells(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("world.getCells", call_args)?;
        Ok(decode("world.getCells", reply)?)
    }

    pub(crate) fn get_map_item(loader: &impl Loader, item_id: i32) -> Result<(), CallError> {
        let call_args = vec![Value::Number(f64::from(item_id))];
        let reply = loader.call("world.getMapItem", call_args)?;
        Ok(decode("world.getMapItem", reply)?)
    }

    pub(crate) fn get_monster(loader: &impl Loader, selector: Value) -> Result<Value, CallError> {
        let call_args = vec![selector];
        let reply = loader.call("world.getMonster", call_args)?;
        Ok(decode("world.getMonster", reply)?)
    }

    pub(crate) fn get_monsters(loader: &impl Loader) -> Result<Value, CallError> {
        let call_args = vec![];
        let reply = loader.call("world.getMonsters", call_args)?;
        Ok(decode("world.getMonsters", reply)?)
    }

    pub(crate) fn is_action_available(
        loader: &impl Loader,
        game_action: crate::bridge::GameAction,
    ) -> Result<bool, CallError> {
        let call_args = vec![Value::from(game_action)];
        let reply = loader.call("world.isActionAvailable", call_args)?;
        Ok(decode("world.isActionAvailable", reply)?)
    }

    pub(crate) fn is_cell_ready(
        loader: &impl Loader,
        cell: &str,
        pad: Option<&str>,
    ) -> Result<bool, CallError> {
        let call_args = vec![
            Value::String(cell.to_owned()),
            pad.map_or(Value::Null, |value| Value::String(value.to_owned())),
        ];
        let reply = loader.call("world.isCellReady", call_args)?;
        Ok(decode("world.isCellReady", reply)?)
    }

    pub(crate) fn is_loaded(loader: &impl Loader) -> Result<bool, CallError> {
        let call_args = vec![];
        let reply = loader.call("world.isLoaded", call_args)?;
        Ok(decode("world.isLoaded", reply)?)
    }

    pub(crate) fn is_monster_available(
        loader: &impl Loader,
        monster_map_id: f64,
    ) -> Result<bool, CallError> {
        let call_args = vec![Value::Number(monster_map_id)];
        let reply = loader.call("world.isMonsterAvailable", call_args)?;
        Ok(decode("world.isMonsterAvailable", reply)?)
    }

    pub(crate) fn load_swf(loader: &impl Loader, swf: &str) -> Result<(), CallError> {
        let call_args = vec![Value::String(swf.to_owned())];
        let reply = loader.call("world.loadSwf", call_args)?;
        Ok(decode("world.loadSwf", reply)?)
    }

    pub(crate) fn reload(loader: &impl Loader) -> Result<(), CallError> {
        let call_args = vec![];
        let reply = loader.call("world.reload", call_args)?;
        Ok(decode("world.reload", reply)?)
    }

    pub(crate) fn request_monster_drops(
        loader: &impl Loader,
        monster_map_id: f64,
    ) -> Result<bool, CallError> {
        let call_args = vec![Value::Number(monster_map_id)];
        let reply = loader.call("world.requestMonsterDrops", call_args)?;
        Ok(decode("world.requestMonsterDrops", reply)?)
    }

    pub(crate) fn set_spawn_point(
        loader: &impl Loader,
        cell: Option<&str>,
        pad: Option<&str>,
    ) -> Result<(), CallError> {
        let call_args = vec![
            cell.map_or(Value::Null, |value| Value::String(value.to_owned())),
            pad.map_or(Value::Null, |value| Value::String(value.to_owned())),
        ];
        let reply = loader.call("world.setSpawnPoint", call_args)?;
        Ok(decode("world.setSpawnPoint", reply)?)
    }
}
