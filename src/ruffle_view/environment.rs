use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Div, Entity, FocusHandle, Focusable as _,
    FontWeight, Global, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _,
    Pixels, Render, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions,
    actions, div, prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, StyledExt as _, Theme, Tone,
    components::Chips,
    h_flex,
    primitives::{
        Button, Checkbox, CheckedState, Dialog, Icon, Input, InputEvent, Switch,
        button::{Size, Variant},
    },
    theme::{DISABLED_OPACITY, ROW, TEXT_BODY, tabular},
    v_flex,
};

use super::{
    Client,
    client::CloseSession,
    form::{LABEL_WIDTH, note, row, row_label, section, switch_row},
    shown_tab::{self, ShownTab, Target, watch_changes},
};
use crate::{
    automation::{
        boosts::Owned,
        drops::{self, Waiting},
        quests::{self, Registered},
        register_items,
        settings::Automation,
    },
    bridge::CallError,
    domain::{inventory::ItemQuery, player::Boost},
    ruffle::Command,
};

actions!(mira, [OpenEnvironment]);

const WIDTH: f32 = 760.0;
const HEIGHT: f32 = 600.0;
const MIN_WIDTH: f32 = 340.0;
const MIN_HEIGHT: f32 = 280.0;
const COLUMNS_FROM: Pixels = px(560.0);
const MAX_LIST: usize = 1024;

type Setting<C> = (&'static str, &'static str, fn(&mut C) -> &mut bool);
type Answer<T> = async_channel::Receiver<Result<T, CallError>>;

struct Open(WindowHandle<Environment>);

impl Global for Open {}

