use serde::Deserialize;
use serde_json::json;

use super::{automation::apply, calls::Reply};
use crate::{
    automation::{quests::Registered, register_items, settings::Automation, unregister_items},
    domain::inventory::ItemQuery,
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum QuestRequest {
    Register {
        quests: Vec<Registered>,
    },
    Unregister {
        quest_ids: Vec<i32>,
    },
    UnregisterAll,
    Registered,
    Config,
    Configure {
        enabled: Option<bool>,
        accept_rewards: Option<bool>,
        accept_requirements: Option<bool>,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum DropRequest {
    Register {
        items: Vec<ItemQuery>,
    },
    Unregister {
        items: Vec<ItemQuery>,
    },
    UnregisterAll,
    Registered,
    Config,
    Configure {
        enabled: Option<bool>,
        accept_ac: Option<bool>,
        accept_ac_member_only: Option<bool>,
        accept_non_ac: Option<bool>,
        accept_non_ac_member_only: Option<bool>,
        reject_others: Option<bool>,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum BoostRequest {
    Register { items: Vec<ItemQuery> },
    Unregister { items: Vec<ItemQuery> },
    UnregisterAll,
    Registered,
    Config,
    Configure { enabled: Option<bool> },
}

pub(super) fn serve_quests(automation: &Automation, request: QuestRequest) -> Reply {
    Ok(match request {
        QuestRequest::Register { quests } => {
            automation.update_quests(|state| state.register(quests));
            json!(null)
        }
        QuestRequest::Unregister { quest_ids } => {
            automation.update_quests(|state| state.unregister(&quest_ids));
            json!(null)
        }
        QuestRequest::UnregisterAll => {
            automation.update_quests(|state| state.quests.clear());
            json!(null)
        }
        QuestRequest::Registered => json!(automation.quests().quests),
        QuestRequest::Config => json!(automation.quests().config),
        QuestRequest::Configure {
            enabled,
            accept_rewards,
            accept_requirements,
        } => json!(
            automation
                .update_quests(|state| {
                    let config = &mut state.config;
                    apply(&mut config.enabled, enabled);
                    apply(&mut config.accept_rewards, accept_rewards);
                    apply(&mut config.accept_requirements, accept_requirements);
                })
                .config
        ),
    })
}

pub(super) fn serve_drops(automation: &Automation, request: DropRequest) -> Reply {
    Ok(match request {
        DropRequest::Register { items } => {
            automation.update_drops(|state| register_items(&mut state.items, items));
            json!(null)
        }
        DropRequest::Unregister { items } => {
            automation.update_drops(|state| state.unregister(&items));
            json!(null)
        }
        DropRequest::UnregisterAll => {
            automation.update_drops(|state| state.clear());
            json!(null)
        }
        DropRequest::Registered => json!(automation.drops().items),
        DropRequest::Config => json!(automation.drops().config),
        DropRequest::Configure {
            enabled,
            accept_ac,
            accept_ac_member_only,
            accept_non_ac,
            accept_non_ac_member_only,
            reject_others,
        } => json!(
            automation
                .update_drops(|state| {
                    let config = &mut state.config;
                    apply(&mut config.enabled, enabled);
                    apply(&mut config.accept_ac, accept_ac);
                    apply(&mut config.accept_ac_member_only, accept_ac_member_only);
                    apply(&mut config.accept_non_ac, accept_non_ac);
                    apply(
                        &mut config.accept_non_ac_member_only,
                        accept_non_ac_member_only,
                    );
                    apply(&mut config.reject_others, reject_others);
                })
                .config
        ),
    })
}

pub(super) fn serve_boosts(automation: &Automation, request: BoostRequest) -> Reply {
    Ok(match request {
        BoostRequest::Register { items } => {
            automation.update_boosts(|state| register_items(&mut state.items, items));
            json!(null)
        }
        BoostRequest::Unregister { items } => {
            automation.update_boosts(|state| unregister_items(&mut state.items, &items));
            json!(null)
        }
        BoostRequest::UnregisterAll => {
            automation.update_boosts(|state| state.items.clear());
            json!(null)
        }
        BoostRequest::Registered => json!(automation.boosts().items),
        BoostRequest::Config => json!(automation.boosts().config),
        BoostRequest::Configure { enabled } => json!(
            automation
                .update_boosts(|state| apply(&mut state.config.enabled, enabled))
                .config
        ),
    })
}

#[cfg(test)]
mod tests {
    use serde::de::DeserializeOwned;

    use super::*;

    fn request<T: DeserializeOwned>(value: serde_json::Value) -> T {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn quests_register_once_and_a_second_registration_replaces_the_reward() {
        let automation = Automation::default();
        for quests in [
            json!([{ "questId": 7 }, { "questId": 8, "rewardItemId": 50 }, { "questId": 0 }]),
            json!([{ "questId": 8, "rewardItemId": 51 }, { "questId": 7 }]),
        ] {
            serve_quests(
                &automation,
                request(json!({ "op": "register", "quests": quests })),
            )
            .unwrap();
        }
        assert_eq!(
            serve_quests(&automation, request(json!({ "op": "registered" }))),
            Ok(json!([
                { "questId": 7, "rewardItemId": null },
                { "questId": 8, "rewardItemId": 51 },
            ]))
        );
        serve_quests(
            &automation,
            request(json!({ "op": "unregister", "questIds": [7, 99] })),
        )
        .unwrap();
        assert_eq!(automation.quests().quests.len(), 1);
    }

    #[test]
    fn items_match_names_ignoring_case_and_ids_separately() {
        let automation = Automation::default();
        serve_drops(
            &automation,
            request(json!({ "op": "register", "items": ["Gem", " gem ", { "itemId": 5 }, ""] })),
        )
        .unwrap();
        assert_eq!(
            serve_drops(&automation, request(json!({ "op": "registered" }))),
            Ok(json!(["Gem", { "itemId": 5 }]))
        );
        serve_drops(
            &automation,
            request(json!({ "op": "unregister", "items": ["GEM"] })),
        )
        .unwrap();
        assert_eq!(
            serve_drops(&automation, request(json!({ "op": "registered" }))),
            Ok(json!([{ "itemId": 5 }]))
        );
        serve_drops(&automation, request(json!({ "op": "unregisterAll" }))).unwrap();
        assert!(automation.drops().items.is_empty());
    }

    #[test]
    fn configure_changes_only_the_given_fields_and_keeps_registrations() {
        let automation = Automation::default();
        serve_boosts(
            &automation,
            request(json!({ "op": "register", "items": ["XP Boost"] })),
        )
        .unwrap();
        assert_eq!(
            serve_boosts(
                &automation,
                request(json!({ "op": "configure", "enabled": false }))
            ),
            Ok(json!({ "enabled": false }))
        );
        assert_eq!(automation.boosts().items.len(), 1);
        assert_eq!(
            serve_drops(
                &automation,
                request(json!({ "op": "configure", "acceptAc": true, "rejectOthers": true }))
            ),
            Ok(json!({
                "enabled": true,
                "acceptAc": true,
                "acceptAcMemberOnly": false,
                "acceptNonAc": false,
                "acceptNonAcMemberOnly": false,
                "rejectOthers": true,
            }))
        );
        assert!(
            serde_json::from_value::<QuestRequest>(json!({ "op": "configure", "accept": true }))
                .is_err()
        );
    }
}
