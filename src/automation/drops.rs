use serde::Serialize;
use tokio::sync::watch;

use super::{quests, unregister_items};
use crate::{
    bridge::{CallError, Event, Loader, Packets, Subscription},
    domain::{
        self,
        drops::{self, DropItemPacket, DroppedItem},
        inventory::{Item, ItemQuery, same_name},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Config {
    pub(crate) enabled: bool,
    pub(crate) accept_ac: bool,
    pub(crate) accept_ac_member_only: bool,
    pub(crate) accept_non_ac: bool,
    pub(crate) accept_non_ac_member_only: bool,
    pub(crate) reject_others: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            accept_ac: false,
            accept_ac_member_only: false,
            accept_non_ac: false,
            accept_non_ac_member_only: false,
            reject_others: false,
        }
    }
}

impl Config {
    fn accepts(&self, item: &Item) -> bool {
        match (item.coins(), item.member()) {
            (true, false) => self.accept_ac,
            (true, true) => self.accept_ac_member_only,
            (false, false) => self.accept_non_ac,
            (false, true) => self.accept_non_ac_member_only,
        }
    }

    fn acts_on_unregistered(&self) -> bool {
        self.accept_ac
            || self.accept_ac_member_only
            || self.accept_non_ac
            || self.accept_non_ac_member_only
            || self.reject_others
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct State {
    pub(crate) items: Vec<ItemQuery>,
    pub(crate) sounds: Vec<ItemQuery>,
    pub(crate) held: Vec<i32>,
    pub(crate) config: Config,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Waiting {
    pub(crate) item_id: i32,
    pub(crate) name: String,
    pub(crate) quantity: i32,
}

impl State {
    pub(crate) fn unregister(&mut self, items: &[ItemQuery]) {
        unregister_items(&mut self.items, items);
        unregister_items(&mut self.sounds, items);
    }

    pub(crate) fn clear(&mut self) {
        let items = std::mem::take(&mut self.items);
        unregister_items(&mut self.sounds, &items);
    }

    pub(crate) fn rings(&self, query: &ItemQuery) -> bool {
        self.sounds.iter().any(|sound| sound.same(query))
    }

    pub(crate) fn set_sound(&mut self, query: &ItemQuery, on: bool) {
        unregister_items(&mut self.sounds, std::slice::from_ref(query));
        if on {
            self.sounds.push(query.clone());
        }
    }

    fn rings_for(&self, dropped: &DroppedItem, quest_items: &[String]) -> bool {
        let matches = |query: &ItemQuery| match query {
            ItemQuery::Name(name) => same_name(name, &dropped.name),
            ItemQuery::ItemId { item_id } => dropped.item_id == Some(*item_id),
        };
        let shown = self.items.iter().any(matches)
            || quest_items
                .iter()
                .any(|name| same_name(name, &dropped.name));
        shown && self.sounds.iter().any(matches)
    }
}

enum Action {
    Accept,
    Reject,
}

pub(crate) async fn run(
    loader: impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    mut state: watch::Receiver<State>,
    mut quest_state: watch::Receiver<quests::State>,
    ring: impl Fn(),
    release: impl Fn(&[i32]),
    report: impl Fn(&[String]),
) {
    let dropped = Packets::<DropItemPacket>::subscribe(&subscribe);
    let mut quest_items = Vec::new();
    loop {
        state.borrow_and_update();
        quest_state.borrow_and_update();
        let checked = settle(
            &loader,
            &subscribe,
            &state,
            &quest_state,
            &release,
            &report,
            &mut quest_items,
        )
        .await;
        if let Err(error) = checked {
            tracing::debug!(%error, "drop rules skipped a check");
        }
        tokio::select! {
            packet = dropped.next() => if let Ok(packet) = packet {
                let (rings, dropped_again) = {
                    let state = state.borrow();
                    let rings = packet
                        .items
                        .iter()
                        .any(|item| state.rings_for(item, &quest_items));
                    let again: Vec<i32> = state
                        .held
                        .iter()
                        .copied()
                        .filter(|id| packet.items.iter().any(|item| item.item_id == Some(*id)))
                        .collect();
                    (rings, again)
                };
                if rings {
                    ring();
                }
                if !dropped_again.is_empty() {
                    release(&dropped_again);
                }
            },
            changed = state.changed() => if changed.is_err() {
                return;
            },
            changed = quest_state.changed() => if changed.is_err() {
                return;
            },
        }
    }
}

async fn settle(
    loader: &impl Loader,
    subscribe: &impl Fn(fn(&Event) -> bool) -> Subscription,
    live: &watch::Receiver<State>,
    live_quests: &watch::Receiver<quests::State>,
    release: &impl Fn(&[i32]),
    report: &impl Fn(&[String]),
    quest_items: &mut Vec<String>,
) -> Result<(), CallError> {
    let state = live.borrow().clone();
    let quests = live_quests.borrow().clone();
    let config = state.config;
    let quest_drops = !quests.quests.is_empty()
        && (quests.config.accept_rewards || quests.config.accept_requirements);
    if !quest_drops {
        show(quest_items, &[], report);
    }
    if !config.enabled
        || (state.items.is_empty() && !quest_drops && !config.acts_on_unregistered())
        || !domain::has_avatar(loader)?
    {
        return Ok(());
    }
    let (quest_names, all_quests_loaded) = quests.drop_names(loader).await?;
    let quest_names = distinct(quest_names);
    show(quest_items, &quest_names, report);
    let waiting = drops::items(loader)?;
    let gone: Vec<i32> = state
        .held
        .iter()
        .copied()
        .filter(|id| !waiting.iter().any(|item| item.item_id() == *id))
        .collect();
    if !gone.is_empty() {
        release(&gone);
        return Ok(());
    }
    let mut registered = state.items;
    registered.extend(quest_names.into_iter().map(ItemQuery::Name));
    for item in waiting {
        if state.held.contains(&item.item_id()) {
            continue;
        }
        let action = if registered.iter().any(|query| item.matches(query)) || config.accepts(&item)
        {
            Action::Accept
        } else if config.reject_others && all_quests_loaded {
            Action::Reject
        } else {
            continue;
        };
        if live.has_changed().unwrap_or(true) || live_quests.has_changed().unwrap_or(true) {
            return Ok(());
        }
        let query = item.query();
        let done = match action {
            Action::Accept => {
                let (mut changes, mut quest_changes) = (live.clone(), live_quests.clone());
                tokio::select! {
                    biased;
                    _ = changes.changed() => return Ok(()),
                    _ = quest_changes.changed() => return Ok(()),
                    done = drops::accept(loader, subscribe, &query) => done?,
                }
            }
            Action::Reject => drops::reject(loader, &query)?,
        };
        if !done {
            tracing::debug!(?query, "drop rules couldn't handle a drop");
        }
    }
    Ok(())
}

fn distinct(names: Vec<String>) -> Vec<String> {
    let mut distinct: Vec<String> = Vec::new();
    for name in names {
        if !distinct.iter().any(|known| same_name(known, &name)) {
            distinct.push(name);
        }
    }
    distinct
}

fn show(quest_items: &mut Vec<String>, names: &[String], report: &impl Fn(&[String])) {
    if quest_items.as_slice() != names {
        *quest_items = names.to_vec();
        report(names);
    }
}

pub(crate) async fn newly_taken(
    loader: &impl Loader,
    drops: &State,
    before: &quests::State,
    after: &quests::State,
) -> Result<Vec<Waiting>, CallError> {
    if !drops.config.enabled || !domain::has_avatar(loader)? {
        return Ok(Vec::new());
    }
    let queries =
        |names: Vec<String>| -> Vec<ItemQuery> { names.into_iter().map(ItemQuery::Name).collect() };
    let before = queries(before.drop_names(loader).await?.0);
    let after = queries(after.drop_names(loader).await?.0);
    Ok(drops::items(loader)?
        .into_iter()
        .filter(|item| {
            let taken = |queries: &[ItemQuery]| queries.iter().any(|query| item.matches(query));
            taken(&after)
                && !taken(&before)
                && !taken(&drops.items)
                && !drops.config.accepts(item)
                && !drops.held.contains(&item.item_id())
        })
        .map(|item| Waiting {
            item_id: item.item_id(),
            name: item.name().to_owned(),
            quantity: item.quantity,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use serde_json::json;
    use tokio::{task::LocalSet, time::sleep};

    use super::*;
    use crate::{
        automation::{quests::Registered, register_items},
        bridge::{Events, Value},
        domain::testing::FakeLoader,
    };

    struct Drop {
        id: f64,
        name: &'static str,
        coins: bool,
        member: bool,
    }

    const GEM: Drop = Drop {
        id: 1.0,
        name: "Gem",
        coins: false,
        member: false,
    };
    const CAPE: Drop = Drop {
        id: 2.0,
        name: "Cape",
        coins: true,
        member: false,
    };
    const ARMOR: Drop = Drop {
        id: 3.0,
        name: "Armor",
        coins: true,
        member: true,
    };
    const PRIZE: Drop = Drop {
        id: 4.0,
        name: "Prize",
        coins: false,
        member: false,
    };

    #[derive(Default)]
    struct List {
        quest_loaded: bool,
        register_on_accept: Option<&'static str>,
        rings: u32,
        loading: bool,
        waiting: Vec<Drop>,
        accepted: Vec<&'static str>,
        rejected: Vec<&'static str>,
        quest_items: Vec<String>,
    }

    fn object(fields: Vec<(&str, Value)>) -> Value {
        Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    fn row(drop: &Drop) -> Value {
        object(vec![
            ("itemId", Value::Number(drop.id)),
            ("name", Value::String(drop.name.into())),
            ("quantity", Value::Number(1.0)),
            ("coins", Value::Bool(drop.coins)),
            ("member", Value::Bool(drop.member)),
        ])
    }

    fn take(list: &mut List, args: &[Value]) -> Option<Drop> {
        let [Value::Object(selector)] = args else {
            panic!("the drop list needs a selector: {args:?}");
        };
        let Some(Value::Number(id)) = selector.get("itemId") else {
            panic!("drops are handled by id: {selector:?}");
        };
        let index = list.waiting.iter().position(|drop| drop.id == *id)?;
        Some(list.waiting.remove(index))
    }

    type Fixture = (
        Rc<RefCell<List>>,
        Rc<Events>,
        Rc<watch::Sender<State>>,
        watch::Sender<quests::State>,
    );

    fn fake(
        state: Rc<RefCell<List>>,
        script: Rc<watch::Sender<State>>,
        publisher: Rc<Events>,
    ) -> FakeLoader {
        FakeLoader::new(move |callback, args| {
            let mut list = state.borrow_mut();
            Ok(match callback {
                "auth.isLoggedIn" | "world.isLoaded" => Value::Bool(true),
                "player.isLoaded" => Value::Bool(!list.loading),
                "flash.isNull" => Value::Bool(false),
                "drops.getItems" => Value::List(list.waiting.iter().map(row).collect()),
                "drops.accept" => match take(&mut list, args) {
                    Some(drop) => {
                        list.accepted.push(drop.name);
                        if let Some(name) = list.register_on_accept.take() {
                            register(&script, name);
                        }
                        publisher.publish(&Event::ExtensionResponse {
                            packet: json!({ "type": "json", "dataObj": {
                                "cmd": "getDrop", "ItemID": drop.id, "bSuccess": 1
                            } })
                            .to_string(),
                        });
                        Value::Number(drop.id)
                    }
                    None => Value::Number(0.0),
                },
                "drops.reject" => match take(&mut list, args) {
                    Some(drop) => {
                        list.rejected.push(drop.name);
                        Value::Bool(true)
                    }
                    None => Value::Bool(false),
                },
                "quests.get" if list.quest_loaded => object(vec![
                    ("id", Value::Number(7.0)),
                    ("name", Value::String("Quest".into())),
                    ("rewards", Value::List(vec![row(&PRIZE)])),
                ]),
                "quests.get" => Value::Null,
                "quests.load" => {
                    list.quest_loaded = true;
                    Value::String("sent".into())
                }
                other => panic!("unexpected call {other} {args:?}"),
            })
        })
    }

    fn start(waiting: Vec<Drop>) -> Fixture {
        let list = Rc::new(RefCell::new(List {
            waiting,
            ..List::default()
        }));
        let (rules, receiver) = watch::channel(State::default());
        let rules = Rc::new(rules);
        let events = Rc::new(Events::default());
        let loader = fake(list.clone(), rules.clone(), events.clone());
        let (quests, quest_receiver) = watch::channel(quests::State::default());
        let subscriber = events.clone();
        let (rings, releasing, reporting) = (list.clone(), rules.clone(), list.clone());
        tokio::task::spawn_local(run(
            loader,
            move |wants| subscriber.subscribe(wants),
            receiver,
            quest_receiver,
            move || rings.borrow_mut().rings += 1,
            move |released: &[i32]| {
                releasing.send_modify(|state| state.held.retain(|id| !released.contains(id)));
            },
            move |names: &[String]| reporting.borrow_mut().quest_items = names.to_vec(),
        ));
        (list, events, rules, quests)
    }

    fn drop_item(events: &Events, drop: &Drop) {
        events.publish(&Event::ExtensionResponse {
            packet: json!({ "type": "json", "dataObj": {
                "cmd": "dropItem",
                "items": { drop.id.to_string(): { "ItemID": drop.id, "sName": drop.name, "iQty": 1 } }
            } })
            .to_string(),
        });
    }

    async fn settle() {
        sleep(Duration::from_millis(50)).await;
    }

    fn register(rules: &watch::Sender<State>, name: &str) {
        rules.send_modify(|state| {
            register_items(&mut state.items, vec![ItemQuery::Name(name.into())])
        });
    }

    #[tokio::test(start_paused = true)]
    async fn registered_drops_are_accepted_whenever_they_wait_and_others_are_left_alone() {
        LocalSet::new()
            .run_until(async {
                let (list, events, rules, _quests) = start(vec![GEM, CAPE]);
                register(&rules, "gem");
                settle().await;
                assert_eq!(list.borrow().accepted, ["Gem"]);

                list.borrow_mut().waiting.push(GEM);
                events.publish(&Event::ExtensionResponse {
                    packet:
                        json!({ "type": "json", "dataObj": { "cmd": "dropItem", "items": {} } })
                            .to_string(),
                });
                settle().await;
                let list = list.borrow();
                assert_eq!(list.accepted, ["Gem", "Gem"]);
                assert!(list.rejected.is_empty());
                assert_eq!(list.waiting.len(), 1, "the unregistered cape still waits");
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_drop_registered_while_another_is_accepted_is_not_rejected() {
        LocalSet::new()
            .run_until(async {
                let (list, _events, rules, _quests) = start(vec![GEM, CAPE]);
                list.borrow_mut().register_on_accept = Some("Cape");
                rules.send_modify(|state| {
                    register_items(&mut state.items, vec![ItemQuery::Name("Gem".into())]);
                    state.config.reject_others = true;
                });
                settle().await;
                let list = list.borrow();
                assert_eq!(list.accepted, ["Gem", "Cape"]);
                assert!(list.rejected.is_empty());
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn the_config_accepts_by_kind_and_rejects_the_rest_only_when_asked() {
        LocalSet::new()
            .run_until(async {
                let (list, _events, rules, _quests) = start(vec![GEM, CAPE, ARMOR]);
                rules.send_modify(|state| state.config.accept_ac = true);
                settle().await;
                assert_eq!(list.borrow().accepted, ["Cape"]);
                assert!(list.borrow().rejected.is_empty());

                rules.send_modify(|state| state.config.reject_others = true);
                settle().await;
                assert_eq!(list.borrow().rejected, ["Gem", "Armor"]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_paused_config_keeps_its_registrations_and_resumes() {
        LocalSet::new()
            .run_until(async {
                let (list, events, rules, _quests) = start(vec![GEM]);
                rules.send_modify(|state| {
                    state.config.enabled = false;
                    state.config.reject_others = true;
                });
                register(&rules, "Gem");
                settle().await;
                assert!(list.borrow().accepted.is_empty() && list.borrow().rejected.is_empty());

                rules.send_modify(|state| state.config.enabled = true);
                settle().await;
                assert_eq!(list.borrow().accepted, ["Gem"]);

                {
                    let mut list = list.borrow_mut();
                    list.loading = true;
                    list.waiting.push(GEM);
                }
                events.publish(&Event::ExtensionResponse {
                    packet: json!({ "type": "json", "dataObj": { "cmd": "dropItem" } }).to_string(),
                });
                settle().await;
                rules.send_modify(|state| state.config.enabled = false);
                list.borrow_mut().loading = false;
                sleep(Duration::from_secs(1)).await;
                assert_eq!(
                    list.borrow().accepted,
                    ["Gem"],
                    "an accept waiting on the player is dropped when the rules pause"
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn registered_quest_rewards_are_accepted_when_the_quest_config_asks() {
        LocalSet::new()
            .run_until(async {
                let (list, events, rules, quests) = start(Vec::new());
                let drop_prize = || {
                    list.borrow_mut().waiting.push(PRIZE);
                    events.publish(&Event::ExtensionResponse {
                        packet: json!({ "type": "json", "dataObj": { "cmd": "dropItem" } })
                            .to_string(),
                    });
                };
                rules.send_modify(|state| state.config.reject_others = true);
                quests.send_modify(|state| {
                    state.register([Registered {
                        quest_id: 7,
                        reward_item_id: None,
                    }])
                });
                drop_prize();
                settle().await;
                assert_eq!(list.borrow().rejected, ["Prize"]);

                quests.send_modify(|state| state.config.accept_rewards = true);
                drop_prize();
                settle().await;
                assert_eq!(list.borrow().accepted, ["Prize"], "the quest loads first");
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_drop_with_a_sound_rings_even_while_the_rules_are_paused() {
        LocalSet::new()
            .run_until(async {
                let (list, events, rules, _quests) = start(Vec::new());
                register(&rules, "Gem");
                rules.send_modify(|state| {
                    state.set_sound(&ItemQuery::Name("gem".into()), true);
                    state.set_sound(&ItemQuery::Name("Cape".into()), true);
                    state.config.enabled = false;
                });
                settle().await;

                drop_item(&events, &CAPE);
                drop_item(&events, &GEM);
                settle().await;
                assert_eq!(list.borrow().rings, 1, "only shown drops ring");

                rules.send_modify(|state| state.unregister(&[ItemQuery::Name("GEM".into())]));
                drop_item(&events, &GEM);
                settle().await;
                assert_eq!(list.borrow().rings, 1);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_held_drop_waits_untouched_until_it_drops_again_or_leaves_the_list() {
        LocalSet::new()
            .run_until(async {
                let (list, events, rules, _quests) = start(vec![GEM, CAPE]);
                rules.send_modify(|state| {
                    register_items(&mut state.items, vec![ItemQuery::Name("Gem".into())]);
                    state.config.reject_others = true;
                    state.held = vec![1];
                });
                settle().await;
                assert!(list.borrow().accepted.is_empty());
                assert_eq!(list.borrow().rejected, ["Cape"]);
                assert_eq!(rules.borrow().held, [1]);

                drop_item(&events, &GEM);
                settle().await;
                assert_eq!(list.borrow().accepted, ["Gem"]);
                assert!(rules.borrow().held.is_empty());

                rules.send_modify(|state| state.held = vec![1]);
                settle().await;
                assert!(
                    rules.borrow().held.is_empty(),
                    "a hold ends once its drop leaves the list"
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn turning_on_quest_rewards_reports_the_waiting_drops_nothing_else_picks_up() {
        let list = Rc::new(RefCell::new(List {
            waiting: vec![GEM, CAPE, PRIZE],
            ..List::default()
        }));
        let loader = fake(
            list,
            Rc::new(watch::channel(State::default()).0),
            Rc::new(Events::default()),
        );
        let mut drops = State::default();
        register_items(&mut drops.items, vec![ItemQuery::Name("Gem".into())]);
        drops.config.accept_ac = true;
        let before = quests::State {
            quests: vec![Registered {
                quest_id: 7,
                reward_item_id: None,
            }],
            ..quests::State::default()
        };
        let mut after = before.clone();
        after.config.accept_rewards = true;
        let names = |waiting: Vec<Waiting>| -> Vec<String> {
            waiting.into_iter().map(|waiting| waiting.name).collect()
        };

        let waiting = newly_taken(&loader, &drops, &before, &after).await;
        assert_eq!(waiting.map(names), Ok(vec!["Prize".to_owned()]));
        let waiting = newly_taken(&loader, &drops, &after, &after).await;
        assert_eq!(waiting.map(names), Ok(Vec::new()), "already picked up");

        drops.held = vec![4];
        let waiting = newly_taken(&loader, &drops, &before, &after).await;
        assert_eq!(waiting.map(names), Ok(Vec::new()), "held drops stay");

        drops.held.clear();
        drops.config.enabled = false;
        let waiting = newly_taken(&loader, &drops, &before, &after).await;
        assert_eq!(waiting.map(names), Ok(Vec::new()), "nothing is picked up");
    }

    #[tokio::test(start_paused = true)]
    async fn a_beep_on_a_quest_item_rings_only_while_its_quest_box_is_ticked() {
        LocalSet::new()
            .run_until(async {
                let (list, events, rules, quests) = start(Vec::new());
                quests.send_modify(|state| {
                    state.register([Registered {
                        quest_id: 7,
                        reward_item_id: None,
                    }]);
                    state.config.accept_rewards = true;
                });
                rules.send_modify(|state| state.set_sound(&ItemQuery::Name("prize".into()), true));
                settle().await;
                assert_eq!(list.borrow().quest_items, ["Prize"]);
                drop_item(&events, &PRIZE);
                settle().await;
                assert_eq!(list.borrow().rings, 1);

                quests.send_modify(|state| state.config.accept_rewards = false);
                settle().await;
                assert!(list.borrow().quest_items.is_empty());
                drop_item(&events, &PRIZE);
                settle().await;
                assert_eq!(
                    list.borrow().rings,
                    1,
                    "its pill is gone, so it stays quiet"
                );
            })
            .await;
    }
}
