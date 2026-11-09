use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, Context, Div, FocusHandle, FontWeight, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement as _, Pixels, Point,
    Role, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Toggled, Window,
    anchored, deferred, div, point, prelude::FluentBuilder as _, px,
};

use super::{
    RuffleView,
    ui::{self, Glyph},
};
use crate::{
    automation::{
        auto_relogin::Phase,
        auto_zone::Room,
        drops::Config as DropConfig,
        game_settings::{RenderingMode, Settings as GameSettings},
        quests::{Config as QuestConfig, Registered},
        register_items,
        settings::Automation,
        unregister_items,
    },
    domain::{
        inventory::ItemQuery,
        world::{Location, PADS},
    },
    ruffle::Command,
};

const LAST_SERVER: &str = "Last server";
const MAX_DIGITS: usize = 3;
const MAX_QUEST_DIGITS: usize = 6;
const MAX_TEXT: usize = 32;
const MAX_ITEM_NAME: usize = 64;
const SUBMENU_WIDTH: Pixels = px(200.0);
const SUBMENU_MAX_HEIGHT: Pixels = px(360.0);
const SUBMENU_OVERLAP: Pixels = px(2.0);
const BORDER: Pixels = px(1.0);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BarMenu {
    Options,
    AutoZone,
    AutoRelogin,
    Script,
    Cell,
    Pad,
    Environment,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Submenu {
    Rooms,
    Servers,
    Rendering,
}

impl BarMenu {
    fn submenu(self) -> Option<Submenu> {
        match self {
            BarMenu::AutoZone => Some(Submenu::Rooms),
            BarMenu::AutoRelogin => Some(Submenu::Servers),
            BarMenu::Options => Some(Submenu::Rendering),
            BarMenu::Script | BarMenu::Cell | BarMenu::Pad | BarMenu::Environment => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Delay,
    Retries,
    Backoff,
    BackoffCap,
    WalkSpeed,
    FrameRate,
    CustomName,
    CustomGuild,
    AddQuest,
    AddDrop,
    AddBoost,
}

impl Field {
    const RELOGIN: [Field; 4] = [
        Field::Delay,
        Field::Retries,
        Field::Backoff,
        Field::BackoffCap,
    ];
    const MOVEMENT: [Field; 2] = [Field::WalkSpeed, Field::FrameRate];
    const IDENTITY: [Field; 2] = [Field::CustomName, Field::CustomGuild];
    const ADD: [Field; 3] = [Field::AddQuest, Field::AddDrop, Field::AddBoost];

    fn label(self) -> &'static str {
        match self {
            Field::Delay => "Delay",
            Field::Retries => "Retries",
            Field::Backoff => "Backoff",
            Field::BackoffCap => "Backoff cap",
            Field::WalkSpeed => "Walk speed",
            Field::FrameRate => "Frame rate",
            Field::CustomName => "Name",
            Field::CustomGuild => "Guild",
            Field::AddQuest => "Add quest ID",
            Field::AddDrop => "Add drop",
            Field::AddBoost => "Add boost",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Field::Delay | Field::Backoff | Field::BackoffCap => "s",
            _ => "",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Field::CustomName => "Own name",
            Field::CustomGuild => "Own guild",
            Field::AddDrop | Field::AddBoost => "Item name",
            _ => "",
        }
    }

    fn is_text(self) -> bool {
        matches!(
            self,
            Field::CustomName | Field::CustomGuild | Field::AddDrop | Field::AddBoost
        )
    }

    fn steps(self) -> bool {
        !self.is_text() && self != Field::AddQuest
    }

    fn max_len(self) -> usize {
        match self {
            Field::AddQuest => MAX_QUEST_DIGITS,
            Field::AddDrop | Field::AddBoost => MAX_ITEM_NAME,
            _ if self.is_text() => MAX_TEXT,
            _ => MAX_DIGITS,
        }
    }

    fn value(self, automation: &Automation) -> String {
        let relogin = || automation.auto_relogin();
        let game = || automation.game_settings();
        match self {
            Field::Delay => relogin().delay_seconds.to_string(),
            Field::Retries => relogin().retries.to_string(),
            Field::Backoff => relogin().backoff_seconds.to_string(),
            Field::BackoffCap => relogin().backoff_cap_seconds.to_string(),
            Field::WalkSpeed => game().walk_speed.to_string(),
            Field::FrameRate => automation
                .frame_rate()
                .map(|fps| fps.to_string())
                .unwrap_or_default(),
            Field::CustomName => game().custom_name.unwrap_or_default(),
            Field::CustomGuild => game().custom_guild.unwrap_or_default(),
            Field::AddQuest | Field::AddDrop | Field::AddBoost => String::new(),
        }
    }

    fn commit(self, text: String, view: &mut RuffleView, cx: &mut Context<RuffleView>) {
        match self {
            Field::CustomName => {
                return view.change_game_settings(|settings| settings.custom_name = Some(text), cx);
            }
            Field::CustomGuild => {
                return view
                    .change_game_settings(|settings| settings.custom_guild = Some(text), cx);
            }
            Field::AddDrop => {
                view.automation.update_drops(|state| {
                    register_items(&mut state.items, vec![ItemQuery::Name(text)])
                });
                return cx.notify();
            }
            Field::AddBoost => {
                view.automation.update_boosts(|state| {
                    register_items(&mut state.items, vec![ItemQuery::Name(text)])
                });
                return cx.notify();
            }
            _ => {}
        }
        let Ok(value) = text.parse() else {
            return;
        };
        match self {
            Field::Delay => view.change_auto_relogin(|settings| settings.delay_seconds = value, cx),
            Field::Retries => view.change_auto_relogin(|settings| settings.retries = value, cx),
            Field::Backoff => {
                view.change_auto_relogin(|settings| settings.backoff_seconds = value, cx)
            }
            Field::BackoffCap => {
                view.change_auto_relogin(|settings| settings.backoff_cap_seconds = value, cx)
            }
            Field::WalkSpeed => {
                view.change_game_settings(|settings| settings.walk_speed = value, cx)
            }
            Field::FrameRate => {
                view.automation.set_frame_rate(value);
                cx.notify();
            }
            Field::AddQuest => {
                view.automation.update_quests(|state| {
                    state.register([Registered {
                        quest_id: value as i32,
                        reward_item_id: None,
                    }])
                });
                cx.notify();
            }
            Field::CustomName | Field::CustomGuild | Field::AddDrop | Field::AddBoost => {}
        }
    }

    fn editable(self, automation: &Automation) -> bool {
        self != Field::FrameRate || !automation.game_settings().rendering.minimal()
    }

    fn after(self, backwards: bool, automation: &Automation) -> Field {
        let group: &[Field] = match self {
            Field::Delay | Field::Retries | Field::Backoff | Field::BackoffCap => &Field::RELOGIN,
            Field::AddQuest | Field::AddDrop | Field::AddBoost => &Field::ADD,
            _ => &[
                Field::WalkSpeed,
                Field::FrameRate,
                Field::CustomName,
                Field::CustomGuild,
            ],
        };
        let index = group.iter().position(|field| *field == self).unwrap();
        let count = group.len();
        (1..count)
            .map(|step| {
                group[if backwards {
                    (index + count - step) % count
                } else {
                    (index + step) % count
                }]
            })
            .find(|field| field.editable(automation))
            .unwrap_or(self)
    }
}

type Toggle = (
    &'static str,
    &'static str,
    fn(&mut GameSettings) -> &mut bool,
);

const TOGGLES: [Toggle; 8] = [
    ("infinite-range", "Infinite range", |s| {
        &mut s.infinite_range
    }),
    ("provoke-cell", "Provoke cell", |s| &mut s.provoke_cell),
    ("enemy-magnet", "Enemy magnet", |s| &mut s.enemy_magnet),
    ("hide-players", "Hide players", |s| &mut s.hide_players),
    ("skip-cutscenes", "Skip cutscenes", |s| {
        &mut s.skip_cutscenes
    }),
    ("animations", "Animations", |s| &mut s.animations),
    ("collisions", "Collisions", |s| &mut s.collisions),
    ("ads", "Ads", |s| &mut s.ads),
];

struct Edit {
    field: Field,
    text: String,
    fresh: bool,
}

pub(super) struct BarState {
    open: Option<BarMenu>,
    submenu: Option<(Submenu, Point<Pixels>)>,
    aim_from: Option<Point<Pixels>>,
    edit: Option<Edit>,
    location: Option<Location>,
    focus: FocusHandle,
    triggers: Rc<Cell<[Bounds<Pixels>; 7]>>,
    submenu_rows: Rc<Cell<[Bounds<Pixels>; 3]>>,
    submenu_panel: Rc<Cell<Bounds<Pixels>>>,
}

impl BarState {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            open: None,
            submenu: None,
            aim_from: None,
            edit: None,
            location: None,
            focus: cx.focus_handle(),
            triggers: Default::default(),
            submenu_rows: Default::default(),
            submenu_panel: Default::default(),
        }
    }

    pub(super) fn open(&self) -> Option<BarMenu> {
        self.open
    }

    pub(super) fn located(&mut self, location: Option<Location>) {
        self.location = location;
    }
}

