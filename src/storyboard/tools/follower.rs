use std::collections::HashMap;

use gpui::{
    Context, Entity, Focusable as _, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use mira_ui::{
    components::Chips,
    h_flex,
    primitives::{
        Field, Input, InputEvent, Select, Switch,
        button::{Size, Variant},
    },
    v_flex,
};

use super::super::{Screen, view::*};
use crate::{
    automation::follower::{self, Settings},
    domain::profiles::GENERIC,
};

#[derive(Clone, Copy)]
enum Entries {
    Maps,
    Monsters,
}

impl Entries {
    fn label(self) -> &'static str {
        match self {
            Self::Maps => "Search maps",
            Self::Monsters => "Attack priority",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Self::Maps => "Searched in order if goto is refused. Include a room number when known.",
            Self::Monsters => "Attack these monsters first, in order, when they're available.",
        }
    }

    fn items(self, settings: &Settings) -> Vec<SharedString> {
        match self {
            Self::Maps => settings
                .fallbacks
                .iter()
                .cloned()
                .map(SharedString::from)
                .collect(),
            Self::Monsters => settings
                .attack_first
                .iter()
                .map(follower::monster_label)
                .map(SharedString::from)
                .collect(),
        }
    }

    fn remove(self, settings: &mut Settings, index: usize) {
        match self {
            Self::Maps if index < settings.fallbacks.len() => {
                settings.fallbacks.remove(index);
            }
            Self::Monsters if index < settings.attack_first.len() => {
                settings.attack_first.remove(index);
            }
            _ => {}
        }
    }

    fn reorder(self, settings: &mut Settings, from: usize, to: usize) {
        fn move_entry<T>(entries: &mut Vec<T>, from: usize, to: usize) {
            if from < entries.len() && to < entries.len() {
                let entry = entries.remove(from);
                entries.insert(to, entry);
            }
        }
        match self {
            Self::Maps => move_entry(&mut settings.fallbacks, from, to),
            Self::Monsters => move_entry(&mut settings.attack_first, from, to),
        }
    }
}

pub(in crate::storyboard) struct Follower {
    target: Entity<Target>,
    account: Option<SharedString>,
    settings: HashMap<SharedString, Settings>,
    player: Entity<Input>,
    maps: Entity<Input>,
    monsters: Entity<Input>,
    blur: Option<[Subscription; 3]>,
}

impl Follower {
    pub(in crate::storyboard) fn new(sessions: Entity<Sessions>, cx: &mut Context<Self>) -> Self {
        let target = Target::new(sessions, cx);
        let player = field("Player", "Player name", "", false, cx);
        let maps = field("Add a search map", "abbey-4071", "", false, cx);
        let monsters = field(
            "Add a priority monster",
            "Monster name or id:3",
            "",
            false,
            cx,
        );
        let mut initial = Settings {
            enabled: true,
            target: "Vesper".into(),
            copy_walk: true,
            fight: true,
            profile: Some("Relic farm".into()),
            ..Settings::default()
        };
        follower::add_maps(&mut initial.fallbacks, "abbey-4071, westgate-4071");
        follower::add_monsters(&mut initial.attack_first, "Abbey sentinel, id:3");
        let mut view = Self {
            target,
            account: None,
            settings: HashMap::from([("Aster".into(), initial)]),
            player,
            maps,
            monsters,
            blur: None,
        };
        cx.observe(&view.target, |view, _, cx| view.retarget(cx))
            .detach();
        cx.subscribe(&view.player, |view, _, event, cx| {
            if matches!(event, InputEvent::Submitted) {
                view.commit_player(cx);
            }
            cx.notify();
        })
        .detach();
        for (input, entries) in [
            (&view.maps, Entries::Maps),
            (&view.monsters, Entries::Monsters),
        ] {
            cx.subscribe(input, move |view, _, event, cx| {
                if matches!(event, InputEvent::Submitted) {
                    view.add(entries, cx);
                }
                cx.notify();
            })
            .detach();
        }
        view.retarget(cx);
        view
    }

