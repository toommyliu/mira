use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Div, Entity, EntityId, FocusHandle,
    FontWeight, Global, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Subscription,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, actions, div,
    prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, Theme,
    primitives::{
        Button, Checkbox, CheckedState, Dialog, Flow, Icon, Input, InputEvent, Pose, Sortable,
        Switch, Tabs,
        button::{Size, Variant},
    },
    theme::{LINE_SMALL, TEXT_BODY, TEXT_SMALL},
};

use super::{
    BAR_HEIGHT, Client,
    client::{CloseSession, TRAFFIC_LIGHTS_WIDTH, drag_area},
    shown_tab::{self, ShownTab, watch_changes},
};
use crate::{
    automation::{
        drops,
        quests::{self, Registered},
        register_items,
        settings::Automation,
        unregister_items,
    },
    domain::inventory::ItemQuery,
    ruffle::Command,
};

actions!(mira, [OpenEnvironment]);

const WIDTH: f32 = 520.0;
const HEIGHT: f32 = 640.0;
const MAX_NAME: usize = 64;
const MAX_ID: usize = 9;

type Setting<C> = (&'static str, &'static str, fn(&mut C) -> &mut bool);

struct Open(WindowHandle<Environment>);

impl Global for Open {}

pub(crate) fn open(client: Entity<Client>, cx: &mut App) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0)
        && handle
            .update(cx, |environment, window, cx| {
                window.activate_window();
                environment.show_active(cx);
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
        let environment = cx.new(|cx| Environment::new(client, window, cx));
        window.focus(&environment.read(cx).focus.clone(), cx);
        environment
    }) {
        Ok(handle) => cx.set_global(Open(handle)),
        Err(error) => tracing::error!(%error, "couldn't open the environment"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Drops,
    Quests,
    Boosts,
}

impl Section {
    const ALL: [Section; 3] = [Section::Drops, Section::Quests, Section::Boosts];

    fn name(self) -> &'static str {
        match self {
            Section::Drops => "Drops",
            Section::Quests => "Quests",
            Section::Boosts => "Boosts",
        }
    }

    fn count(self, automation: &Automation) -> usize {
        match self {
            Section::Drops => automation.drops().items.len(),
            Section::Quests => automation.quests().quests.len(),
            Section::Boosts => automation.boosts().items.len(),
        }
    }

    fn clear(self, automation: &Automation) {
        match self {
            Section::Drops => {
                automation.update_drops(drops::State::clear);
            }
            Section::Quests => {
                automation.update_quests(|state| state.quests.clear());
            }
            Section::Boosts => {
                automation.update_boosts(|state| state.items.clear());
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confirm {
    ApplyToAll,
    Clear(Section),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Drop,
    Quest,
    Reward,
    Boost,
}

impl Field {
    const ALL: [Field; 4] = [Field::Drop, Field::Quest, Field::Reward, Field::Boost];

    fn placeholder(self) -> &'static str {
        match self {
            Field::Drop => "Item name or ID",
            Field::Quest => "Quest ID",
            Field::Reward => "Reward ID (optional)",
            Field::Boost => "Boost name",
        }
    }

    fn section(self) -> Section {
        match self {
            Field::Drop => Section::Drops,
            Field::Quest | Field::Reward => Section::Quests,
            Field::Boost => Section::Boosts,
        }
    }

    fn max_length(self) -> usize {
        match self {
            Field::Drop | Field::Boost => MAX_NAME,
            Field::Quest | Field::Reward => MAX_ID,
        }
    }
}

pub(crate) struct Environment {
    client: Entity<Client>,
    focus: FocusHandle,
    section: Section,
    shown: Option<ShownTab>,
    inputs: [Entity<Input>; 4],
    confirm: Option<Confirm>,
    _subscriptions: Vec<Subscription>,
}

impl Environment {
    fn new(client: Entity<Client>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let inputs = Field::ALL.map(|field| {
            cx.new(|cx| {
                Input::new(field.placeholder(), cx)
                    .placeholder(field.placeholder())
                    .max_length(field.max_length())
            })
        });
        let mut subscriptions: Vec<Subscription> = Field::ALL
            .into_iter()
            .zip(&inputs)
            .map(|(field, input)| {
                cx.subscribe_in(
                    input,
                    window,
                    move |this, input, event, _, cx| match event {
                        InputEvent::Changed => {
                            input.update(cx, |input, cx| input.set_invalid(false, cx));
                        }
                        InputEvent::Submitted => this.add(field.section(), cx),
                    },
                )
            })
            .collect();
        subscriptions.push(cx.observe(&client, |this, _, cx| this.show(None, cx)));
        let mut environment = Self {
            client,
            focus: cx.focus_handle(),
            section: Section::Drops,
            shown: None,
            inputs,
            confirm: None,
            _subscriptions: subscriptions,
        };
        environment.show_active(cx);
        environment
    }

    fn show_active(&mut self, cx: &mut Context<Self>) {
        let active = shown_tab::active(&self.client, cx);
        self.show(active, cx);
    }

    fn show(&mut self, wanted: Option<EntityId>, cx: &mut Context<Self>) {
        let switched = shown_tab::show(
            &mut self.shown,
            &self.client,
            wanted,
            |this: &mut Self, cx| this.show(None, cx),
            |automation, cx| {
                vec![
                    watch_changes(automation.watch_drops(), cx),
                    watch_changes(automation.watch_quests(), cx),
                    watch_changes(automation.watch_boosts(), cx),
                ]
            },
            cx,
        );
        if switched {
            self.confirm = None;
        }
    }

    fn input(&self, field: Field) -> &Entity<Input> {
        &self.inputs[field as usize]
    }

    fn text(&self, field: Field, cx: &App) -> String {
        self.input(field).read(cx).text().trim().to_owned()
    }

    fn reject(&self, field: Field, cx: &mut Context<Self>) {
        self.input(field)
            .update(cx, |input, cx| input.set_invalid(true, cx));
    }

    fn clear_input(&self, field: Field, cx: &mut Context<Self>) {
        self.input(field)
            .update(cx, |input, cx| input.set_text("", cx));
    }

    fn add(&mut self, section: Section, cx: &mut Context<Self>) {
        let Some(automation) = self.shown.as_ref().map(|shown| shown.automation.clone()) else {
            return;
        };
        match section {
            Section::Drops => {
                let Some(item) = item_query(&self.text(Field::Drop, cx)) else {
                    return;
                };
                automation.update_drops(|state| register_items(&mut state.items, vec![item]));
                self.clear_input(Field::Drop, cx);
            }
            Section::Quests => {
                let Some(quest_id) = positive_id(&self.text(Field::Quest, cx)) else {
                    return self.reject(Field::Quest, cx);
                };
                let reward = self.text(Field::Reward, cx);
                let reward_item_id = match reward.as_str() {
                    "" => None,
                    reward => match positive_id(reward) {
                        Some(id) => Some(id),
                        None => return self.reject(Field::Reward, cx),
                    },
                };
                automation.update_quests(|state| {
                    state.register([Registered {
                        quest_id,
                        reward_item_id,
                    }])
                });
                self.clear_input(Field::Quest, cx);
                self.clear_input(Field::Reward, cx);
            }
            Section::Boosts => {
                let name = self.text(Field::Boost, cx);
                if name.is_empty() {
                    return;
                }
                automation.update_boosts(|state| {
                    register_items(&mut state.items, vec![ItemQuery::Name(name)])
                });
                self.clear_input(Field::Boost, cx);
            }
        }
        cx.notify();
    }

    fn fetch_boosts(&self, cx: &mut Context<Self>) {
        if let Some(shown) = &self.shown {
            let _ = shown
                .view
                .update(cx, |view, _| view.session.command(Command::FetchBoosts));
        }
    }

    fn confirmed(&mut self, cx: &mut Context<Self>) {
        let (Some(confirm), Some(shown)) = (self.confirm.take(), &self.shown) else {
            return;
        };
        match confirm {
            Confirm::Clear(section) => section.clear(&shown.automation),
            Confirm::ApplyToAll => {
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

    fn add_row(&self, fields: &[Field], cx: &mut Context<Self>) -> Div {
        let section = fields[0].section();
        div()
            .flex()
            .gap(px(6.0))
            .children(
                fields
                    .iter()
                    .map(|field| div().flex_1().min_w_0().child(self.input(*field).clone())),
            )
            .child(
                Button::new(("add", section as usize), "Add")
                    .on_click(cx.listener(move |this, _, _, cx| this.add(section, cx))),
            )
    }

    fn drops_section(&self, theme: &Theme, automation: &Automation, cx: &mut Context<Self>) -> Div {
        let state = automation.drops();
        let config = state.config;
        let rules: [Setting<drops::Config>; 4] = [
            ("drops-ac", "AC", |config| &mut config.accept_ac),
            ("drops-ac-member", "Member-only AC", |config| {
                &mut config.accept_ac_member_only
            }),
            ("drops-non-ac", "Non-AC", |config| &mut config.accept_non_ac),
            ("drops-non-ac-member", "Member-only non-AC", |config| {
                &mut config.accept_non_ac_member_only
            }),
        ];
        let checks: Vec<AnyElement> = rules
            .into_iter()
            .map(|(id, label, setting)| {
                let automation = automation.clone();
                check(id, label, *setting(&mut config.clone()), move |on| {
                    automation.update_drops(|state| *setting(&mut state.config) = on);
                })
            })
            .collect();
        let rejecting = automation.clone();
        let reject = check(
            "drops-reject",
            "Reject drops that aren't listed or picked up above",
            config.reject_others,
            move |on| {
                rejecting.update_drops(|state| state.config.reject_others = on);
            },
        );
        let rows = state
            .items
            .iter()
            .enumerate()
            .map(|(index, query)| {
                let rings = state.rings(query);
                let (sound, remove) = (automation.clone(), automation.clone());
                let (sounded, removed) = (query.clone(), query.clone());
                entry(theme, ("drop", index), query)
                    .child(
                        Checkbox::new(("drop-sound", index), "Beep", checked(rings)).on_toggle(
                            move |on, _, _| {
                                sound.update_drops(|state| state.set_sound(&sounded, on));
                            },
                        ),
                    )
                    .child(remove_button(("drop-remove", index), move || {
                        remove
                            .update_drops(|state| state.unregister(std::slice::from_ref(&removed)));
                    }))
                    .into_any_element()
            })
            .collect();
        let switch = automation.clone();
        section(
            Switch::new("drops-enabled", "Pick up and reject", config.enabled).on_toggle(
                move |on, _, _| {
                    switch.update_drops(|state| state.config.enabled = on);
                },
            ),
            Some(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(rules_row(theme, "Also pick up", checks))
                    .child(reject),
            ),
            self.add_row(&[Field::Drop], cx),
            list_box(theme, rows, "No drops listed."),
        )
    }

    fn quests_section(
        &self,
        theme: &Theme,
        automation: &Automation,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = automation.quests();
        let config = state.config;
        let rows = state
            .quests
            .iter()
            .enumerate()
            .map(|(index, registered)| {
                let quest_id = registered.quest_id;
                let remove = automation.clone();
                entry_text(
                    theme,
                    ("quest", index),
                    quest_id.to_string().into(),
                    registered
                        .reward_item_id
                        .map(|reward| format!("Reward {reward}").into()),
                )
                .child(remove_button(("quest-remove", index), move || {
                    remove.update_quests(|state| state.unregister(&[quest_id]));
                }))
                .into_any_element()
            })
            .collect();
        let rules: [Setting<quests::Config>; 2] = [
            ("quests-rewards", "Their rewards", |config| {
                &mut config.accept_rewards
            }),
            ("quests-requirements", "The items they need", |config| {
                &mut config.accept_requirements
            }),
        ];
        let checks = rules
            .into_iter()
            .map(|(id, label, setting)| {
                let automation = automation.clone();
                check(id, label, *setting(&mut config.clone()), move |on| {
                    automation.update_quests(|state| *setting(&mut state.config) = on);
                })
            })
            .collect();
        let switch = automation.clone();
        section(
            Switch::new("quests-enabled", "Accept and turn in", config.enabled).on_toggle(
                move |on, _, _| {
                    switch.update_quests(|state| state.config.enabled = on);
                },
            ),
            Some(rules_row(theme, "Pick up", checks)),
            self.add_row(&[Field::Quest, Field::Reward], cx),
            list_box(theme, rows, "No quests listed."),
        )
    }

    fn boosts_section(
        &self,
        theme: &Theme,
        automation: &Automation,
        cx: &mut Context<Self>,
    ) -> Div {
        let state = automation.boosts();
        let queries = state.items.clone();
        let count = queries.len();
        let row_theme = theme.clone();
        let row_automation = automation.clone();
        let list = Sortable::new(
            div().id("boosts").flex().flex_col().gap(px(6.0)),
            Flow::Column,
            (0..count).map(|index| SharedString::from(format!("boost-{index}"))),
            move |(index, pose), _, _| {
                let (remove, removed) = (row_automation.clone(), queries[index].clone());
                entry(&row_theme, ("boost", index), &queries[index])
                    .rounded(row_theme.radius.control)
                    .border_1()
                    .border_color(row_theme.hairline)
                    .bg(if pose == Pose::Lifted {
                        row_theme.overlay
                    } else {
                        row_theme.surface
                    })
                    .child(remove_button(("boost-remove", index), move || {
                        remove.update_boosts(|state| {
                            unregister_items(&mut state.items, std::slice::from_ref(&removed))
                        });
                    }))
                    .into_any_element()
            },
            {
                let automation = automation.clone();
                move |from, to, _, _| {
                    automation.update_boosts(|state| {
                        if from < state.items.len() && to < state.items.len() {
                            let item = state.items.remove(from);
                            state.items.insert(to, item);
                        }
                    });
                }
            },
        );
        let switch = automation.clone();
        section(
            Switch::new("boosts-enabled", "Keep active", state.config.enabled).on_toggle(
                move |on, _, _| {
                    switch.update_boosts(|state| state.config.enabled = on);
                },
            ),
            None,
            self.add_row(&[Field::Boost], cx).child(
                Button::new("fetch-boosts", "Fetch")
                    .tooltip("Add every boost in your inventory and bank")
                    .on_click(cx.listener(|this, _, _, cx| this.fetch_boosts(cx))),
            ),
            if count == 0 {
                empty_note(theme, "No boosts listed.").into_any_element()
            } else {
                list.into_any_element()
            },
        )
    }
}

fn positive_id(text: &str) -> Option<i32> {
    text.parse().ok().filter(|id| *id > 0)
}

fn item_query(text: &str) -> Option<ItemQuery> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(match text.parse::<i32>() {
        Ok(item_id) if item_id > 0 => ItemQuery::ItemId { item_id },
        _ => ItemQuery::Name(text.to_owned()),
    })
}

fn checked(on: bool) -> CheckedState {
    if on {
        CheckedState::Checked
    } else {
        CheckedState::Unchecked
    }
}

fn check(
    id: &'static str,
    label: &'static str,
    on: bool,
    toggle: impl Fn(bool) + 'static,
) -> AnyElement {
    Checkbox::new(id, label, checked(on))
        .on_toggle(move |on, _, _| toggle(on))
        .into_any_element()
}

fn entry(theme: &Theme, id: (&'static str, usize), query: &ItemQuery) -> Stateful<Div> {
    match query {
        ItemQuery::Name(name) => entry_text(theme, id, name.clone().into(), None),
        ItemQuery::ItemId { item_id } => {
            entry_text(theme, id, item_id.to_string().into(), Some("ID".into()))
        }
    }
}

fn entry_text(
    theme: &Theme,
    id: (&'static str, usize),
    title: SharedString,
    note: Option<SharedString>,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.0))
        .min_h(px(40.0))
        .px(px(12.0))
        .py(px(6.0))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_baseline()
                .gap(px(6.0))
                .child(div().truncate().child(title))
                .children(note.map(|note| {
                    div()
                        .flex_none()
                        .text_size(TEXT_SMALL)
                        .text_color(theme.text_tertiary)
                        .child(note)
                })),
        )
}

fn remove_button(id: (&'static str, usize), remove: impl Fn() + 'static) -> Button {
    Button::icon(id, Icon::X, "Remove")
        .size(Size::Small)
        .on_click(move |_, _, _| remove())
}

fn section(switch: Switch, rules: Option<Div>, adder: Div, list: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(div().flex().items_center().child(switch))
        .children(rules)
        .child(adder)
        .child(list)
}

fn rules_row(theme: &Theme, lead: &'static str, checks: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(16.0))
        .gap_y(px(6.0))
        .child(
            div()
                .text_size(TEXT_SMALL)
                .text_color(theme.text_tertiary)
                .child(lead),
        )
        .children(checks)
}

fn empty_note(theme: &Theme, text: &'static str) -> Div {
    div()
        .py(px(24.0))
        .px(px(16.0))
        .text_center()
        .text_size(TEXT_SMALL)
        .line_height(LINE_SMALL)
        .text_color(theme.text_tertiary)
        .child(text)
}

fn list_box(theme: &Theme, rows: Vec<AnyElement>, empty: &'static str) -> AnyElement {
    if rows.is_empty() {
        return empty_note(theme, empty).into_any_element();
    }
    div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(theme.radius.panel)
        .bg(theme.surface)
        .border_1()
        .border_color(theme.hairline)
        .children(rows.into_iter().enumerate().map(|(index, row)| {
            div()
                .when(index > 0, |row| {
                    row.border_t_1().border_color(theme.hairline)
                })
                .child(row)
        }))
        .into_any_element()
}

impl Render for Environment {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let tabs = self.client.read(cx).views().len();
        let followed = self
            .shown
            .as_ref()
            .map(|shown| (shown.id, shown.automation.clone()));
        let picker = shown_tab::picker(
            "environment-tab",
            &self.client,
            self.shown.as_ref(),
            |this: &mut Self, id, cx| this.show(Some(id), cx),
            cx,
        );
        let (body, labels, clearable) = match &followed {
            Some((_, automation)) => {
                let body = match self.section {
                    Section::Drops => self.drops_section(&theme, automation, cx),
                    Section::Quests => self.quests_section(&theme, automation, cx),
                    Section::Boosts => self.boosts_section(&theme, automation, cx),
                };
                let labels: Vec<SharedString> = Section::ALL
                    .into_iter()
                    .map(|section| match section.count(automation) {
                        0 => section.name().into(),
                        count => format!("{} · {count}", section.name()).into(),
                    })
                    .collect();
                (
                    body.into_any_element(),
                    labels,
                    self.section.count(automation) > 0,
                )
            }
            None => (
                empty_note(&theme, "Open a tab to set up its environment.").into_any_element(),
                Section::ALL.map(|section| section.name().into()).to_vec(),
                false,
            ),
        };
        let (confirm_title, confirm_body, confirm_action) = match self.confirm {
            Some(Confirm::ApplyToAll) => (
                format!(
                    "Copy this environment to {} other tabs?",
                    tabs.saturating_sub(1)
                ),
                "Their lists and rules are replaced with this tab's. Each tab keeps its own on and off switches.",
                "Copy",
            ),
            Some(Confirm::Clear(section)) => (
                format!("Clear every listed {}?", section.name().to_lowercase()),
                "The rules and switches stay as they are.",
                "Clear",
            ),
            None => (String::new(), "", ""),
        };
        let section = self.section;
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
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(BAR_HEIGHT)
                    .pr(px(16.0))
                    .child(drag_area().flex_none().w(TRAFFIC_LIGHTS_WIDTH))
                    .child(
                        drag_area()
                            .flex_1()
                            .flex()
                            .items_center()
                            .child(div().font_weight(FontWeight::MEDIUM).child("Environment")),
                    )
                    .child(picker),
            )
            .child(div().flex_none().px(px(20.0)).child(
                Tabs::new("environment-sections", "Sections", labels, section as usize).on_select(
                    {
                        let this = cx.weak_entity();
                        move |index, _, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.section = Section::ALL[index];
                                cx.notify();
                            });
                        }
                    },
                ),
            ))
            .child(
                div()
                    .id("environment-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(20.0))
                    .child(body),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .border_t_1()
                    .border_color(theme.hairline)
                    .child(
                        Button::new("apply-to-all", "Copy to all tabs…")
                            .size(Size::Small)
                            .disabled(followed.is_none() || tabs < 2)
                            .tooltip("Give every other tab this tab's lists and rules")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm = Some(Confirm::ApplyToAll);
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new(
                            "clear-section",
                            format!("Clear {}", section.name().to_lowercase()),
                        )
                        .size(Size::Small)
                        .variant(Variant::Ghost)
                        .disabled(!clearable)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.confirm = Some(Confirm::Clear(section));
                            cx.notify();
                        })),
                    ),
            )
            .child(
                Dialog::new("environment-confirm", self.confirm.is_some(), confirm_title)
                    .alert()
                    .description(confirm_body)
                    .action(
                        Button::new("confirm-cancel", "Cancel").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.confirm = None;
                                cx.notify();
                            },
                        )),
                    )
                    .action(
                        Button::new("confirm-accept", confirm_action)
                            .variant(if matches!(self.confirm, Some(Confirm::Clear(_))) {
                                Variant::Danger
                            } else {
                                Variant::Primary
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.confirmed(cx))),
                    )
                    .on_close({
                        let this = cx.weak_entity();
                        move |_, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.confirm = None;
                                cx.notify();
                            });
                        }
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_is_added_by_id_when_the_text_is_a_positive_number() {
        assert_eq!(
            item_query(" 1234 "),
            Some(ItemQuery::ItemId { item_id: 1234 })
        );
        assert_eq!(
            item_query("Spider Silk"),
            Some(ItemQuery::Name("Spider Silk".into()))
        );
        assert_eq!(item_query("0"), Some(ItemQuery::Name("0".into())));
        assert_eq!(item_query("  "), None);
    }
}
