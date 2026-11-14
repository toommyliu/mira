use std::{
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, EntityId, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, ParentElement as _, Pixels, Render, Role, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity, Window, actions,
    div, point, prelude::FluentBuilder as _, px,
};

use super::{
    Changed, Details, Glance, Identified, RuffleView, ScriptPhase, ScriptState, Summary, Target,
    Updated,
    launches::{self, Launch, Launches},
    manager::OpenAccounts,
    script::CloseTab,
    scripts::OpenScripts,
};
use mira_ui::{
    ActiveTheme as _, Theme, Tone,
    components::format_keystroke,
    h_flex,
    primitives::{
        Button, HoverCard, Icon, Kbd, MenuItem, Tab as TabItem, Tabs,
        button::{Size, Variant},
    },
    theme::{CONTROL_SM, LINE_BODY, LINE_SMALL, TEXT_BODY, TEXT_SMALL},
};

use crate::{
    accounts::{Account, AccountId, Accounts},
    army::Armies,
    ruffle::{Inbox, Msg, extras},
    script_inputs, settings,
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

pub(super) const SIDEBAR_WIDTH: Pixels = px(208.0);
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

fn shortcut_for(shortcuts: &[usize], index: usize) -> Option<usize> {
    shortcuts
        .iter()
        .position(|&tab| tab == index)
        .map(|position| position + 1)
}

pub(crate) fn bind_keys(cx: &mut App) {
    super::script_dialog::bind_keys(cx);
    super::manager::bind_keys(cx);
    cx.bind_keys([
        gpui::KeyBinding::new("cmd-t", NewSession, None),
        gpui::KeyBinding::new("cmd-w", CloseSession, None),
        gpui::KeyBinding::new("cmd-}", NextSession, None),
        gpui::KeyBinding::new("cmd-{", PreviousSession, None),
        gpui::KeyBinding::new("ctrl-tab", NextSession, None),
        gpui::KeyBinding::new("ctrl-shift-tab", PreviousSession, None),
        gpui::KeyBinding::new("cmd-shift-a", OpenAccounts, None),
        gpui::KeyBinding::new("cmd-shift-s", OpenScripts, None),
        gpui::KeyBinding::new("ctrl-cmd-s", ToggleSidebar, None),
    ]);
    cx.bind_keys(
        [
            "cmd-1", "cmd-2", "cmd-3", "cmd-4", "cmd-5", "cmd-6", "cmd-7", "cmd-8", "cmd-9",
        ]
        .into_iter()
        .enumerate()
        .map(|(index, keys)| gpui::KeyBinding::new(keys, SelectSession(index), None)),
    );
}

pub struct Client {
    data: PathBuf,
    accounts: PathBuf,
    device: reqwest::header::HeaderValue,
    armies: Armies,
    extras: Rc<extras::Store>,
    sessions: Vec<Session>,
    active: usize,
    layout: Layout,
    sidebar: Sidebar,
    numbered: usize,
    focus: FocusHandle,
    tab_scroll: ScrollHandle,
    shortcut_hints: bool,
    shortcut_hint_delay: Option<Task<()>>,
    launches: Launches,
    pacing: bool,
    unopened: HashMap<AccountId, SharedString>,
    ended: HashMap<AccountId, ScriptState>,
    _activation: Subscription,
}

struct Session {
    number: usize,
    label: Option<SharedString>,
    account: Option<AccountId>,
    view: Entity<RuffleView>,
    player: Inbox,
    _changes: Subscription,
    _moved: Subscription,
    _identified: Subscription,
    _close: Subscription,
}

impl Session {
    fn title(&self, cx: &App) -> SharedString {
        self.label
            .clone()
            .or_else(|| Some(self.view.read(cx).character()?.to_owned().into()))
            .unwrap_or_else(|| "New tab".into())
    }
}

struct SessionTab {
    number: usize,
    view: EntityId,
    title: SharedString,
    glance: Option<Glance>,
    details: Details,
    hint: Option<usize>,
}

type OpenTool = fn(Entity<Client>, Target, &mut App);

fn tool_menu(client: &WeakEntity<Client>, view: EntityId, title: &SharedString) -> Vec<MenuItem> {
    let tools: [(&str, OpenTool); 4] = [
        ("Environment", super::open_environment),
        ("Follower", super::open_follower),
        ("Loader/grabber", super::open_loader_grabber),
        ("Packets", super::open_packets),
    ];
    std::iter::once(MenuItem::header(format!("Open for {title}")))
        .chain(tools.into_iter().map(|(label, open)| {
            let client = client.clone();
            MenuItem::action(label, move |_, cx| {
                if let Some(client) = client.upgrade() {
                    cx.defer(move |cx| open(client, Target::Pinned(view), cx));
                }
            })
        }))
        .collect()
}

fn tab_card(
    id: impl Into<gpui::ElementId>,
    title: SharedString,
    details: Details,
) -> Option<HoverCard> {
    if details.server.is_none() && details.script.is_none() && details.error.is_none() {
        return None;
    }
    let mut card = HoverCard::new(id, title);
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
        accounts: PathBuf,
        device: reqwest::header::HeaderValue,
        armies: Armies,
        blank_tab: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<Entity<Client>> {
        let client = cx.new(|cx| Client {
            extras: Rc::new(extras::Store::open(data.join("as3.json"))),
            data,
            accounts,
            device,
            armies,
            sessions: Vec::new(),
            active: 0,
            layout: Layout::Single,
            sidebar: if settings::sidebar(cx) {
                Sidebar::Full
            } else {
                Sidebar::Hidden
            },
            numbered: 0,
            focus: cx.focus_handle(),
            tab_scroll: ScrollHandle::new(),
            shortcut_hints: false,
            shortcut_hint_delay: None,
            launches: Launches::default(),
            pacing: false,
            unopened: HashMap::new(),
            ended: HashMap::new(),
            _activation: cx.observe_window_activation(window, |this: &mut Self, window, cx| {
                if !window.is_window_active() {
                    this.hide_shortcut_hints(cx);
                }
            }),
        });
        if blank_tab {
            client.update(cx, |client, cx| client.open_session(None, window, cx))?;
        }
        Ok(client)
    }

    #[cfg(test)]
    pub(crate) fn empty_for_tests(folder: &std::path::Path, cx: &mut App) -> Entity<Client> {
        cx.new(|cx| Client {
            extras: Rc::new(extras::Store::open(folder.join("as3.json"))),
            data: folder.to_path_buf(),
            accounts: folder.join("accounts.json"),
            device: reqwest::header::HeaderValue::from_static("test"),
            armies: Armies::new(folder.join("armies")),
            sessions: Vec::new(),
            active: 0,
            layout: Layout::Single,
            sidebar: Sidebar::Hidden,
            numbered: 0,
            focus: cx.focus_handle(),
            tab_scroll: ScrollHandle::new(),
            shortcut_hints: false,
            shortcut_hint_delay: None,
            launches: Launches::default(),
            pacing: false,
            unopened: HashMap::new(),
            ended: HashMap::new(),
            _activation: Subscription::new(|| {}),
        })
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
        if account.is_none() {
            view.read(cx).session.watch_sign_in();
        }
        self.numbered += 1;
        self.sessions.push(Session {
            number: self.numbered,
            label: account.map(|account| account.name.clone().into()),
            account: account.map(Account::id),
            view: view.clone(),
            player,
            _changes: cx.subscribe(&view, |_, _, _: &Changed, cx| cx.notify()),
            _moved: cx.subscribe(&view, |_, _, _: &Updated, cx| cx.emit(Updated)),
            _identified: cx.subscribe(&view, |this, view, Identified(name), cx| {
                this.link(&view, name, cx)
            }),
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

    fn link(&mut self, view: &Entity<RuffleView>, username: &str, cx: &mut Context<Self>) {
        let id = AccountId::of(username);
        if self.session_of(&id).is_some() {
            return;
        }
        let Some(session) = self.sessions.iter_mut().find(|s| &s.view == view) else {
            return;
        };
        let accounts = match Accounts::open(self.accounts.clone()) {
            Ok(accounts) => accounts,
            Err(error) => {
                tracing::warn!(%error, "couldn't match the player to an account");
                return;
            }
        };
        let Some(account) = accounts.get(&id) else {
            return;
        };
        session.label = Some(account.name.clone().into());
        session.account = Some(id);
        view.update(cx, |view, cx| view.adopt(account, cx));
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
        server: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.needs_login(&account.id(), cx) {
            self.wait_to_launch(
                Launch {
                    account: account.id(),
                    script: None,
                    replace: false,
                    server,
                },
                window,
                cx,
            );
        }
    }

    pub(crate) fn run_script(
        &mut self,
        account: &Account,
        script: PathBuf,
        replace: bool,
        server: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = account.id();
        match self.session_of(&id) {
            Some(index) if !self.needs_login(&id, cx) => {
                if self.launches.remove(&id) {
                    cx.notify();
                }
                let launch = script_inputs::launch(script, cx);
                self.sessions[index]
                    .view
                    .update(cx, |view, cx| view.queue_script(launch, replace, cx));
            }
            _ => self.wait_to_launch(
                Launch {
                    account: id,
                    script: Some(script),
                    replace,
                    server,
                },
                window,
                cx,
            ),
        }
    }

    fn needs_login(&self, account: &AccountId, cx: &App) -> bool {
        self.session_of(account)
            .is_none_or(|index| self.sessions[index].view.read(cx).signed_out())
    }

    pub(crate) fn waiting(&self, account: &AccountId) -> Option<(usize, usize)> {
        self.launches.position(account)
    }

    pub(crate) fn unopened(&self, account: &AccountId) -> Option<SharedString> {
        self.unopened.get(account).cloned()
    }

    pub(crate) fn ended(&self, account: &AccountId) -> Option<ScriptState> {
        self.ended.get(account).cloned()
    }

    pub(crate) fn loaded_scripts(&self, cx: &App) -> Vec<PathBuf> {
        self.sessions
            .iter()
            .filter_map(|session| session.view.read(cx).script.entry().cloned())
            .collect()
    }

    fn wait_to_launch(&mut self, launch: Launch, window: &mut Window, cx: &mut Context<Self>) {
        self.unopened.remove(&launch.account);
        self.launches.add(launch);
        cx.notify();
        if self.pacing {
            return;
        }
        self.pacing = true;
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(Ok(Some(wait))) =
                cx.update(|window, cx| this.update(cx, |client, cx| client.launch_due(window, cx)))
            {
                cx.background_executor().timer(wait).await;
            }
        })
        .detach();
    }

    fn launch_due(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Duration> {
        if let Some(launch) = self.launches.take_due(Instant::now(), launches::jitter()) {
            self.open_waiting(launch, window, cx);
            cx.notify();
        }
        let wait = self.launches.wait(Instant::now());
        self.pacing = wait.is_some();
        wait
    }

    fn open_waiting(&mut self, launch: Launch, window: &mut Window, cx: &mut Context<Self>) {
        let Launch {
            account: id,
            script,
            replace,
            server,
        } = launch;
        let account = match Accounts::open(self.accounts.clone()) {
            Ok(accounts) => accounts.get(&id).cloned(),
            Err(error) => {
                self.unopened.insert(id, error.into());
                return;
            }
        };
        let Some(account) = account else {
            return;
        };
        let account = Account {
            server: server.or(account.server),
            ..account
        };
        match self.session_of(&id) {
            Some(index) => {
                let launch = script.map(|script| script_inputs::launch(script, cx));
                self.sessions[index].view.update(cx, |view, cx| {
                    view.sign_in_again(&account, cx);
                    if let Some(launch) = launch {
                        view.queue_script(launch, replace, cx);
                    }
                });
            }
            None => {
                if let Err(error) = self.open_account(&account, script, window, cx) {
                    self.unopened.insert(id, format!("{error:#}").into());
                }
            }
        }
    }

    fn open_account(
        &mut self,
        account: &Account,
        script: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let launch = script.map(|script| script_inputs::launch(script, cx));
        let view = self.open_session(Some(account), window, cx)?;
        view.update(cx, |view, cx| view.sign_in(account, launch, cx));
        Ok(())
    }

    pub(crate) fn restart_script(&mut self, account: &AccountId, cx: &mut Context<Self>) {
        if let Some(index) = self.session_of(account) {
            self.sessions[index]
                .view
                .update(cx, |view, cx| view.start_loaded_script(cx));
        }
    }

    pub(crate) fn stop_script(&mut self, account: &AccountId, cx: &mut Context<Self>) {
        if self.launches.remove(account) {
            cx.notify();
        }
        if let Some(index) = self.session_of(account) {
            self.sessions[index]
                .view
                .update(cx, |view, cx| view.cancel_script(cx));
        }
    }

    pub(super) fn active_view(&self, cx: &App) -> Option<(Entity<RuffleView>, SharedString)> {
        let session = self.sessions.get(self.active)?;
        Some((session.view.clone(), session.title(cx)))
    }

    pub(super) fn titled_views(&self, cx: &App) -> Vec<(Entity<RuffleView>, SharedString)> {
        self.sessions
            .iter()
            .map(|session| (session.view.clone(), session.title(cx)))
            .collect()
    }

    pub(super) fn views(&self) -> Vec<Entity<RuffleView>> {
        self.sessions
            .iter()
            .map(|session| session.view.clone())
            .collect()
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
        if self.launches.remove(account) {
            cx.notify();
        }
        if let Some(index) = self.session_of(account) {
            self.close_session(index, window, cx);
        }
    }

    fn close_session(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.sessions.len() {
            return;
        }
        let session = self.sessions.remove(index);
        if let Some(account) = &session.account
            && self.launches.remove(account)
        {
            cx.notify();
        }
        if let Some(account) = session.account
            && let Some(mut last) = session.view.read(cx).script.state()
        {
            if matches!(
                last.phase,
                ScriptPhase::Running | ScriptPhase::NeedsInput | ScriptPhase::Stopping
            ) {
                last.phase = ScriptPhase::Stopped;
            }
            self.ended.insert(account, last);
        }
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
        super::native::present_next_frame_with_players(window);
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
                reveal_tab(&self.tab_scroll, self.active, self.sessions.len());
                window.focus(&session.view.focus_handle(cx), cx)
            }
            None => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    fn toggle_sidebar(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.sidebar = match self.sidebar {
            Sidebar::Hidden => Sidebar::Full,
            Sidebar::Full => Sidebar::Hidden,
        };
        settings::set_sidebar(self.sidebar == Sidebar::Full, cx);
        super::native::present_next_frame_with_players(window);
        cx.notify();
    }

    fn move_session(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
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
        let rows: Rc<[SessionTab]> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(index, session)| {
                let view = session.view.read(cx);
                SessionTab {
                    number: session.number,
                    view: session.view.entity_id(),
                    title: session.title(cx),
                    glance: view.glance(),
                    details: view.details(),
                    hint: self
                        .shortcut_hints
                        .then(|| shortcut_for(&shortcuts, index))
                        .flatten(),
                }
            })
            .collect();
        let active = self.active;
        let client = cx.weak_entity();
        let selector = client.clone();
        let mover = client.clone();
        let face_theme = theme.clone();
        let tabs = Tabs::new("sessions", "Tabs", rows.len(), move |index| {
            let row = &rows[index];
            let face = tab_face(
                &face_theme,
                &row.title,
                row.glance.clone(),
                index == active,
                row.hint,
            );
            let tab = TabItem::new(row.title.clone());
            let tab = match &row.glance {
                Some(glance) => tab.description(glance.label.clone()),
                None => tab,
            };
            let tab = match tab_card(
                ("tab-card", row.number),
                row.title.clone(),
                row.details.clone(),
            ) {
                Some(card) => tab.child(card.child(face)),
                None => tab.child(face),
            };
            let tab = tab.menu({
                let client = client.clone();
                let (view, title) = (row.view, row.title.clone());
                move |_, _| tool_menu(&client, view, &title)
            });
            if row.hint.is_some() {
                return tab;
            }
            let client = client.clone();
            tab.on_close(move |window, cx| {
                client
                    .update(cx, |client, cx| client.close_session(index, window, cx))
                    .ok();
            })
        })
        .vertical()
        .selected(active)
        .track_scroll(&self.tab_scroll)
        .on_select(move |index, window, cx| {
            selector
                .update(cx, |client, cx| client.select(index, window, cx))
                .ok();
        })
        .on_reorder(move |from, to, _, cx| {
            mover
                .update(cx, |client, cx| client.move_session(from, to, cx))
                .ok();
        });
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
                    .child(
                        Button::icon("new-session", Icon::Plus, "New tab")
                            .size(Size::Small)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Err(error) = this.open_session(None, window, cx) {
                                    tracing::error!(%error, "couldn't open a session");
                                }
                            })),
                    )
                    .child(self.grid_toggle(cx)),
            )
            .child(div().flex_none().child(tabs))
            .child(drag_area().flex_1())
            .child(
                div().flex_none().p(px(8.0)).child(
                    Button::new("accounts", "Accounts")
                        .variant(Variant::Ghost)
                        .w_full()
                        .child(
                            h_flex()
                                .flex_1()
                                .justify_between()
                                .child("Accounts")
                                .children(
                                    window
                                        .highest_precedence_binding_for_action(&OpenAccounts)
                                        .and_then(|binding| {
                                            Some(format_keystroke(
                                                binding.keystrokes().first()?.inner(),
                                            ))
                                        })
                                        .map(|keys| {
                                            div().text_color(theme.text_tertiary).child(keys)
                                        }),
                                ),
                        )
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(OpenAccounts), cx)
                        }),
                ),
            )
    }

    fn sidebar_toggle(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let label = match self.sidebar {
            Sidebar::Hidden => "Show sidebar",
            Sidebar::Full => "Hide sidebar",
        };
        let glyph = match self.sidebar {
            Sidebar::Hidden => Icon::Sidebar,
            Sidebar::Full => Icon::SidebarOpen,
        };
        div()
            .absolute()
            .left(traffic_lights_inset(window))
            .top((super::BAR_HEIGHT - CONTROL_SM) / 2.0)
            .occlude()
            .child(
                Button::icon("sidebar-toggle", glyph, label)
                    .size(Size::Small)
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx))),
            )
    }

    fn grid_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (label, glyph, next) = match self.layout {
            Layout::Single => ("Grid view", Icon::LayoutGrid, Layout::Grid),
            Layout::Grid => ("Single view", Icon::Square, Layout::Single),
        };
        Button::icon("grid", glyph, label)
            .size(Size::Small)
            .on_click(cx.listener(move |this, _, window, cx| this.set_layout(next, window, cx)))
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
                Button::new("open-session", "New tab")
                    .variant(Variant::Outline)
                    .mt(px(8.0))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Err(error) = this.open_session(None, window, cx) {
                            tracing::error!(%error, "couldn't open a session");
                        }
                    })),
            )
    }
}