    fn retarget(&mut self, cx: &mut Context<Self>) {
        let current = self.target.read(cx).resolved(cx).cloned();
        if current == self.account {
            return;
        }
        self.account = current;
        let (name, fight) = self
            .current_mut()
            .map(|settings| (settings.target.clone(), settings.fight))
            .unwrap_or_default();
        self.player.update(cx, |input, cx| input.set_text(name, cx));
        self.maps.update(cx, |input, cx| input.set_text("", cx));
        self.monsters.update(cx, |input, cx| {
            input.set_text("", cx);
            input.set_disabled(!fight, cx);
        });
        cx.notify();
    }

    fn current_mut(&mut self) -> Option<&mut Settings> {
        Some(self.settings.entry(self.account.clone()?).or_default())
    }

    fn commit_player(&mut self, cx: &mut Context<Self>) {
        let name = self.player.read(cx).text().trim().to_owned();
        if let Some(settings) = self.current_mut() {
            if name.is_empty() {
                let saved = settings.target.clone();
                self.player
                    .update(cx, |input, cx| input.set_text(saved, cx));
            } else {
                settings.target = name;
            }
        }
    }

    fn add(&mut self, kind: Entries, cx: &mut Context<Self>) {
        let input = match kind {
            Entries::Maps => self.maps.clone(),
            Entries::Monsters => self.monsters.clone(),
        };
        let value = input.read(cx).text().trim().to_owned();
        if value.is_empty() {
            return;
        }
        if let Some(settings) = self.current_mut() {
            match kind {
                Entries::Maps => follower::add_maps(&mut settings.fallbacks, &value),
                Entries::Monsters if settings.fight => {
                    follower::add_monsters(&mut settings.attack_first, &value)
                }
                Entries::Monsters => return,
            }
            input.update(cx, |input, cx| input.set_text("", cx));
        }
    }

    fn entries(&self, kind: Entries, settings: &Settings, cx: &Context<Self>) -> impl IntoElement {
        let label = kind.label();
        let items = kind.items(settings);
        let populated = !items.is_empty();
        let owner = cx.weak_entity();
        let owner_for_remove = owner.clone();
        let input = match kind {
            Entries::Maps => self.maps.clone(),
            Entries::Monsters => self.monsters.clone(),
        };
        let clear_on_escape = input.clone();
        let restore_focus = input.clone();
        let enabled = matches!(kind, Entries::Maps) || settings.fight;
        let can_add = enabled && !input.read(cx).text().trim().is_empty();
        v_flex()
            .gap(px(6.0))
            .child(Field::new(
                label,
                h_flex()
                    .gap(px(6.0))
                    .child(div().flex_1().min_w_0().child(input).on_key_down(
                        move |event: &KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" {
                                clear_on_escape.update(cx, |input, cx| input.set_text("", cx));
                                cx.stop_propagation();
                            }
                        },
                    ))
                    .child(
                        command(
                            ("add-entry", kind as usize),
                            "Add",
                            Variant::Ghost,
                            change(cx, move |view, cx| view.add(kind, cx)),
                        )
                        .size(Size::Small)
                        .disabled(!can_add),
                    ),
            ))
            .child(meta(kind.note(), cx))
            .when(populated, |body| {
                body.child(
                    Chips::new(label, label, items)
                        .disabled(!enabled)
                        .on_remove(move |index, window, cx| {
                            let removed_last = owner_for_remove
                                .update(cx, |view, cx| {
                                    let Some(settings) = view.current_mut() else {
                                        return false;
                                    };
                                    kind.remove(settings, index);
                                    let empty = kind.items(settings).is_empty();
                                    cx.notify();
                                    empty
                                })
                                .unwrap_or(false);
                            if removed_last {
                                window.focus(&restore_focus.focus_handle(cx), cx);
                            }
                        })
                        .on_reorder(move |from, to, _, cx| {
                            owner
                                .update(cx, |view, cx| {
                                    if let Some(settings) = view.current_mut() {
                                        kind.reorder(settings, from, to);
                                    }
                                    cx.notify();
                                })
                                .ok();
                        }),
                )
            })
    }
}

