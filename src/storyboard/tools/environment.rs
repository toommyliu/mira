use std::collections::HashMap;

use gpui::{
    App, Context, Div, Entity, Focusable as _, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Render, Role, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use mira_ui::{
    components::Chips,
    h_flex,
    primitives::{
        Checkbox, Dialog, Field, Icon, Input, InputEvent, Spinner, Switch, Toast, Toaster,
        button::{Size, Variant},
        icon,
    },
    v_flex,
};

use crate::{
    automation::{drops, register_items, settings::Automation},
    domain::inventory::ItemQuery,
    ruffle_view::environment::{
        Confirm, DROP_KINDS, List, QuestChange, Taken, checked, clear, item_label, items,
        quest_label, read_quest, read_quests, replace_drop, replace_items,
    },
};

use super::super::{Screen, view::*};

const TITLES: [&str; 3] = ["Drops", "Quests", "Boosts"];
const QUEST_DROPS: [(i32, &str); 2] = [(45230, "Abbey voucher"), (45231, "Runed stone")];
const BOOSTS: [(&str, &str, &str, u8); 4] = [
    ("Gold", "Gold Boost! (60 min)", "Bank", 2),
    ("Experience", "XP Boost! (20 min)", "Inventory", 4),
    ("Reputation", "Reputation Boost! (60 min)", "Inventory", 1),
    (
        "Class points",
        "Class Point Boost! (60 min)",
        "Inventory",
        3,
    ),
];

enum Overlay {
    Confirm(Confirm),
    Fetch([bool; BOOSTS.len()]),
    Pickup {
        change: QuestChange,
        drops: Vec<(i32, &'static str, bool)>,
    },
}

#[derive(Default)]
struct PreviewEnvironment {
    automation: Automation,
    picked_drops: Vec<i32>,
    retrieved_boosts: [bool; BOOSTS.len()],
}

pub(in crate::storyboard) struct Environment {
    sessions: Entity<Sessions>,
    target: Entity<Target>,
    account: Option<SharedString>,
    configs: HashMap<SharedString, PreviewEnvironment>,
    inputs: [Entity<Input>; 3],
    overlay: Option<Overlay>,
    quest_error: Option<SharedString>,
    boost_notice: Option<&'static str>,
}

impl Environment {
    pub(in crate::storyboard) fn new(sessions: Entity<Sessions>, cx: &mut Context<Self>) -> Self {
        let target = Target::new(sessions.clone(), cx);
        let inputs =
            List::ALL.map(|list| field(list.input_label(), list.placeholder(), "", false, cx));
        for (list, input) in List::ALL.into_iter().zip(&inputs) {
            cx.subscribe(input, move |view, _, event, cx| {
                if matches!(event, InputEvent::Submitted) {
                    view.add(list, cx);
                }
                cx.notify();
            })
            .detach();
        }
        let sample = Automation::default();
        sample.update_drops(|state| {
            state.items = items("Relic fragment, Abbey seal, 18927");
            state.set_sound(&ItemQuery::Name("Relic fragment".into()), true);
            state.config.accept_ac = true;
        });
        sample.update_quests(|state| {
            state.register(read_quests("2857, 6552:45230").0);
            state.config.accept_rewards = true;
        });
        sample.update_boosts(|state| {
            state.items = items("Reputation Boost! (60 min), Class Point Boost! (60 min)");
        });
        let mut view = Self {
            sessions,
            target,
            account: None,
            configs: HashMap::from([(
                "Aster".into(),
                PreviewEnvironment {
                    automation: sample,
                    ..Default::default()
                },
            )]),
            inputs,
            overlay: None,
            quest_error: None,
            boost_notice: None,
        };
        cx.observe(&view.target, |view, _, cx| view.retarget(cx))
            .detach();
        view.retarget(cx);
        view
    }

    #[cfg(test)]
    pub(in crate::storyboard) fn preview_fetch(mut self) -> Self {
        if let Some(automation) = self.automation() {
            self.open_fetch(&automation);
            if let Some(Overlay::Fetch(picked)) = &mut self.overlay {
                picked[0] = true;
            }
        }
        self
    }

    fn automation(&self) -> Option<Automation> {
        self.config().map(|config| config.automation.clone())
    }

    fn config(&self) -> Option<&PreviewEnvironment> {
        self.configs.get(self.account.as_ref()?)
    }

    fn retarget(&mut self, cx: &mut Context<Self>) {
        let account = self.target.read(cx).resolved(cx).cloned();
        if self.account == account {
            return;
        }
        self.account = account;
        self.overlay = None;
        self.quest_error = None;
        self.boost_notice = None;
        if let Some(name) = self.account.clone() {
            self.configs.entry(name).or_default();
        }
        let enabled = self.automation().map_or([false; 3], |automation| {
            [
                automation.drops().config.enabled,
                automation.quests().config.enabled,
                automation.boosts().config.enabled,
            ]
        });
        for (input, on) in self.inputs.iter().zip(enabled) {
            input.update(cx, |input, cx| {
                input.set_text("", cx);
                input.set_disabled(!on, cx);
                input.set_invalid(false, cx);
            });
        }
        cx.notify();
    }

    fn set_enabled(&mut self, list: List, enabled: bool, cx: &mut Context<Self>) {
        let Some(automation) = self.automation() else {
            return;
        };
        match list {
            List::Drops => {
                automation.update_drops(|state| state.config.enabled = enabled);
            }
            List::Quests => {
                automation.update_quests(|state| state.config.enabled = enabled);
            }
            List::Boosts => {
                automation.update_boosts(|state| state.config.enabled = enabled);
            }
        }
        self.inputs[list as usize].update(cx, |input, cx| input.set_disabled(!enabled, cx));
    }

    fn add(&mut self, list: List, cx: &mut Context<Self>) {
        let Some(automation) = self.automation() else {
            return;
        };
        let input = self.inputs[list as usize].clone();
        let text = input.read(cx).text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let remainder = match list {
            List::Drops => {
                automation.update_drops(|state| register_items(&mut state.items, items(&text)));
                String::new()
            }
            List::Quests => {
                let (quests, unreadable) = read_quests(&text);
                self.quest_error = unreadable.as_ref().map(|error| error.reason.clone());
                if !quests.is_empty() {
                    self.change_quests(&automation, QuestChange::Add(quests));
                }
                unreadable.map_or_else(String::new, |error| error.text)
            }
            List::Boosts => {
                automation.update_boosts(|state| register_items(&mut state.items, items(&text)));
                String::new()
            }
        };
        let invalid = list == List::Quests && self.quest_error.is_some();
        input.update(cx, |input, cx| {
            input.set_text(remainder, cx);
            input.set_invalid(invalid, cx);
        });
    }

    fn edit(&mut self, list: List, index: usize, text: String, cx: &mut Context<Self>) {
        let Some(automation) = self.automation() else {
            return;
        };
        match list {
            List::Drops => {
                automation.update_drops(|state| replace_drop(state, index, &text));
            }
            List::Quests => match read_quest(&text) {
                Ok(quest) => {
                    self.quest_error = None;
                    self.inputs[List::Quests as usize]
                        .update(cx, |input, cx| input.set_invalid(false, cx));
                    self.change_quests(&automation, QuestChange::Edit(index, quest));
                }
                Err(error) => {
                    self.quest_error = Some(error.into());
                    self.inputs[List::Quests as usize]
                        .update(cx, |input, cx| input.set_invalid(true, cx));
                }
            },
            List::Boosts => {
                automation.update_boosts(|state| {
                    replace_items(&mut state.items, index, items(&text));
                });
            }
        }
    }

    fn remove(list: List, index: usize, automation: &Automation) -> bool {
        match list {
            List::Drops => automation
                .update_drops(|state| {
                    if let Some(item) = state.items.get(index).cloned() {
                        state.unregister(&[item]);
                    }
                })
                .items
                .is_empty(),
            List::Quests => automation
                .update_quests(|state| {
                    if let Some(quest) = state.quests.get(index) {
                        state.unregister(&[quest.quest_id]);
                    }
                })
                .quests
                .is_empty(),
            List::Boosts => automation
                .update_boosts(|state| {
                    if index < state.items.len() {
                        state.items.remove(index);
                    }
                })
                .items
                .is_empty(),
        }
    }

    fn change_quests(&mut self, automation: &Automation, change: QuestChange) {
        let drop_state = automation.drops();
        let picked = self
            .config()
            .map_or(&[][..], |config| config.picked_drops.as_slice());
        let mut next = automation.quests();
        change.apply(&mut next);
        let drops: Vec<_> = if drop_state.config.enabled && !next.quests.is_empty() {
            Taken::ALL
                .into_iter()
                .zip(QUEST_DROPS)
                .filter_map(|(kind, (id, name))| {
                    (*kind.flag(&mut next.config)
                        && !drop_state.held.contains(&id)
                        && !picked.contains(&id))
                    .then_some((id, name, true))
                })
                .collect()
        } else {
            Vec::new()
        };
        if drops.is_empty() {
            automation.update_quests(|state| *state = next);
        } else {
            self.overlay = Some(Overlay::Pickup { change, drops });
        }
    }

    fn open_fetch(&mut self, automation: &Automation) {
        let listed = automation.boosts().items;
        let all_listed = BOOSTS.iter().all(|&(_, name, _, _)| {
            listed
                .iter()
                .any(|query| query.same(&ItemQuery::Name(name.into())))
        });
        self.boost_notice = all_listed.then_some("Every boost you own is listed");
        self.overlay = (!all_listed).then_some(Overlay::Fetch([false; BOOSTS.len()]));
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        let (Some(overlay), Some(automation)) = (self.overlay.take(), self.automation()) else {
            return;
        };
        let message = match overlay {
            Overlay::Confirm(Confirm::CopyToAll) => {
                for name in &self.sessions.read(cx).names {
                    if Some(name) != self.account.as_ref() {
                        self.configs
                            .entry(name.clone())
                            .or_default()
                            .automation
                            .copy_environment_from(&automation);
                    }
                }
                "Environment copied"
            }
            Overlay::Confirm(Confirm::ClearAll) => {
                for list in List::ALL {
                    clear(&automation, list);
                }
                "Environment lists cleared"
            }
            Overlay::Confirm(Confirm::Clear(list)) => {
                clear(&automation, list);
                match list {
                    List::Drops => "Drops cleared",
                    List::Quests => "Quests cleared",
                    List::Boosts => "Boosts cleared",
                }
            }
            Overlay::Fetch(picked) => {
                if let Some(config) = self
                    .account
                    .as_ref()
                    .and_then(|name| self.configs.get_mut(name))
                {
                    for (retrieved, picked) in config.retrieved_boosts.iter_mut().zip(picked) {
                        *retrieved |= picked;
                    }
                }
                let chosen = BOOSTS
                    .into_iter()
                    .zip(picked)
                    .filter(|(_, picked)| *picked)
                    .map(|((_, name, _, _), _)| ItemQuery::Name(name.into()))
                    .collect();
                automation.update_boosts(|state| register_items(&mut state.items, chosen));
                "Boosts added"
            }
            Overlay::Pickup { change, drops } => {
                if let Some(config) = self
                    .account
                    .as_ref()
                    .and_then(|name| self.configs.get_mut(name))
                {
                    config.picked_drops.extend(
                        drops
                            .iter()
                            .filter_map(|&(id, _, picked)| picked.then_some(id)),
                    );
                }
                automation.update_drops(|state| {
                    for (id, _, picked) in drops {
                        if !picked && !state.held.contains(&id) {
                            state.held.push(id);
                        }
                    }
                });
                automation.update_quests(|state| change.apply(state));
                "Quest rules updated"
            }
        };
        Toaster::push(Toast::new(message), cx);
    }

    fn panel(
        &self,
        list: List,
        on: bool,
        has_items: bool,
        summary: &'static str,
        cx: &Context<Self>,
    ) -> Div {
        let name = TITLES[list as usize];
        let input = self.inputs[list as usize].clone();
        let blank = input.read(cx).text().trim().is_empty();
        let field = Field::new(
            list.label(),
            h_flex()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(input)
                        .on_key_down(cx.listener(move |view, event: &KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" {
                                if list == List::Quests {
                                    view.quest_error = None;
                                }
                                view.inputs[list as usize].update(cx, |input, cx| {
                                    input.set_text("", cx);
                                    input.set_invalid(false, cx);
                                });
                                cx.stop_propagation();
                            }
                        })),
                )
                .child(
                    command(
                        ("add", list as usize),
                        "Add",
                        Variant::Outline,
                        change(cx, move |view, cx| view.add(list, cx)),
                    )
                    .size(Size::Small)
                    .disabled(!on || blank),
                ),
        )
        .description(list.note())
        .error(
            (list == List::Quests)
                .then(|| self.quest_error.clone())
                .flatten(),
        )
        .when(has_items, |field| {
            field.end_slot(
                command(
                    ("clear-list", list as usize),
                    "Clear…",
                    Variant::Ghost,
                    change(cx, move |view, _| {
                        view.overlay = Some(Overlay::Confirm(Confirm::Clear(list)));
                    }),
                )
                .size(Size::Small),
            )
        });
        v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(8.0))
            .child(
                h_flex()
                    .min_h(px(24.0))
                    .gap(px(8.0))
                    .child(title(name))
                    .child(if list == List::Boosts {
                        self.fetch_controls(cx)
                    } else {
                        spring()
                    })
                    .child(
                        Switch::new(("toggle", list as usize), name, on)
                            .hide_label()
                            .on_toggle(choose(cx, move |view, enabled, cx| {
                                view.set_enabled(list, enabled, cx)
                            })),
                    ),
            )
            .child(meta(summary, cx))
            .child(field)
    }

    fn fetch_controls(&self, cx: &Context<Self>) -> Div {
        let preview = StatusPreview::current(cx);
        let retry_hint = "Bank is unavailable. Try Fetch again.";
        let message = h_flex().id("boost-fetch-status").min_w_0().gap(px(6.0));
        let notice = match preview {
            StatusPreview::Current => self
                .boost_notice
                .map(|message| status(message, true, cx).min_w_0().truncate()),
            StatusPreview::Working => Some(
                message
                    .role(Role::Status)
                    .child(Spinner::new("finding-boosts").color(palette(cx).muted_foreground))
                    .child(meta("Finding boosts…", cx).min_w_0().truncate()),
            ),
            StatusPreview::Problem => Some(
                message
                    .role(Role::Alert)
                    .aria_description(retry_hint)
                    .child(icon(Icon::CircleAlert, palette(cx).destructive))
                    .child(div().min_w_0().truncate().child("Couldn't fetch boosts")),
            ),
        };
        h_flex()
            .flex_1()
            .min_w_0()
            .justify_end()
            .gap(px(8.0))
            .children(notice)
            .child(
                command(
                    "fetch-boosts",
                    "Fetch…",
                    Variant::Ghost,
                    change(cx, |view, _| {
                        if let Some(automation) = view.automation() {
                            view.open_fetch(&automation);
                        }
                    }),
                )
                .size(Size::Small)
                .tooltip(if preview == StatusPreview::Problem {
                    retry_hint
                } else {
                    "Find boosts in inventory and bank"
                })
                .disabled(preview == StatusPreview::Working),
            )
    }

    fn chips(
        &self,
        list: List,
        labels: impl ExactSizeIterator<Item = SharedString>,
        on: bool,
        cx: &Context<Self>,
    ) -> Option<Chips> {
        if labels.len() == 0 {
            return None;
        }
        let label = TITLES[list as usize];
        let owner = cx.weak_entity();
        let editing = owner.clone();
        let input = self.inputs[list as usize].clone();
        Some(
            Chips::new(label, label, labels)
                .disabled(!on)
                .on_remove(move |index, window, cx| {
                    let empty = owner
                        .update(cx, |view, cx| {
                            let empty = view
                                .automation()
                                .is_some_and(|automation| Self::remove(list, index, &automation));
                            cx.notify();
                            empty
                        })
                        .unwrap_or(false);
                    if empty {
                        window.focus(&input.focus_handle(cx), cx);
                    }
                })
                .on_edit(move |index, text, _, cx| {
                    editing
                        .update(cx, |view, cx| {
                            view.edit(list, index, text, cx);
                            cx.notify();
                        })
                        .ok();
                }),
        )
    }

    fn pickup_by_type(config: drops::Config, cx: &Context<Self>) -> Stateful<Div> {
        let [ac, member_ac, non_ac, member_non_ac] = DROP_KINDS;
        let categories = ["AC", "Non-AC"];
        let row = || h_flex().gap(px(8.0));
        let column = || h_flex().flex_none().w(px(56.0)).justify_center();

        v_flex()
            .id("pickup-by-type")
            .role(Role::Group)
            .aria_label("Pick up by type")
            .w_full()
            .max_w(px(256.0))
            .gap(px(2.0))
            .child(
                row()
                    .child(title("Pick up by type").flex_1().min_w_0())
                    .children(categories.map(|label| column().child(meta(label, cx)))),
            )
            .children(
                [
                    ("All players", [ac, non_ac]),
                    ("Members only", [member_ac, member_non_ac]),
                ]
                .into_iter()
                .map(|(membership, choices)| {
                    row()
                        .child(div().flex_1().min_w_0().child(membership))
                        .children(choices.into_iter().zip(categories).map(
                            |((id, _, flag), category)| {
                                let mut current = config;
                                column().child(
                                    Checkbox::new(
                                        id,
                                        format!("{category}, {membership}"),
                                        checked(*flag(&mut current)),
                                    )
                                    .hide_label()
                                    .disabled(!config.enabled)
                                    .on_toggle(choose(
                                        cx,
                                        move |view, on, _| {
                                            if let Some(automation) = view.automation() {
                                                automation.update_drops(|state| {
                                                    *flag(&mut state.config) = on;
                                                });
                                            }
                                        },
                                    )),
                                )
                            },
                        ))
                }),
            )
    }

    fn drops(&self, automation: &Automation, cx: &Context<Self>) -> Div {
        let state = automation.drops();
        let on = state.config.enabled;
        let mark = |queries: Vec<ItemQuery>| {
            let owner = cx.weak_entity();
            let automation = automation.clone();
            move |index, enabled, _: &mut Window, cx: &mut App| {
                if let Some(query) = queries.get(index) {
                    automation.update_drops(|state| state.set_sound(query, enabled));
                    owner.update(cx, |_, cx| cx.notify()).ok();
                }
            }
        };
        let listed = self
            .chips(List::Drops, state.items.iter().map(item_label), on, cx)
            .map(|chips| {
                chips
                    .marked(
                        Icon::Bell,
                        "Beep",
                        state.items.iter().map(|query| state.rings(query)),
                    )
                    .on_mark(mark(state.items.clone()))
            });
        let mut quests = automation.quests();
        let derived: Vec<_> = if quests.quests.is_empty() {
            Vec::new()
        } else {
            Taken::ALL
                .into_iter()
                .zip(QUEST_DROPS)
                .filter_map(|(kind, (_, name))| {
                    let query = ItemQuery::Name(name.into());
                    (*kind.flag(&mut quests.config)
                        && !state.items.iter().any(|listed| listed.same(&query)))
                    .then_some(query)
                })
                .collect()
        };
        self.panel(
            List::Drops,
            on,
            !state.items.is_empty(),
            "Pick up listed items and selected kinds.",
            cx,
        )
        .children(listed)
        .when(!derived.is_empty(), |panel| {
            let marks = derived
                .iter()
                .map(|query| state.rings(query))
                .collect::<Vec<_>>();
            let chips = Chips::new(
                "quest-drops",
                "From your quests",
                derived.iter().map(item_label),
            )
            .quiet()
            .disabled(!on)
            .marked(Icon::Bell, "Beep", marks)
            .on_mark(mark(derived));
            panel.child(
                v_flex()
                    .gap(px(4.0))
                    .child(meta("From your quests", cx))
                    .child(chips),
            )
        })
        .child(Self::pickup_by_type(state.config, cx))
        .child(
            Switch::new("reject", "Reject other drops", state.config.reject_others)
                .disabled(!on)
                .on_toggle(choose(cx, |view, reject, _| {
                    if let Some(automation) = view.automation() {
                        automation.update_drops(|state| state.config.reject_others = reject);
                    }
                })),
        )
    }

    fn quests(&self, automation: &Automation, cx: &Context<Self>) -> Div {
        let state = automation.quests();
        let on = state.config.enabled;
        self.panel(
            List::Quests,
            on,
            !state.quests.is_empty(),
            "Auto-accept and turn in listed quests.",
            cx,
        )
        .children(self.chips(List::Quests, state.quests.iter().map(quest_label), on, cx))
        .child(
            h_flex()
                .id("quest-pickup")
                .role(Role::Group)
                .aria_label("Pick up quest drops")
                .flex_wrap()
                .justify_between()
                .gap(px(8.0))
                .child(title("Pick up quest drops").flex_none())
                .child(
                    h_flex()
                        .flex_none()
                        .gap(px(12.0))
                        .children(Taken::ALL.map(|kind| {
                            let mut current = state.config;
                            Checkbox::new(
                                kind.label(),
                                kind.label(),
                                checked(*kind.flag(&mut current)),
                            )
                            .disabled(!on)
                            .on_toggle(choose(
                                cx,
                                move |view, enabled, _| {
                                    if let Some(automation) = view.automation() {
                                        if enabled {
                                            view.change_quests(
                                                &automation,
                                                QuestChange::Take(kind),
                                            );
                                        } else {
                                            automation.update_quests(|state| {
                                                *kind.flag(&mut state.config) = false;
                                            });
                                        }
                                    }
                                },
                            ))
                        })),
                ),
        )
    }

    fn boosts(&self, automation: &Automation, cx: &Context<Self>) -> Div {
        let state = automation.boosts();
        let on = state.config.enabled;
        self.panel(
            List::Boosts,
            on,
            !state.items.is_empty(),
            "Use one boost of each kind at a time.",
            cx,
        )
        .children(
            self.chips(List::Boosts, state.items.iter().map(item_label), on, cx)
                .map(|chips| {
                    let view = cx.weak_entity();
                    chips.on_reorder(move |from, to, _, cx| {
                        view.update(cx, |view, cx| {
                            if let Some(automation) = view.automation() {
                                automation.update_boosts(|state| {
                                    if from < state.items.len() && to < state.items.len() {
                                        let item = state.items.remove(from);
                                        state.items.insert(to, item);
                                    }
                                });
                                cx.notify();
                            }
                        })
                        .ok();
                    })
                }),
        )
    }

    fn actions(&self, automation: Option<&Automation>, cx: &Context<Self>) -> Div {
        let can_copy = automation.is_some() && self.sessions.read(cx).names.len() > 1;
        let can_clear = automation.is_some_and(|automation| {
            !automation.drops().items.is_empty()
                || !automation.quests().quests.is_empty()
                || !automation.boosts().items.is_empty()
        });
        footer(cx)
            .py(px(8.0))
            .child(
                command(
                    "copy-to-all",
                    "Copy to all tabs…",
                    Variant::Outline,
                    change(cx, |view, _| {
                        view.overlay = Some(Overlay::Confirm(Confirm::CopyToAll));
                    }),
                )
                .size(Size::Small)
                .disabled(!can_copy),
            )
            .child(spring())
            .child(
                command(
                    "clear-all",
                    "Clear all lists…",
                    Variant::Ghost,
                    change(cx, |view, _| {
                        view.overlay = Some(Overlay::Confirm(Confirm::ClearAll));
                    }),
                )
                .size(Size::Small)
                .disabled(!can_clear),
            )
    }

    fn dialog(&self, automation: &Automation, cx: &Context<Self>) -> Dialog {
        let mut rows = None;
        let (title, detail, action, destructive, disabled) =
            match &self.overlay {
                Some(Overlay::Confirm(Confirm::CopyToAll)) => (
                    format!(
                        "Copy this environment to {} other tabs?",
                        self.sessions.read(cx).names.len().saturating_sub(1)
                    ),
                    "Replace their lists and rules. Each tab keeps its own on and off switches.",
                    "Copy".into(),
                    false,
                    false,
                ),
                Some(Overlay::Confirm(Confirm::ClearAll)) => (
                    "Clear every list in this tab?".into(),
                    "Remove listed drops, quests, and boosts. Rules and switches stay as they are.",
                    "Clear lists".into(),
                    true,
                    false,
                ),
                Some(Overlay::Confirm(Confirm::Clear(list))) => (
                    list.clear_title(automation),
                    "Rules and switches stay as they are.",
                    "Clear list".into(),
                    true,
                    false,
                ),
                Some(Overlay::Fetch(picked)) => {
                    let listed = automation.boosts().items;
                    let retrieved = self
                        .config()
                        .map_or([false; BOOSTS.len()], |config| config.retrieved_boosts);
                    rows = Some(v_flex().gap(px(8.0)).children(
                        BOOSTS.into_iter().enumerate().map(
                            |(index, (kind, name, place, count))| {
                                let added = listed
                                    .iter()
                                    .any(|query| query.same(&ItemQuery::Name(name.into())));
                                let location = if added {
                                    "Listed"
                                } else if retrieved[index] {
                                    "Inventory"
                                } else {
                                    place
                                };
                                v_flex().child(meta(kind, cx)).child(
                                    h_flex()
                                        .gap(px(8.0))
                                        .child(
                                            div().flex_1().min_w_0().child(
                                                Checkbox::new(
                                                    ("boost", index),
                                                    name,
                                                    checked(added || picked[index]),
                                                )
                                                .disabled(added)
                                                .on_toggle(choose(cx, move |view, on, _| {
                                                    if let Some(Overlay::Fetch(picked)) =
                                                        &mut view.overlay
                                                    {
                                                        picked[index] = on;
                                                    }
                                                })),
                                            ),
                                        )
                                        .child(meta(location, cx))
                                        .child(meta(format!("×{count}"), cx)),
                                )
                            },
                        ),
                    ));
                    let count = picked.iter().filter(|&&on| on).count();
                    (
                        "Add boosts you own".into(),
                        "A boost selected from the bank will move to inventory.",
                        format!(
                            "Add {count} {}",
                            if count == 1 { "boost" } else { "boosts" }
                        ),
                        false,
                        count == 0,
                    )
                }
                Some(Overlay::Pickup { drops, .. }) => {
                    rows = Some(v_flex().children(drops.iter().enumerate().map(
                        |(index, &(_, label, on))| {
                            Checkbox::new(("waiting-drop", index), label, checked(on)).on_toggle(
                                choose(cx, move |view, on, _| {
                                    if let Some(Overlay::Pickup { drops, .. }) = &mut view.overlay {
                                        drops[index].2 = on;
                                    }
                                }),
                            )
                        },
                    )));
                    (
                        "Apply these quest rules?".into(),
                        "Choose which pending quest drops to pick up now.",
                        "Apply rules".into(),
                        false,
                        false,
                    )
                }
                None => (String::new(), "", String::new(), false, true),
            };
        let mut dialog = Dialog::new("environment-dialog", self.overlay.is_some(), title)
            .alert()
            .description(detail)
            .on_close(change(cx, |view, _| view.overlay = None))
            .action(command(
                "cancel",
                "Cancel",
                Variant::Outline,
                change(cx, |view, _| view.overlay = None),
            ))
            .action(
                command(
                    "confirm",
                    action,
                    if destructive {
                        Variant::Danger
                    } else {
                        Variant::Primary
                    },
                    change(cx, Self::commit),
                )
                .disabled(disabled),
            );
        if let Some(rows) = rows {
            dialog = dialog.body(rows);
        }
        dialog
    }
}

