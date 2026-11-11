use std::{path::PathBuf, rc::Rc, time::Duration};

use anyhow::Context as _;

use gpui::{
    App, AppContext as _, Bounds, Context, DragMoveEvent, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, Modifiers, ModifiersChangedEvent,
    MouseButton, MouseDownEvent, ParentElement as _, Pixels, Render, Role, ScrollHandle,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    Window, actions, anchored, deferred, div, point, prelude::FluentBuilder as _, px,
};

use super::{
    Changed, Details, Glance, RuffleView, Summary,
    manager::OpenAccounts,
    script::CloseTab,
    ui::{self, Glyph},
};
use mira_ui::{
    ActiveTheme as _, Theme, Tone,
    components::format_keystroke,
    motion,
    primitives::{HoverCard, Icon, Tooltip, attach_card, icon, sidebar_icon},
    theme::{CONTROL_SM, LINE_BODY, LINE_SMALL, TEXT_BODY, TEXT_SMALL},
};

use crate::{
    accounts::{Account, AccountId},
    army::Armies,
    ruffle::{Inbox, Msg, extras},
};

actions!(
    mira,
    [
        NewSession,
        CloseSession,
        NextSession,
        PreviousSession,
        ToggleSidebar
    ]
);

#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = mira, no_json)]
struct SelectSession(usize);

const SIDEBAR_WIDTH: Pixels = px(208.0);
const TAB_ROW: Pixels = px(56.0);
const TAB_GAP: Pixels = px(4.0);
const TAB_PADDING: Pixels = px(8.0);
const TAB_TOP: Pixels = px(8.0);
const TAB_BOTTOM: Pixels = px(10.0);
const BAR_PADDING: Pixels = px(6.0);
pub(super) const TRAFFIC_LIGHTS_WIDTH: Pixels = px(78.0);
const SHORTCUT_HINT_DELAY: Duration = Duration::from_millis(400);

fn fully_visible(viewport: (f32, f32), scrolled: f32, tabs: &[(f32, f32)]) -> Vec<usize> {
    tabs.iter()
        .enumerate()
        .filter(|(_, (top, bottom))| {
            top + scrolled >= viewport.0 && bottom + scrolled <= viewport.1
        })
        .map(|(index, _)| index)
        .take(9)
        .collect()
}

fn rest_top(slot: Pixels, from: usize, index: usize) -> Pixels {
    slot + (TAB_ROW + TAB_GAP) * (index as f32 - from as f32)
}

fn drop_index(row_centers: &[f32], from: usize, center: f32) -> usize {
    row_centers
        .iter()
        .enumerate()
        .filter(|&(index, &row)| index != from && row < center)
        .count()
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
        gpui::KeyBinding::new("ctrl-cmd-s", ToggleSidebar, None),
    ]);
    cx.bind_keys((1..=9).map(|number| {
        gpui::KeyBinding::new(&format!("cmd-{number}"), SelectSession(number - 1), None)
    }));
}

pub struct Client {
    data: PathBuf,
    device: reqwest::header::HeaderValue,
    armies: Armies,
    extras: Rc<extras::Store>,
    sessions: Vec<Session>,
    active: usize,
    layout: Layout,
    sidebar: Sidebar,
    dragging: Option<TabDrag>,
    drags: usize,
    numbered: usize,
    focus: FocusHandle,
    tab_scroll: ScrollHandle,
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
    _close: Subscription,
}

fn tab_card(title: SharedString, details: Details) -> Option<HoverCard> {
    if details.server.is_none() && details.script.is_none() && details.error.is_none() {
        return None;
    }
    let mut card = HoverCard::new(title);
    if let Some(server) = details.server {
        card = card.row("Server", server, Tone::Neutral);
    }
    if let Some(script) = details.script {
        card = card.row("Script", script, Tone::Neutral);
    }
    if let Some(error) = details.error {
        card = card.row("Error", error, Tone::Danger);
    }
    Some(card)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    Single,
    Grid,
}

