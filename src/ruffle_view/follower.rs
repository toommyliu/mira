use gpui::{
    App, AppContext as _, Bounds, Context, Div, Entity, FocusHandle, Focusable as _, FontWeight,
    Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels, Render,
    Role, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, actions, div,
    prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, StyledExt as _, Theme, Tone,
    components::Chips,
    h_flex,
    primitives::{Align, Input, InputEvent, Select, Switch},
    theme::{DISABLED_OPACITY, ROW, TEXT_BODY},
    v_flex,
};

use super::{
    Client,
    client::CloseSession,
    shown_tab::{self, ShownTab, Target, watch_changes},
};
use crate::{
    automation::{
        follower::{self, Lost, Settings, Status},
        settings::Automation,
    },
    combat_profiles,
    domain::{player::Refusal, profiles},
};

actions!(mira, [OpenFollower]);

const WIDTH: f32 = 640.0;
const HEIGHT: f32 = 340.0;
const MIN_WIDTH: f32 = 340.0;
const MIN_HEIGHT: f32 = 240.0;
const COLUMNS_FROM: Pixels = px(560.0);
const LABEL_WIDTH: Pixels = px(96.0);
const MAX_NAME: usize = 32;
const MAX_LIST: usize = 256;

struct Open(WindowHandle<Follower>);

impl gpui::Global for Open {}

pub(crate) fn open(client: Entity<Client>, target: Target, cx: &mut App) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0)
        && handle
            .update(cx, |follower, window, cx| {
                window.activate_window();
                follower.retarget(target, cx);
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
            title: Some("Follower".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(12.0))),
        }),
        app_owns_titlebar_drag: true,
        is_minimizable: false,
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let follower = cx.new(|cx| Follower::new(client, target, window, cx));
        window.focus(&follower.read(cx).focus.clone(), cx);
        follower
    }) {
        Ok(handle) => cx.set_global(Open(handle)),
        Err(error) => tracing::error!(%error, "couldn't open the follower"),
    }
}

#[derive(Clone, Copy)]
enum Entries {
    Maps,
    Monsters,
}

enum Edit<'a> {
    Add(&'a str),
    Remove(usize),
    Move(usize, usize),
}

impl Entries {
    fn label(self) -> &'static str {
        match self {
            Entries::Maps => "Search maps",
            Entries::Monsters => "Attack priority",
        }
    }

    fn note(self) -> &'static str {
        match self {
            Entries::Maps => {
                "Joined in order when goto is refused. Add a room number to land in theirs."
            }
            Entries::Monsters => "Attacked first whenever one is up, left to right.",
        }
    }

    fn items(self, settings: &Settings) -> Vec<SharedString> {
        match self {
            Entries::Maps => settings
                .fallbacks
                .iter()
                .map(|map| map.clone().into())
                .collect(),
            Entries::Monsters => settings
                .attack_first
                .iter()
                .map(|query| follower::monster_label(query).into())
                .collect(),
        }
    }

    fn apply(self, settings: &mut Settings, edit: Edit) {
        match self {
            Entries::Maps => edit_list(&mut settings.fallbacks, edit, follower::add_maps),
            Entries::Monsters => {
                edit_list(&mut settings.attack_first, edit, follower::add_monsters)
            }
        }
    }
}

fn edit_list<T>(items: &mut Vec<T>, edit: Edit, add: fn(&mut Vec<T>, &str)) {
    match edit {
        Edit::Add(text) => add(items, text),
        Edit::Remove(index) if index < items.len() => {
            items.remove(index);
        }
        Edit::Move(from, to) if from < items.len() && to < items.len() => {
            let item = items.remove(from);
            items.insert(to, item);
        }
        Edit::Remove(_) | Edit::Move(..) => {}
    }
}

pub(crate) struct Follower {
    client: Entity<Client>,
    focus: FocusHandle,
    tab: Target,
    shown: Option<ShownTab>,
    target: Entity<Input>,
    add_map: Entity<Input>,
    add_monster: Entity<Input>,
    _subscriptions: Vec<Subscription>,
}

