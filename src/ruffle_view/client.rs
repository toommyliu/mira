use std::{cell::Cell, path::PathBuf, rc::Rc, time::Duration};

use anyhow::Context as _;

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, Modifiers, ModifiersChangedEvent, MouseButton, MouseDownEvent,
    ParentElement as _, Pixels, Render, Role, ScrollHandle, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Toggled, Window, actions,
    anchored, deferred, div, point, prelude::FluentBuilder as _, px,
};

use super::{
    Changed, RuffleView, Summary,
    manager::OpenAccounts,
    ui::{self, Glyph},
};
use crate::{
    accounts::{Account, AccountId},
    army::Armies,
    ruffle::{Inbox, Msg},
};

actions!(
    mira,
    [NewSession, CloseSession, NextSession, PreviousSession]
);

#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = mira, no_json)]
struct SelectSession(usize);

pub(crate) const STRIP_HEIGHT: Pixels = px(36.0);
pub(super) const TRAFFIC_LIGHTS_WIDTH: Pixels = px(78.0);
const SHORTCUT_HINT_DELAY: Duration = Duration::from_millis(400);

fn fully_visible(viewport: (f32, f32), scrolled: f32, tabs: &[(f32, f32)]) -> Vec<usize> {
    tabs.iter()
        .enumerate()
        .filter(|(_, (left, right))| {
            left + scrolled >= viewport.0 && right + scrolled <= viewport.1
        })
        .map(|(index, _)| index)
        .take(9)
        .collect()
}

fn shortcut_for(shortcuts: &[usize], index: usize) -> Option<usize> {
    shortcuts
        .iter()
        .position(|&tab| tab == index)
        .map(|position| position + 1)
}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("cmd-t", NewSession, None),
        gpui::KeyBinding::new("cmd-w", CloseSession, None),
        gpui::KeyBinding::new("cmd-}", NextSession, None),
        gpui::KeyBinding::new("cmd-{", PreviousSession, None),
        gpui::KeyBinding::new("ctrl-tab", NextSession, None),
        gpui::KeyBinding::new("ctrl-shift-tab", PreviousSession, None),
        gpui::KeyBinding::new("cmd-shift-a", OpenAccounts, None),
    ]);
    cx.bind_keys((1..=9).map(|number| {
        gpui::KeyBinding::new(&format!("cmd-{number}"), SelectSession(number - 1), None)
    }));
}

pub struct Client {
    data: PathBuf,
    device: reqwest::header::HeaderValue,
    armies: Armies,
    sessions: Vec<Session>,
    active: usize,
    layout: Layout,
    numbered: usize,
    focus: FocusHandle,
    tab_scroll: ScrollHandle,
    tab_list: Option<FocusHandle>,
    tab_list_trigger: Rc<Cell<Bounds<Pixels>>>,
    shortcut_hints: bool,
    shortcut_hint_delay: Option<Task<()>>,
    _activation: Subscription,
}

struct Session {
    number: usize,
    label: SharedString,
    account: Option<AccountId>,
    view: Entity<RuffleView>,
    player: Inbox,
    _changes: Subscription,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    Tabs,
    Grid,
}

fn grid_shape(count: usize) -> (usize, usize) {
    let columns = (count as f64).sqrt().ceil().max(1.0) as usize;
    (columns, count.div_ceil(columns).max(1))
}