impl Render for Environment {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let chrome = tool_chrome(Screen::Environment, Some(self.target.clone()), cx);
        let automation = self.automation();
        let body = if let Some(automation) = &automation {
            let drops = self.drops(automation, cx);
            let quests_and_boosts = v_flex()
                .gap(px(14.0))
                .child(self.quests(automation, cx))
                .child(divider(cx))
                .child(self.boosts(automation, cx));
            let panels = if window_width(Screen::Environment, window) >= px(560.0) {
                h_flex()
                    .items_start()
                    .min_h_full()
                    .child(div().flex_1().min_w_0().p(px(16.0)).child(drops))
                    .child(
                        div()
                            .flex_none()
                            .self_stretch()
                            .w(px(1.0))
                            .bg(palette(cx).border),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .p(px(16.0))
                            .child(quests_and_boosts),
                    )
            } else {
                v_flex()
                    .gap(px(14.0))
                    .p(px(16.0))
                    .child(drops)
                    .child(divider(cx))
                    .child(quests_and_boosts)
            };
            scroll("environment").child(panels).into_any_element()
        } else {
            empty(
                "No tabs open",
                "Open a tab to configure its automation.",
                cx,
            )
            .into_any_element()
        };
        frame(chrome, cx)
            .child(body)
            .child(self.actions(automation.as_ref(), cx))
            .children(
                automation
                    .as_ref()
                    .map(|automation| self.dialog(automation, cx)),
            )
    }
}