impl Follower {
    fn new(
        client: Entity<Client>,
        tab: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let target = cx.new(|cx| {
            Input::new("Player", cx)
                .placeholder("Player name")
                .max_length(MAX_NAME)
        });
        let add = |label: &'static str, placeholder: &'static str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                Input::new(label, cx)
                    .placeholder(placeholder)
                    .max_length(MAX_LIST)
            })
        };
        let add_map = add("Add a map to search", "ultradage-9999", cx);
        let add_monster = add("Add a priority monster", "Staff of Inversion, id:3", cx);
        let mut subscriptions = vec![
            cx.subscribe(&target, |this, _, event, cx| match event {
                InputEvent::Changed => cx.notify(),
                InputEvent::Submitted => this.commit_target(cx),
            }),
            cx.on_blur(&target.focus_handle(cx), window, |this, _, cx| {
                this.commit_target(cx)
            }),
            cx.observe(&client, |this, _, cx| this.show(cx)),
        ];
        for (input, entries) in [(&add_map, Entries::Maps), (&add_monster, Entries::Monsters)] {
            subscriptions.push(cx.subscribe(input, move |this, _, event, cx| {
                if let InputEvent::Submitted = event {
                    this.add(entries, cx);
                }
            }));
            subscriptions.push(
                cx.on_blur(&input.focus_handle(cx), window, move |this, _, cx| {
                    this.add(entries, cx)
                }),
            );
        }
        let mut follower = Self {
            client,
            focus: cx.focus_handle(),
            tab,
            shown: None,
            target,
            add_map,
            add_monster,
            _subscriptions: subscriptions,
        };
        follower.show(cx);
        follower
    }

    fn retarget(&mut self, tab: Target, cx: &mut Context<Self>) {
        self.tab = tab;
        self.show(cx);
    }

    fn show(&mut self, cx: &mut Context<Self>) {
        let switched = shown_tab::show(
            &mut self.shown,
            &mut self.tab,
            &self.client,
            |this: &mut Self, cx| this.show(cx),
            |automation, cx| {
                vec![
                    watch_changes(automation.watch_follower(), cx),
                    watch_changes(automation.watch_follower_status(), cx),
                ]
            },
            cx,
        );
        if switched {
            self.fill(cx);
        }
    }

    fn fill(&mut self, cx: &mut Context<Self>) {
        let target = self
            .automation()
            .map(|automation| automation.follower().target)
            .unwrap_or_default();
        self.target
            .update(cx, |input, cx| input.set_text(target, cx));
        for input in [&self.add_map, &self.add_monster] {
            input.update(cx, |input, cx| input.set_text("", cx));
        }
    }

    fn automation(&self) -> Option<&Automation> {
        self.shown.as_ref().map(|shown| &shown.automation)
    }

    fn input(&self, entries: Entries) -> &Entity<Input> {
        match entries {
            Entries::Maps => &self.add_map,
            Entries::Monsters => &self.add_monster,
        }
    }

    fn typed_target(&self, cx: &App) -> String {
        self.target.read(cx).text().trim().to_owned()
    }

    fn commit_target(&mut self, cx: &mut Context<Self>) {
        let Some(automation) = self.automation().cloned() else {
            return;
        };
        let name = self.typed_target(cx);
        if name.is_empty() {
            self.revert_target(cx);
        } else {
            automation.update_follower(|settings| settings.target = name);
        }
    }

    fn revert_target(&mut self, cx: &mut Context<Self>) {
        let saved = self
            .automation()
            .map(|automation| automation.follower().target)
            .unwrap_or_default();
        self.target.update(cx, |input, cx| {
            if input.text() != saved {
                input.set_text(saved, cx);
            }
        });
    }

    fn toggle(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(automation) = self.automation().cloned() else {
            return;
        };
        if !on {
            automation.update_follower(|settings| settings.enabled = false);
            return;
        }
        let name = self.typed_target(cx);
        if name.is_empty() {
            return;
        }
        automation.update_follower(|settings| {
            settings.target = name;
            settings.enabled = true;
        });
    }

    fn add(&mut self, entries: Entries, cx: &mut Context<Self>) {
        let input = self.input(entries).clone();
        let text = input.read(cx).text().to_owned();
        if text.trim().is_empty() {
            return;
        }
        if let Some(automation) = self.automation() {
            automation.update_follower(|settings| entries.apply(settings, Edit::Add(&text)));
        }
        input.update(cx, |input, cx| input.set_text("", cx));
    }

    fn body(
        &self,
        theme: &Theme,
        automation: &Automation,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let settings = automation.follower();
        let this = cx.weak_entity();
        let unnamed = !settings.enabled && self.typed_target(cx).is_empty();
        let follow = Switch::new("follow", "Follow", settings.enabled)
            .disabled(unnamed)
            .on_toggle(move |on, _, cx| {
                let _ = this.update(cx, |this, cx| this.toggle(on, cx));
            });
        let walking = automation.clone();
        let copy_walk = Switch::new("follower-copy-walk", "Copy walk", settings.copy_walk)
            .on_toggle(move |on, _, _| {
                walking.update_follower(|settings| settings.copy_walk = on);
            });
        let fighting = automation.clone();
        let fight =
            Switch::new("follower-fight", "Fight", settings.fight).on_toggle(move |on, _, _| {
                fighting.update_follower(|settings| settings.fight = on);
            });
        self.add_monster
            .update(cx, |input, cx| input.set_disabled(!settings.fight, cx));
        let status = if unnamed {
            Some(("Needs a player name".to_owned(), theme.text_tertiary))
        } else {
            live_status(&settings, automation.follower_status()).map(|(text, tone)| {
                let (_, _, color) = theme.tone(tone);
                (text, color)
            })
        };
        let player = div().child(self.target.clone()).on_key_down(cx.listener(
            move |this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.revert_target(cx);
                }
            },
        ));
        let following = v_flex()
            .gap(px(8.0))
            .child(section(
                "Follow",
                "Goes wherever they go, across maps and cells.",
                status,
                follow,
                theme,
            ))
            .child(row("Player", true, player))
            .child(
                h_flex()
                    .justify_between()
                    .gap(px(12.0))
                    .min_h(ROW)
                    .child(row_label("Copy walk", true))
                    .child(copy_walk.hide_label()),
            )
            .child(self.entries(Entries::Maps, &settings, automation, true, theme));
        let fighting = v_flex()
            .gap(px(8.0))
            .child(section(
                "Fight",
                "Attacks monsters in their cell with this profile.",
                None,
                fight,
                theme,
            ))
            .child(row(
                "Profile",
                settings.fight,
                profile_select(&settings, automation, cx),
            ))
            .child(self.entries(
                Entries::Monsters,
                &settings,
                automation,
                settings.fight,
                theme,
            ));
        if window.viewport_size().width >= COLUMNS_FROM {
            h_flex()
                .items_start()
                .min_h_full()
                .child(div().flex_1().min_w_0().p(px(16.0)).child(following))
                .child(div().self_stretch().w(px(1.0)).bg(theme.hairline))
                .child(div().flex_1().min_w_0().p(px(16.0)).child(fighting))
        } else {
            v_flex()
                .gap(px(14.0))
                .p(px(16.0))
                .child(following)
                .child(div().h(px(1.0)).bg(theme.hairline))
                .child(fighting)
        }
    }

    fn entries(
        &self,
        entries: Entries,
        settings: &Settings,
        automation: &Automation,
        enabled: bool,
        theme: &Theme,
    ) -> Div {
        let input = self.input(entries).clone();
        let typed = div()
            .child(input.clone())
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    input.update(cx, |input, cx| input.set_text("", cx));
                }
            });
        let field = v_flex()
            .gap(px(10.0))
            .child(row(entries.label(), enabled, typed))
            .child(note(entries.note(), enabled, theme).mt(px(-4.0)));
        let items = entries.items(settings);
        if items.is_empty() {
            return field;
        }
        let (removing, moving) = (automation.clone(), automation.clone());
        let emptied = self.input(entries).clone();
        field.child(
            Chips::new(entries.label(), entries.label(), items)
                .disabled(!enabled)
                .on_remove(move |index, window, cx| {
                    let left = removing
                        .update_follower(|settings| entries.apply(settings, Edit::Remove(index)));
                    if entries.items(&left).is_empty() {
                        window.focus(&emptied.focus_handle(cx), cx);
                    }
                })
                .on_reorder(move |from, to, _, _| {
                    moving
                        .update_follower(|settings| entries.apply(settings, Edit::Move(from, to)));
                }),
        )
    }
}