pub(crate) fn open(client: Entity<Client>, target: Target, cx: &mut App) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0)
        && handle
            .update(cx, |environment, window, cx| {
                window.activate_window();
                environment.retarget(target, cx);
            })
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(WIDTH), px(HEIGHT)),
            cx,
        ))),
        window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
        titlebar: Some(TitlebarOptions {
            title: Some("Environment".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(12.0))),
        }),
        app_owns_titlebar_drag: true,
        is_minimizable: false,
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let environment = cx.new(|cx| Environment::new(client, target, window, cx));
        window.focus(&environment.read(cx).focus.clone(), cx);
        environment
    }) {
        Ok(handle) => cx.set_global(Open(handle)),
        Err(error) => tracing::error!(%error, "couldn't open the environment"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum List {
    Drops,
    Quests,
    Boosts,
}

impl List {
    const ALL: [List; 3] = [List::Drops, List::Quests, List::Boosts];

    fn label(self) -> &'static str {
        match self {
            List::Drops => "Pick up",
            List::Quests => "Turn in",
            List::Boosts => "Use",
        }
    }

    fn input_label(self) -> &'static str {
        match self {
            List::Drops => "Add drops to pick up",
            List::Quests => "Add quests to turn in",
            List::Boosts => "Add boosts to use",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            List::Drops => "Tainted Gem, 18927",
            List::Quests => "2857, 6552:45230",
            List::Boosts => "Gold Boost! (60 min)",
        }
    }

    fn clear_title(self, automation: &Automation) -> String {
        let (count, one, many) = match self {
            List::Drops => (automation.drops().items.len(), "drop", "drops"),
            List::Quests => (automation.quests().quests.len(), "quest", "quests"),
            List::Boosts => (automation.boosts().items.len(), "boost", "boosts"),
        };
        format!("Clear {count} {}?", if count == 1 { one } else { many })
    }

    fn note(self) -> &'static str {
        match self {
            List::Drops => "Click a bell to beep when that item drops.",
            List::Quests => "Specify an item ID after a colon to choose a reward.",
            List::Boosts => "First listed of each kind wins.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirm {
    CopyToAll,
    Clear(List),
    ClearAll,
}

struct Unreadable {
    text: String,
    reason: SharedString,
}

enum Fetch {
    Idle,
    Finding {
        _reply: Task<()>,
    },
    Picking {
        found: Vec<Owned>,
        picked: Vec<bool>,
    },
    Adding {
        _reply: Task<()>,
    },
    Said(SharedString, Tone),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Taken {
    Rewards,
    Requirements,
}

impl Taken {
    const ALL: [Taken; 2] = [Taken::Rewards, Taken::Requirements];

    fn label(self) -> &'static str {
        match self {
            Taken::Rewards => "Rewards",
            Taken::Requirements => "Requirements",
        }
    }

    fn flag(self, config: &mut quests::Config) -> &mut bool {
        match self {
            Taken::Rewards => &mut config.accept_rewards,
            Taken::Requirements => &mut config.accept_requirements,
        }
    }
}

enum QuestChange {
    Take(Taken),
    Add(Vec<Registered>),
    Edit(usize, Registered),
}

impl QuestChange {
    fn apply(&self, state: &mut quests::State) {
        match self {
            QuestChange::Take(taken) => *taken.flag(&mut state.config) = true,
            QuestChange::Add(quests) => state.register(quests.iter().copied()),
            QuestChange::Edit(index, quest) => {
                if *index < state.quests.len() {
                    state.quests[*index] = *quest;
                    let mut at = 0;
                    state.quests.retain(|kept| {
                        let duplicate = at != *index && kept.quest_id == quest.quest_id;
                        at += 1;
                        !duplicate
                    });
                }
            }
        }
    }
}

enum Pickup {
    Idle,
    Checking {
        _reply: Task<()>,
        change: QuestChange,
    },
    Asking {
        change: QuestChange,
        waiting: Vec<Waiting>,
        picked: Vec<bool>,
    },
}

pub(crate) struct Environment {
    client: Entity<Client>,
    focus: FocusHandle,
    target: Target,
    shown: Option<ShownTab>,
    inputs: [Entity<Input>; 3],
    unreadable: Option<Unreadable>,
    fetch: Fetch,
    pickup: Pickup,
    confirm: Option<Confirm>,
    _subscriptions: Vec<Subscription>,
}

impl Environment {
    fn new(
        client: Entity<Client>,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let inputs = List::ALL.map(|list| {
            cx.new(|cx| {
                Input::new(list.input_label(), cx)
                    .placeholder(list.placeholder())
                    .max_length(MAX_LIST)
            })
        });
        let mut subscriptions = vec![cx.observe(&client, |this, _, cx| this.show(cx))];
        for (list, input) in List::ALL.into_iter().zip(&inputs) {
            subscriptions.push(
                cx.subscribe(input, move |this, input, event, cx| match event {
                    InputEvent::Submitted => this.add(list, cx),
                    InputEvent::Changed => {
                        let text = input.read(cx).text();
                        if list == List::Quests
                            && this
                                .unreadable
                                .as_ref()
                                .is_some_and(|unreadable| unreadable.text != text)
                        {
                            this.unreadable = None;
                            cx.notify();
                        }
                    }
                }),
            );
            subscriptions.push(
                cx.on_blur(&input.focus_handle(cx), window, move |this, _, cx| {
                    this.add(list, cx)
                }),
            );
        }
        let mut environment = Self {
            client,
            focus: cx.focus_handle(),
            target,
            shown: None,
            inputs,
            unreadable: None,
            fetch: Fetch::Idle,
            pickup: Pickup::Idle,
            confirm: None,
            _subscriptions: subscriptions,
        };
        environment.show(cx);
        environment
    }

    fn retarget(&mut self, target: Target, cx: &mut Context<Self>) {
        self.target = target;
        self.show(cx);
    }

    fn show(&mut self, cx: &mut Context<Self>) {
        let switched = shown_tab::show(
            &mut self.shown,
            &mut self.target,
            &self.client,
            |this: &mut Self, cx| this.show(cx),
            |automation, cx| {
                vec![
                    watch_changes(automation.watch_drops(), cx),
                    watch_changes(automation.watch_quests(), cx),
                    watch_changes(automation.watch_boosts(), cx),
                    watch_changes(automation.watch_quest_items(), cx),
                ]
            },
            cx,
        );
        if switched {
            self.reset(cx);
        }
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        for input in &self.inputs {
            input.update(cx, |input, cx| input.set_text("", cx));
        }
        self.unreadable = None;
        self.fetch = Fetch::Idle;
        self.pickup = Pickup::Idle;
        self.confirm = None;
    }

    fn automation(&self) -> Option<&Automation> {
        self.shown.as_ref().map(|shown| &shown.automation)
    }

    fn input(&self, list: List) -> &Entity<Input> {
        &self.inputs[list as usize]
    }

    fn add(&mut self, list: List, cx: &mut Context<Self>) {
        let Some(automation) = self.automation().cloned() else {
            return;
        };
        let input = self.input(list).clone();
        let text = input.read(cx).text().to_owned();
        if text.trim().is_empty() {
            return;
        }
        let mut left = String::new();
        match list {
            List::Drops => {
                automation.update_drops(|state| register_items(&mut state.items, items(&text)));
            }
            List::Boosts => {
                automation.update_boosts(|state| register_items(&mut state.items, items(&text)));
            }
            List::Quests => {
                if !matches!(self.pickup, Pickup::Idle) {
                    return;
                }
                let (quests, unreadable) = read_quests(&text);
                if let Some(unreadable) = &unreadable {
                    left.clone_from(&unreadable.text);
                }
                self.unreadable = unreadable;
                if !quests.is_empty() {
                    self.change_quests(QuestChange::Add(quests), cx);
                }
            }
        }
        input.update(cx, |input, cx| input.set_text(left, cx));
        cx.notify();
    }

    fn ask<T>(
        &self,
        command: impl FnOnce(async_channel::Sender<Result<T, CallError>>) -> Command,
        cx: &mut App,
    ) -> Option<Answer<T>> {
        let view = self.shown.as_ref()?.view.upgrade()?;
        let (reply, answer) = async_channel::bounded(1);
        view.update(cx, |view, _| view.session.command(command(reply)));
        Some(answer)
    }

    fn fetch_boosts(&mut self, cx: &mut Context<Self>) {
        let Some(answer) = self.ask(Command::FetchBoosts, cx) else {
            return;
        };
        let reply = cx.spawn(async move |this, cx| {
            let found = answer.recv().await;
            let _ = this.update(cx, |this, cx| {
                this.fetch = match found {
                    Ok(Ok(found)) => this.offer(found),
                    Ok(Err(error)) => {
                        tracing::warn!(%error, "couldn't fetch boosts");
                        Fetch::Said("Couldn't fetch boosts".into(), Tone::Danger)
                    }
                    Err(_) => Fetch::Said("The tab closed".into(), Tone::Danger),
                };
                cx.notify();
            });
        });
        self.fetch = Fetch::Finding { _reply: reply };
        cx.notify();
    }

    fn offer(&self, found: Vec<Owned>) -> Fetch {
        let listed = self
            .automation()
            .map(|automation| automation.boosts().items)
            .unwrap_or_default();
        if found.is_empty() {
            Fetch::Said("No boosts found".into(), Tone::Neutral)
        } else if found.iter().all(|owned| is_listed(&listed, owned)) {
            Fetch::Said("Every boost you own is listed".into(), Tone::Neutral)
        } else {
            let picked = vec![false; found.len()];
            Fetch::Picking { found, picked }
        }
    }

    fn add_picked(&mut self, cx: &mut Context<Self>) {
        let Fetch::Picking { found, picked } = std::mem::replace(&mut self.fetch, Fetch::Idle)
        else {
            return;
        };
        let picked: Vec<Owned> = found
            .into_iter()
            .zip(picked)
            .filter_map(|(owned, picked)| picked.then_some(owned))
            .collect();
        let Some(answer) = self.ask(|missed| Command::TakeBoosts { picked, missed }, cx) else {
            return cx.notify();
        };
        let reply = cx.spawn(async move |this, cx| {
            let missed = answer.recv().await;
            let _ = this.update(cx, |this, cx| {
                this.fetch = match missed {
                    Ok(Ok(missed)) => match missed.len() {
                        0 => Fetch::Idle,
                        1 => Fetch::Said("Couldn't withdraw 1 boost".into(), Tone::Warning),
                        count => Fetch::Said(
                            format!("Couldn't withdraw {count} boosts").into(),
                            Tone::Warning,
                        ),
                    },
                    Ok(Err(error)) => {
                        tracing::warn!(%error, "couldn't add boosts");
                        Fetch::Said("Couldn't add boosts".into(), Tone::Danger)
                    }
                    Err(_) => Fetch::Said("The tab closed".into(), Tone::Danger),
                };
                cx.notify();
            });
        });
        self.fetch = Fetch::Adding { _reply: reply };
        cx.notify();
    }

    fn change_quests(&mut self, change: QuestChange, cx: &mut Context<Self>) {
        let Some(automation) = self.automation().cloned() else {
            return;
        };
        let mut after = automation.quests();
        change.apply(&mut after);
        let sweeps = automation.drops().config.enabled
            && (after.config.accept_rewards || after.config.accept_requirements);
        let answer = sweeps
            .then(|| self.ask(|waiting| Command::QuestPickups { after, waiting }, cx))
            .flatten();
        let Some(answer) = answer else {
            automation.update_quests(|state| change.apply(state));
            return cx.notify();
        };
        let reply = cx.spawn(async move |this, cx| {
            let waiting = answer.recv().await;
            let _ = this.update(cx, |this, cx| {
                let Pickup::Checking { change, .. } =
                    std::mem::replace(&mut this.pickup, Pickup::Idle)
                else {
                    return;
                };
                match waiting {
                    Ok(Ok(waiting)) if !waiting.is_empty() => {
                        let picked = vec![true; waiting.len()];
                        this.pickup = Pickup::Asking {
                            change,
                            waiting,
                            picked,
                        };
                    }
                    Ok(found) => {
                        if let Err(error) = found {
                            tracing::warn!(%error, "couldn't check the drop list");
                        }
                        if let Some(automation) = this.automation() {
                            automation.update_quests(|state| change.apply(state));
                        }
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        });
        self.pickup = Pickup::Checking {
            _reply: reply,
            change,
        };
        cx.notify();
    }

    fn confirm_pickup(&mut self, cx: &mut Context<Self>) {
        let Pickup::Asking {
            change,
            waiting,
            picked,
        } = std::mem::replace(&mut self.pickup, Pickup::Idle)
        else {
            return;
        };
        let Some(automation) = self.automation() else {
            return;
        };
        let held: Vec<i32> = waiting
            .iter()
            .zip(picked)
            .filter_map(|(waiting, picked)| (!picked).then_some(waiting.item_id))
            .collect();
        if !held.is_empty() {
            automation.update_drops(|state| {
                for id in held {
                    if !state.held.contains(&id) {
                        state.held.push(id);
                    }
                }
            });
        }
        automation.update_quests(|state| change.apply(state));
        cx.notify();
    }

    fn pick_waiting(&mut self, index: usize, on: bool, cx: &mut Context<Self>) {
        if let Pickup::Asking { picked, .. } = &mut self.pickup
            && let Some(picked) = picked.get_mut(index)
        {
            *picked = on;
            cx.notify();
        }
    }

    fn pick(&mut self, index: usize, on: bool, cx: &mut Context<Self>) {
        if let Fetch::Picking { picked, .. } = &mut self.fetch
            && let Some(picked) = picked.get_mut(index)
        {
            *picked = on;
            cx.notify();
        }
    }

    fn confirmed(&mut self, cx: &mut Context<Self>) {
        let (Some(confirm), Some(shown)) = (self.confirm.take(), &self.shown) else {
            return;
        };
        match confirm {
            Confirm::Clear(list) => clear(&shown.automation, list),
            Confirm::ClearAll => {
                for list in List::ALL {
                    clear(&shown.automation, list);
                }
            }
            Confirm::CopyToAll => {
                for view in self.client.read(cx).views() {
                    if view.entity_id() != shown.id {
                        view.read(cx)
                            .automation
                            .copy_environment_from(&shown.automation);
                    }
                }
            }
        }
        cx.notify();
    }

    fn list_field(
        &self,
        list: List,
        enabled: bool,
        note: Div,
        chips: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Div {
        let input = self.input(list).clone();
        input.update(cx, |input, cx| input.set_disabled(!enabled, cx));
        let typed = div()
            .child(input.clone())
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    input.update(cx, |input, cx| input.set_text("", cx));
                }
            });
        v_flex()
            .gap(px(10.0))
            .child(row(list.label(), enabled, typed))
            .child(note.mt(px(-4.0)))
            .children(chips)
    }

    fn clear_button(&self, list: List, cx: &mut Context<Self>) -> Button {
        Button::new(("clear-list", list as usize), "Clear")
            .size(Size::Small)
            .variant(Variant::Ghost)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.confirm = Some(Confirm::Clear(list));
                cx.notify();
            }))
    }

    fn edit_quest(&mut self, index: usize, text: String, cx: &mut Context<Self>) {
        match read_quest(text.trim()) {
            Ok(quest) => {
                self.unreadable = None;
                if matches!(self.pickup, Pickup::Idle) {
                    self.change_quests(QuestChange::Edit(index, quest), cx);
                }
            }
            Err(reason) => {
                self.unreadable = Some(Unreadable {
                    text: self.input(List::Quests).read(cx).text().to_owned(),
                    reason: reason.into(),
                });
                cx.notify();
            }
        }
    }

    fn refocus(&self, list: List, cx: &App) -> impl Fn(bool, &mut Window, &mut App) + 'static {
        let focus = self.input(list).focus_handle(cx);
        move |emptied, window, cx| {
            if emptied {
                window.focus(&focus, cx);
            }
        }
    }

    fn drops(&self, theme: &Theme, automation: &Automation, cx: &mut Context<Self>) -> Div {
        let state = automation.drops();
        let on = state.config.enabled;
        let switching = automation.clone();
        let switch = Switch::new("drops-enabled", "Drops", on).on_toggle(move |on, _, _| {
            switching.update_drops(|state| state.config.enabled = on);
        });
        let listed = (!state.items.is_empty()).then(|| {
            let (marking, removing, editing, refocus) = (
                automation.clone(),
                automation.clone(),
                automation.clone(),
                self.refocus(List::Drops, cx),
            );
            Chips::new("drops", "Drops", state.items.iter().map(item_label))
                .disabled(!on)
                .marked(
                    Icon::Bell,
                    "Beep",
                    state.items.iter().map(|query| state.rings(query)),
                )
                .on_mark(move |index, on, _, _| {
                    marking.update_drops(|state| {
                        if let Some(query) = state.items.get(index).cloned() {
                            state.set_sound(&query, on);
                        }
                    });
                })
                .on_edit(move |index, text, _, _| {
                    editing.update_drops(|state| {
                        let rang = state
                            .items
                            .get(index)
                            .is_some_and(|query| state.rings(query));
                        let (old, new) = replace_items(&mut state.items, index, items(&text));
                        if let Some(old) = old {
                            state.set_sound(&old, false);
                        }
                        for query in new.iter().filter(|_| rang) {
                            state.set_sound(query, true);
                        }
                    });
                })
                .on_remove(move |index, window, cx| {
                    let left = removing.update_drops(|state| {
                        if let Some(query) = state.items.get(index).cloned() {
                            state.unregister(&[query]);
                        }
                    });
                    refocus(left.items.is_empty(), window, cx);
                })
                .end_slot(self.clear_button(List::Drops, cx))
        });
        let from_quests: Vec<String> = automation
            .quest_items()
            .into_iter()
            .filter(|name| {
                let query = ItemQuery::Name(name.clone());
                !state.items.iter().any(|listed| listed.same(&query))
            })
            .collect();
        let derived = (!from_quests.is_empty()).then(|| {
            let marking = automation.clone();
            let names = from_quests.clone();
            v_flex()
                .gap(px(6.0))
                .child(
                    div()
                        .text_caption()
                        .text_color(theme.text_tertiary)
                        .when(!on, |caption| caption.opacity(DISABLED_OPACITY))
                        .child("From your quests"),
                )
                .child(
                    Chips::new(
                        "quest-drops",
                        "From your quests",
                        from_quests
                            .iter()
                            .map(|name| SharedString::from(name.clone())),
                    )
                    .quiet()
                    .disabled(!on)
                    .marked(
                        Icon::Bell,
                        "Beep",
                        from_quests
                            .iter()
                            .map(|name| state.rings(&ItemQuery::Name(name.clone()))),
                    )
                    .on_mark(move |index, on, _, _| {
                        if let Some(name) = names.get(index) {
                            marking.update_drops(|state| {
                                state.set_sound(&ItemQuery::Name(name.clone()), on)
                            });
                        }
                    }),
                )
        });
        let chips = match (listed, derived) {
            (None, None) => None,
            (listed, derived) => Some(
                v_flex()
                    .gap(px(10.0))
                    .children(listed)
                    .children(derived)
                    .into_any_element(),
            ),
        };
        let kinds: [Setting<drops::Config>; 4] = [
            ("drops-ac", "AC", |config| &mut config.accept_ac),
            ("drops-ac-member", "Member-only AC", |config| {
                &mut config.accept_ac_member_only
            }),
            ("drops-non-ac", "Non-AC", |config| &mut config.accept_non_ac),
            ("drops-non-ac-member", "Member-only non-AC", |config| {
                &mut config.accept_non_ac_member_only
            }),
        ];
        let kinds = v_flex().children(kinds.into_iter().map(|(id, label, setting)| {
            let automation = automation.clone();
            h_flex().min_h(ROW).child(
                Checkbox::new(id, label, checked(*setting(&mut state.config.clone())))
                    .disabled(!on)
                    .on_toggle(move |on, _, _| {
                        automation.update_drops(|state| *setting(&mut state.config) = on);
                    }),
            )
        }));
        let rejecting = automation.clone();
        let reject = Switch::new("drops-reject", "Reject others", state.config.reject_others)
            .on_toggle(move |on, _, _| {
                rejecting.update_drops(|state| state.config.reject_others = on);
            });
        v_flex()
            .gap(px(8.0))
            .child(section(
                "Drops",
                "Picks up listed drops and any kind ticked below.",
                None,
                switch.hide_label(),
                theme,
            ))
            .child(self.list_field(
                List::Drops,
                on,
                note(List::Drops.note(), on, theme),
                chips,
                cx,
            ))
            .child(
                h_flex()
                    .items_start()
                    .gap(px(12.0))
                    .child(
                        h_flex()
                            .flex_none()
                            .w(LABEL_WIDTH)
                            .min_h(ROW)
                            .child(row_label("Also pick up", on)),
                    )
                    .child(div().flex_1().min_w_0().child(kinds)),
            )
            .child(
                v_flex()
                    .gap(px(10.0))
                    .child(switch_row("Reject others", on, reject))
                    .child(note("Rejects every drop not picked up above.", on, theme).mt(px(-4.0))),
            )
    }

    fn quests(&self, theme: &Theme, automation: &Automation, cx: &mut Context<Self>) -> Div {
        let state = automation.quests();
        let on = state.config.enabled;
        let switching = automation.clone();
        let switch = Switch::new("quests-enabled", "Quests", on).on_toggle(move |on, _, _| {
            switching.update_quests(|state| state.config.enabled = on);
        });
        let chips = (!state.quests.is_empty()).then(|| {
            let (removing, refocus) = (automation.clone(), self.refocus(List::Quests, cx));
            let editing = cx.weak_entity();
            Chips::new("quests", "Quests", state.quests.iter().map(quest_label))
                .disabled(!on)
                .on_edit(move |index, text, _, cx| {
                    let _ = editing.update(cx, |this, cx| this.edit_quest(index, text, cx));
                })
                .on_remove(move |index, window, cx| {
                    let left = removing.update_quests(|state| {
                        if let Some(registered) = state.quests.get(index) {
                            let quest_id = registered.quest_id;
                            state.unregister(&[quest_id]);
                        }
                    });
                    refocus(left.quests.is_empty(), window, cx);
                })
                .end_slot(self.clear_button(List::Quests, cx))
                .into_any_element()
        });
        let message = match &self.unreadable {
            Some(unreadable) => div()
                .text_small()
                .text_color(theme.danger_text)
                .child(unreadable.reason.clone()),
            None => note(List::Quests.note(), on, theme),
        };
        let settled = matches!(self.pickup, Pickup::Idle);
        let pickups = h_flex()
            .gap(px(12.0))
            .children(Taken::ALL.into_iter().map(|taken| {
                let (automation, this) = (automation.clone(), cx.weak_entity());
                div().flex_1().min_w_0().child(
                    Checkbox::new(
                        ("quests-take", taken as usize),
                        taken.label(),
                        checked(*taken.flag(&mut state.config.clone())),
                    )
                    .disabled(!on || !settled)
                    .on_toggle(move |on, _, cx| {
                        if on {
                            let _ = this.update(cx, |this, cx| {
                                this.change_quests(QuestChange::Take(taken), cx)
                            });
                        } else {
                            automation
                                .update_quests(|state| *taken.flag(&mut state.config) = false);
                        }
                    }),
                )
            }));
        let status = matches!(self.pickup, Pickup::Checking { .. })
            .then(|| ("Checking drops…".into(), theme.text_tertiary));
        v_flex()
            .gap(px(8.0))
            .child(section(
                "Quests",
                "Accepts listed quests and turns them in when done.",
                status,
                switch.hide_label(),
                theme,
            ))
            .child(self.list_field(List::Quests, on, message, chips, cx))
            .child(row("Also pick up", on, pickups))
    }

    fn boosts(&self, theme: &Theme, automation: &Automation, cx: &mut Context<Self>) -> Div {
        let state = automation.boosts();
        let on = state.config.enabled;
        let switching = automation.clone();
        let switch = Switch::new("boosts-enabled", "Boosts", on).on_toggle(move |on, _, _| {
            switching.update_boosts(|state| state.config.enabled = on);
        });
        let fetch = Button::new("fetch-boosts", "Fetch…")
            .size(Size::Small)
            .variant(Variant::Ghost)
            .tooltip("Find boosts in your inventory and bank")
            .busy(matches!(
                self.fetch,
                Fetch::Finding { .. } | Fetch::Adding { .. }
            ))
            .on_click(cx.listener(|this, _, _, cx| this.fetch_boosts(cx)));
        let status = match &self.fetch {
            Fetch::Said(text, tone) => Some((text.clone(), theme.tone(*tone).2)),
            Fetch::Idle | Fetch::Finding { .. } | Fetch::Picking { .. } | Fetch::Adding { .. } => {
                None
            }
        };
        let chips = (!state.items.is_empty()).then(|| {
            let (removing, editing, refocus) = (
                automation.clone(),
                automation.clone(),
                self.refocus(List::Boosts, cx),
            );
            Chips::new("boosts", "Boosts", state.items.iter().map(item_label))
                .disabled(!on)
                .on_edit(move |index, text, _, _| {
                    editing.update_boosts(|state| {
                        replace_items(&mut state.items, index, items(&text));
                    });
                })
                .on_remove(move |index, window, cx| {
                    let left = removing.update_boosts(|state| {
                        if index < state.items.len() {
                            state.items.remove(index);
                        }
                    });
                    refocus(left.items.is_empty(), window, cx);
                })
                .end_slot(self.clear_button(List::Boosts, cx))
                .into_any_element()
        });
        v_flex()
            .gap(px(8.0))
            .child(section(
                "Boosts",
                "Uses one of each kind when it runs out.",
                status,
                h_flex()
                    .gap(px(10.0))
                    .child(fetch)
                    .child(switch.hide_label()),
                theme,
            ))
            .child(self.list_field(
                List::Boosts,
                on,
                note(List::Boosts.note(), on, theme),
                chips,
                cx,
            ))
    }

    fn body(
        &self,
        theme: &Theme,
        automation: &Automation,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let drops = self.drops(theme, automation, cx);
        let others = v_flex()
            .gap(px(14.0))
            .child(self.quests(theme, automation, cx))
            .child(div().h(px(1.0)).bg(theme.hairline))
            .child(self.boosts(theme, automation, cx));
        if window.viewport_size().width >= COLUMNS_FROM {
            h_flex()
                .items_start()
                .min_h_full()
                .child(div().flex_1().min_w_0().p(px(16.0)).child(drops))
                .child(div().self_stretch().w(px(1.0)).bg(theme.hairline))
                .child(div().flex_1().min_w_0().p(px(16.0)).child(others))
        } else {
            v_flex()
                .gap(px(14.0))
                .p(px(16.0))
                .child(drops)
                .child(div().h(px(1.0)).bg(theme.hairline))
                .child(others)
        }
    }

    fn fetch_dialog(&self, theme: &Theme, cx: &mut Context<Self>) -> Dialog {
        let (found, picked) = match &self.fetch {
            Fetch::Picking { found, picked } => (found.as_slice(), picked.as_slice()),
            _ => (&[][..], &[][..]),
        };
        let listed = self
            .automation()
            .map(|automation| automation.boosts().items)
            .unwrap_or_default();
        let count = picked.iter().filter(|picked| **picked).count();
        let mut rows = v_flex().gap(px(2.0));
        for (index, owned) in found.iter().enumerate() {
            if index == 0 || found[index - 1].kind != owned.kind {
                rows = rows.child(
                    div()
                        .when(index > 0, |kind| kind.pt(px(8.0)))
                        .text_caption()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text_tertiary)
                        .child(kind_name(owned.kind)),
                );
            }
            let already = is_listed(&listed, owned);
            let this = cx.weak_entity();
            let place = if already {
                "Listed"
            } else if owned.banked {
                "Bank"
            } else {
                ""
            };
            rows = rows.child(pick_row(
                Checkbox::new(
                    ("found-boost", index),
                    owned.name.clone(),
                    checked(already || picked.get(index).copied().unwrap_or(false)),
                )
                .disabled(already)
                .on_toggle(move |on, _, cx| {
                    let _ = this.update(cx, |this, cx| this.pick(index, on, cx));
                }),
                Some(place),
                owned.quantity,
                theme,
            ));
        }
        let closing = cx.weak_entity();
        Dialog::new(
            "found-boosts",
            matches!(self.fetch, Fetch::Picking { .. }),
            "Add boosts you own",
        )
        .alert()
        .width(px(420.0))
        .description("Adding one from your bank moves it to your inventory.")
        .body(pick_list("found-boosts-list", rows))
        .action(
            Button::new("found-cancel", "Cancel").on_click(cx.listener(|this, _, _, cx| {
                this.fetch = Fetch::Idle;
                cx.notify();
            })),
        )
        .action(
            Button::new(
                "found-add",
                match count {
                    1 => "Add 1 boost".to_owned(),
                    count => format!("Add {count} boosts"),
                },
            )
            .variant(Variant::Primary)
            .disabled(count == 0)
            .on_click(cx.listener(|this, _, _, cx| this.add_picked(cx))),
        )
        .on_close(move |_, cx| {
            let _ = closing.update(cx, |this, cx| {
                this.fetch = Fetch::Idle;
                cx.notify();
            });
        })
    }

    fn pickup_dialog(&self, theme: &Theme, cx: &mut Context<Self>) -> Dialog {
        let (change, waiting, picked) = match &self.pickup {
            Pickup::Asking {
                change,
                waiting,
                picked,
            } => (Some(change), waiting.as_slice(), picked.as_slice()),
            _ => (None, &[][..], &[][..]),
        };
        let (title, cause, action) = match change {
            Some(QuestChange::Take(Taken::Rewards)) => (
                "Turn on Rewards?".to_owned(),
                "Your listed quests reward these",
                "Turn on",
            ),
            Some(QuestChange::Take(Taken::Requirements)) => (
                "Turn on Requirements?".to_owned(),
                "Your listed quests need these",
                "Turn on",
            ),
            Some(QuestChange::Add(quests)) => (
                match quests.len() {
                    1 => "Add 1 quest?".to_owned(),
                    count => format!("Add {count} quests?"),
                },
                "The quests you're adding reward or need these",
                "Add",
            ),
            Some(QuestChange::Edit(_, quest)) => (
                format!("Change to quest {}?", quest.quest_id),
                "That quest rewards or needs these",
                "Change",
            ),
            None => (String::new(), "", ""),
        };
        let this = cx.weak_entity();
        let rows = v_flex()
            .gap(px(2.0))
            .children(waiting.iter().enumerate().map(|(index, waiting)| {
                let this = this.clone();
                pick_row(
                    Checkbox::new(
                        ("waiting-drop", index),
                        waiting.name.clone(),
                        checked(picked.get(index).copied().unwrap_or(false)),
                    )
                    .on_toggle(move |on, _, cx| {
                        let _ = this.update(cx, |this, cx| this.pick_waiting(index, on, cx));
                    }),
                    None,
                    waiting.quantity,
                    theme,
                )
            }));
        let closing = cx.weak_entity();
        Dialog::new("waiting-drops", change.is_some(), title)
            .alert()
            .width(px(420.0))
            .description(format!(
                "{cause}, and they're in the drop list now. Ticked ones are picked up right away. The rest stay where they are."
            ))
            .body(pick_list("waiting-drops-list", rows))
            .action(
                Button::new("waiting-cancel", "Cancel").on_click(cx.listener(|this, _, _, cx| {
                    this.pickup = Pickup::Idle;
                    cx.notify();
                })),
            )
            .action(
                Button::new("waiting-accept", action)
                    .variant(Variant::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.confirm_pickup(cx))),
            )
            .on_close(move |_, cx| {
                let _ = closing.update(cx, |this, cx| {
                    this.pickup = Pickup::Idle;
                    cx.notify();
                });
            })
    }

    fn confirm_dialog(&self, tabs: usize, cx: &mut Context<Self>) -> Dialog {
        let (title, body, action) = match self.confirm {
            Some(Confirm::CopyToAll) => (
                format!(
                    "Copy this environment to {} other tabs?",
                    tabs.saturating_sub(1)
                ),
                "Their lists and rules are replaced with this tab's. Each tab keeps its own on and off switches.",
                "Copy",
            ),
            Some(Confirm::Clear(list)) => (
                self.automation()
                    .map(|automation| list.clear_title(automation))
                    .unwrap_or_default(),
                "Rules and switches stay as they are.",
                "Clear",
            ),
            Some(Confirm::ClearAll) => (
                "Clear every list in this tab?".to_owned(),
                "This removes its drops, quests and boosts. Rules and switches stay as they are.",
                "Clear",
            ),
            None => (String::new(), "", ""),
        };
        let closing = cx.weak_entity();
        Dialog::new("environment-confirm", self.confirm.is_some(), title)
            .alert()
            .description(body)
            .action(
                Button::new("confirm-cancel", "Cancel").on_click(cx.listener(|this, _, _, cx| {
                    this.confirm = None;
                    cx.notify();
                })),
            )
            .action(
                Button::new("confirm-accept", action)
                    .variant(if self.confirm == Some(Confirm::CopyToAll) {
                        Variant::Primary
                    } else {
                        Variant::Danger
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.confirmed(cx))),
            )
            .on_close(move |_, cx| {
                let _ = closing.update(cx, |this, cx| {
                    this.confirm = None;
                    cx.notify();
                });
            })
    }
}

