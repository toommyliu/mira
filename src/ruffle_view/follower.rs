use gpui::{
    App, AppContext as _, Bounds, Context, Div, Entity, EntityId, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions, actions, div, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, Theme,
    primitives::{Field, Input, InputEvent, Select, Switch},
    theme::{LINE_SMALL, TEXT_BODY, TEXT_SMALL},
};

use super::{
    BAR_HEIGHT, Client,
    client::{CloseSession, TRAFFIC_LIGHTS_WIDTH, drag_area},
    shown_tab::{self, ShownTab, watch_changes},
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

const WIDTH: f32 = 420.0;
const HEIGHT: f32 = 620.0;
const MAX_NAME: usize = 32;
const MAX_LIST: usize = 256;

struct Open(WindowHandle<Follower>);

impl gpui::Global for Open {}

pub(crate) fn open(client: Entity<Client>, cx: &mut App) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0)
        && handle
            .update(cx, |follower, window, cx| {
                window.activate_window();
                follower.show_active(cx);
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
            title: Some("Follower".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(12.0))),
        }),
        app_owns_titlebar_drag: true,
        is_minimizable: false,
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let follower = cx.new(|cx| Follower::new(client, window, cx));
        window.focus(&follower.read(cx).focus.clone(), cx);
        follower
    }) {
        Ok(handle) => cx.set_global(Open(handle)),
        Err(error) => tracing::error!(%error, "couldn't open the follower"),
    }
}

pub(crate) struct Follower {
    client: Entity<Client>,
    focus: FocusHandle,
    shown: Option<ShownTab>,
    target: Entity<Input>,
    fallbacks: Entity<Input>,
    attack_first: Entity<Input>,
    _subscriptions: [Subscription; 4],
}

impl Follower {
    fn new(client: Entity<Client>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let target = cx.new(|cx| {
            Input::new("Player", cx)
                .placeholder("Player name")
                .max_length(MAX_NAME)
        });
        let list = |label: &'static str, placeholder: &'static str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                Input::new(label, cx)
                    .placeholder(placeholder)
                    .max_length(MAX_LIST)
            })
        };
        let fallbacks = list("Fallback maps", "ultradage-9999, tercessuinotlim-9999", cx);
        let attack_first = list("Priority", "Staff of Inversion, id:3", cx);
        let subscriptions = [
            cx.subscribe_in(&target, window, |this, input, event, _, cx| match event {
                InputEvent::Changed => {
                    input.update(cx, |input, cx| input.set_invalid(false, cx));
                }
                InputEvent::Submitted => this.change_target(cx),
            }),
            cx.subscribe(&fallbacks, |this, input, event, cx| {
                if let InputEvent::Changed = event {
                    let maps = follower::parse_maps(input.read(cx).text());
                    this.change(|settings| settings.fallbacks = maps);
                }
            }),
            cx.subscribe(&attack_first, |this, input, event, cx| {
                if let InputEvent::Changed = event {
                    let monsters = follower::parse_monsters(input.read(cx).text());
                    this.change(|settings| settings.attack_first = monsters);
                }
            }),
            cx.observe(&client, |this, _, cx| this.show(None, cx)),
        ];
        let mut follower = Self {
            client,
            focus: cx.focus_handle(),
            shown: None,
            target,
            fallbacks,
            attack_first,
            _subscriptions: subscriptions,
        };
        follower.show_active(cx);
        follower
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
                    watch_changes(automation.watch_follower(), cx),
                    watch_changes(automation.watch_follower_status(), cx),
                ]
            },
            cx,
        );
        if switched {
            let settings = self
                .automation()
                .map(Automation::follower)
                .unwrap_or_default();
            let texts = [
                (&self.target, settings.target),
                (&self.fallbacks, settings.fallbacks.join(", ")),
                (
                    &self.attack_first,
                    follower::format_monsters(&settings.attack_first),
                ),
            ];
            for (input, text) in texts {
                input.update(cx, |input, cx| input.set_text(text, cx));
            }
        }
    }

    fn change(&self, change: impl FnOnce(&mut Settings)) {
        if let Some(automation) = self.automation() {
            automation.update_follower(change);
        }
    }

    fn automation(&self) -> Option<&Automation> {
        self.shown.as_ref().map(|shown| &shown.automation)
    }

    fn typed_target(&self, cx: &mut Context<Self>) -> Option<String> {
        let name = self.target.read(cx).text().trim().to_owned();
        if name.is_empty() {
            self.target
                .update(cx, |input, cx| input.set_invalid(true, cx));
            return None;
        }
        Some(name)
    }

    fn change_target(&mut self, cx: &mut Context<Self>) {
        if let (Some(automation), Some(name)) = (self.automation().cloned(), self.typed_target(cx))
        {
            automation.update_follower(|settings| settings.target = name);
        }
    }

    fn toggle(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(automation) = self.automation().cloned() else {
            return;
        };
        if !on {
            automation.update_follower(|settings| settings.enabled = false);
            return;
        }
        if let Some(name) = self.typed_target(cx) {
            automation.update_follower(|settings| {
                settings.target = name;
                settings.enabled = true;
            });
        }
    }

    fn body(&self, theme: &Theme, automation: &Automation, cx: &mut Context<Self>) -> Div {
        let settings = automation.follower();
        let status = status_text(&settings, automation.follower_status());
        let this = cx.weak_entity();
        let follow =
            Switch::new("follow", "Follow", settings.enabled).on_toggle(move |on, _, cx| {
                let _ = this.update(cx, |this, cx| this.toggle(on, cx));
            });
        let follow = match status {
            Some(status) => follow.description(status),
            None => follow,
        };
        let walking = automation.clone();
        let copy_walk = Switch::new("follower-copy-walk", "Copy walk", settings.copy_walk)
            .description("Walks where they walk in their cell.")
            .on_toggle(move |on, _, _| {
                walking.update_follower(|settings| settings.copy_walk = on);
            });
        let fighting = automation.clone();
        let fight = Switch::new("follower-fight", "Fight", settings.fight)
            .description("Attacks monsters in their cell with the profile below.")
            .on_toggle(move |on, _, _| {
                fighting.update_follower(|settings| settings.fight = on);
            });
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
        let profile = Select::new(
            "follower-profile",
            "Profile",
            names.iter().map(|name| {
                SharedString::from(name.as_deref().unwrap_or(profiles::GENERIC).to_owned())
            }),
            picked,
        )
        .on_select(move |index, _, _| {
            let profile = names[index].clone();
            choosing.update_follower(|settings| settings.profile = profile);
        });
        div()
            .flex()
            .flex_col()
            .gap(px(20.0))
            .child(
                Field::new("Player", self.target.clone())
                    .description("Press Return to switch players while following."),
            )
            .child(follow)
            .child(group(theme).child(copy_walk).child(
                Field::new("Fallback maps", self.fallbacks.clone()).description(
                    "Joined in order when goto is refused. Add a room number to land in theirs.",
                ),
            ))
            .child(
                group(theme)
                    .child(fight)
                    .child(Field::new("Profile", profile))
                    .child(
                        Field::new("Priority", self.attack_first.clone()).description(
                            "Names or id:3, comma separated. Attacked first whenever one is up.",
                        ),
                    ),
            )
    }
}