#[derive(Clone, Copy)]
struct TabDrag {
    number: usize,
    generation: usize,
    grab: Pixels,
    top: Pixels,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sidebar {
    Hidden,
    Full,
}

impl Sidebar {
    fn width(self) -> Pixels {
        match self {
            Sidebar::Hidden => px(0.0),
            Sidebar::Full => SIDEBAR_WIDTH,
        }
    }
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
            extras: Rc::new(extras::Store::open(data.join("as3.json"))),
            data,
            device,
            armies,
            sessions: Vec::new(),
            active: 0,
            layout: Layout::Single,
            sidebar: Sidebar::Hidden,
            dragging: None,
            drags: 0,
            numbered: 0,
            focus: cx.focus_handle(),
            tab_scroll: ScrollHandle::new(),
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
            Rc::clone(&self.extras),
            window,
            cx,
        )?;
        let window_size = window.viewport_size();
        let left = self.sidebar.width();
        let player = Bounds::new(
            point(left, super::BAR_HEIGHT),
            gpui::size(
                (window_size.width - left).max(px(1.0)),
                (window_size.height - super::BAR_HEIGHT).max(px(1.0)),
            ),
        );
        let inset = bar_inset(window, left);
        view.update(cx, |view, _| {
            view.bar_inset = inset;
            view.layout(player, window)
        });
        let player = view.read(cx).session.inbox();
        self.numbered += 1;
        self.sessions.push(Session {
            number: self.numbered,
            label: account.map_or_else(|| "New tab".into(), |account| account.name.clone().into()),
            account: account.map(Account::id),
            view: view.clone(),
            player,
            _changes: cx.subscribe(&view, |_, _, _: &Changed, cx| cx.notify()),
            _close: cx.subscribe_in(&view, window, |this, view, _: &CloseTab, window, cx| {
                if let Some(index) = this.sessions.iter().position(|s| &s.view == view) {
                    this.close_session(index, window, cx);
                }
            }),
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
                .update(cx, |view, cx| view.present(tiled, active, window, cx));
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

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar = match self.sidebar {
            Sidebar::Hidden => Sidebar::Full,
            Sidebar::Full => Sidebar::Hidden,
        };
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

    fn drag_tab(&mut self, pointer: Pixels, cx: &mut Context<Self>) {
        if let Some(drag) = self.dragging.as_mut() {
            drag.top = pointer - drag.grab;
            cx.notify();
        }
    }

    fn drop_tab(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.dragging.take() else {
            return;
        };
        if let Some((_, to)) = self.drag_target(drag) {
            self.move_session(drag.number, to, cx);
        }
        cx.notify();
    }

    fn cancel_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !cx.has_active_drag() || self.dragging.take().is_none() {
            return false;
        }
        cx.stop_active_drag(window);
        cx.notify();
        true
    }

    fn drag_target(&self, drag: TabDrag) -> Option<(usize, usize)> {
        let (from, slot) = self.drag_slot(drag)?;
        let centers: Vec<_> = (0..self.sessions.len())
            .map(|index| f32::from(rest_top(slot, from, index) + TAB_ROW / 2.0))
            .collect();
        let center = f32::from(drag.top + TAB_ROW / 2.0);
        Some((from, drop_index(&centers, from, center)))
    }

    fn drag_slot(&self, drag: TabDrag) -> Option<(usize, Pixels)> {
        let from = self
            .sessions
            .iter()
            .position(|session| session.number == drag.number)?;
        let slot = self.tab_scroll.bounds_for_item(from)?.top() + self.tab_scroll.offset().y;
        Some((from, slot))
    }

    fn lifted_top(&self, drag: TabDrag) -> Option<Pixels> {
        let (from, slot) = self.drag_slot(drag)?;
        let list = self.tab_scroll.bounds();
        let first = rest_top(slot, from, 0);
        let last = rest_top(slot, from, self.sessions.len().checked_sub(1)?);
        let min = first.max(list.top());
        let max = last.min(list.bottom() - TAB_ROW).max(min);
        Some(drag.top.max(min).min(max))
    }

    fn lifted_tab(&self, theme: &Theme, cx: &App) -> Option<impl IntoElement + use<>> {
        let drag = self.dragging.filter(|_| cx.has_active_drag())?;
        let index = self
            .sessions
            .iter()
            .position(|session| session.number == drag.number)?;
        let session = &self.sessions[index];
        let row = self.tab_scroll.bounds_for_item(index)?;
        let top = self.lifted_top(drag)?;
        Some(deferred(
            anchored().position(point(row.left(), top)).child(
                div()
                    .flex()
                    .w(row.size.width)
                    .h(TAB_ROW)
                    .px(TAB_PADDING)
                    .pt(TAB_TOP)
                    .pb(TAB_BOTTOM)
                    .rounded(theme.radius.item)
                    .bg(theme.overlay)
                    .shadow(theme.floating())
                    .text_size(TEXT_BODY)
                    .text_color(theme.text)
                    .child(tab_face(
                        theme,
                        &session.label,
                        session.view.read(cx).glance(),
                        index == self.active,
                        None,
                    )),
            ),
        ))
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
        if self.sidebar == Sidebar::Hidden {
            return (0..self.sessions.len().min(9)).collect();
        }
        let viewport = self.tab_scroll.bounds();
        let tabs: Vec<_> = (0..self.sessions.len())
            .map_while(|index| self.tab_scroll.bounds_for_item(index))
            .map(|tab| (f32::from(tab.top()), f32::from(tab.bottom())))
            .collect();
        fully_visible(
            (f32::from(viewport.top()), f32::from(viewport.bottom())),
            f32::from(self.tab_scroll.offset().y),
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

    fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let shortcuts = self.shortcut_tabs();
        let drag = self.dragging.filter(|_| cx.has_active_drag());
        let target = drag.and_then(|drag| Some((drag.generation, self.drag_target(drag)?)));
        let tabs: Vec<_> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(index, session)| {
                let hint = self
                    .shortcut_hints
                    .then(|| shortcut_for(&shortcuts, index))
                    .flatten();
                self.tab(&theme, index, session, hint, target, window, cx)
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(SIDEBAR_WIDTH)
            .h_full()
            .bg(theme.well)
            .border_r_1()
            .border_color(theme.hairline)
            .text_size(TEXT_BODY)
            .text_color(theme.text)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.0))
                    .h(super::BAR_HEIGHT)
                    .pr(px(8.0))
                    .child(drag_area().flex_1())
                    .child(Tooltip::new(
                        "new-session-tip",
                        "New tab",
                        control(&theme, "new-session")
                            .size(CONTROL_SM)
                            .justify_center()
                            .role(Role::Button)
                            .aria_label("New tab")
                            .child(icon(Icon::Plus, theme.text_secondary))
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Err(error) = this.open_session(None, window, cx) {
                                    tracing::error!(%error, "couldn't open a session");
                                }
                            })),
                    ))
                    .child(self.grid_toggle(&theme, cx)),
            )
            .child(
                div()
                    .id("tabs")
                    .flex()
                    .flex_col()
                    .gap(TAB_GAP)
                    .px(px(8.0))
                    .py(px(4.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.tab_scroll)
                    .on_drag_move(
                        cx.listener(|this, event: &DragMoveEvent<DraggedTab>, _, cx| {
                            this.drag_tab(event.event.position.y, cx)
                        }),
                    )
                    .children(tabs),
            )
            .child(drag_area().flex_1())
            .child(
                div().flex_none().p(px(8.0)).child(
                    control(&theme, "accounts")
                        .w_full()
                        .justify_between()
                        .role(Role::Button)
                        .text_color(theme.text_secondary)
                        .child("Accounts")
                        .children(
                            window
                                .highest_precedence_binding_for_action(&OpenAccounts)
                                .and_then(|binding| {
                                    Some(format_keystroke(binding.keystrokes().first()?.inner()))
                                })
                                .map(|keys| div().text_color(theme.text_tertiary).child(keys)),
                        )
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(OpenAccounts), cx)
                        }),
                ),
            )
            .children(self.lifted_tab(&theme, cx))
    }

    fn tab(
        &self,
        theme: &Theme,
        index: usize,
        session: &Session,
        hint: Option<usize>,
        target: Option<(usize, (usize, usize))>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let active = index == self.active;
        let glance = session.view.read(cx).glance();
        let description = glance.as_ref().map(|glance| glance.label.clone());
        let row = div()
            .id(("tab", session.number))
            .relative()
            .flex()
            .flex_none()
            .h(TAB_ROW)
            .px(TAB_PADDING)
            .pt(TAB_TOP)
            .pb(TAB_BOTTOM)
            .rounded(theme.radius.item)
            .role(Role::Tab)
            .aria_label(session.label.clone())
            .when_some(description, |tab, description: SharedString| {
                tab.aria_description(description)
            })
            .aria_selected(active)
            .child(tab_face(
                theme,
                &session.label,
                glance,
                active,
                hint.filter(|_| target.is_none()),
            ));
        let row = match tab_card(session.label.clone(), session.view.read(cx).details()) {
            Some(card) => attach_card(row, ("tab-card", session.number), card, window, cx),
            None => row,
        };
        let Some((generation, (from, to))) = target else {
            return self.resting_tab(row, theme, index, session, hint, cx);
        };
        if index == from {
            return row.invisible().into_any_element();
        }
        let shift = if from < index && index <= to {
            -1.0
        } else if to <= index && index < from {
            1.0
        } else {
            0.0
        };
        let offset = motion::travel(
            ("tab-shift", generation * 1_000_000 + session.number),
            shift * f32::from(TAB_ROW + TAB_GAP),
            window,
            cx,
        );
        row.top(px(offset))
            .map(|tab| {
                if active {
                    tab.bg(theme.selected).text_color(theme.text)
                } else {
                    tab.text_color(theme.text_secondary)
                }
            })
            .into_any_element()
    }

    fn resting_tab(
        &self,
        row: Stateful<gpui::Div>,
        theme: &Theme,
        index: usize,
        session: &Session,
        hint: Option<usize>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let active = index == self.active;
        row.group("tab")
            .map(|tab| {
                if active {
                    tab.bg(theme.selected).text_color(theme.text)
                } else {
                    ui::pressable(theme, tab).text_color(theme.text_secondary)
                }
            })
            .when(hint.is_none(), |tab| {
                tab.child(
                    div()
                        .id(("close-session", session.number))
                        .absolute()
                        .right(px(4.0))
                        .top(TAB_TOP)
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
                        .hover(|close| close.bg(theme.fill_hover))
                        .active(|close| close.bg(theme.fill_pressed))
                        .role(Role::Button)
                        .aria_label("Close tab")
                        .child(ui::glyph(Glyph::Close, theme.text_secondary))
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
                },
                {
                    let client = cx.weak_entity();
                    move |tab, grabbed_at, _, cx| {
                        let _ = client.update(cx, |client, cx| {
                            let top = client
                                .tab_scroll
                                .bounds_for_item(index)
                                .map_or(Pixels::ZERO, |row| {
                                    row.top() + client.tab_scroll.offset().y
                                });
                            client.drags += 1;
                            client.dragging = Some(TabDrag {
                                number: tab.number,
                                generation: client.drags,
                                grab: grabbed_at.y,
                                top,
                            });
                            cx.notify();
                        });
                        cx.new(|_| tab.clone())
                    }
                },
            )
            .into_any_element()
    }

    fn sidebar_toggle(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let label = match self.sidebar {
            Sidebar::Hidden => "Show sidebar",
            Sidebar::Full => "Hide sidebar",
        };
        let toggle = control(theme, "sidebar-toggle")
            .size(CONTROL_SM)
            .justify_center()
            .role(Role::Button)
            .aria_label(label)
            .child(sidebar_icon(
                self.sidebar == Sidebar::Full,
                theme.text_secondary,
            ))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)));
        div()
            .absolute()
            .left(sidebar_toggle_left(window))
            .top((super::BAR_HEIGHT - CONTROL_SM) / 2.0)
            .occlude()
            .child(Tooltip::new("sidebar-toggle-tip", label, toggle))
    }

    fn grid_toggle(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (label, glyph, next) = match self.layout {
            Layout::Single => ("Grid view", Icon::LayoutGrid, Layout::Grid),
            Layout::Grid => ("Single view", Icon::Square, Layout::Single),
        };
        let toggle = control(theme, "grid")
            .size(CONTROL_SM)
            .justify_center()
            .role(Role::Button)
            .aria_label(label)
            .child(icon(glyph, theme.text_secondary))
            .on_click(cx.listener(move |this, _, window, cx| this.set_layout(next, window, cx)));
        Tooltip::new("grid-tip", label, toggle)
    }

    fn tile(
        &self,
        theme: &Theme,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (columns, rows) = grid_shape(self.sessions.len());
        let (row, column) = (index / columns, index % columns);
        div()
            .id(("tile", self.sessions[index].number))
            .relative()
            .flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .border_color(theme.base)
            .when(column > 0, |tile| tile.border_l_1())
            .when(column + 1 < columns, |tile| tile.border_r_1())
            .when(row > 0, |tile| tile.border_t_1())
            .when(row + 1 < rows, |tile| tile.border_b_1())
            .child(self.sessions[index].view.clone())
            .on_any_mouse_down(cx.listener(move |this, _, window, cx| {
                if this.active != index {
                    this.select(index, window, cx);
                }
            }))
    }

    fn grid(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
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
                            cell.child(self.tile(&theme, index, cx))
                        } else {
                            cell.bg(theme.base)
                        }
                    }))
            }))
    }

    fn empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .bg(theme.base)
            .text_size(TEXT_BODY)
            .text_color(theme.text_secondary)
            .child("No tabs are open.")
            .child(
                control(theme, "open-session")
                    .mt(px(8.0))
                    .px(px(12.0))
                    .border_1()
                    .border_color(theme.hairline)
                    .text_color(theme.text)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .role(Role::Button)
                    .child("New tab")
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
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

fn tab_face(
    theme: &Theme,
    label: &SharedString,
    glance: Option<Glance>,
    active: bool,
    hint: Option<usize>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .flex_1()
        .min_w_0()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .h(LINE_BODY)
                .when(hint.is_none(), |line| line.pr(px(14.0)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .line_height(LINE_BODY)
                        .truncate()
                        .when(active, |name| name.font_weight(gpui::FontWeight::MEDIUM))
                        .child(label.clone()),
                )
                .children(hint.map(|number| shortcut_badge(theme, number))),
        )
        .child(
            div()
                .h(LINE_SMALL)
                .truncate()
                .text_size(TEXT_SMALL)
                .line_height(LINE_SMALL)
                .children(glance.map(|glance| {
                    div()
                        .text_color(match glance.tone {
                            Tone::Neutral => theme.text_tertiary,
                            tone => theme.tone(tone).2,
                        })
                        .child(glance.label)
                })),
        )
}

fn shortcut_badge(theme: &Theme, number: usize) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(18.0))
        .px(px(5.0))
        .rounded(px(4.0))
        .bg(theme.fill_hover)
        .text_size(TEXT_SMALL)
        .text_color(theme.text_secondary)
        .child(format!("⌘{number}"))
}