impl Render for Follower {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.blur.is_none() {
            self.blur = Some([
                cx.on_blur(&self.player.focus_handle(cx), window, |view, _, cx| {
                    view.commit_player(cx);
                    cx.notify();
                }),
                cx.on_blur(&self.maps.focus_handle(cx), window, |view, _, cx| {
                    view.add(Entries::Maps, cx)
                }),
                cx.on_blur(&self.monsters.focus_handle(cx), window, |view, _, cx| {
                    view.add(Entries::Monsters, cx)
                }),
            ]);
        }
        let frame = frame(
            tool_chrome(Screen::Follower, Some(self.target.clone()), cx),
            cx,
        );
        let Some(settings) = self
            .account
            .as_ref()
            .and_then(|name| self.settings.get(name))
        else {
            return frame.child(empty(
                "No tabs open",
                "Open a game session to follow a player.",
                cx,
            ));
        };
        let wide = window_width(Screen::Follower, window) >= px(560.0);
        let preview = StatusPreview::current(cx);
        let player = self.player.read(cx).text().trim();
        let options = [GENERIC, "Relic farm", "Boss opener", "Guild support"];
        let selected = options
            .iter()
            .position(|&option| option == settings.profile.as_deref().unwrap_or(GENERIC));
        let follow = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(8.0))
            .child(
                h_flex()
                    .min_h(px(24.0))
                    .gap(px(8.0))
                    .child(title("Follow"))
                    .child(spring())
                    .when(
                        settings.enabled && preview == StatusPreview::Current,
                        |row| {
                            row.child(
                                status(format!("Following {}", settings.target), true, cx)
                                    .truncate(),
                            )
                        },
                    )
                    .child(
                        Switch::new("follow", "Follow player", settings.enabled)
                            .hide_label()
                            .disabled(!settings.enabled && player.is_empty())
                            .on_toggle(choose(cx, |view, on, cx| {
                                if on {
                                    view.commit_player(cx);
                                }
                                if let Some(settings) = view.current_mut() {
                                    settings.enabled = on;
                                }
                            })),
                    ),
            )
            .children(
                settings
                    .enabled
                    .then(|| {
                        status_preview(
                            format!("Going to {}…", settings.target),
                            "Room full",
                            format!(
                                "Couldn't reach {} after searching {} maps.",
                                settings.target,
                                settings.fallbacks.len()
                            ),
                            cx,
                        )
                    })
                    .flatten(),
            )
            .child(Field::new(
                "Player",
                div().child(self.player.clone()).on_key_down(cx.listener(
                    |view, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape" {
                            let name = view
                                .current_mut()
                                .map(|settings| settings.target.clone())
                                .unwrap_or_default();
                            view.player.update(cx, |input, cx| input.set_text(name, cx));
                            cx.stop_propagation();
                        }
                    },
                )),
            ))
            .child(
                Switch::new("copy-walk", "Copy walk", settings.copy_walk).on_toggle(choose(
                    cx,
                    |view, on, _| {
                        if let Some(settings) = view.current_mut() {
                            settings.copy_walk = on;
                        }
                    },
                )),
            )
            .child(self.entries(Entries::Maps, settings, cx));
        let fight = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(8.0))
            .child(
                h_flex()
                    .min_h(px(24.0))
                    .gap(px(8.0))
                    .child(title("Fight"))
                    .child(spring())
                    .child(
                        Switch::new("fight", "Fight", settings.fight)
                            .hide_label()
                            .on_toggle(choose(cx, |view, on, cx| {
                                if let Some(settings) = view.current_mut() {
                                    settings.fight = on;
                                }
                                view.monsters
                                    .update(cx, |input, cx| input.set_disabled(!on, cx));
                            })),
                    ),
            )
            .child(Field::new(
                "Profile",
                Select::new("profile", "Profile", options, selected)
                    .disabled(!settings.fight)
                    .full_width()
                    .on_select(choose(cx, move |view, selected: usize, _| {
                        if let Some(settings) = view.current_mut() {
                            settings.profile =
                                (selected != 0).then(|| options[selected].to_owned());
                        }
                    })),
            ))
            .child(self.entries(Entries::Monsters, settings, cx));
        frame.child(scroll("follower").p(px(16.0)).child(if wide {
            h_flex()
                .items_start()
                .gap(px(24.0))
                .child(follow)
                .child(fight)
        } else {
            v_flex()
                .gap(px(16.0))
                .child(follow)
                .child(divider(cx))
                .child(fight)
        }))
    }
}
