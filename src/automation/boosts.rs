use std::time::Duration;

use serde::Serialize;
use tokio::{sync::watch, time::sleep};

use crate::{
    bridge::{CallError, Event, Loader, Subscription},
    domain::{
        self,
        inventory::{self, ItemQuery},
        player,
    },
};

const CHECK_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Config {
    pub(crate) enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct State {
    pub(crate) items: Vec<ItemQuery>,
    pub(crate) config: Config,
}

pub(crate) async fn run(
    loader: impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    mut state: watch::Receiver<State>,
) {
    let mut warned = Vec::new();
    loop {
        let current = state.borrow_and_update().clone();
        let active = current.config.enabled && !current.items.is_empty();
        if active && let Err(error) = settle(&loader, &subscribe, &current.items, &mut warned).await
        {
            tracing::debug!(%error, "registered boosts skipped a check");
        }
        tokio::select! {
            changed = state.changed() => if changed.is_err() {
                return;
            },
            () = sleep(CHECK_EVERY), if active => {}
        }
    }
}

async fn settle(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    items: &[ItemQuery],
    warned: &mut Vec<ItemQuery>,
) -> Result<(), CallError> {
    if !domain::ready(loader)? {
        return Ok(());
    }
    let mut checked = Vec::new();
    for query in items {
        let Some(item) = inventory::item(loader, query)? else {
            continue;
        };
        let Some(kind) = item.boost() else {
            if !warned.iter().any(|known| known.same(query)) {
                tracing::warn!(?query, "a registered boost isn't a boost item");
                warned.push(query.clone());
            }
            continue;
        };
        if checked.contains(&kind) {
            continue;
        }
        checked.push(kind);
        if !player::has_boost(loader, kind)? {
            inventory::use_item(loader, subscribe, &item.query()).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use serde_json::json;
    use tokio::task::LocalSet;

    use super::*;
    use crate::{
        automation::register_items,
        bridge::{Events, Value},
        domain::testing::FakeLoader,
    };

    const ITEMS: [(f64, &str, &str, &str); 4] = [
        (1.0, "Short XP Boost", "ServerUse", "xpboost::20"),
        (2.0, "Long XP Boost", "ServerUse", "xpboost::60"),
        (3.0, "Gold Boost", "ServerUse", "gboost::60"),
        (4.0, "Potion", "Item", ""),
    ];

    #[derive(Default)]
    struct Avatar {
        active: Vec<String>,
        used: Vec<&'static str>,
    }

    fn start(active: &[&str]) -> (Rc<RefCell<Avatar>>, watch::Sender<State>) {
        let avatar = Rc::new(RefCell::new(Avatar {
            active: active.iter().map(|kind| (*kind).to_owned()).collect(),
            ..Avatar::default()
        }));
        let events = Rc::new(Events::default());
        let (state, publisher) = (avatar.clone(), events.clone());
        let loader = FakeLoader::new(move |callback, args| {
            let mut avatar = state.borrow_mut();
            let find = || {
                let [Value::Object(selector)] = args else {
                    panic!("{callback} needs a selector: {args:?}");
                };
                ITEMS.iter().find(|(id, name, ..)| match selector.get("itemId") {
                    Some(Value::Number(wanted)) => id == wanted,
                    _ => matches!(selector.get("name"), Some(Value::String(wanted)) if wanted.eq_ignore_ascii_case(name)),
                })
            };
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" | "player.isLoaded" => Value::Bool(true),
                "inventory.getItem" => match find() {
                    Some((id, name, category, link)) => Value::Object(
                        [
                            ("itemId", Value::Number(*id)),
                            ("name", Value::String((*name).into())),
                            ("category", Value::String((*category).into())),
                            ("link", Value::String((*link).into())),
                        ]
                        .into_iter()
                        .map(|(key, value)| (key.to_owned(), value))
                        .collect(),
                    ),
                    None => Value::Null,
                },
                "player.hasActiveBoost" => {
                    let [Value::String(kind)] = args else {
                        panic!("hasActiveBoost needs a kind: {args:?}");
                    };
                    Value::Bool(avatar.active.contains(kind))
                }
                "inventory.use" => {
                    let (id, name, _, link) = find().expect("only owned items are used");
                    avatar.used.push(name);
                    let kind = if link.starts_with("xpboost") {
                        "xp"
                    } else {
                        "gold"
                    };
                    avatar.active.push(kind.into());
                    publisher.publish(&Event::ExtensionResponse {
                        packet: json!({ "type": "json", "dataObj": {
                            "cmd": "turnIn", "sItems": format!("{id}:1")
                        } })
                        .to_string(),
                    });
                    Value::Bool(true)
                }
                other => panic!("unexpected call {other} {args:?}"),
            })
        });
        let (settings, receiver) = watch::channel(State::default());
        let subscriber = events.clone();
        tokio::task::spawn_local(run(
            loader,
            move |wants| subscriber.subscribe(wants),
            receiver,
        ));
        (avatar, settings)
    }

    fn register(state: &watch::Sender<State>, names: &[&str]) {
        state.send_modify(|state| {
            register_items(
                &mut state.items,
                names
                    .iter()
                    .map(|name| ItemQuery::Name((*name).into()))
                    .collect(),
            )
        });
    }

    #[tokio::test(start_paused = true)]
    async fn an_inactive_boost_is_renewed_with_the_first_registered_item_of_its_kind() {
        LocalSet::new()
            .run_until(async {
                let (avatar, state) = start(&["gold"]);
                register(
                    &state,
                    &[
                        "Potion",
                        "Long XP Boost",
                        "Short XP Boost",
                        "Gold Boost",
                        "Missing",
                    ],
                );
                sleep(CHECK_EVERY * 3).await;
                assert_eq!(avatar.borrow().used, ["Long XP Boost"]);

                avatar.borrow_mut().active.clear();
                sleep(CHECK_EVERY + Duration::from_millis(50)).await;
                assert_eq!(
                    avatar.borrow().used,
                    ["Long XP Boost", "Long XP Boost", "Gold Boost"]
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_paused_config_uses_nothing() {
        LocalSet::new()
            .run_until(async {
                let (avatar, state) = start(&[]);
                state.send_modify(|state| state.config.enabled = false);
                register(&state, &["Gold Boost"]);
                sleep(CHECK_EVERY * 3).await;
                assert!(avatar.borrow().used.is_empty());
            })
            .await;
    }
}