fn pick_row(checkbox: Checkbox, tag: Option<&'static str>, quantity: i32, theme: &Theme) -> Div {
    h_flex()
        .min_h(ROW)
        .gap(px(8.0))
        .child(div().flex_1().min_w_0().child(checkbox))
        .when_some(tag, |row, tag| {
            row.child(
                div()
                    .flex_none()
                    .w(px(48.0))
                    .text_small()
                    .text_color(theme.text_tertiary)
                    .child(tag),
            )
        })
        .child(
            div()
                .flex_none()
                .w(px(40.0))
                .text_right()
                .text_small()
                .font_features(tabular())
                .text_color(theme.text_tertiary)
                .child(format!("×{quantity}")),
        )
}

fn pick_list(id: &'static str, rows: Div) -> Stateful<Div> {
    div()
        .id(id)
        .max_h(px(320.0))
        .overflow_y_scroll()
        .child(rows)
}

fn clear(automation: &Automation, list: List) {
    match list {
        List::Drops => {
            automation.update_drops(drops::State::clear);
        }
        List::Quests => {
            automation.update_quests(|state| state.quests.clear());
        }
        List::Boosts => {
            automation.update_boosts(|state| state.items.clear());
        }
    }
}

fn replace_items(
    list: &mut Vec<ItemQuery>,
    index: usize,
    new: Vec<ItemQuery>,
) -> (Option<ItemQuery>, Vec<ItemQuery>) {
    if index >= list.len() || new.is_empty() {
        return (None, Vec::new());
    }
    let old = list.remove(index);
    let mut added = Vec::new();
    for query in new {
        if !list.iter().any(|known| known.same(&query)) {
            list.insert(index + added.len(), query.clone());
            added.push(query);
        }
    }
    (Some(old), added)
}

