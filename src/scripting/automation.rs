use serde::{Deserialize, Deserializer};
use serde_json::json;

use super::calls::Reply;
use crate::automation::{auto_relogin, game_settings::RenderingMode, settings::Automation};

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
        rendering: Option<RenderingMode>,
        animations: Option<bool>,
        collisions: Option<bool>,
        ads: Option<bool>,
        walk_speed: Option<u32>,
        frame_rate: Option<u32>,
        #[serde(default, deserialize_with = "present")]
        custom_name: Option<Option<String>>,
        #[serde(default, deserialize_with = "present")]
        custom_guild: Option<Option<String>>,
        anti_counter: Option<bool>,
        auto_zone: Option<bool>,
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

pub(super) fn serve_settings(automation: &Automation, request: SettingsRequest) -> Reply {
    let (settings, frame_rate) = match request {
        SettingsRequest::Get => (automation.game_settings(), automation.frame_rate()),
        SettingsRequest::Update {
            infinite_range,
            provoke_cell,
            enemy_magnet,
            skip_cutscenes,
            hide_players,
            rendering,
            animations,
            collisions,
            ads,
            walk_speed,
            frame_rate,
            custom_name,
            custom_guild,
            anti_counter,
            auto_zone,
        } => {
            let settings = automation.update_game_settings(|settings| {
                apply(&mut settings.infinite_range, infinite_range);
                apply(&mut settings.provoke_cell, provoke_cell);
                apply(&mut settings.enemy_magnet, enemy_magnet);
                apply(&mut settings.skip_cutscenes, skip_cutscenes);
                apply(&mut settings.hide_players, hide_players);
                if let Some(mode) = rendering {
                    settings.rendering.set(mode);
                }
                apply(&mut settings.animations, animations);
                apply(&mut settings.collisions, collisions);
                apply(&mut settings.ads, ads);
                apply(&mut settings.walk_speed, walk_speed);
                apply(&mut settings.custom_name, custom_name);
                apply(&mut settings.custom_guild, custom_guild);
                apply(&mut settings.anti_counter, anti_counter);
                apply(&mut settings.auto_zone, auto_zone);
            });
            let frame_rate = match frame_rate {
                Some(fps) => automation.set_frame_rate(fps),
                None => automation.frame_rate(),
            };
            (settings, frame_rate)
        }
    };
    let mut reply = json!(settings);
    reply["frameRate"] = json!(frame_rate);
    Ok(reply)
}

pub(super) fn apply<T>(field: &mut T, value: Option<T>) {
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
    fn a_settings_update_returns_every_setting_as_stored() {
        let automation = Automation::default();
        let update = serde_json::from_value(json!({
            "op": "update",
            "hidePlayers": true,
            "autoZone": true,
            "rendering": "interface-only",
            "walkSpeed": 500,
            "frameRate": 500,
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
                "rendering": "interface-only",
                "animations": true,
                "collisions": true,
                "ads": true,
                "walkSpeed": 99,
                "frameRate": 60,
                "customName": "player1",
                "customGuild": null,
                "antiCounter": true,
                "autoZone": true,
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
    fn unknown_fields_are_rejected() {
        assert!(
            serde_json::from_value::<ReloginRequest>(json!({ "op": "update", "delay": 3 }))
                .is_err()
        );
    }
}