impl Client {
    pub(crate) fn new(
        data: PathBuf,
        device: reqwest::header::HeaderValue,
        armies: Armies,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<Entity<Client>> {
        let client = cx.new(|cx| Client {
            data,
            device,
            armies,
            sessions: Vec::new(),
            active: 0,
            layout: Layout::Tabs,
            numbered: 0,
            focus: cx.focus_handle(),
            tab_scroll: ScrollHandle::new(),
            tab_list: None,
            tab_list_trigger: Rc::default(),
            shortcut_hints: false,
            shortcut_hint_delay: None,
            _activation: cx.observe_window_activation(window, |this: &mut Self, window, cx| {
                if !window.is_window_active() {
                    this.hide_shortcut_hints(cx);
                }
            }),
        });
        client.update(cx, |client, cx| client.open_session(None, window, cx))?;
        Ok(client)
    }

    fn open_session(
        &mut self,
        account: Option<&Account>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<Entity<RuffleView>> {
        let view = RuffleView::new(
            self.data.clone(),
            self.device.clone(),
            self.armies.clone(),
            window,
            cx,
        )?;
        let window_size = window.viewport_size();
        let top = STRIP_HEIGHT + super::BAR_HEIGHT;
        let player = Bounds::new(
            point(px(0.0), top),
            gpui::size(
                window_size.width.max(px(1.0)),
                (window_size.height - top).max(px(1.0)),
            ),
        );
        view.update(cx, |view, _| view.layout(player, window));
        let player = view.read(cx).session.inbox();
        self.numbered += 1;
        self.sessions.push(Session {
            number: self.numbered,
            label: account.map_or_else(
                || format!("Session {}", self.numbered).into(),
                |account| account.name.clone().into(),
            ),
            account: account.map(Account::id),
            view: view.clone(),
            player,
            _changes: cx.subscribe(&view, |_, _, _: &Changed, cx| cx.notify()),
        });
        self.active = self.sessions.len() - 1;
        self.show(window, cx);
        Ok(view)
    }

    fn session_of(&self, account: &AccountId) -> Option<usize> {
        self.sessions
            .iter()
            .position(|session| session.account.as_ref() == Some(account))
    }

    pub(crate) fn summary(&self, account: &AccountId, cx: &App) -> Option<Summary> {
        let index = self.session_of(account)?;
        Some(self.sessions[index].view.read(cx).summary())
    }

    pub(crate) fn launch(
        &mut self,
        account: &Account,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        match self.session_of(&account.id()) {
            Some(index) => {
                self.sessions[index]
                    .view
                    .update(cx, |view, cx| view.retry_sign_in(account, cx));
                Ok(())
            }
            None => self.open_account(account, account.script.clone(), window, cx),
        }
    }

    pub(crate) fn run_script(
        &mut self,
        account: &Account,
        script: PathBuf,
        replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        match self.session_of(&account.id()) {
            Some(index) => {
                self.sessions[index].view.update(cx, |view, cx| {
                    view.retry_sign_in(account, cx);
                    view.queue_script(script, replace, cx);
                });
                Ok(())
            }
            None => self.open_account(account, Some(script), window, cx),
        }
    }

    fn open_account(
        &mut self,
        account: &Account,
        script: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let view = self
            .open_session(Some(account), window, cx)
            .with_context(|| format!("couldn't open {}", account.name))?;
        view.update(cx, |view, cx| view.sign_in(account, script, cx));
        Ok(())
    }

    pub(crate) fn stop_script(&mut self, account: &AccountId, cx: &mut Context<Self>) {
        if let Some(index) = self.session_of(account) {
            self.sessions[index]
                .view
                .update(cx, |view, cx| view.cancel_script(cx));
        }
    }

    pub(crate) fn show_account(
        &mut self,
        account: &AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.session_of(account) {
            self.select(index, window, cx);
            window.activate_window();
        }
    }

    pub(crate) fn close_account(
        &mut self,
        account: &AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.session_of(account) {
            self.close_session(index, window, cx);
        }
    }

    fn close_session(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.sessions.len() {
            return;
        }
        self.sessions.remove(index);
        if index < self.active || self.active == self.sessions.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.show(window, cx);
    }

    fn select(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.sessions.len() {
            self.active = index;
            self.show(window, cx);
        }
    }

    fn set_layout(&mut self, layout: Layout, window: &mut Window, cx: &mut Context<Self>) {
        self.layout = layout;
        self.show(window, cx);
    }

    fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tiled = self.layout == Layout::Grid;
        for (index, session) in self.sessions.iter().enumerate() {
            let active = index == self.active;
            session
                .view
                .update(cx, |view, cx| view.present(tiled, active, cx));
        }
        match self.sessions.get(self.active) {
            Some(session) => {
                self.tab_scroll.scroll_to_item(self.active);
                window.focus(&session.view.focus_handle(cx), cx)
            }
            None => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    fn move_session(&mut self, number: usize, to: usize, cx: &mut Context<Self>) {
        let Some(from) = self.sessions.iter().position(|s| s.number == number) else {
            return;
        };
        let to = to.min(self.sessions.len() - 1);
        if from == to {
            return;
        }
        let active = self.sessions[self.active].number;
        let session = self.sessions.remove(from);
        self.sessions.insert(to, session);
        self.active = self
            .sessions
            .iter()
            .position(|s| s.number == active)
            .unwrap_or(0);
        cx.notify();
    }

    fn modifiers_changed(&mut self, modifiers: Modifiers, cx: &mut Context<Self>) {
        if modifiers != Modifiers::command() {
            self.hide_shortcut_hints(cx);
        } else if self.shortcut_hint_delay.is_none() {
            self.shortcut_hint_delay = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SHORTCUT_HINT_DELAY).await;
                let _ = this.update(cx, |this: &mut Self, cx| {
                    this.shortcut_hints = true;
                    cx.notify();
                });
            }));
        }
    }

    fn hide_shortcut_hints(&mut self, cx: &mut Context<Self>) {
        self.shortcut_hint_delay = None;
        if std::mem::take(&mut self.shortcut_hints) {
            cx.notify();
        }
    }

    fn shortcut_tabs(&self) -> Vec<usize> {
        let viewport = self.tab_scroll.bounds();
        let tabs: Vec<_> = (0..self.sessions.len())
            .map_while(|index| self.tab_scroll.bounds_for_item(index))
            .map(|tab| (f32::from(tab.left()), f32::from(tab.right())))
            .collect();
        fully_visible(
            (f32::from(viewport.left()), f32::from(viewport.right())),
            f32::from(self.tab_scroll.offset().x),
            &tabs,
        )
    }

    fn step(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.sessions.len() as isize;
        if count > 1 {
            let index = (self.active as isize + by).rem_euclid(count) as usize;
            self.select(index, window, cx);
        }
    }

    fn strip(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let shortcuts = self.shortcut_tabs();
        let tabs: Vec<_> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(index, session)| {
                let hint = self
                    .shortcut_hints
                    .then(|| shortcut_for(&shortcuts, index))
                    .flatten();
                self.tab(index, session, hint, cx)
            })
            .collect();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.0))
            .w_full()
            .h(STRIP_HEIGHT)
            // The window draws a 1px highlight over the strip's top row.
            .pt(px(1.0))
            .pr(px(6.0))
            .bg(ui::bar_background())
            .border_b_1()
            .border_color(ui::hairline())
            .text_size(ui::TEXT_SIZE)
            .text_color(ui::text())
            .on_drop(cx.listener(|this, tab: &DraggedTab, _, cx| {
                this.move_session(tab.number, usize::MAX, cx)
            }))
            .child(drag_area().flex_none().w(TRAFFIC_LIGHTS_WIDTH))
            .child(
                div()
                    .id("tabs")
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .min_w_0()
                    .overflow_x_scroll()
                    .track_scroll(&self.tab_scroll)
                    .children(tabs),
            )
            .child(
                control("new-session")
                    .w(ui::ROW_HEIGHT)
                    .justify_center()
                    .role(Role::Button)
                    .aria_label("New session")
                    .child(ui::glyph(Glyph::Plus, ui::secondary_text()))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Err(error) = this.open_session(None, window, cx) {
                            tracing::error!(%error, "couldn't open a session");
                        }
                    })),
            )
            .child(self.tab_list_trigger(cx))
            .child(drag_area().flex_1())
            .child(
                control("accounts")
                    .role(Role::Button)
                    .text_color(ui::secondary_text())
                    .child("Accounts")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenAccounts), cx)),
            )
            .child(self.grid_toggle(cx))
    }

    fn tab_list_trigger(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let bounds = Rc::clone(&self.tab_list_trigger);
        let open = self.tab_list.is_some();
        div()
            .on_children_prepainted(move |children, _, _| bounds.set(children[0]))
            .child(
                control("tab-list")
                    .w(ui::ROW_HEIGHT)
                    .justify_center()
                    .role(Role::Button)
                    .aria_label("All sessions")
                    .aria_expanded(open)
                    .when(open, |trigger| trigger.bg(ui::pressed()))
                    .tooltip(ui::tooltip("All sessions"))
                    .child(ui::glyph(Glyph::Down, ui::secondary_text()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            if this.tab_list.is_some() {
                                this.close_tab_list(window, cx);
                            } else {
                                let focus = cx.focus_handle();
                                window.focus(&focus, cx);
                                this.tab_list = Some(focus);
                                cx.notify();
                            }
                        }),
                    ),
            )
    }

    fn close_tab_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab_list.take().is_some() {
            self.show(window, cx);
        }
    }

    fn tab_list(&self, focus: &FocusHandle, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let anchor = self.tab_list_trigger.get();
        let trigger = Rc::clone(&self.tab_list_trigger);
        let shortcuts = self.shortcut_tabs();
        let rows: Vec<_> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(index, session)| {
                let active = index == self.active;
                let label = session.label.clone();
                ui::pressable(ui::row(("tab-list-item", session.number)))
                    .role(Role::MenuItemRadio)
                    .aria_label(label.clone())
                    .aria_toggled(if active {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap(px(8.0))
                            .child(div().w(px(12.0)).flex_none().when(active, |check| {
                                check.child(ui::glyph(Glyph::Check, ui::text()))
                            }))
                            .child(div().truncate().child(label)),
                    )
                    .children(shortcut_for(&shortcuts, index).map(|number| {
                        div()
                            .flex_none()
                            .text_size(ui::SMALL_TEXT_SIZE)
                            .text_color(ui::tertiary_text())
                            .child(format!("⌘{number}"))
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.tab_list = None;
                        this.select(index, window, cx)
                    }))
            })
            .collect();
        deferred(
            anchored()
                .position(point(anchor.left(), anchor.bottom() + px(6.0)))
                .snap_to_window_with_margin(px(8.0))
                .child(
                    ui::panel("tab-list-panel", px(220.0))
                        .role(Role::Menu)
                        .aria_label("All sessions")
                        .max_h(px(360.0))
                        .overflow_y_scroll()
                        .track_focus(focus)
                        .children(rows)
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                            if event.keystroke.key == "escape" {
                                cx.stop_propagation();
                                this.close_tab_list(window, cx);
                            }
                        }))
                        .on_mouse_down_out(cx.listener(
                            move |this, event: &MouseDownEvent, window, cx| {
                                if !trigger.get().contains(&event.position) {
                                    this.close_tab_list(window, cx);
                                }
                            },
                        )),
                ),
        )
        .with_priority(1)
    }

    fn tab(
        &self,
        index: usize,
        session: &Session,
        hint: Option<usize>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = index == self.active;
        let label = session.label.clone();
        control(("tab", session.number))
            .group("tab")
            .role(Role::Tab)
            .aria_label(label.clone())
            .aria_selected(active)
            .gap(px(6.0))
            .pr(px(4.0))
            .flex_shrink(1.0)
            .min_w(px(112.0))
            .max_w(px(180.0))
            .text_color(if active {
                ui::text()
            } else {
                ui::secondary_text()
            })
            .when(active, |tab| {
                tab.bg(ui::pressed()).font_weight(gpui::FontWeight::MEDIUM)
            })
            .child(div().flex_1().truncate().child(label.clone()))
            .children(hint.map(shortcut_badge))
            .when(hint.is_none(), |tab| {
                tab.child(
                    div()
                        .id(("close-session", session.number))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(18.0))
                        .rounded(px(4.0))
                        .cursor_pointer()
                        .when(!active, |close| {
                            close
                                .invisible()
                                .group_hover("tab", |close| close.visible())
                        })
                        .hover(|close| close.bg(ui::hover()))
                        .active(|close| close.bg(ui::pressed()))
                        .role(Role::Button)
                        .aria_label("Close session")
                        .child(ui::glyph(Glyph::Close, ui::secondary_text()))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_session(index, window, cx)
                        })),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| this.select(index, window, cx)))
            .on_drag(
                DraggedTab {
                    number: session.number,
                    label,
                    active,
                },
                |tab, _, _, cx| cx.new(|_| tab.clone()),
            )
            .drag_over::<DraggedTab>(|style, _, _, _| style.bg(ui::hover()))
            .on_drop(cx.listener(move |this, tab: &DraggedTab, _, cx| {
                this.move_session(tab.number, index, cx)
            }))
    }

    fn grid_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let on = self.layout == Layout::Grid;
        control("grid")
            .w(ui::ROW_HEIGHT)
            .justify_center()
            .role(Role::Button)
            .aria_label("Grid view")
            .aria_toggled(if on { Toggled::True } else { Toggled::False })
            .when(on, |toggle| toggle.bg(ui::pressed()))
            .tooltip(ui::tooltip("Grid view"))
            .child(ui::glyph(
                Glyph::Grid,
                if on { ui::text() } else { ui::secondary_text() },
            ))
            .on_click(cx.listener(move |this, _, window, cx| {
                let layout = if on { Layout::Tabs } else { Layout::Grid };
                this.set_layout(layout, window, cx)
            }))
    }

    fn tile(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let highlight = self.sessions.len() > 1 && index == self.active;
        div()
            .id(("tile", self.sessions[index].number))
            .flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .border_1()
            .border_color(if highlight {
                ui::accent()
            } else {
                ui::bar_background()
            })
            .child(self.sessions[index].view.clone())
            .on_any_mouse_down(cx.listener(move |this, _, window, cx| {
                if this.active != index {
                    this.select(index, window, cx);
                }
            }))
    }

    fn grid(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (columns, rows) = grid_shape(self.sessions.len());
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .min_h_0()
            .children((0..rows).map(|row| {
                div()
                    .flex()
                    .w_full()
                    .min_h_0()
                    .flex_basis(px(0.0))
                    .flex_grow(1.0)
                    .children((0..columns).map(|column| {
                        let index = row * columns + column;
                        let cell = div()
                            .flex()
                            .h_full()
                            .min_w_0()
                            .flex_basis(px(0.0))
                            .flex_grow(1.0);
                        if index < self.sessions.len() {
                            cell.child(self.tile(index, cx))
                        } else {
                            cell.bg(ui::bar_background())
                        }
                    }))
            }))
    }

    fn empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .bg(ui::bar_background())
            .text_size(ui::TEXT_SIZE)
            .text_color(ui::secondary_text())
            .child("No sessions are open.")
            .child(
                control("open-session")
                    .mt(px(8.0))
                    .px(px(12.0))
                    .border_1()
                    .border_color(ui::hairline())
                    .text_color(ui::text())
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .role(Role::Button)
                    .child("New session")
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Err(error) = this.open_session(None, window, cx) {
                            tracing::error!(%error, "couldn't open a session");
                        }
                    })),
            )
    }
}