impl RuffleView {
    fn toggle_menu(&mut self, menu: BarMenu, window: &mut Window, cx: &mut Context<Self>) {
        if self.bar.open == Some(menu) {
            self.close_menu(window, cx);
            return;
        }
        self.commit_edit(cx);
        self.session.release_keys();
        self.bar.open = Some(menu);
        self.bar.submenu = None;
        window.focus(&self.bar.focus, cx);
        cx.notify();
    }

    pub(super) fn dismiss_menu(&mut self, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.bar.open = None;
        self.bar.submenu = None;
    }

    pub(super) fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_menu(cx);
        window.focus(&self.focus, cx);
        self.restore_modifiers(window);
        cx.notify();
    }

    fn panel_mouse_down_out(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = event.position;
        if self
            .bar
            .triggers
            .get()
            .iter()
            .any(|bounds| bounds.contains(&at))
            || (self.bar.submenu.is_some() && self.bar.submenu_panel.get().contains(&at))
        {
            return;
        }
        self.close_menu(window, cx);
        if self.player_bounds.contains(&at) {
            self.mouse.swallow(event.button);
            cx.stop_propagation();
        }
    }

    fn panel_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let shift = event.keystroke.modifiers.shift;
        if key == "escape" {
            if self.bar.edit.take().is_none() && self.bar.submenu.take().is_none() {
                self.close_menu(window, cx);
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }
        let Some(field) = self.bar.edit.as_ref().map(|edit| edit.field) else {
            return;
        };
        match key {
            "enter" => self.commit_edit(cx),
            "tab" => {
                self.commit_edit(cx);
                self.start_edit(field.after(shift, &self.automation));
            }
            "up" | "down" if field.steps() => {
                let stored = field.value(&self.automation).parse().unwrap_or(0);
                let edit = self.bar.edit.as_mut().unwrap();
                let step = if shift { 10 } else { 1 };
                let draft = edit.text.parse().unwrap_or(stored);
                let stepped = if key == "up" {
                    draft + step
                } else {
                    draft - step
                };
                edit.text = stepped.max(0).to_string();
                self.commit_edit(cx);
                self.start_edit(field);
            }
            "backspace" => {
                let edit = self.bar.edit.as_mut().unwrap();
                if edit.fresh {
                    edit.text.clear();
                } else {
                    edit.text.pop();
                }
                edit.fresh = false;
            }
            _ if field.is_text() => {
                let modifiers = event.keystroke.modifiers;
                let Some(typed) = event.keystroke.key_char.as_deref() else {
                    return;
                };
                if modifiers.platform || modifiers.control {
                    return;
                }
                let edit = self.bar.edit.as_mut().unwrap();
                if edit.fresh {
                    edit.text.clear();
                    edit.fresh = false;
                }
                if edit.text.chars().count() + typed.chars().count() <= field.max_len() {
                    edit.text.push_str(typed);
                }
            }
            digit if digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()) => {
                let edit = self.bar.edit.as_mut().unwrap();
                if edit.fresh {
                    edit.text.clear();
                    edit.fresh = false;
                }
                if edit.text.len() < field.max_len() {
                    edit.text.push_str(digit);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn start_edit(&mut self, field: Field) {
        if !field.editable(&self.automation) {
            self.bar.edit = None;
            return;
        }
        self.bar.edit = Some(Edit {
            field,
            text: field.value(&self.automation),
            fresh: true,
        });
    }

    fn commit_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.bar.edit.take() else {
            return;
        };
        edit.field.commit(edit.text, self, cx);
        cx.notify();
    }

    fn open_submenu(&mut self, submenu: Submenu, cx: &mut Context<Self>) {
        if self.bar.submenu.is_some_and(|(open, _)| open == submenu) {
            return;
        }
        self.commit_edit(cx);
        let row = self.bar.submenu_rows.get()[submenu as usize];
        self.bar.submenu = Some((
            submenu,
            point(
                row.right() + ui::PANEL_PADDING + BORDER - SUBMENU_OVERLAP,
                row.top() - ui::PANEL_PADDING - BORDER,
            ),
        ));
        cx.notify();
    }

    fn travel(&mut self, cell: String, pad: String, window: &mut Window, cx: &mut Context<Self>) {
        self.session.command(Command::Jump { cell, pad });
        self.close_menu(window, cx);
    }

    fn close_submenu(&mut self) {
        self.bar.submenu = None;
        self.bar.aim_from = None;
    }

    fn panel_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.bar.open else {
            return;
        };
        let at = event.position;
        let Some(submenu) = menu.submenu() else {
            return;
        };
        if self.bar.submenu_rows.get()[submenu as usize].contains(&at) {
            self.bar.aim_from = Some(at);
            self.open_submenu(submenu, cx);
            return;
        }
        let Some((_, origin)) = self.bar.submenu else {
            return;
        };
        let panel = self.bar.submenu_panel.get();
        let near_edge = if panel.size.height > Pixels::ZERO {
            (panel.origin, panel.bottom_left())
        } else {
            (origin, point(origin.x, origin.y + SUBMENU_MAX_HEIGHT))
        };
        if let Some(from) = self.bar.aim_from
            && heading_toward(from, at, near_edge)
        {
            self.bar.aim_from = Some(at);
            return;
        }
        self.close_submenu();
        cx.notify();
    }
}