fn positive_id(text: &str) -> Option<i32> {
    text.trim().parse().ok().filter(|id| *id > 0)
}

fn items(text: &str) -> Vec<ItemQuery> {
    text.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| match positive_id(entry) {
            Some(item_id) => ItemQuery::ItemId { item_id },
            None => ItemQuery::Name(entry.to_owned()),
        })
        .collect()
}

fn read_quests(text: &str) -> (Vec<Registered>, Option<Unreadable>) {
    let mut quests = Vec::new();
    let mut left = Vec::new();
    let mut reason = None;
    for entry in text
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        match read_quest(entry) {
            Ok(quest) => quests.push(quest),
            Err(why) => {
                reason.get_or_insert(why);
                left.push(entry);
            }
        }
    }
    let unreadable = reason.map(|reason| Unreadable {
        text: left.join(", "),
        reason: reason.into(),
    });
    (quests, unreadable)
}

fn read_quest(entry: &str) -> Result<Registered, String> {
    let (quest, reward) = entry.split_once(':').unwrap_or((entry, ""));
    let quest_id = positive_id(quest).ok_or_else(|| format!("Not a quest ID: {}", quest.trim()))?;
    let reward = reward.trim();
    let reward_item_id = match reward {
        "" => None,
        reward => Some(positive_id(reward).ok_or_else(|| format!("Not an item ID: {reward}"))?),
    };
    Ok(Registered {
        quest_id,
        reward_item_id,
    })
}