fn group(theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .pt(px(16.0))
        .border_t_1()
        .border_color(theme.hairline)
}

fn status_text(settings: &Settings, status: Status) -> Option<String> {
    let target = settings.target.trim();
    let lost = match status {
        _ if !settings.enabled => {
            return Some("Goes wherever this player goes, across maps and cells.".into());
        }
        Status::Off => return None,
        Status::Waiting => return Some("Not in game".into()),
        Status::Travelling => return Some(format!("Going to {target}")),
        Status::Following => return Some(format!("Following {target}")),
        Status::Lost(lost) => lost,
    };
    let reason = match lost {
        Lost::Yourself => return Some(format!("Signed in as {target}")),
        Lost::Refused(Refusal::LockedZone) => "Locked zone",
        Lost::Refused(Refusal::IgnoringGoto) => "Goto blocked",
        Lost::Refused(Refusal::RoomFull) => "Room full",
        Lost::Missed => "Not found",
        Lost::Blocked => "Cell unreachable",
    };
    let fallbacks = if matches!(lost, Lost::Refused(_)) && !settings.fallbacks.is_empty() {
        ", fallbacks failed"
    } else {
        ""
    };
    Some(format!("{reason}{fallbacks} · retrying"))
}

impl Render for Follower {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let picker = shown_tab::picker(
            "follower-tab",
            &self.client,
            self.shown.as_ref(),
            |this: &mut Self, id, cx| this.show(Some(id), cx),
            cx,
        );
        let body = match self.automation().cloned() {
            Some(automation) => self.body(&theme, &automation, cx),
            None => div()
                .py(px(24.0))
                .text_center()
                .text_size(TEXT_SMALL)
                .line_height(LINE_SMALL)
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
                            .child(div().font_weight(FontWeight::MEDIUM).child("Follower")),
                    )
                    .child(picker),
            )
            .child(
                div()
                    .id("follower-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(20.0))
                    .child(body),
            )
    }
}