#[derive(Clone)]
struct DraggedTab {
    number: usize,
    label: SharedString,
    active: bool,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .h(px(26.0))
            .px(px(10.0))
            .rounded(ui::CONTROL_RADIUS)
            .bg(ui::panel_background())
            .border_1()
            .border_color(ui::hairline())
            .text_size(ui::TEXT_SIZE)
            .text_color(if self.active {
                ui::text()
            } else {
                ui::secondary_text()
            })
            .child(self.label.clone())
    }
}

fn shortcut_badge(number: usize) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(18.0))
        .px(px(5.0))
        .rounded(px(4.0))
        .bg(ui::hover())
        .text_size(ui::SMALL_TEXT_SIZE)
        .text_color(ui::secondary_text())
        .child(format!("⌘{number}"))
}

fn control(id: impl Into<gpui::ElementId>) -> Stateful<gpui::Div> {
    ui::pressable(
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .h(px(26.0))
            .px(px(10.0))
            .rounded(ui::CONTROL_RADIUS),
    )
}

pub(super) fn drag_area() -> gpui::Div {
    div()
        .h_full()
        .on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
            if event.click_count == 2 {
                window.titlebar_double_click();
            } else {
                window.start_window_move();
            }
        })
}

impl Render for Client {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.sessions.is_empty() {
            self.empty(cx).into_any_element()
        } else if self.layout == Layout::Grid {
            self.grid(cx).into_any_element()
        } else {
            div()
                .flex()
                .flex_1()
                .w_full()
                .min_h_0()
                .child(self.sessions[self.active].view.clone())
                .into_any_element()
        };
        div()
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .on_action(cx.listener(|this, _: &NewSession, window, cx| {
                if let Err(error) = this.open_session(None, window, cx) {
                    tracing::error!(%error, "couldn't open a session");
                }
            }))
            .on_action(cx.listener(|this, _: &CloseSession, window, cx| {
                this.close_session(this.active, window, cx)
            }))
            .on_action(cx.listener(|this, _: &NextSession, window, cx| this.step(1, window, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousSession, window, cx| this.step(-1, window, cx)),
            )
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                this.modifiers_changed(event.modifiers, cx)
            }))
            .on_action(cx.listener(|this, action: &SelectSession, window, cx| {
                if let Some(&index) = this.shortcut_tabs().get(action.0) {
                    this.select(index, window, cx)
                }
            }))
            .child(self.strip(cx))
            .child(body)
            .when_some(self.tab_list.clone(), |root, focus| {
                root.child(self.tab_list(&focus, cx))
            })
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // Each session's view waits for its player thread when dropped; stopping every player first lets those teardowns overlap.
        for session in &self.sessions {
            session.player.post(Msg::Stop);
        }
    }
}

impl Focusable for Client {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_number_only_the_tabs_fully_in_view() {
        let tabs: Vec<_> = (0..12)
            .map(|index| (index as f32 * 100.0, index as f32 * 100.0 + 90.0))
            .collect();
        assert_eq!(fully_visible((0.0, 450.0), 0.0, &tabs), [0, 1, 2, 3]);
        assert_eq!(fully_visible((0.0, 450.0), -150.0, &tabs), [2, 3, 4, 5]);
        assert_eq!(
            fully_visible((0.0, 2000.0), 0.0, &tabs),
            (0..9).collect::<Vec<_>>()
        );
    }

    #[test]
    fn grid_is_as_square_as_the_session_count_allows() {
        assert_eq!(grid_shape(1), (1, 1));
        assert_eq!(grid_shape(2), (2, 1));
        assert_eq!(grid_shape(3), (2, 2));
        assert_eq!(grid_shape(4), (2, 2));
        assert_eq!(grid_shape(5), (3, 2));
        assert_eq!(grid_shape(7), (3, 3));
    }
}