fn heading_toward(
    from: Point<Pixels>,
    at: Point<Pixels>,
    (top, bottom): (Point<Pixels>, Point<Pixels>),
) -> bool {
    let side = |a: Point<Pixels>, b: Point<Pixels>, p: Point<Pixels>| {
        f32::from(b.x - a.x) * f32::from(p.y - a.y) - f32::from(b.y - a.y) * f32::from(p.x - a.x)
    };
    let (d1, d2, d3) = (
        side(from, top, at),
        side(top, bottom, at),
        side(bottom, from, at),
    );
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

fn zone_value(view: &RuffleView) -> SharedString {
    let settings = view.automation.auto_zone();
    match (settings.enabled, settings.room) {
        (true, Some(room)) => room.label().into(),
        (true, None) => "No map".into(),
        (false, _) => "Off".into(),
    }
}

fn relogin_value(view: &RuffleView) -> SharedString {
    let settings = view.automation.auto_relogin();
    match &view.auto_relogin {
        _ if !settings.enabled => "Off".into(),
        Phase::Attempt { attempt, of } => format!("Attempt {attempt} of {of}").into(),
        Phase::Waiting | Phase::Backoff { .. } => "Waiting".into(),
        Phase::Failed(_) => "Stopped".into(),
        Phase::Off | Phase::Armed => settings
            .server
            .clone()
            .unwrap_or_else(|| LAST_SERVER.into())
            .into(),
    }
}

fn status(phase: &Phase) -> Option<String> {
    match phase {
        Phase::Off => None,
        Phase::Armed => Some("Watching the connection".into()),
        Phase::Waiting => Some("Connection lost, waiting to log in".into()),
        Phase::Attempt { attempt, of } => Some(format!("Logging in, attempt {attempt} of {of}")),
        Phase::Backoff { attempt, of } => Some(format!(
            "Attempt {attempt} of {of} failed, waiting to retry"
        )),
        Phase::Failed(reason) => Some(format!("Stopped: {reason}")),
    }
}

pub(super) fn bar(view: &RuffleView, cx: &mut Context<RuffleView>) -> impl IntoElement {
    let attack = view.automation.auto_attack().enabled;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.0))
        .w_full()
        .h(ui::BAR_HEIGHT)
        .px(px(6.0))
        .bg(ui::bar_background())
        .border_b_1()
        .border_color(ui::hairline())
        .text_size(ui::TEXT_SIZE)
        .text_color(ui::text())
        .child(trigger(view, BarMenu::Options, "Options", None, cx))
        .child(trigger(
            view,
            BarMenu::AutoZone,
            "Auto zone",
            Some(zone_value(view)),
            cx,
        ))
        .child(trigger(
            view,
            BarMenu::AutoRelogin,
            "Auto relogin",
            Some(relogin_value(view)),
            cx,
        ))
        .child(
            bar_control("auto-attack")
                .role(Role::Switch)
                .aria_label("Auto attack")
                .aria_toggled(if attack {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .gap(px(8.0))
                .child("Auto attack")
                .child(ui::switch(attack))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change_auto_attack(|settings| settings.enabled = !attack, cx)
                })),
        )
        .child(trigger(
            view,
            BarMenu::Environment,
            "Environment",
            environment_value(view),
            cx,
        ))
        .child(trigger(
            view,
            BarMenu::Cell,
            "Cell",
            place(view, |location| &location.cell),
            cx,
        ))
        .child(trigger(
            view,
            BarMenu::Pad,
            "Pad",
            place(view, |location| &location.pad),
            cx,
        ))
        .child(
            bar_control("bank")
                .role(Role::Button)
                .aria_label("Bank")
                .child("Bank")
                .on_click(cx.listener(|this, _, _, _| this.session.command(Command::OpenBank))),
        )
        .child(div().flex_1())
        .child(trigger(
            view,
            BarMenu::Script,
            "Script",
            Some(view.script.label()),
            cx,
        ))
}