fn profile_select(settings: &Settings, automation: &Automation, cx: &App) -> Select {
    let library = combat_profiles::library(cx);
    let names: Vec<Option<String>> = std::iter::once(None)
        .chain(library.names().map(|name| Some(name.to_owned())))
        .collect();
    let picked = names
        .iter()
        .position(|name| match (name, &settings.profile) {
            (None, None) => true,
            (Some(name), Some(chosen)) => profiles::same_name(name, chosen),
            _ => false,
        });
    let choosing = automation.clone();
    Select::new(
        "follower-profile",
        "Profile",
        names.iter().map(|name| {
            SharedString::from(name.as_deref().unwrap_or(profiles::GENERIC).to_owned())
        }),
        picked,
    )
    .full_width()
    .align(Align::End)
    .disabled(!settings.fight)
    .on_select(move |index, _, _| {
        let profile = names[index].clone();
        choosing.update_follower(|settings| settings.profile = profile);
    })
}

fn note(text: &'static str, enabled: bool, theme: &Theme) -> Div {
    div()
        .text_small()
        .text_color(theme.text_tertiary)
        .when(!enabled, |note| note.opacity(DISABLED_OPACITY))
        .child(text)
}

fn section(
    title: &'static str,
    subtitle: &'static str,
    status: Option<(String, Hsla)>,
    switch: Switch,
    theme: &Theme,
) -> Div {
    let header = h_flex()
        .gap(px(12.0))
        .min_h(ROW)
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .justify_end()
                .gap(px(10.0))
                .when_some(status, |row, (text, color)| {
                    row.child(
                        div()
                            .id("follow-status")
                            .role(Role::Status)
                            .aria_label(text.clone())
                            .min_w_0()
                            .truncate()
                            .text_small()
                            .text_color(color)
                            .child(text),
                    )
                })
                .child(switch.hide_label()),
        );
    v_flex()
        .gap(px(4.0))
        .child(header)
        .child(note(subtitle, true, theme))
}