fn item_label(query: &ItemQuery) -> SharedString {
    match query {
        ItemQuery::Name(name) => name.clone().into(),
        ItemQuery::ItemId { item_id } => item_id.to_string().into(),
    }
}

fn quest_label(registered: &Registered) -> SharedString {
    match registered.reward_item_id {
        Some(reward) => format!("{}:{reward}", registered.quest_id).into(),
        None => registered.quest_id.to_string().into(),
    }
}

fn is_listed(listed: &[ItemQuery], owned: &Owned) -> bool {
    let query = ItemQuery::Name(owned.name.clone());
    listed.iter().any(|known| known.same(&query))
}

fn kind_name(kind: Boost) -> &'static str {
    match kind {
        Boost::Gold => "Gold",
        Boost::Xp => "Experience",
        Boost::Rep => "Reputation",
        Boost::ClassPoints => "Class points",
    }
}

fn checked(on: bool) -> CheckedState {
    if on {
        CheckedState::Checked
    } else {
        CheckedState::Unchecked
    }
}

impl Render for Environment {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let tabs = self.client.read(cx).views().len();
        let header = shown_tab::header(
            "Environment",
            &self.client,
            self.shown.as_ref(),
            self.target,
            Self::retarget,
            window,
            cx,
        );
        let automation = self.automation().cloned();
        let clearable = automation.as_ref().is_some_and(|automation| {
            !(automation.drops().items.is_empty()
                && automation.quests().quests.is_empty()
                && automation.boosts().items.is_empty())
        });
        let body = match &automation {
            Some(automation) => self.body(&theme, automation, window, cx),
            None => div()
                .py(px(24.0))
                .text_center()
                .text_small()
                .text_color(theme.text_tertiary)
                .child("No tabs open"),
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.base)
            .text_size(TEXT_BODY)
            .text_color(theme.text)
            .key_context(FOCUS_SCOPE)
            .id("environment")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseSession, window, _| window.remove_window()))
            .child(mira_ui::nav::pointer_probe())
            .child(header)
            .child(
                div()
                    .id("environment-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .py(px(10.0))
                    .border_t_1()
                    .border_color(theme.hairline)
                    .child(
                        Button::new("copy-to-all", "Copy to all tabs…")
                            .size(Size::Small)
                            .disabled(automation.is_none() || tabs < 2)
                            .tooltip("Give every other tab this tab's lists and rules")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm = Some(Confirm::CopyToAll);
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("clear-all", "Clear all…")
                            .size(Size::Small)
                            .variant(Variant::Ghost)
                            .disabled(!clearable)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm = Some(Confirm::ClearAll);
                                cx.notify();
                            })),
                    ),
            )
            .child(self.fetch_dialog(&theme, cx))
            .child(self.pickup_dialog(&theme, cx))
            .child(self.confirm_dialog(tabs, cx))
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn open(cx: &mut TestAppContext) -> (Entity<Environment>, Automation, &mut VisualTestContext) {
        let folder = std::env::temp_dir().join(format!("mira-environment-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        cx.update(mira_ui::init);
        let automation = Automation::default();
        let shown = automation.clone();
        let (environment, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            let client = Client::empty_for_tests(&folder, cx);
            let mut environment = Environment::new(client, Target::Selected, window, cx);
            environment.shown = Some(ShownTab::fake("Alt 1", shown));
            environment
        });
        cx.run_until_parked();
        (environment, automation, cx)
    }

    fn focus(environment: &Entity<Environment>, list: List, cx: &mut VisualTestContext) {
        let input = environment.read_with(cx, |environment, _| environment.input(list).clone());
        cx.update(|window, cx| window.focus(&input.focus_handle(cx), cx));
        cx.run_until_parked();
    }

    fn field(environment: &Entity<Environment>, list: List, cx: &mut VisualTestContext) -> String {
        environment.read_with(cx, |environment, cx| {
            environment.input(list).read(cx).text().to_owned()
        })
    }

    fn reason(environment: &Entity<Environment>, cx: &mut VisualTestContext) -> Option<String> {
        environment.read_with(cx, |environment, _| {
            environment
                .unreadable
                .as_ref()
                .map(|unreadable| unreadable.reason.to_string())
        })
    }

    fn press(cx: &mut VisualTestContext, keys: &'static str) {
        cx.simulate_keystrokes(keys);
        cx.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse(keys).unwrap(),
        });
        cx.run_until_parked();
    }

    fn typed(cx: &mut VisualTestContext, text: &str) {
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    #[gpui::test]
    fn pasted_quests_are_added_and_unreadable_ones_stay_with_the_reason(cx: &mut TestAppContext) {
        let (environment, automation, cx) = open(cx);
        focus(&environment, List::Quests, cx);
        typed(cx, "2857, 6552:45230, abc");
        press(cx, "enter");
        let quest = |quest_id, reward_item_id| Registered {
            quest_id,
            reward_item_id,
        };
        assert_eq!(
            automation.quests().quests,
            [quest(2857, None), quest(6552, Some(45230))]
        );
        assert_eq!(field(&environment, List::Quests, cx), "abc");
        assert_eq!(
            reason(&environment, cx).as_deref(),
            Some("Not a quest ID: abc")
        );

        typed(cx, "d");
        assert_eq!(reason(&environment, cx), None);

        press(cx, "escape");
        typed(cx, "4432:x");
        press(cx, "enter");
        assert_eq!(
            reason(&environment, cx).as_deref(),
            Some("Not an item ID: x")
        );
        assert_eq!(automation.quests().quests.len(), 2);
    }

    #[gpui::test]
    fn drops_take_names_and_ids_on_return_or_leaving_the_field(cx: &mut TestAppContext) {
        let (environment, automation, cx) = open(cx);
        focus(&environment, List::Drops, cx);
        typed(cx, "Tainted Gem, 18927");
        press(cx, "enter");
        typed(cx, "1234, tainted gem");
        let window_focus = environment.read_with(cx, |environment, _| environment.focus.clone());
        cx.update(|window, cx| window.focus(&window_focus, cx));
        cx.run_until_parked();
        assert_eq!(
            automation.drops().items,
            [
                ItemQuery::Name("Tainted Gem".into()),
                ItemQuery::ItemId { item_id: 18927 },
                ItemQuery::ItemId { item_id: 1234 },
            ]
        );
        assert_eq!(field(&environment, List::Drops, cx), "");

        focus(&environment, List::Drops, cx);
        typed(cx, "Void Aura");
        press(cx, "escape");
        assert_eq!(field(&environment, List::Drops, cx), "");
        assert_eq!(automation.drops().items.len(), 3);
    }

    #[gpui::test]
    fn a_list_whose_section_is_off_can_still_be_cleared(cx: &mut TestAppContext) {
        let (environment, automation, cx) = open(cx);
        automation.update_boosts(|state| {
            register_items(&mut state.items, items("Gold Boost, XP Boost"));
            state.config.enabled = false;
        });
        let root = environment.update(cx, |environment, cx| {
            cx.notify();
            environment.focus.clone()
        });
        cx.update(|window, cx| window.focus(&root, cx));
        cx.run_until_parked();
        for keys in ["shift-tab", "shift-tab", "enter", "tab", "enter"] {
            press(cx, keys);
        }
        assert!(automation.boosts().items.is_empty());
        assert!(!automation.boosts().config.enabled);
    }

    #[gpui::test]
    fn confirming_waiting_drops_holds_the_unticked_ones_and_applies_the_change(
        cx: &mut TestAppContext,
    ) {
        let (environment, automation, cx) = open(cx);
        let waiting = |item_id, name: &str| Waiting {
            item_id,
            name: name.into(),
            quantity: 1,
        };
        environment.update(cx, |environment, cx| {
            environment.pickup = Pickup::Asking {
                change: QuestChange::Take(Taken::Rewards),
                waiting: vec![waiting(4, "Prize"), waiting(5, "Gem")],
                picked: vec![true, true],
            };
            environment.pick_waiting(1, false, cx);
            environment.confirm_pickup(cx);
        });
        assert!(automation.quests().config.accept_rewards);
        assert_eq!(automation.drops().held, [5]);
    }

    #[test]
    fn edits_keep_their_place_and_merge_into_an_existing_entry() {
        let quest = |quest_id, reward_item_id| Registered {
            quest_id,
            reward_item_id,
        };
        let mut state = quests::State {
            quests: vec![quest(1, None), quest(2, None), quest(3, None)],
            ..quests::State::default()
        };
        QuestChange::Edit(2, quest(1, Some(9))).apply(&mut state);
        assert_eq!(state.quests, [quest(2, None), quest(1, Some(9))]);

        let mut drops = vec![
            ItemQuery::Name("Gem".into()),
            ItemQuery::Name("Ore".into()),
            ItemQuery::Name("Cape".into()),
        ];
        replace_items(&mut drops, 0, items("Shard, 18927, ore"));
        assert_eq!(
            drops,
            [
                ItemQuery::Name("Shard".into()),
                ItemQuery::ItemId { item_id: 18927 },
                ItemQuery::Name("Ore".into()),
                ItemQuery::Name("Cape".into()),
            ]
        );
    }
}