fn bar_control(id: &'static str) -> Stateful<Div> {
    ui::pressable(
        div()
            .id(id)
            .flex()
            .items_center()
            .h(ui::ROW_HEIGHT)
            .px(px(8.0))
            .rounded(ui::CONTROL_RADIUS),
    )
}

fn trigger(
    view: &RuffleView,
    menu: BarMenu,
    title: &'static str,
    value: Option<SharedString>,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement {
    let triggers = view.bar.triggers.clone();
    let open = view.bar.open == Some(menu);
    let label = match &value {
        Some(value) => SharedString::from(format!("{title}: {value}")),
        None => title.into(),
    };
    let id = match menu {
        BarMenu::Options => "options",
        BarMenu::AutoZone => "auto-zone",
        BarMenu::AutoRelogin => "auto-relogin",
        BarMenu::Script => "script",
        BarMenu::Cell => "cell",
        BarMenu::Pad => "pad",
        BarMenu::Environment => "environment",
    };
    div()
        .on_children_prepainted(move |bounds, _, _| {
            let mut all = triggers.get();
            all[menu as usize] = bounds[0];
            triggers.set(all);
        })
        .child(
            bar_control(id)
                .role(Role::Button)
                .aria_label(label)
                .aria_expanded(open)
                .gap(px(7.0))
                .when(open, |trigger| trigger.bg(ui::pressed()))
                .child(title)
                .children(value.map(|value| div().text_color(ui::secondary_text()).child(value)))
                .child(ui::glyph(Glyph::Down, ui::secondary_text()))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.toggle_menu(menu, window, cx)
                    }),
                ),
        )
}

