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
use crate::automation::{
    auth::auto_relogin::{Phase, Settings},
    world::auto_zone::Room,
};

const LAST_SERVER: &str = "Last server";
const MAX_DIGITS: usize = 3;
const SUBMENU_WIDTH: Pixels = px(200.0);
const SUBMENU_MAX_HEIGHT: Pixels = px(360.0);
const SUBMENU_OVERLAP: Pixels = px(2.0);
const BORDER: Pixels = px(1.0);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BarMenu {
    AutoZone,
    AutoRelogin,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Submenu {
    Rooms,
    Servers,
}

impl BarMenu {
    fn submenu(self) -> Submenu {
        match self {
            BarMenu::AutoZone => Submenu::Rooms,
            BarMenu::AutoRelogin => Submenu::Servers,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Delay,
    Retries,
    Backoff,
    BackoffCap,
}

impl Field {
    const ALL: [Field; 4] = [
        Field::Delay,
        Field::Retries,
        Field::Backoff,
        Field::BackoffCap,
    ];

    fn label(self) -> &'static str {
        match self {
            Field::Delay => "Delay",
            Field::Retries => "Retries",
            Field::Backoff => "Backoff",
            Field::BackoffCap => "Backoff cap",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Field::Retries => "",
            Field::Delay | Field::Backoff | Field::BackoffCap => "s",
        }
    }

    fn range(self, settings: &Settings) -> (u32, u32) {
        match self {
            Field::Delay => (0, Settings::MAX_DELAY_SECONDS),
            Field::Retries => (0, Settings::MAX_RETRIES),
            Field::Backoff => (1, Settings::MAX_BACKOFF_SECONDS),
            Field::BackoffCap => (settings.backoff_seconds, Settings::MAX_BACKOFF_SECONDS),
        }
    }

    fn get(self, settings: &Settings) -> u32 {
        match self {
            Field::Delay => settings.delay_seconds,
            Field::Retries => settings.retries,
            Field::Backoff => settings.backoff_seconds,
            Field::BackoffCap => settings.backoff_cap_seconds,
        }
    }

    fn set(self, settings: &mut Settings, value: u32) {
        let (min, max) = self.range(settings);
        let value = value.clamp(min, max);
        match self {
            Field::Delay => settings.delay_seconds = value,
            Field::Retries => settings.retries = value,
            Field::Backoff => settings.backoff_seconds = value,
            Field::BackoffCap => settings.backoff_cap_seconds = value,
        }
    }

    fn after(self, backwards: bool) -> Field {
        let index = Field::ALL.iter().position(|field| *field == self).unwrap();
        let count = Field::ALL.len();
        Field::ALL[if backwards {
            (index + count - 1) % count
        } else {
            (index + 1) % count
        }]
    }
}

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
    focus: FocusHandle,
    triggers: Rc<Cell<[Bounds<Pixels>; 2]>>,
    submenu_rows: Rc<Cell<[Bounds<Pixels>; 2]>>,
    submenu_panel: Rc<Cell<Bounds<Pixels>>>,
}

impl BarState {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            open: None,
            submenu: None,
            aim_from: None,
            edit: None,
            focus: cx.focus_handle(),
            triggers: Default::default(),
            submenu_rows: Default::default(),
            submenu_panel: Default::default(),
        }
    }

    pub(super) fn open(&self) -> Option<BarMenu> {
        self.open
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

    fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.bar.open = None;
        self.bar.submenu = None;
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
                self.start_edit(field.after(shift));
            }
            "up" | "down" => {
                let stored = i64::from(field.get(self.automation.auto_relogin()));
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
            digit if digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()) => {
                let edit = self.bar.edit.as_mut().unwrap();
                if edit.fresh {
                    edit.text.clear();
                    edit.fresh = false;
                }
                if edit.text.len() < MAX_DIGITS {
                    edit.text.push_str(digit);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn start_edit(&mut self, field: Field) {
        let value = field.get(self.automation.auto_relogin());
        self.bar.edit = Some(Edit {
            field,
            text: value.to_string(),
            fresh: true,
        });
    }

    fn commit_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.bar.edit.take() else {
            return;
        };
        if let Ok(value) = edit.text.parse() {
            self.change_auto_relogin(|settings| edit.field.set(settings, value), cx);
        }
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

    fn close_submenu(&mut self) {
        self.bar.submenu = None;
        self.bar.aim_from = None;
    }

    fn panel_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.bar.open else {
            return;
        };
        let at = event.position;
        let submenu = menu.submenu();
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
        .child(trigger(
            view,
            BarMenu::AutoZone,
            "Auto zone",
            zone_value(view),
            cx,
        ))
        .child(trigger(
            view,
            BarMenu::AutoRelogin,
            "Auto relogin",
            relogin_value(view),
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
    value: SharedString,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement {
    let triggers = view.bar.triggers.clone();
    let open = view.bar.open == Some(menu);
    let id = match menu {
        BarMenu::AutoZone => "auto-zone",
        BarMenu::AutoRelogin => "auto-relogin",
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
                .aria_label(SharedString::from(format!("{title}: {value}")))
                .aria_expanded(open)
                .gap(px(7.0))
                .when(open, |trigger| trigger.bg(ui::pressed()))
                .child(title)
                .child(div().text_color(ui::secondary_text()).child(value))
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
        BarMenu::AutoZone => zone_panel(view, cx),
        BarMenu::AutoRelogin => relogin_panel(view, cx),
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
        .children(Field::ALL.map(|field| field_row(view, field, cx)))
}

fn field_row(
    view: &RuffleView,
    field: Field,
    cx: &mut Context<RuffleView>,
) -> impl IntoElement + use<> {
    let settings = view.automation.auto_relogin();
    let edit = view.bar.edit.as_ref().filter(|edit| edit.field == field);
    let text = edit.map_or_else(|| field.get(settings).to_string(), |edit| edit.text.clone());
    let name = field.label();
    ui::row(SharedString::from(format!("{name}-row")))
        .child(name)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    ui::number_field(
                        SharedString::from(format!("{name}-field")),
                        text,
                        edit.is_some(),
                        edit.is_some_and(|edit| edit.fresh),
                    )
                    .role(Role::SpinButton)
                    .aria_label(name)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.commit_edit(cx);
                            this.start_edit(field);
                            cx.notify();
                        }),
                    ),
                )
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
            div()
                .px(px(8.0))
                .py(px(6.0))
                .text_size(ui::SMALL_TEXT_SIZE)
                .text_color(ui::tertiary_text())
                .child("Log in to list the other servers")
                .into_any_element()
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
