use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, ClickEvent, Context, Div, ElementId, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement as _, Pixels, Point, RenderOnce, Role, ScrollHandle,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Toggled, Window,
    anchored, deferred, div, point, prelude::FluentBuilder as _, px,
};

use mira_ui::{
    ActiveTheme as _, PointerCursor as _, Theme, motion,
    nav::{self, Axis, Step},
    primitives::{Icon, Scrollbar, Spinner, icon},
    theme::{CONTROL_SM, DISABLED_OPACITY, PANEL_INSET, ROW, TEXT_BODY, TEXT_SMALL},
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
        game_settings::{FrameRate, RenderingMode, Settings as GameSettings},
        quests::{Config as QuestConfig, Registered},
        register_items,
        settings::Automation,
        unregister_items,
    },
    domain::{
        inventory::ItemQuery,
        player::Identity,
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
const VISIBLE_CELLS: f32 = 10.5;
const SCROLLBAR_CLEARANCE: Pixels = px(2.0);
const BORDER: Pixels = px(1.0);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BarMenu {
    Windows,
    Options,
    AutoZone,
    AutoRelogin,
    Script,
    Jump,
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
            BarMenu::Windows | BarMenu::Script | BarMenu::Jump | BarMenu::Environment => None,
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

    fn is_default(self, automation: &Automation) -> bool {
        let game = automation.game_settings();
        match self {
            Field::WalkSpeed => game.walk_speed == GameSettings::default().walk_speed,
            Field::FrameRate => automation
                .frame_rate()
                .is_none_or(|fps| fps == FrameRate::DEFAULT),
            Field::CustomName => game.custom_name.is_none(),
            Field::CustomGuild => game.custom_guild.is_none(),
            _ => true,
        }
    }

    fn reset(self, view: &mut RuffleView, cx: &mut Context<RuffleView>) {
        match self {
            Field::WalkSpeed => view.change_game_settings(
                |settings| settings.walk_speed = GameSettings::default().walk_speed,
                cx,
            ),
            Field::FrameRate => {
                view.automation.set_frame_rate(FrameRate::DEFAULT);
                cx.notify();
            }
            Field::CustomName => {
                view.change_game_settings(|settings| settings.custom_name = None, cx)
            }
            Field::CustomGuild => {
                view.change_game_settings(|settings| settings.custom_guild = None, cx)
            }
            _ => {}
        }
    }

    fn needs_game(self) -> bool {
        matches!(
            self,
            Field::WalkSpeed | Field::FrameRate | Field::CustomName | Field::CustomGuild
        )
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

const TOGGLES: [Toggle; 9] = [
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
    ("anti-counter", "Anti counter", |s| &mut s.anti_counter),
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
    jump: Option<Spot>,
    cells_scroll: ScrollHandle,
    location: Option<Location>,
    identity: Option<Identity>,
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
            jump: None,
            cells_scroll: ScrollHandle::new(),
            location: None,
            identity: None,
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
        if location.is_none()
            && self
                .edit
                .as_ref()
                .is_some_and(|edit| edit.field.needs_game())
        {
            self.edit = None;
        }
        self.location = location;
    }

    pub(super) fn identified(&mut self, identity: Option<Identity>) {
        self.identity = identity;
    }

    pub(super) fn character(&self) -> Option<&str> {
        self.identity
            .as_ref()
            .map(|identity| identity.name.as_str())
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
        self.bar.jump = None;
        if menu == BarMenu::Jump
            && let Some(location) = &self.bar.location
        {
            let spot = current_spot(location);
            self.bar.jump = Some(spot);
            self.bar.cells_scroll.scroll_to_item(spot.row);
        }
        window.focus(&self.bar.focus, cx);
        cx.notify();
    }

    pub(super) fn dismiss_menu(&mut self, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.bar.open = None;
        self.bar.submenu = None;
        self.bar.jump = None;
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
        if self.bar.open == Some(BarMenu::Jump) {
            let Some(location) = &self.bar.location else {
                return;
            };
            match jump_key(location, self.bar.jump, event) {
                Some(JumpKey::Move(spot)) => {
                    self.bar.jump = Some(spot);
                    if spot.column == Column::Cell {
                        self.bar.cells_scroll.scroll_to_item(spot.row);
                    }
                }
                Some(JumpKey::Travel {
                    cell,
                    pad,
                    keep_open,
                }) => self.travel(cell, pad, keep_open, window, cx),
                None => return,
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

    fn in_game(&self) -> bool {
        self.bar.location.is_some()
    }

    fn field_enabled(&self, field: Field) -> bool {
        field.editable(&self.automation) && (!field.needs_game() || self.in_game())
    }

    fn start_edit(&mut self, field: Field) {
        if !self.field_enabled(field) {
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
                row.right() + PANEL_INSET + BORDER - SUBMENU_OVERLAP,
                row.top() - PANEL_INSET - BORDER,
            ),
        ));
        cx.notify();
    }

    fn travel(
        &mut self,
        cell: String,
        pad: String,
        keep_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.session.command(Command::Jump { cell, pad });
        if !keep_open {
            self.close_menu(window, cx);
        }
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Column {
    Cell,
    Pad,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Spot {
    column: Column,
    row: usize,
}

#[derive(PartialEq, Debug)]
enum JumpKey {
    Move(Spot),
    Travel {
        cell: String,
        pad: String,
        keep_open: bool,
    },
}

fn current_spot(location: &Location) -> Spot {
    if location.cells.is_empty() {
        let row = PADS
            .iter()
            .position(|pad| pad.eq_ignore_ascii_case(&location.pad));
        return Spot {
            column: Column::Pad,
            row: row.unwrap_or(0),
        };
    }
    let row = location
        .cells
        .iter()
        .position(|cell| *cell == location.cell);
    Spot {
        column: Column::Cell,
        row: row.unwrap_or(0),
    }
}

fn jump_key(location: &Location, at: Option<Spot>, event: &KeyDownEvent) -> Option<JumpKey> {
    let len = |column| match column {
        Column::Cell => location.cells.len(),
        Column::Pad => PADS.len(),
    };
    let labels = |column| -> Vec<(usize, String)> {
        match column {
            Column::Cell => location.cells.iter().cloned().enumerate().collect(),
            Column::Pad => PADS.iter().map(|pad| pad.to_string()).enumerate().collect(),
        }
    };
    let key = event.keystroke.key.as_str();
    let across = match key {
        "left" => Some(Column::Cell),
        "right" => Some(Column::Pad),
        "tab" => Some(match at.map(|spot| spot.column) {
            Some(Column::Cell) => Column::Pad,
            _ => Column::Cell,
        }),
        _ => None,
    };
    let Some(at) = at.filter(|spot| spot.row < len(spot.column)) else {
        let start = current_spot(location);
        if across.is_some() || Step::from_key(event, Axis::Vertical).is_some() {
            return Some(JumpKey::Move(start));
        }
        return nav::typeahead(key, labels(start.column).into_iter(), None).map(|row| {
            JumpKey::Move(Spot {
                column: start.column,
                row,
            })
        });
    };
    if key == "enter" || key == "space" {
        let (cell, pad) = match at.column {
            Column::Cell => (location.cells[at.row].clone(), location.pad.clone()),
            Column::Pad => (location.cell.clone(), PADS[at.row].to_string()),
        };
        return Some(JumpKey::Travel {
            cell,
            pad,
            keep_open: event.keystroke.modifiers.shift,
        });
    }
    if let Some(column) = across {
        return (len(column) > 0).then(|| {
            JumpKey::Move(Spot {
                column,
                row: at.row.min(len(column) - 1),
            })
        });
    }
    Step::from_key(event, Axis::Vertical)
        .and_then(|step| nav::step(Some(at.row), len(at.column), step, true, |_| true))
        .or_else(|| nav::typeahead(key, labels(at.column).into_iter(), Some(at.row)))
        .map(|row| {
            JumpKey::Move(Spot {
                column: at.column,
                row,
            })
        })
}

enum Detail {
    Text(SharedString),
    Working(SharedString),
    Waiting(SharedString),
}

#[derive(IntoElement)]
struct Busy {
    id: ElementId,
    label: SharedString,
    waiting: bool,
}

impl RenderOnce for Busy {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let target = if self.waiting { 0.0 } else { 1.0 };
        let spinning = motion::travel((self.id.clone(), "spinning"), target, window, cx);
        let layer = |opacity: f32| div().absolute().inset_0().opacity(opacity);
        div()
            .relative()
            .flex_none()
            .size(px(14.0))
            .when(spinning < 1.0, |busy| {
                let clock = icon(Icon::Clock, cx.theme().text);
                let clock = if cx.reduce_motion() {
                    clock.into_any_element()
                } else {
                    motion::breathe((self.id.clone(), "pulse"), clock).into_any_element()
                };
                busy.child(
                    layer(1.0 - spinning)
                        .id((self.id.clone(), "clock"))
                        .role(Role::ProgressIndicator)
                        .aria_label(self.label.clone())
                        .child(clock),
                )
            })
            .when(spinning > 0.0, |busy| {
                busy.child(layer(spinning).child(Spinner::new(self.id).label(self.label)))
            })
    }
}

fn relogin_value(view: &RuffleView) -> Option<Detail> {
    if !view.automation.auto_relogin().enabled {
        return None;
    }
    match &view.auto_relogin {
        Phase::Attempt { attempt, of } => {
            Some(Detail::Working(format!("Attempt {attempt} of {of}").into()))
        }
        Phase::Waiting | Phase::Backoff { .. } => Some(Detail::Waiting("Waiting".into())),
        Phase::Failed(_) => Some(Detail::Text("Stopped".into())),
        Phase::Off | Phase::Armed => None,
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
    let theme = cx.theme().clone();
    let attack = view.automation.auto_attack().enabled;
    let live = view.in_game();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.0))
        .w_full()
        .h(ui::BAR_HEIGHT)
        .pr(px(6.0))
        .child(super::client::drag_area().flex_none().w(view.bar_inset))
        .bg(theme.base)
        .border_b_1()
        .border_color(theme.border)
        .text_size(TEXT_BODY)
        .text_color(theme.text)
        .child(trigger(
            &theme,
            view,
            BarMenu::Windows,
            "Windows",
            None,
            None,
            cx,
            true,
        ))
        .child(trigger(
            &theme,
            view,
            BarMenu::Script,
            "Script",
            None,
            None,
            cx,
            true,
        ))
        .child(trigger(
            &theme,
            view,
            BarMenu::Options,
            "Options",
            None,
            None,
            cx,
            true,
        ))
        .child(trigger(
            &theme,
            view,
            BarMenu::AutoZone,
            "Auto zone",
            None,
            Some(view.automation.auto_zone().enabled),
            cx,
            true,
        ))
        .child(trigger(
            &theme,
            view,
            BarMenu::AutoRelogin,
            "Auto relogin",
            relogin_value(view),
            Some(view.automation.auto_relogin().enabled),
            cx,
            true,
        ))
        .child(super::client::drag_area().flex_1())
        .child(
            bar_control(&theme, "auto-attack", live)
                .flex_none()
                .role(Role::Switch)
                .aria_label("Auto attack")
                .aria_toggled(if attack {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .gap(px(8.0))
                .child("Auto attack")
                .child(ui::switch(&theme, attack))
                .when(live, |control| {
                    control.on_click(cx.listener(move |this, _, _, cx| {
                        this.change_auto_attack(|settings| settings.enabled = !attack, cx)
                    }))
                }),
        )
        .child(trigger(
            &theme,
            view,
            BarMenu::Jump,
            "Jump",
            None,
            None,
            cx,
            live,
        ))
        .child(
            div()
                .flex_none()
                .mx(px(4.0))
                .w(px(1.0))
                .h(px(16.0))
                .bg(theme.hairline),
        )
        .child(
            bar_control(&theme, "bank", live)
                .flex_none()
                .role(Role::Button)
                .aria_label("Bank")
                .child("Bank")
                .when(live, |control| {
                    control.on_click(
                        cx.listener(|this, _, _, _| this.session.command(Command::OpenBank)),
                    )
                }),
        )
}

fn bar_control(theme: &Theme, id: &'static str, enabled: bool) -> Stateful<Div> {
    press(
        theme,
        div()
            .id(id)
            .flex()
            .items_center()
            .h(CONTROL_SM)
            .px(px(8.0))
            .rounded(theme.radius.control),
        enabled,
    )
}

fn trigger(
    theme: &Theme,
    view: &RuffleView,
    menu: BarMenu,
    title: &'static str,
    value: Option<Detail>,
    on: Option<bool>,
    cx: &mut Context<RuffleView>,
    enabled: bool,
) -> impl IntoElement {
    let triggers = view.bar.triggers.clone();
    let open = view.bar.open == Some(menu);
    let shrinks = matches!(value, Some(Detail::Text(_)));
    let state = match on {
        Some(true) => ", on",
        Some(false) => ", off",
        None => "",
    };
    let label = match &value {
        Some(Detail::Text(value) | Detail::Working(value) | Detail::Waiting(value)) => {
            SharedString::from(format!("{title}{state}: {value}"))
        }
        None => SharedString::from(format!("{title}{state}")),
    };
    let id = match menu {
        BarMenu::Windows => "windows",
        BarMenu::Options => "options",
        BarMenu::AutoZone => "auto-zone",
        BarMenu::AutoRelogin => "auto-relogin",
        BarMenu::Script => "script",
        BarMenu::Jump => "jump",
        BarMenu::Environment => "environment",
    };
    div()
        .map(|slot| {
            if shrinks {
                slot.min_w_0()
            } else {
                slot.flex_none()
            }
        })
        .on_children_prepainted(move |bounds, _, _| {
            let mut all = triggers.get();
            all[menu as usize] = bounds[0];
            triggers.set(all);
        })
        .child(
            bar_control(theme, id, enabled)
                .role(Role::Button)
                .aria_label(label)
                .aria_expanded(open)
                .when(shrinks, |trigger| trigger.min_w_0())
                .gap(px(7.0))
                .when(open, |trigger| trigger.bg(theme.fill_pressed))
                .child(
                    div()
                        .flex_none()
                        .when(on == Some(false), |title| {
                            title.text_color(theme.text_secondary)
                        })
                        .child(title),
                )
                .children(value.map(|value| {
                    let waiting = matches!(value, Detail::Waiting(_));
                    match value {
                        Detail::Text(value) => div()
                            .min_w_0()
                            .max_w(px(120.0))
                            .truncate()
                            .text_color(theme.text_secondary)
                            .child(value)
                            .into_any_element(),
                        Detail::Working(label) | Detail::Waiting(label) => Busy {
                            id: SharedString::from(format!("{id}-working")).into(),
                            label,
                            waiting,
                        }
                        .into_any_element(),
                    }
                }))
                .when(enabled, |trigger| {
                    trigger.on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.toggle_menu(menu, window, cx)
                        }),
                    )
                }),
        )
}

pub(super) fn overlay(
    view: &RuffleView,
    menu: BarMenu,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement {
    let theme = cx.theme().clone();
    let anchor = view.bar.triggers.get()[menu as usize];
    let panel = match menu {
        BarMenu::Windows => windows_panel(&theme, cx),
        BarMenu::Options => options_panel(&theme, view, cx),
        BarMenu::AutoZone => zone_panel(&theme, view, cx),
        BarMenu::AutoRelogin => relogin_panel(&theme, view, cx),
        BarMenu::Script => script_panel(&theme, view, cx),
        BarMenu::Jump => travel_panel(&theme, view, cx),
        BarMenu::Environment => environment_panel(&theme, view, cx),
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
            overlay.child(submenu_panel(&theme, view, submenu, at, cx))
        })
}

fn toggle_row(
    theme: &Theme,
    id: &'static str,
    label: &'static str,
    on: bool,
    enabled: bool,
) -> Stateful<Div> {
    press(theme, ui::row(theme, id), enabled)
        .role(Role::Switch)
        .aria_label(label)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .child(div().font_weight(FontWeight::MEDIUM).child(label))
        .child(ui::switch(theme, on))
}

fn press(theme: &Theme, row: Stateful<Div>, enabled: bool) -> Stateful<Div> {
    if enabled {
        ui::pressable(theme, row)
    } else {
        row.opacity(DISABLED_OPACITY)
    }
}

fn choice_row(
    theme: &Theme,
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
) -> Stateful<Div> {
    ui::pressable(theme, choice(theme, id, label, selected))
}

fn choice(
    theme: &Theme,
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
) -> Stateful<Div> {
    ui::row(theme, id)
        .role(Role::MenuItemRadio)
        .justify_start()
        .gap(px(6.0))
        .pl(px(6.0))
        .child(div().flex_none().w(px(12.0)).when(selected, |column| {
            column.child(icon(Icon::Check, theme.text).size(px(12.0)))
        }))
        .child(div().min_w_0().truncate().child(label.into()))
}

fn submenu_row(
    theme: &Theme,
    view: &RuffleView,
    submenu: Submenu,
    label: &'static str,
    value: SharedString,
    cx: &mut Context<RuffleView>,
    enabled: bool,
) -> impl IntoElement + use<> {
    let rows = view.bar.submenu_rows.clone();
    let open = view.bar.submenu.is_some_and(|(open, _)| open == submenu);
    div()
        .on_children_prepainted(move |bounds, _, _| {
            let mut all = rows.get();
            all[submenu as usize] = if enabled {
                bounds[0]
            } else {
                Bounds::default()
            };
            rows.set(all);
        })
        .child(
            press(theme, ui::row(theme, label), enabled)
                .role(Role::MenuItem)
                .aria_expanded(open)
                .when(open, |row| row.bg(theme.fill_pressed))
                .child(div().flex_none().child(label))
                .child(
                    div()
                        .flex()
                        .min_w_0()
                        .items_center()
                        .gap(px(4.0))
                        .text_color(theme.text_secondary)
                        .child(div().min_w_0().truncate().child(value))
                        .child(
                            div()
                                .flex_none()
                                .child(ui::glyph(Glyph::Right, theme.text_secondary)),
                        ),
                )
                .when(enabled, |row| {
                    row.on_click(cx.listener(move |this, _, _, cx| this.open_submenu(submenu, cx)))
                }),
        )
}

fn zone_panel(theme: &Theme, view: &RuffleView, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    let settings = view.automation.auto_zone();
    ui::panel(theme, "auto-zone-panel", px(220.0))
        .child(
            toggle_row(
                theme,
                "auto-zone-enabled",
                "Auto zone",
                settings.enabled,
                true,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.change_auto_zone(|settings| settings.enabled = !settings.enabled, cx)
            })),
        )
        .child(submenu_row(
            theme,
            view,
            Submenu::Rooms,
            "Map",
            settings.room.map_or("None", Room::label).into(),
            cx,
            true,
        ))
}

fn options_panel(theme: &Theme, view: &RuffleView, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    let mut settings = view.automation.game_settings();
    let live = view.in_game();
    let mut cells: Vec<_> = TOGGLES
        .map(|(id, label, setting)| {
            toggle_row(theme, id, label, *setting(&mut settings), live)
                .when(live, |row| {
                    row.on_click(cx.listener(move |this, _, _, cx| {
                        this.change_game_settings(
                            |settings| {
                                let on = setting(settings);
                                *on = !*on;
                            },
                            cx,
                        )
                    }))
                })
                .into_any_element()
        })
        .into();
    cells.insert(
        3,
        submenu_row(
            theme,
            view,
            Submenu::Rendering,
            "Rendering",
            settings.rendering.mode().label().into(),
            cx,
            live,
        )
        .into_any_element(),
    );
    cells.extend(
        Field::MOVEMENT.map(|field| number_chip(theme, view, field, cx).into_any_element()),
    );
    ui::panel(theme, "options-panel", px(392.0))
        .child(ui::section(theme, "Gameplay"))
        .child(grid(cells))
        .child(ui::separator(theme))
        .child(ui::section(theme, "Identity"))
        .child(grid(Field::IDENTITY.map(|field| {
            stacked_field(theme, view, field, cx).into_any_element()
        })))
        .child(ui::separator(theme))
        .child(ui::section(theme, "Actions"))
        .child(grid(
            [
                ("reload-map", "Reload map", Command::ReloadMap),
                ("set-spawnpoint", "Set spawnpoint", Command::SetSpawnPoint),
            ]
            .map(|(id, label, command)| {
                press(theme, ui::row(theme, id), live)
                    .role(Role::MenuItem)
                    .child(label)
                    .when(live, |row| {
                        row.on_click(cx.listener(move |this, _, window, cx| {
                            this.session.command(command.clone());
                            this.close_menu(window, cx);
                        }))
                    })
                    .into_any_element()
            }),
        ))
}

fn grid(cells: impl IntoIterator<Item = gpui::AnyElement>) -> Div {
    div()
        .grid()
        .grid_cols(2)
        .children(cells.into_iter().map(|cell| div().min_w_0().child(cell)))
}

fn script_panel(theme: &Theme, view: &RuffleView, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    let panel = ui::panel(theme, "script-panel", px(264.0))
        .when_some(view.script.status(), |panel, text| {
            panel.child(div().pt(px(6.0)).child(ui::status(theme, text)))
        });
    if view.script.running() {
        return panel.child(
            ui::pressable(theme, ui::row(theme, "script-stop"))
                .role(Role::MenuItem)
                .child("Stop")
                .on_click(cx.listener(|this, _, _, cx| this.stop_script(cx))),
        );
    }
    panel
        .when_some(view.script.loaded(), |panel, name| {
            panel.child(
                ui::pressable(theme, ui::row(theme, "script-start"))
                    .role(Role::MenuItem)
                    .child(format!("Start {name}"))
                    .on_click(cx.listener(|this, _, _, cx| this.start_loaded_script(cx))),
            )
        })
        .child(
            ui::pressable(theme, ui::row(theme, "script-load"))
                .role(Role::MenuItem)
                .child("Load script…")
                .on_click(cx.listener(|this, _, window, cx| this.load_script(window, cx))),
        )
}

const WINDOWS: [&str; 5] = [
    "Combat profiles",
    "Environment",
    "Follower",
    "Loader/grabber",
    "Packets",
];

fn windows_panel(theme: &Theme, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    ui::panel(theme, "windows-panel", px(200.0)).children(WINDOWS.iter().enumerate().map(
        |(index, label)| {
            ui::pressable(theme, ui::row(theme, ("window", index)))
                .role(Role::MenuItem)
                .child(*label)
                .on_click(cx.listener(|this, _, window, cx| this.close_menu(window, cx)))
        },
    ))
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
    theme: &Theme,
    (kind, index): (&'static str, usize),
    label: SharedString,
    detail: Option<SharedString>,
    remove: impl Fn(&mut RuffleView, &mut Context<RuffleView>) + 'static,
    cx: &mut Context<RuffleView>,
) -> Stateful<Div> {
    ui::row(theme, (kind, index))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .gap(px(6.0))
                .overflow_hidden()
                .child(div().truncate().child(label.clone()))
                .children(
                    detail.map(|detail| div().text_color(theme.text_secondary).child(detail)),
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
                .rounded(theme.radius.control)
                .pointer_cursor(theme)
                .hover(|style| style.bg(theme.fill_hover))
                .active(|style| style.bg(theme.fill_pressed))
                .role(Role::Button)
                .aria_label(SharedString::from(format!("Unregister {label}")))
                .child(ui::glyph(Glyph::Close, theme.text_secondary))
                .on_click(cx.listener(move |this, _, _, cx| remove(this, cx))),
        )
}

fn unregister_all_row(
    theme: &Theme,
    id: &'static str,
    clear: fn(&Automation),
    cx: &mut Context<RuffleView>,
) -> Stateful<Div> {
    ui::pressable(theme, ui::row(theme, id))
        .role(Role::MenuItem)
        .text_color(theme.text_secondary)
        .child("Unregister all")
        .on_click(cx.listener(move |this, _, _, cx| {
            clear(&this.automation);
            cx.notify();
        }))
}

fn environment_panel(
    theme: &Theme,
    view: &RuffleView,
    cx: &mut Context<RuffleView>,
) -> Stateful<Div> {
    let quests = view.automation.quests();
    let drops = view.automation.drops();
    let boosts = view.automation.boosts();
    ui::panel(theme, "environment-panel", px(392.0))
        .max_h(px(560.0))
        .overflow_y_scroll()
        .child(ui::section(theme, "Quests"))
        .child(
            toggle_row(
                theme,
                "quests-enabled",
                "Accept and complete",
                quests.config.enabled,
                true,
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
                    toggle_row(theme, id, label, *setting(&mut config), true).on_click(cx.listener(
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
                theme,
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
        .child(field_row(theme, view, Field::AddQuest, cx))
        .when(!quests.quests.is_empty(), |panel| {
            panel.child(unregister_all_row(
                theme,
                "quests-clear",
                |automation| {
                    automation.update_quests(|state| state.quests.clear());
                },
                cx,
            ))
        })
        .child(ui::separator(theme))
        .child(ui::section(theme, "Drops"))
        .child(
            toggle_row(
                theme,
                "drops-enabled",
                "Accept and reject",
                drops.config.enabled,
                true,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.automation
                    .update_drops(|state| state.config.enabled = !state.config.enabled);
                cx.notify();
            })),
        )
        .children(drops.items.iter().enumerate().map(|(index, query)| {
            let removed = query.clone();
            registered_row(
                theme,
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
        .child(field_row(theme, view, Field::AddDrop, cx))
        .when(!drops.items.is_empty(), |panel| {
            panel.child(unregister_all_row(
                theme,
                "drops-clear",
                |automation| {
                    automation.update_drops(|state| state.items.clear());
                },
                cx,
            ))
        })
        .child(ui::section(theme, "Accept unregistered drops"))
        .child(
            div()
                .grid()
                .grid_cols(2)
                .children(DROP_KINDS.map(|(id, label, setting)| {
                    let mut config = drops.config;
                    toggle_row(theme, id, label, *setting(&mut config), true).on_click(cx.listener(
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
                theme,
                "drops-reject",
                "Reject everything else",
                drops.config.reject_others,
                true,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.automation
                    .update_drops(|state| state.config.reject_others = !state.config.reject_others);
                cx.notify();
            })),
        )
        .child(ui::separator(theme))
        .child(ui::section(theme, "Boosts"))
        .child(
            toggle_row(
                theme,
                "boosts-enabled",
                "Use boosts",
                boosts.config.enabled,
                true,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.automation
                    .update_boosts(|state| state.config.enabled = !state.config.enabled);
                cx.notify();
            })),
        )
        .children(boosts.items.iter().enumerate().map(|(index, query)| {
            let removed = query.clone();
            registered_row(
                theme,
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
        .child(field_row(theme, view, Field::AddBoost, cx))
        .when(!boosts.items.is_empty(), |panel| {
            panel.child(unregister_all_row(
                theme,
                "boosts-clear",
                |automation| {
                    automation.update_boosts(|state| state.items.clear());
                },
                cx,
            ))
        })
}

fn travel_panel(theme: &Theme, view: &RuffleView, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    let Some(location) = &view.bar.location else {
        return ui::panel(theme, "travel-panel", px(200.0)).child(note(theme, "Log in to travel"));
    };
    let pressed = theme.fill_pressed;
    let jump_row = |spot: Spot, label: SharedString, selected: bool| {
        let id = match spot.column {
            Column::Cell => "cell",
            Column::Pad => "pad",
        };
        choice(theme, (id, spot.row), label, selected)
            .pointer_cursor(theme)
            .when(view.bar.jump == Some(spot), |row| row.bg(theme.fill_hover))
            .active(move |style| style.bg(pressed))
            .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| {
                if this.bar.jump != Some(spot) {
                    this.bar.jump = Some(spot);
                    cx.notify();
                }
            }))
    };
    let cells: Vec<_> = location
        .cells
        .iter()
        .enumerate()
        .map(|(row, cell)| {
            let (cell, pad) = (cell.clone(), location.pad.clone());
            let spot = Spot {
                column: Column::Cell,
                row,
            };
            jump_row(spot, cell.clone().into(), cell == location.cell).on_click(cx.listener(
                move |this, event: &ClickEvent, window, cx| {
                    let keep_open = event.modifiers().shift;
                    this.travel(cell.clone(), pad.clone(), keep_open, window, cx)
                },
            ))
        })
        .collect();
    let pads: Vec<_> = PADS
        .iter()
        .enumerate()
        .map(|(row, pad)| {
            let in_cell = location
                .cell_pads
                .iter()
                .any(|known| known.eq_ignore_ascii_case(pad));
            let cell = location.cell.clone();
            let spot = Spot {
                column: Column::Pad,
                row,
            };
            jump_row(spot, (*pad).into(), location.pad.eq_ignore_ascii_case(pad))
                .when(!in_cell, |row| row.text_color(theme.text_tertiary))
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    let keep_open = event.modifiers().shift;
                    this.travel(cell.clone(), pad.to_string(), keep_open, window, cx)
                }))
        })
        .collect();
    let column = |label| {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .child(ui::section(theme, label))
    };
    ui::panel(theme, "travel-panel", px(300.0))
        .flex_row()
        .child(
            column("Cell").child(
                div()
                    .relative()
                    .child(
                        div()
                            .id("travel-cells")
                            .flex()
                            .flex_col()
                            .max_h(ROW * VISIBLE_CELLS)
                            .when(location.cells.len() as f32 > VISIBLE_CELLS, |list| {
                                list.pr(Scrollbar::GUTTER + SCROLLBAR_CLEARANCE)
                            })
                            .overflow_y_scroll()
                            .track_scroll(&view.bar.cells_scroll)
                            .when(cells.is_empty(), |list| list.child(note(theme, "No cells")))
                            .children(cells),
                    )
                    .child(Scrollbar::new(
                        "travel-cells-scrollbar",
                        view.bar.cells_scroll.clone(),
                    )),
            ),
        )
        .child(
            div()
                .flex_none()
                .w(px(1.0))
                .mx(PANEL_INSET)
                .bg(theme.hairline),
        )
        .child(column("Pad").children(pads))
}

fn note(theme: &Theme, text: &'static str) -> impl IntoElement {
    div()
        .px(px(8.0))
        .py(px(6.0))
        .text_size(TEXT_SMALL)
        .text_color(theme.text_tertiary)
        .child(text)
}

fn relogin_panel(theme: &Theme, view: &RuffleView, cx: &mut Context<RuffleView>) -> Stateful<Div> {
    let settings = view.automation.auto_relogin();
    ui::panel(theme, "auto-relogin-panel", px(264.0))
        .child(
            toggle_row(
                theme,
                "auto-relogin-enabled",
                "Auto relogin",
                settings.enabled,
                true,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.change_auto_relogin(|settings| settings.enabled = !settings.enabled, cx)
            })),
        )
        .when_some(status(&view.auto_relogin), |panel, text| {
            panel.child(ui::status(theme, text))
        })
        .child(ui::separator(theme))
        .child(submenu_row(
            theme,
            view,
            Submenu::Servers,
            "Server",
            settings
                .server
                .clone()
                .unwrap_or_else(|| LAST_SERVER.into())
                .into(),
            cx,
            true,
        ))
        .child(ui::separator(theme))
        .child(ui::section(theme, "Login attempts"))
        .children(Field::RELOGIN.map(|field| field_row(theme, view, field, cx)))
}

fn field_row(
    theme: &Theme,
    view: &RuffleView,
    field: Field,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement + use<> {
    let name = field.label();
    ui::row(theme, SharedString::from(format!("{name}-row")))
        .child(name)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(field_input(theme, view, field, ui::Shape::Box, cx))
                .child(
                    div()
                        .w(px(8.0))
                        .text_color(theme.text_tertiary)
                        .child(field.unit()),
                ),
        )
}

fn number_chip(
    theme: &Theme,
    view: &RuffleView,
    field: Field,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement + use<> {
    ui::row(theme, SharedString::from(format!("{}-chip", field.label())))
        .pr(px(2.0))
        .when(!view.in_game(), |row| row.opacity(DISABLED_OPACITY))
        .child(div().font_weight(FontWeight::MEDIUM).child(field.label()))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(2.0))
                .child(reset_button(theme, view, field, cx))
                .child(field_input(theme, view, field, ui::Shape::Pill, cx)),
        )
}

fn stacked_field(
    theme: &Theme,
    view: &RuffleView,
    field: Field,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement + use<> {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .px(px(8.0))
        .when(!view.in_game(), |field| field.opacity(DISABLED_OPACITY))
        .py(px(4.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(field.label())
                .child(reset_button(theme, view, field, cx)),
        )
        .child(field_input(theme, view, field, ui::Shape::Box, cx).w_full())
}

fn reset_button(
    theme: &Theme,
    view: &RuffleView,
    field: Field,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement + use<> {
    let enabled = !field.is_default(&view.automation) && view.field_enabled(field);
    let button = div()
        .id(SharedString::from(format!("{}-reset", field.label())))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(20.0))
        .rounded(theme.radius.control)
        .role(Role::Button)
        .aria_label(format!("Reset {}", field.label().to_lowercase()))
        .child(
            icon(
                Icon::RotateCcw,
                if enabled {
                    theme.text_secondary
                } else {
                    theme.text_disabled
                },
            )
            .size(px(12.0)),
        );
    if !enabled {
        return button;
    }
    ui::pressable(theme, button).on_click(cx.listener(move |this, _, _, cx| {
        this.bar.edit = None;
        field.reset(this, cx);
    }))
}

fn field_input(
    theme: &Theme,
    view: &RuffleView,
    field: Field,
    shape: ui::Shape,
    cx: &mut Context<RuffleView>,
) -> Stateful<Div> {
    let edit = view.bar.edit.as_ref().filter(|edit| edit.field == field);
    let editing = edit.is_some();
    let fresh = edit.is_some_and(|edit| edit.fresh);
    let text = edit.map_or_else(|| field.value(&view.automation), |edit| edit.text.clone());
    let name = field.label();
    let id = SharedString::from(format!("{name}-field"));
    let input = if field.is_text() {
        let identity = view.bar.identity.as_ref();
        let placeholder = match field {
            Field::CustomName => identity
                .map(|identity| identity.name.clone())
                .or_else(|| view.username().map(str::to_owned)),
            Field::CustomGuild => identity.and_then(|identity| identity.guild.clone()),
            _ => Some(field.placeholder().to_owned()),
        };
        ui::text_field(
            theme,
            id,
            text,
            placeholder.unwrap_or_default(),
            editing,
            fresh,
        )
        .role(Role::TextInput)
    } else {
        ui::number_field(theme, id, text, editing, fresh, shape).role(Role::SpinButton)
    }
    .when(!view.field_enabled(field), |input| input.cursor_default())
    .when(!field.editable(&view.automation), |input| {
        input
            .text_color(theme.text_tertiary)
            .cursor_default()
            .aria_description("Minimal rendering holds the frame rate at 2")
    });
    input.aria_label(name).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.commit_edit(cx);
            this.start_edit(field);
            cx.notify();
        }),
    )
}

fn submenu_panel(
    theme: &Theme,
    view: &RuffleView,
    submenu: Submenu,
    at: Point<Pixels>,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement {
    let panel = ui::panel(theme, "bar-submenu", SUBMENU_WIDTH)
        .max_h(SUBMENU_MAX_HEIGHT)
        .overflow_y_scroll();
    let panel = match submenu {
        Submenu::Rooms => {
            let selected = view.automation.auto_zone().room;
            panel.children(Room::ALL.map(|room| {
                choice_row(theme, room.label(), room.label(), selected == Some(room)).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.change_auto_zone(|settings| settings.room = Some(room), cx);
                        this.close_submenu();
                    }),
                )
            }))
        }
        Submenu::Servers => servers(theme, view, panel, cx),
        Submenu::Rendering => {
            let selected = view.automation.game_settings().rendering.mode();
            panel.children(RenderingMode::ALL.map(|mode| {
                choice_row(theme, mode.label(), mode.label(), selected == mode).on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.change_game_settings(|settings| settings.rendering.set(mode), cx);
                    }),
                )
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

fn servers(
    theme: &Theme,
    view: &RuffleView,
    panel: Stateful<Div>,
    cx: &mut Context<RuffleView>,
) -> Stateful<Div> {
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
        })
    };
    panel
        .child(
            choice_row(theme, "server-last", LAST_SERVER, saved.is_none()).on_click(choose(None)),
        )
        .child(if view.servers.is_empty() {
            note(theme, "Log in to list the other servers").into_any_element()
        } else {
            ui::separator(theme).into_any_element()
        })
        .children(names.into_iter().enumerate().map(|(index, name)| {
            let selected = saved.as_deref() == Some(name.as_str());
            choice_row(theme, ("server", index), name.clone(), selected)
                .on_click(choose(Some(name)))
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

    fn press(location: &Location, at: Option<Spot>, key: &'static str) -> Option<JumpKey> {
        let event = KeyDownEvent {
            keystroke: gpui::Keystroke::parse(key).unwrap(),
            is_held: false,
            prefer_character_input: false,
        };
        jump_key(location, at, &event)
    }

    #[test]
    fn jump_moves_between_cells_and_pads_from_the_keyboard() {
        let location = Location {
            cell: "r2".into(),
            pad: "Left".into(),
            cells: vec!["Enter".into(), "r2".into(), "r3".into()],
            cell_pads: vec!["Left".into()],
        };
        let spot = |column, row| Some(Spot { column, row });
        let moved = |column, row| Some(JumpKey::Move(Spot { column, row }));
        assert_eq!(press(&location, None, "down"), moved(Column::Cell, 1));
        assert_eq!(
            press(&location, spot(Column::Cell, 2), "down"),
            moved(Column::Cell, 0)
        );
        assert_eq!(
            press(&location, spot(Column::Cell, 2), "right"),
            moved(Column::Pad, 2)
        );
        assert_eq!(
            press(&location, spot(Column::Pad, 7), "left"),
            moved(Column::Cell, 2)
        );
        assert_eq!(
            press(&location, spot(Column::Cell, 1), "tab"),
            moved(Column::Pad, 1)
        );
        assert_eq!(
            press(&location, spot(Column::Cell, 0), "r"),
            moved(Column::Cell, 1)
        );
        assert_eq!(
            press(&location, spot(Column::Cell, 2), "enter"),
            Some(JumpKey::Travel {
                cell: "r3".into(),
                pad: "Left".into(),
                keep_open: false,
            })
        );
        assert_eq!(
            press(&location, spot(Column::Cell, 2), "shift-enter"),
            Some(JumpKey::Travel {
                cell: "r3".into(),
                pad: "Left".into(),
                keep_open: true,
            })
        );
        assert_eq!(
            press(&location, spot(Column::Pad, 4), "space"),
            Some(JumpKey::Travel {
                cell: "r2".into(),
                pad: "Top".into(),
                keep_open: false,
            })
        );
        assert_eq!(
            press(&location, spot(Column::Pad, 4), "shift-space"),
            Some(JumpKey::Travel {
                cell: "r2".into(),
                pad: "Top".into(),
                keep_open: true,
            })
        );
        assert_eq!(press(&location, spot(Column::Cell, 0), "x"), None);
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