pub(super) fn overlay(
    view: &RuffleView,
    menu: BarMenu,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement {
    let anchor = view.bar.triggers.get()[menu as usize];
    let panel = match menu {
        BarMenu::Options => options_panel(view, cx),
        BarMenu::AutoZone => zone_panel(view, cx),
        BarMenu::AutoRelogin => relogin_panel(view, cx),
        BarMenu::Script => script_panel(view, cx),
        BarMenu::Cell | BarMenu::Pad => travel_panel(view, menu, cx),
        BarMenu::Environment => environment_panel(view, cx),
    }
    .track_focus(&view.bar.focus)
    .on_key_down(cx.listener(RuffleView::panel_key_down))
    .on_mouse_down_out(cx.listener(RuffleView::panel_mouse_down_out))
    .on_mouse_move(cx.listener(RuffleView::panel_mouse_move))
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _, _, cx| this.commit_edit(cx)),
    );
    div()
        .absolute()
        .child(
            deferred(
                anchored()
                    .position(point(anchor.left(), anchor.bottom() + px(6.0)))
                    .snap_to_window_with_margin(px(8.0))
                    .child(panel),
            )
            .with_priority(1),
        )
        .when_some(view.bar.submenu, |overlay, (submenu, at)| {
            overlay.child(submenu_panel(view, submenu, at, cx))
        })
}

fn toggle_row(id: &'static str, label: &'static str, on: bool) -> Stateful<Div> {
    ui::pressable(ui::row(id))
        .role(Role::Switch)
        .aria_label(label)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .child(div().font_weight(FontWeight::MEDIUM).child(label))
        .child(ui::switch(on))
}

fn choice_row(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
) -> Stateful<Div> {
    ui::pressable(ui::row(id))
        .role(Role::MenuItemRadio)
        .justify_start()
        .gap(px(6.0))
        .pl(px(6.0))
        .child(div().flex_none().w(px(12.0)).when(selected, |column| {
            column.child(ui::glyph(Glyph::Check, ui::text()))
        }))
        .child(label.into())
}

fn submenu_row(
    view: &RuffleView,
    submenu: Submenu,
    label: &'static str,
    value: SharedString,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement + use<> {
    let rows = view.bar.submenu_rows.clone();
    let open = view.bar.submenu.is_some_and(|(open, _)| open == submenu);
    div()
        .on_children_prepainted(move |bounds, _, _| {
            let mut all = rows.get();
            all[submenu as usize] = bounds[0];
            rows.set(all);
        })
        .child(
            ui::pressable(ui::row(label))
                .role(Role::MenuItem)
                .aria_expanded(open)
                .when(open, |row| row.bg(ui::pressed()))
                .child(label)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .text_color(ui::secondary_text())
                        .child(value)
                        .child(ui::glyph(Glyph::Right, ui::secondary_text())),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.open_submenu(submenu, cx))),
        )
}

fn zone_panel(view: &RuffleView, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    let settings = view.automation.auto_zone();
    ui::panel("auto-zone-panel", px(220.0))
        .child(
            toggle_row("auto-zone-enabled", "Auto zone", settings.enabled).on_click(cx.listener(
                |this, _, _, cx| {
                    this.change_auto_zone(|settings| settings.enabled = !settings.enabled, cx)
                },
            )),
        )
        .child(submenu_row(
            view,
            Submenu::Rooms,
            "Map",
            settings.room.map_or("None", Room::label).into(),
            cx,
        ))
}

fn options_panel(view: &RuffleView, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    let mut settings = view.automation.game_settings();
    ui::panel("options-panel", px(392.0))
        .child(ui::section("Gameplay"))
        .child(
            div()
                .grid()
                .grid_cols(2)
                .children(TOGGLES.map(|(id, label, setting)| {
                    toggle_row(id, label, *setting(&mut settings)).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.change_game_settings(
                                |settings| {
                                    let on = setting(settings);
                                    *on = !*on;
                                },
                                cx,
                            )
                        },
                    ))
                })),
        )
        .child(submenu_row(
            view,
            Submenu::Rendering,
            "Rendering",
            settings.rendering.mode().label().into(),
            cx,
        ))
        .children(Field::MOVEMENT.map(|field| field_row(view, field, cx)))
        .child(ui::separator())
        .child(ui::section("Identity"))
        .children(Field::IDENTITY.map(|field| field_row(view, field, cx)))
}

fn script_panel(view: &RuffleView, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    let panel = ui::panel("script-panel", px(264.0))
        .when_some(view.script.status(), |panel, text| {
            panel.child(div().pt(px(6.0)).child(ui::status(text)))
        });
    if view.script.running() {
        return panel.child(
            ui::pressable(ui::row("script-stop"))
                .role(Role::MenuItem)
                .child("Stop")
                .on_click(cx.listener(|this, _, _, cx| this.stop_script(cx))),
        );
    }
    panel
        .when_some(view.script.loaded(), |panel, name| {
            panel.child(
                ui::pressable(ui::row("script-start"))
                    .role(Role::MenuItem)
                    .child(format!("Start {name}"))
                    .on_click(cx.listener(|this, _, _, cx| this.start_loaded_script(cx))),
            )
        })
        .child(
            ui::pressable(ui::row("script-load"))
                .role(Role::MenuItem)
                .child("Load script…")
                .on_click(cx.listener(|this, _, window, cx| this.load_script(window, cx))),
        )
}