pub(super) fn tab_face(
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
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .line_height(LINE_BODY)
                        .truncate()
                        .when(active, |name| name.font_weight(gpui::FontWeight::MEDIUM))
                        .child(label.clone()),
                )
                .children(hint.map(|number| Kbd::new(format!("⌘{number}")))),
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

pub(super) fn reveal_tab(scroll: &ScrollHandle, index: usize, count: usize) {
    if index == 0 {
        scroll.set_offset(gpui::Point::default());
    } else if index + 1 == count {
        scroll.scroll_to_bottom();
    } else {
        scroll.scroll_to_item(index);
    }
}

fn traffic_lights_inset(window: &Window) -> Pixels {
    if window.is_fullscreen() {
        BAR_PADDING
    } else {
        TRAFFIC_LIGHTS_WIDTH
    }
}

fn bar_inset(window: &Window, sidebar_width: Pixels) -> Pixels {
    (traffic_lights_inset(window) + CONTROL_SM - sidebar_width).max(BAR_PADDING)
}

pub(crate) fn drag_area() -> gpui::Div {
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

pub(crate) fn bar(cx: &App) -> gpui::Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.0))
        .h(super::BAR_HEIGHT)
        .pr(BAR_PADDING)
        .bg(theme.base)
        .border_b_1()
        .border_color(theme.border)
}