fn row_label(label: &'static str, enabled: bool) -> Div {
    div()
        .min_w_0()
        .truncate()
        .when(!enabled, |label| label.opacity(DISABLED_OPACITY))
        .child(label)
}

fn row(label: &'static str, enabled: bool, control: impl IntoElement) -> Div {
    h_flex()
        .gap(px(12.0))
        .min_h(ROW)
        .child(row_label(label, enabled).flex_none().w(LABEL_WIDTH))
        .child(div().flex_1().min_w_0().child(control))
}

fn live_status(settings: &Settings, status: Status) -> Option<(String, Tone)> {
    if !settings.enabled {
        return None;
    }
    let target = settings.target.trim();
    let lost = match status {
        Status::Off => return None,
        Status::Waiting => return Some(("Not in game".into(), Tone::Neutral)),
        Status::Travelling => return Some((format!("Going to {target}"), Tone::Neutral)),
        Status::Following => return Some((format!("Following {target}"), Tone::Accent)),
        Status::Lost(lost) => lost,
    };
    let reason = match lost {
        Lost::Yourself => return Some((format!("Signed in as {target}"), Tone::Danger)),
        Lost::Refused(Refusal::LockedZone) => "Locked zone",
        Lost::Refused(Refusal::IgnoringGoto) => "Goto blocked",
        Lost::Refused(Refusal::RoomFull) => "Room full",
        Lost::Missed => "Not found",
        Lost::Blocked => "Cell unreachable",
    };
    let text = match settings.fallbacks.len() {
        _ if !matches!(lost, Lost::Refused(_)) => reason.to_owned(),
        0 => reason.to_owned(),
        1 => format!("{reason}, searched 1 map"),
        searched => format!("{reason}, searched {searched} maps"),
    };
    Some((text, Tone::Warning))
}

impl Render for Follower {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let header = shown_tab::header(
            "Follower",
            &self.client,
            self.shown.as_ref(),
            self.tab,
            Self::retarget,
            window,
            cx,
        );
        let body = match self.automation().cloned() {
            Some(automation) => self.body(&theme, &automation, window, cx),
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
            .id("follower")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseSession, window, _| window.remove_window()))
            .child(mira_ui::nav::pointer_probe())
            .child(header)
            .child(
                div()
                    .id("follower-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(body),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn open(cx: &mut TestAppContext) -> (Entity<Follower>, Automation, &mut VisualTestContext) {
        let folder = std::env::temp_dir().join(format!("mira-follower-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        cx.update(|cx| {
            mira_ui::init(cx);
            combat_profiles::init(folder.join("profiles.json"), cx);
        });
        let automation = Automation::default();
        automation.update_follower(|settings| settings.target = "Lead".into());
        let shown = automation.clone();
        let (follower, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            let client = Client::empty_for_tests(&folder, cx);
            let mut follower = Follower::new(client, Target::Selected, window, cx);
            follower.shown = Some(ShownTab::fake("Alt 1", shown));
            follower.fill(cx);
            follower
        });
        cx.run_until_parked();
        (follower, automation, cx)
    }

    fn press(cx: &mut VisualTestContext, keys: &str) {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
    }

    fn typed(cx: &mut VisualTestContext, text: &str) {
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    #[gpui::test]
    fn the_player_name_applies_on_return_or_on_leaving_the_field(cx: &mut TestAppContext) {
        let (follower, automation, cx) = open(cx);
        let (target, focus) = follower.read_with(cx, |follower, _| {
            (follower.target.clone(), follower.focus.clone())
        });
        cx.update(|window, cx| window.focus(&target.focus_handle(cx), cx));
        typed(cx, "er");
        assert_eq!(automation.follower().target, "Lead");
        press(cx, "enter");
        assert_eq!(automation.follower().target, "Leader");
        typed(cx, "s");
        press(cx, "escape");
        assert_eq!(
            target.read_with(cx, |input, _| input.text().to_owned()),
            "Leader"
        );
        typed(cx, "2");
        cx.update(|window, cx| window.focus(&focus, cx));
        cx.run_until_parked();
        assert_eq!(automation.follower().target, "Leader2");
    }
}