fn environment_value(view: &RuffleView) -> Option<SharedString> {
    let (quests, drops, boosts) = (
        view.automation.quests(),
        view.automation.drops(),
        view.automation.boosts(),
    );
    let registered = quests.quests.len() + drops.items.len() + boosts.items.len();
    let rules = DropConfig {
        enabled: drops.config.enabled,
        ..DropConfig::default()
    } != drops.config;
    match (registered, rules) {
        (0, false) => None,
        (0, true) => Some("Drop rules".into()),
        (count, _) => Some(format!("{count} registered").into()),
    }
}

type QuestToggle = (
    &'static str,
    &'static str,
    fn(&mut QuestConfig) -> &mut bool,
);
type DropToggle = (&'static str, &'static str, fn(&mut DropConfig) -> &mut bool);

const DROP_KINDS: [DropToggle; 4] = [
    ("drops-ac", "AC", |c| &mut c.accept_ac),
    ("drops-ac-member", "AC, members only", |c| {
        &mut c.accept_ac_member_only
    }),
    ("drops-non-ac", "Non-AC", |c| &mut c.accept_non_ac),
    ("drops-non-ac-member", "Non-AC, members only", |c| {
        &mut c.accept_non_ac_member_only
    }),
];

const QUEST_DROPS: [QuestToggle; 2] = [
    ("quests-rewards", "Accept rewards", |c| {
        &mut c.accept_rewards
    }),
    ("quests-requirements", "Accept requirements", |c| {
        &mut c.accept_requirements
    }),
];

fn item_label(query: &ItemQuery) -> SharedString {
    match query {
        ItemQuery::Name(name) => name.clone().into(),
        ItemQuery::ItemId { item_id } => format!("Item {item_id}").into(),
    }
}

fn registered_row(
    (kind, index): (&'static str, usize),
    label: SharedString,
    detail: Option<SharedString>,
    remove: impl Fn(&mut RuffleView, &mut Context<RuffleView>) + 'static,
    cx: &mut Context<RuffleView>,
) -> Stateful<Div> {
    ui::row((kind, index))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .gap(px(6.0))
                .overflow_hidden()
                .child(div().truncate().child(label.clone()))
                .children(
                    detail.map(|detail| div().text_color(ui::secondary_text()).child(detail)),
                ),
        )
        .child(
            div()
                .id(gpui::ElementId::named_usize(
                    format!("{kind}-remove"),
                    index,
                ))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(20.0))
                .rounded(ui::CONTROL_RADIUS)
                .cursor_pointer()
                .hover(|style| style.bg(ui::hover()))
                .active(|style| style.bg(ui::pressed()))
                .role(Role::Button)
                .aria_label(SharedString::from(format!("Unregister {label}")))
                .child(ui::glyph(Glyph::Close, ui::secondary_text()))
                .on_click(cx.listener(move |this, _, _, cx| remove(this, cx))),
        )
}

fn unregister_all_row(
    id: &'static str,
    clear: fn(&Automation),
    cx: &mut Context<RuffleView>,
) -> Stateful<Div> {
    ui::pressable(ui::row(id))
        .role(Role::MenuItem)
        .text_color(ui::secondary_text())
        .child("Unregister all")
        .on_click(cx.listener(move |this, _, _, cx| {
            clear(&this.automation);
            cx.notify();
        }))
}