pub(crate) fn window_bar(window: &Window, cx: &App) -> gpui::Div {
    bar(cx).child(drag_area().flex_none().w(traffic_lights_inset(window)))
}

pub(crate) fn bar_title(title: impl Into<SharedString>, cx: &App) -> gpui::Div {
    drag_area()
        .flex_none()
        .flex()
        .items_center()
        .px(px(8.0))
        .text_size(TEXT_BODY)
        .line_height(LINE_BODY)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(cx.theme().text_secondary)
        .child(title.into())
}

pub(crate) fn bar_separator(cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .mx(px(4.0))
        .w(px(1.0))
        .h(px(16.0))
        .bg(cx.theme().hairline)
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
            .child(mira_ui::nav::pointer_probe())
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
            .on_action(
                cx.listener(|this, _: &ToggleSidebar, window, cx| this.toggle_sidebar(window, cx)),
            )
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

impl gpui::EventEmitter<Updated> for Client {}

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
    fn grid_is_as_square_as_the_session_count_allows() {
        assert_eq!(grid_shape(1), (1, 1));
        assert_eq!(grid_shape(2), (2, 1));
        assert_eq!(grid_shape(3), (2, 2));
        assert_eq!(grid_shape(4), (2, 2));
        assert_eq!(grid_shape(5), (3, 2));
        assert_eq!(grid_shape(7), (3, 3));
    }
}