fn control(theme: &Theme, id: impl Into<gpui::ElementId>) -> Stateful<gpui::Div> {
    ui::pressable(
        theme,
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .h(px(26.0))
            .px(px(10.0))
            .rounded(theme.radius.control),
    )
}

fn sidebar_toggle_left(window: &Window) -> Pixels {
    if window.is_fullscreen() {
        BAR_PADDING
    } else {
        TRAFFIC_LIGHTS_WIDTH
    }
}

fn bar_inset(window: &Window, sidebar_width: Pixels) -> Pixels {
    (sidebar_toggle_left(window) + CONTROL_SM - sidebar_width).max(BAR_PADDING)
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let inset = bar_inset(window, self.sidebar.width());
        for session in &self.sessions {
            if session.view.read(cx).bar_inset != inset {
                session.view.update(cx, |view, _| view.bar_inset = inset);
            }
        }
        let (bar_background, bar_border) = (cx.theme().base, cx.theme().border);
        let game_bar = !self.sessions.is_empty();
        let body = if self.sessions.is_empty() {
            self.empty(cx).into_any_element()
        } else if self.layout == Layout::Grid {
            let bar = self.sessions[self.active]
                .view
                .update(cx, |view, cx| view.bar(cx));
            div()
                .flex()
                .flex_col()
                .flex_1()
                .w_full()
                .min_h_0()
                .child(bar)
                .child(self.grid(cx))
                .into_any_element()
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
            .id("client")
            .role(Role::Group)
            .track_focus(&self.focus)
            .flex()
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
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_drop(cx.listener(|this, _: &DraggedTab, _, cx| this.drop_tab(cx)))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.cancel_drag(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|this, action: &SelectSession, window, cx| {
                if let Some(&index) = this.shortcut_tabs().get(action.0) {
                    this.select(index, window, cx)
                }
            }))
            .when(self.sidebar == Sidebar::Full, |root| {
                root.child(self.sidebar(window, cx))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .when(self.sidebar == Sidebar::Hidden && !game_bar, |column| {
                        column.child(
                            drag_area()
                                .flex_none()
                                .w_full()
                                .h(super::BAR_HEIGHT)
                                .bg(bar_background)
                                .border_b_1()
                                .border_color(bar_border),
                        )
                    })
                    .child(body),
            )
            .child(self.sidebar_toggle(window, cx))
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
            .map(|index| (index as f32 * 30.0, index as f32 * 30.0 + 28.0))
            .collect();
        assert_eq!(fully_visible((0.0, 130.0), 0.0, &tabs), [0, 1, 2, 3]);
        assert_eq!(fully_visible((0.0, 130.0), -45.0, &tabs), [2, 3, 4]);
        assert_eq!(
            fully_visible((0.0, 1000.0), 0.0, &tabs),
            (0..9).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_dragged_tab_lands_after_every_row_whose_center_it_passed() {
        let rows = [10.0, 40.0, 70.0, 100.0];
        assert_eq!(drop_index(&rows, 1, -50.0), 0);
        assert_eq!(drop_index(&rows, 1, 30.0), 1);
        assert_eq!(drop_index(&rows, 1, 80.0), 2);
        assert_eq!(drop_index(&rows, 1, 500.0), 3);
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