fn environment_panel(view: &RuffleView, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    let quests = view.automation.quests();
    let drops = view.automation.drops();
    let boosts = view.automation.boosts();
    ui::panel("environment-panel", px(392.0))
        .max_h(px(560.0))
        .overflow_y_scroll()
        .child(ui::section("Quests"))
        .child(
            toggle_row(
                "quests-enabled",
                "Accept and complete",
                quests.config.enabled,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.automation
                    .update_quests(|state| state.config.enabled = !state.config.enabled);
                cx.notify();
            })),
        )
        .child(
            div()
                .grid()
                .grid_cols(2)
                .children(QUEST_DROPS.map(|(id, label, setting)| {
                    let mut config = quests.config;
                    toggle_row(id, label, *setting(&mut config)).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.automation.update_quests(|state| {
                                let on = setting(&mut state.config);
                                *on = !*on;
                            });
                            cx.notify();
                        },
                    ))
                })),
        )
        .children(quests.quests.iter().enumerate().map(|(index, registered)| {
            let quest_id = registered.quest_id;
            registered_row(
                ("quest", index),
                format!("Quest {quest_id}").into(),
                registered
                    .reward_item_id
                    .map(|reward| format!("reward {reward}").into()),
                move |this, cx| {
                    this.automation
                        .update_quests(|state| state.unregister(&[quest_id]));
                    cx.notify();
                },
                cx,
            )
        }))
        .child(field_row(view, Field::AddQuest, cx))
        .when(!quests.quests.is_empty(), |panel| {
            panel.child(unregister_all_row(
                "quests-clear",
                |automation| {
                    automation.update_quests(|state| state.quests.clear());
                },
                cx,
            ))
        })
        .child(ui::separator())
        .child(ui::section("Drops"))
        .child(
            toggle_row("drops-enabled", "Accept and reject", drops.config.enabled).on_click(
                cx.listener(|this, _, _, cx| {
                    this.automation
                        .update_drops(|state| state.config.enabled = !state.config.enabled);
                    cx.notify();
                }),
            ),
        )
        .children(drops.items.iter().enumerate().map(|(index, query)| {
            let removed = query.clone();
            registered_row(
                ("drop", index),
                item_label(query),
                None,
                move |this, cx| {
                    this.automation.update_drops(|state| {
                        unregister_items(&mut state.items, std::slice::from_ref(&removed))
                    });
                    cx.notify();
                },
                cx,
            )
        }))
        .child(field_row(view, Field::AddDrop, cx))
        .when(!drops.items.is_empty(), |panel| {
            panel.child(unregister_all_row(
                "drops-clear",
                |automation| {
                    automation.update_drops(|state| state.items.clear());
                },
                cx,
            ))
        })
        .child(ui::section("Accept unregistered drops"))
        .child(
            div()
                .grid()
                .grid_cols(2)
                .children(DROP_KINDS.map(|(id, label, setting)| {
                    let mut config = drops.config;
                    toggle_row(id, label, *setting(&mut config)).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.automation.update_drops(|state| {
                                let on = setting(&mut state.config);
                                *on = !*on;
                            });
                            cx.notify();
                        },
                    ))
                })),
        )
        .child(
            toggle_row(
                "drops-reject",
                "Reject everything else",
                drops.config.reject_others,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.automation
                    .update_drops(|state| state.config.reject_others = !state.config.reject_others);
                cx.notify();
            })),
        )
        .child(ui::separator())
        .child(ui::section("Boosts"))
        .child(
            toggle_row("boosts-enabled", "Use boosts", boosts.config.enabled).on_click(
                cx.listener(|this, _, _, cx| {
                    this.automation
                        .update_boosts(|state| state.config.enabled = !state.config.enabled);
                    cx.notify();
                }),
            ),
        )
        .children(boosts.items.iter().enumerate().map(|(index, query)| {
            let removed = query.clone();
            registered_row(
                ("boost", index),
                item_label(query),
                None,
                move |this, cx| {
                    this.automation.update_boosts(|state| {
                        unregister_items(&mut state.items, std::slice::from_ref(&removed))
                    });
                    cx.notify();
                },
                cx,
            )
        }))
        .child(field_row(view, Field::AddBoost, cx))
        .when(!boosts.items.is_empty(), |panel| {
            panel.child(unregister_all_row(
                "boosts-clear",
                |automation| {
                    automation.update_boosts(|state| state.items.clear());
                },
                cx,
            ))
        })
}

fn place(view: &RuffleView, part: fn(&Location) -> &String) -> Option<SharedString> {
    view.bar
        .location
        .as_ref()
        .map(part)
        .filter(|name| !name.is_empty())
        .map(|name| name.clone().into())
}

fn travel_panel(view: &RuffleView, menu: BarMenu, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    let panel = ui::panel("travel-panel", px(200.0))
        .max_h(SUBMENU_MAX_HEIGHT)
        .overflow_y_scroll();
    let Some(location) = &view.bar.location else {
        return panel.child(note("Log in to travel"));
    };
    if menu == BarMenu::Pad {
        return panel.children(PADS.iter().enumerate().map(|(index, pad)| {
            let in_cell = location
                .cell_pads
                .iter()
                .any(|known| known.eq_ignore_ascii_case(pad));
            let cell = location.cell.clone();
            choice_row(("pad", index), *pad, location.pad.eq_ignore_ascii_case(pad))
                .when(!in_cell, |row| row.text_color(ui::tertiary_text()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.travel(cell.clone(), pad.to_string(), window, cx)
                }))
        }));
    }
    if location.cells.is_empty() {
        return panel.child(note("No cells"));
    }
    panel.children(location.cells.iter().enumerate().map(|(index, cell)| {
        let (cell, pad) = (cell.clone(), location.pad.clone());
        choice_row(("cell", index), cell.clone(), cell == location.cell).on_click(cx.listener(
            move |this, _, window, cx| this.travel(cell.clone(), pad.clone(), window, cx),
        ))
    }))
}

fn note(text: &'static str) -> impl IntoElement {
    div()
        .px(px(8.0))
        .py(px(6.0))
        .text_size(ui::SMALL_TEXT_SIZE)
        .text_color(ui::tertiary_text())
        .child(text)
}

fn relogin_panel(view: &RuffleView, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    let settings = view.automation.auto_relogin();
    ui::panel("auto-relogin-panel", px(264.0))
        .child(
            toggle_row("auto-relogin-enabled", "Auto relogin", settings.enabled).on_click(
                cx.listener(|this, _, _, cx| {
                    this.change_auto_relogin(|settings| settings.enabled = !settings.enabled, cx)
                }),
            ),
        )
        .when_some(status(&view.auto_relogin), |panel, text| {
            panel.child(ui::status(text))
        })
        .child(ui::separator())
        .child(submenu_row(
            view,
            Submenu::Servers,
            "Server",
            settings
                .server
                .clone()
                .unwrap_or_else(|| LAST_SERVER.into())
                .into(),
            cx,
        ))
        .child(ui::separator())
        .child(ui::section("Login attempts"))
        .children(Field::RELOGIN.map(|field| field_row(view, field, cx)))
}

