use serde::{Deserialize, Deserializer};
use serde_json::json;

use super::calls::Reply;
use crate::automation::{
    auto_relogin,
    auto_zone::{self, Room},
    settings::Automation,
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum SettingsRequest {
    Get,
    Update {
        infinite_range: Option<bool>,
        provoke_cell: Option<bool>,
        enemy_magnet: Option<bool>,
        skip_cutscenes: Option<bool>,
        hide_players: Option<bool>,
        lag_killer: Option<bool>,
        animations: Option<bool>,
        collisions: Option<bool>,
        ads: Option<bool>,
        walk_speed: Option<u32>,
        #[serde(default, deserialize_with = "present")]
        custom_name: Option<Option<String>>,
        #[serde(default, deserialize_with = "present")]
        custom_guild: Option<Option<String>>,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ReloginRequest {
    Get,
    Update {
        enabled: Option<bool>,
        #[serde(default, deserialize_with = "present")]
        server: Option<Option<String>>,
        delay_seconds: Option<u32>,
        retries: Option<u32>,
        backoff_seconds: Option<u32>,
        backoff_cap_seconds: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ZoneRequest {
    Get,
    Update {
        enabled: Option<bool>,
        #[serde(default, deserialize_with = "present")]
        map: Option<Option<Room>>,
    },
}

fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

pub(super) fn serve_relogin(automation: &Automation, request: ReloginRequest) -> Reply {
    let settings = match request {
        ReloginRequest::Get => automation.auto_relogin(),
        ReloginRequest::Update {
            enabled,
            server,
            delay_seconds,
            retries,
            backoff_seconds,
            backoff_cap_seconds,
        } => automation.update_auto_relogin(|settings| {
            apply(&mut settings.enabled, enabled);
            apply(&mut settings.server, server);
            apply(&mut settings.delay_seconds, delay_seconds);
            apply(&mut settings.retries, retries);
            apply(&mut settings.backoff_seconds, backoff_seconds);
            apply(&mut settings.backoff_cap_seconds, backoff_cap_seconds);
        }),
    };
    Ok(relogin(&settings))
}

pub(super) fn serve_zone(automation: &Automation, request: ZoneRequest) -> Reply {
    let settings = match request {
        ZoneRequest::Get => automation.auto_zone(),
        ZoneRequest::Update { enabled, map } => automation.update_auto_zone(|settings| {
            apply(&mut settings.enabled, enabled);
            apply(&mut settings.room, map);
        }),
    };
    Ok(zone(settings))
}

pub(super) fn serve_settings(automation: &Automation, request: SettingsRequest) -> Reply {
    let settings = match request {
        SettingsRequest::Get => automation.game_settings(),
        SettingsRequest::Update {
            infinite_range,
            provoke_cell,
            enemy_magnet,
            skip_cutscenes,
            hide_players,
            lag_killer,
            animations,
            collisions,
            ads,
            walk_speed,
            custom_name,
            custom_guild,
        } => automation.update_game_settings(|settings| {
            apply(&mut settings.infinite_range, infinite_range);
            apply(&mut settings.provoke_cell, provoke_cell);
            apply(&mut settings.enemy_magnet, enemy_magnet);
            apply(&mut settings.skip_cutscenes, skip_cutscenes);
            apply(&mut settings.hide_players, hide_players);
            apply(&mut settings.lag_killer, lag_killer);
            apply(&mut settings.animations, animations);
            apply(&mut settings.collisions, collisions);
            apply(&mut settings.ads, ads);
            apply(&mut settings.walk_speed, walk_speed);
            apply(&mut settings.custom_name, custom_name);
            apply(&mut settings.custom_guild, custom_guild);
        }),
    };
    Ok(json!(settings))
}

fn apply<T>(field: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *field = value;
    }
}

fn relogin(settings: &auto_relogin::Settings) -> serde_json::Value {
    json!({
        "enabled": settings.enabled,
        "server": settings.server,
        "delaySeconds": settings.delay_seconds,
        "retries": settings.retries,
        "backoffSeconds": settings.backoff_seconds,
        "backoffCapSeconds": settings.backoff_cap_seconds,
    })
}

fn zone(settings: auto_zone::Settings) -> serde_json::Value {
    json!({ "enabled": settings.enabled, "map": settings.room })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relogin_request(value: serde_json::Value) -> ReloginRequest {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn an_update_changes_only_the_fields_it_names() {
        let automation = Automation::default();
        automation.update_auto_relogin(|settings| settings.server = Some("Twilly".into()));
        let reply = serve_relogin(
            &automation,
            relogin_request(json!({ "op": "update", "enabled": true, "retries": 5 })),
        );
        assert_eq!(
            reply,
            Ok(json!({
                "enabled": true,
                "server": "Twilly",
                "delaySeconds": 3,
                "retries": 5,
                "backoffSeconds": 5,
                "backoffCapSeconds": 60,
            }))
        );
        assert!(automation.auto_relogin().enabled);
    }

    #[test]
    fn a_null_server_clears_it_and_out_of_range_values_come_back_clamped() {
        let automation = Automation::default();
        automation.update_auto_relogin(|settings| settings.server = Some("Twilly".into()));
        let reply = serve_relogin(
            &automation,
            relogin_request(json!({ "op": "update", "server": null, "delaySeconds": 99999 })),
        )
        .unwrap();
        assert_eq!(reply["server"], json!(null));
        assert_eq!(
            reply["delaySeconds"],
            json!(auto_relogin::Settings::MAX_DELAY_SECONDS)
        );
        assert_eq!(automation.auto_relogin().server, None);
    }

    #[test]
    fn auto_zone_reads_and_writes_the_map() {
        let automation = Automation::default();
        let update =
            serde_json::from_value(json!({ "op": "update", "enabled": true, "map": "queeniona" }))
                .unwrap();
        assert_eq!(
            serve_zone(&automation, update),
            Ok(json!({ "enabled": true, "map": "queeniona" }))
        );
        assert_eq!(automation.auto_zone().room, Some(Room::Queeniona));
        let clear = serde_json::from_value(json!({ "op": "update", "map": null })).unwrap();
        assert_eq!(
            serve_zone(&automation, clear),
            Ok(json!({ "enabled": true, "map": null }))
        );
    }

    #[test]
    fn a_settings_update_returns_every_setting_as_stored() {
        let automation = Automation::default();
        let update = serde_json::from_value(json!({
            "op": "update",
            "hidePlayers": true,
            "walkSpeed": 500,
            "customName": "  player1  ",
        }))
        .unwrap();
        assert_eq!(
            serve_settings(&automation, update),
            Ok(json!({
                "infiniteRange": false,
                "provokeCell": false,
                "enemyMagnet": false,
                "skipCutscenes": false,
                "hidePlayers": true,
                "lagKiller": false,
                "animations": true,
                "collisions": true,
                "ads": true,
                "walkSpeed": 99,
                "customName": "player1",
                "customGuild": null,
            }))
        );
        let clear = serde_json::from_value(json!({ "op": "update", "customName": null })).unwrap();
        assert_eq!(
            serve_settings(&automation, clear).unwrap()["customName"],
            json!(null)
        );
        assert!(automation.game_settings().hide_players);
    }

    #[test]
    fn unknown_fields_and_maps_are_rejected() {
        for value in [
            json!({ "op": "update", "map": "battleon" }),
            json!({ "op": "update", "room": "queeniona" }),
        ] {
            assert!(serde_json::from_value::<ZoneRequest>(value).is_err());
        }
        assert!(
            serde_json::from_value::<ReloginRequest>(json!({ "op": "update", "delay": 3 }))
                .is_err()
        );
    }
}