fn field_row(
    view: &RuffleView,
    field: Field,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement + use<> {
    let edit = view.bar.edit.as_ref().filter(|edit| edit.field == field);
    let editing = edit.is_some();
    let fresh = edit.is_some_and(|edit| edit.fresh);
    let text = edit.map_or_else(|| field.value(&view.automation), |edit| edit.text.clone());
    let name = field.label();
    let id = SharedString::from(format!("{name}-field"));
    let input = if field.is_text() {
        ui::text_field(id, text, field.placeholder(), editing, fresh).role(Role::TextInput)
    } else {
        ui::number_field(id, text, editing, fresh).role(Role::SpinButton)
    }
    .when(!field.editable(&view.automation), |input| {
        input
            .text_color(ui::tertiary_text())
            .cursor_default()
            .aria_description("Minimal rendering holds the frame rate at 2")
    });
    ui::row(SharedString::from(format!("{name}-row")))
        .child(name)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(input.aria_label(name).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.commit_edit(cx);
                        this.start_edit(field);
                        cx.notify();
                    }),
                ))
                .child(
                    div()
                        .w(px(8.0))
                        .text_color(ui::tertiary_text())
                        .child(field.unit()),
                ),
        )
}

fn submenu_panel(
    view: &RuffleView,
    submenu: Submenu,
    at: Point<Pixels>,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement {
    let panel = ui::panel("bar-submenu", SUBMENU_WIDTH)
        .max_h(SUBMENU_MAX_HEIGHT)
        .overflow_y_scroll();
    let panel = match submenu {
        Submenu::Rooms => {
            let selected = view.automation.auto_zone().room;
            panel.children(Room::ALL.map(|room| {
                choice_row(room.label(), room.label(), selected == Some(room)).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.change_auto_zone(|settings| settings.room = Some(room), cx);
                        this.close_submenu();
                    }),
                )
            }))
        }
        Submenu::Servers => servers(view, panel, cx),
        Submenu::Rendering => {
            let selected = view.automation.game_settings().rendering.mode();
            panel.children(RenderingMode::ALL.map(|mode| {
                choice_row(mode.label(), mode.label(), selected == mode).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.change_game_settings(|settings| settings.rendering.set(mode), cx);
                        this.close_submenu();
                    },
                ))
            }))
        }
    };
    let bounds = view.bar.submenu_panel.clone();
    deferred(
        anchored()
            .position(at)
            .snap_to_window_with_margin(px(8.0))
            .child(
                div()
                    .on_children_prepainted(move |all, _, _| bounds.set(all[0]))
                    .child(panel),
            ),
    )
    .with_priority(2)
}

fn servers(view: &RuffleView, panel: Stateful<Div>, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    let saved = view.automation.auto_relogin().server.clone();
    let mut names = view.servers.clone();
    if let Some(saved) = &saved
        && !names.contains(saved)
    {
        names.push(saved.clone());
    }
    let choose = |server: Option<String>| {
        cx.listener(move |this: &mut RuffleView, _, _, cx| {
            let server = server.clone();
            this.change_auto_relogin(|settings| settings.server = server, cx);
            this.close_submenu();
        })
    };
    panel
        .child(choice_row("server-last", LAST_SERVER, saved.is_none()).on_click(choose(None)))
        .child(if view.servers.is_empty() {
            note("Log in to list the other servers").into_any_element()
        } else {
            ui::separator().into_any_element()
        })
        .children(names.into_iter().enumerate().map(|(index, name)| {
            let selected = saved.as_deref() == Some(name.as_str());
            choice_row(("server", index), name.clone(), selected).on_click(choose(Some(name)))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::game_settings::RenderingMode;

    #[test]
    fn tab_skips_the_frame_rate_while_minimal_rendering_holds_it() {
        let automation = Automation::default();
        assert_eq!(Field::WalkSpeed.after(false, &automation), Field::FrameRate);
        automation.update_game_settings(|settings| settings.rendering.set(RenderingMode::Minimal));
        assert_eq!(
            Field::WalkSpeed.after(false, &automation),
            Field::CustomName
        );
        assert_eq!(Field::CustomName.after(true, &automation), Field::WalkSpeed);
    }

    #[test]
    fn moving_toward_the_submenu_keeps_it_open() {
        let near_edge = (point(px(300.0), px(100.0)), point(px(300.0), px(400.0)));
        let from = point(px(250.0), px(150.0));
        assert!(heading_toward(from, point(px(270.0), px(170.0)), near_edge));
        assert!(heading_toward(from, point(px(280.0), px(140.0)), near_edge));
        assert!(!heading_toward(
            from,
            point(px(250.0), px(180.0)),
            near_edge
        ));
        assert!(!heading_toward(
            from,
            point(px(230.0), px(150.0)),
            near_edge
        ));
    }
}
