use mira_ui::StyledExt as _;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, EntityId, FocusHandle, Focusable, Global,
    InteractiveElement as _, IntoElement, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, ParentElement as _, Pixels, Render, Role, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, TitlebarOptions, WeakEntity,
    Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowOptions, actions, div,
    point, prelude::FluentBuilder as _, px, size,
};

use super::{
    Changed, Details, Glance, Identified, RuffleView, ScriptPhase, ScriptState, Summary, Target,
    Updated,
    launches::{self, Launch, Launches, Pace},
    script::{CloseTab, Finished},
};
use mira_ui::{
    ActiveTheme as _, Theme, Tone, h_flex,
    primitives::{
        Align, Button, Dialog, HoverCard, Icon, Kbd, MenuItem, Placement, Side, Tab as TabItem,
        Tabs,
        button::{Size, Variant},
    },
    theme::CONTROL_SM,
};

use crate::{
    accounts::{Account, AccountId, Accounts},
    army::Armies,
    ruffle::{Inbox, Msg, extras},
    script_inputs,
    scripting::Values,
    settings,
    shared_objects::SharedObjects,
};

actions!(
    mira,
    [
        OpenClient,
        NewSession,
        CloseSession,
        NextSession,
        PreviousSession,
        ToggleSidebar,
        ToggleGrid
    ]
);

#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = mira, no_json)]
struct SelectSession(usize);

const BAR_PADDING: Pixels = px(6.0);
pub(super) const TRAFFIC_LIGHTS_WIDTH: Pixels = px(78.0);
const SHORTCUT_HINT_DELAY: Duration = Duration::from_millis(400);
const STATUS_LINGER: Duration = Duration::from_millis(1500);
const STATUS_PADDING: Pixels = px(16.0);

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
    super::scripts::bind_keys(cx);
    cx.bind_keys([
        gpui::KeyBinding::new("cmd-t", NewSession, None),
        gpui::KeyBinding::new("cmd-w", CloseSession, None),
        gpui::KeyBinding::new("cmd-}", NextSession, None),
        gpui::KeyBinding::new("cmd-{", PreviousSession, None),
        gpui::KeyBinding::new("ctrl-tab", NextSession, None),
        gpui::KeyBinding::new("ctrl-shift-tab", PreviousSession, None),
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
    status: Option<Status>,
    launches: Launches,
    pacing: bool,
    unopened: HashMap<AccountId, SharedString>,
    ended: HashMap<AccountId, ScriptState>,
    closing: Option<Closing>,
    _activation: Subscription,
}

struct Closing {
    open: bool,
    view: EntityId,
    title: SharedString,
}

struct Status {
    states: [SharedString; 2],
    on: bool,
    entered: Instant,
    left: Option<Instant>,
    _hide: Task<()>,
}

struct Session {
    number: usize,
    view: Entity<RuffleView>,
    player: Inbox,
    _changes: Subscription,
    _moved: Subscription,
    _identified: Subscription,
    _finished: Subscription,
    _close: Subscription,
}

impl Session {
    fn title(&self, cx: &App) -> SharedString {
        let view = self.view.read(cx);
        view.account_name()
            .or_else(|| view.character())
            .map_or_else(|| "New tab".into(), |title| title.to_owned().into())
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

impl SessionTab {
    fn tab(&self, theme: &Theme, active: bool) -> TabItem {
        let mut tab = TabItem::new(self.title.clone())
            .child(tab_title(theme, &self.title, active, self.hint))
            .detail(tab_glance(theme, self.glance.clone()));
        if let Some(glance) = &self.glance {
            tab = tab.description(glance.label.clone());
        }
        let Details {
            server,
            script,
            error,
        } = &self.details;
        if server.is_none() && script.is_none() && error.is_none() {
            return tab;
        }
        let mut card = HoverCard::new(("tab-card", self.number), self.title.clone())
            .placement(Placement::new(Side::Right, Align::Start));
        if let Some(server) = server {
            card = card.row("Server", server.clone(), Tone::Neutral);
        }
        if let Some(script) = script {
            card = card.path_row("Script", script.clone());
        }
        if let Some(error) = error {
            card = card.row("Error", error.clone(), Tone::Danger);
        }
        tab.hover_card(card)
    }
}

type OpenTool = fn(Entity<Client>, Target, &mut App);

fn tool_menu(client: &WeakEntity<Client>, view: EntityId, title: &SharedString) -> Vec<MenuItem> {
    let tools: [(&str, OpenTool); 3] = [
        ("Environment", super::open_environment),
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    Single,
    Grid,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sidebar {
    Hidden,
    Full,
}

impl Sidebar {
    fn width(self, theme: &Theme) -> Pixels {
        match self {
            Sidebar::Hidden => px(0.0),
            Sidebar::Full => theme.layout.sidebar,
        }
    }
}

fn grid_shape(count: usize) -> (usize, usize) {
    let columns = (count as f64).sqrt().ceil().max(1.0) as usize;
    (columns, count.div_ceil(columns).max(1))
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Home {
    Shared,
    Setup(String),
}

impl Home {
    fn title(&self) -> SharedString {
        match self {
            Home::Shared => "Mira".into(),
            Home::Setup(name) => name.clone().into(),
        }
    }
}

struct GameWindow {
    home: Home,
    handle: WindowHandle<Client>,
    client: WeakEntity<Client>,
}

#[derive(Clone)]
struct Shared {
    data: PathBuf,
    accounts: PathBuf,
    device: reqwest::header::HeaderValue,
    armies: Armies,
    extras: Rc<extras::Store>,
    pace: Pace,
}

struct Game {
    shared: Shared,
    windows: Vec<GameWindow>,
    finished: HashMap<(AccountId, PathBuf), Instant>,
}

impl Global for Game {}

pub(crate) fn init(
    data: PathBuf,
    accounts: PathBuf,
    device: reqwest::header::HeaderValue,
    armies: Armies,
    cx: &mut App,
) {
    super::script_names::init(cx);
    cx.set_global(Game {
        shared: Shared {
            extras: Rc::new(extras::Store::open(data.join("as3.json"))),
            data,
            accounts,
            device,
            armies,
            pace: Pace::default(),
        },
        windows: Vec::new(),
        finished: HashMap::new(),
    });
    cx.on_window_closed(|cx, closed| {
        let windows = &mut cx.global_mut::<Game>().windows;
        let Some(index) = windows
            .iter()
            .position(|game| game.handle.window_id() == closed)
        else {
            return;
        };
        let client = windows.remove(index).client.entity_id();
        cx.defer(move |cx| super::close_tools(client, cx));
    })
    .detach();
}

fn live(cx: &App) -> impl Iterator<Item = (&GameWindow, Entity<Client>)> {
    cx.try_global::<Game>()
        .into_iter()
        .flat_map(|game| &game.windows)
        .filter_map(|game| Some((game, game.client.upgrade()?)))
}

pub(crate) fn clients(cx: &App) -> Vec<Entity<Client>> {
    live(cx).map(|(_, client)| client).collect()
}

pub(crate) fn open_tabs(cx: &App) -> usize {
    live(cx)
        .map(|(_, client)| client.read(cx).sessions.len())
        .sum()
}

pub(crate) fn holding(account: &AccountId, cx: &App) -> Option<WindowHandle<Client>> {
    live(cx)
        .find(|(_, client)| client.read(cx).holds(account, cx))
        .map(|(game, _)| game.handle)
}

pub(crate) fn finished_at(account: &AccountId, entry: &Path, cx: &App) -> Option<Instant> {
    cx.try_global::<Game>()?
        .finished
        .get(&(account.clone(), entry.to_path_buf()))
        .copied()
}

pub(crate) fn open_launch_window(cx: &mut App) {
    if !cx.windows().is_empty() {
        return;
    }
    let Some(game) = cx.try_global::<Game>() else {
        return;
    };
    match settings::start(cx) {
        settings::Start::Client => {
            if let Err(error) = open(Home::Shared, true, cx) {
                tracing::error!(%error, "couldn't open Mira");
            }
        }
        settings::Start::Manager => {
            super::open_manager(game.shared.accounts.clone(), game.shared.device.clone(), cx);
        }
    }
}

pub(super) fn frontmost(cx: &App) -> Option<Entity<Client>> {
    live(cx).next().map(|(_, client)| client)
}

pub(crate) fn current(cx: &mut App) -> anyhow::Result<WindowHandle<Client>> {
    let frontmost = live(cx).next().map(|(game, _)| game.handle);
    match frontmost {
        Some(handle) => Ok(handle),
        None => open(Home::Shared, true, cx),
    }
}

pub(crate) fn rename_setup(old: &str, name: &str, cx: &mut App) {
    if !cx.has_global::<Game>() {
        return;
    }
    let Some(window) = cx
        .global_mut::<Game>()
        .windows
        .iter_mut()
        .find(|game| game.home == Home::Setup(old.to_owned()))
    else {
        return;
    };
    window.home = Home::Setup(name.to_owned());
    let (handle, title) = (window.handle, window.home.title());
    handle
        .update(cx, |_, window, _| window.set_window_title(&title))
        .ok();
}

fn activated(window: gpui::WindowId, cx: &mut App) {
    let windows = &mut cx.global_mut::<Game>().windows;
    if let Some(index) = windows
        .iter()
        .position(|game| game.handle.window_id() == window)
    {
        let game = windows.remove(index);
        windows.insert(0, game);
    }
}

pub(crate) fn open(
    home: Home,
    blank_tab: bool,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<Client>> {
    if let Some((game, _)) = live(cx).find(|(game, _)| game.home == home) {
        return Ok(game.handle);
    }
    let shared = cx
        .try_global::<Game>()
        .ok_or_else(|| anyhow::anyhow!("The client isn't set up"))?
        .shared
        .clone();
    let chrome = cx.theme().layout.chrome;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(960.0), px(550.0) + chrome),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(home.title()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(12.0), px(12.0))),
        }),
        window_background: WindowBackgroundAppearance::Transparent,
        window_min_size: Some(size(px(920.0), px(528.0) + chrome)),
        app_owns_titlebar_drag: true,
        ..Default::default()
    };
    let window = cx.open_window(options, |window, cx| {
        settings::follow_system(window);
        let view = Client::new(shared, blank_tab, window, cx).expect("create the AQW player view");
        window.focus(&view.read(cx).focus_handle(cx), cx);
        view
    })?;
    let client = window.entity(cx)?.downgrade();
    cx.global_mut::<Game>().windows.insert(
        0,
        GameWindow {
            home,
            handle: window,
            client,
        },
    );
    Ok(window)
}

impl Client {
    fn new(
        shared: Shared,
        blank_tab: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> anyhow::Result<Entity<Client>> {
        let Shared {
            data,
            accounts,
            device,
            armies,
            extras,
            pace,
        } = shared;
        let client = cx.new(|cx| {
            cx.observe_global::<crate::updates::Updates>(|_, cx| cx.notify())
                .detach();
            cx.observe_global::<super::script_names::Names>(|_, cx| cx.notify())
                .detach();
            Client {
                extras,
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
                status: None,
                launches: Launches::paced(pace),
                pacing: false,
                unopened: HashMap::new(),
                ended: HashMap::new(),
                closing: None,
                _activation: cx.observe_window_activation(window, |this: &mut Self, window, cx| {
                    if window.is_window_active() {
                        activated(window.window_handle().window_id(), cx);
                    } else {
                        this.hide_shortcut_hints(cx);
                    }
                }),
            }
        });
        if blank_tab {
            client.update(cx, |client, cx| client.open_session(None, window, cx))?;
        }
        Ok(client)
    }

    #[cfg(test)]
    pub(crate) fn empty_for_tests(folder: &std::path::Path, cx: &mut App) -> Entity<Client> {
        cx.new(|cx| {
            cx.observe_global::<crate::updates::Updates>(|_, cx| cx.notify())
                .detach();
            cx.observe_global::<super::script_names::Names>(|_, cx| cx.notify())
                .detach();
            Client {
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
                status: None,
                launches: Launches::default(),
                pacing: false,
                unopened: HashMap::new(),
                ended: HashMap::new(),
                closing: None,
                _activation: Subscription::new(|| {}),
            }
        })
    }

    fn open_session(
        &mut self,
        account: Option<&Account>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<Entity<RuffleView>> {
        let shared_objects = cx.global::<SharedObjects>();
        let storage = match account.filter(|_| settings::separate_aqw_settings(cx)) {
            Some(account) => shared_objects.account(&account.id())?,
            None => shared_objects.shared(),
        };
        let view = RuffleView::new(
            self.data.clone(),
            storage,
            self.device.clone(),
            self.armies.clone(),
            Rc::clone(&self.extras),
            window,
            cx,
        )?;
        let window_size = window.viewport_size();
        let left = self.sidebar.width(cx.theme());
        let player = Bounds::new(
            point(left, cx.theme().layout.chrome),
            gpui::size(
                (window_size.width - left).max(px(1.0)),
                (window_size.height - cx.theme().layout.chrome).max(px(1.0)),
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
            view: view.clone(),
            player,
            _changes: cx.subscribe(&view, |_, _, _: &Changed, cx| cx.notify()),
            _moved: cx.subscribe(&view, |_, _, _: &Updated, cx| cx.emit(Updated)),
            _identified: cx.subscribe(&view, |this, view, Identified(name), cx| {
                this.link(&view, name, cx)
            }),
            _finished: cx.subscribe(&view, |this, view, Finished(entry), cx| {
                if let Some(account) = view.read(cx).account_id() {
                    this.record_finished(account, entry.clone(), cx);
                }
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
        let this = cx.entity_id();
        if self.session_of(&id, cx).is_some()
            || clients(cx)
                .iter()
                .any(|client| client.entity_id() != this && client.read(cx).holds(&id, cx))
        {
            return;
        }
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
        view.update(cx, |view, cx| view.adopt(account, cx));
    }

    fn session_of(&self, account: &AccountId, cx: &App) -> Option<usize> {
        self.sessions
            .iter()
            .position(|session| session.view.read(cx).account_id().as_ref() == Some(account))
    }

    pub(crate) fn holds(&self, account: &AccountId, cx: &App) -> bool {
        self.session_of(account, cx).is_some() || self.launches.position(account).is_some()
    }

    pub(crate) fn uses_settings_of(&self, account: &AccountId, cx: &App) -> bool {
        self.sessions
            .iter()
            .any(|session| session.view.read(cx).settings_account.as_ref() == Some(account))
    }

    pub(crate) fn summary(&self, account: &AccountId, cx: &App) -> Option<Summary> {
        let index = self.session_of(account, cx)?;
        Some(self.sessions[index].view.read(cx).summary(cx))
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
        match self.session_of(&id, cx) {
            Some(index) if !self.needs_login(&id, cx) => {
                if self.launches.remove(&id) {
                    cx.notify();
                }
                let values = match script_inputs::values(&script, cx) {
                    Ok(values) => values,
                    Err(error) => {
                        self.unopened.insert(id, format!("{error:#}").into());
                        cx.notify();
                        return;
                    }
                };
                self.sessions[index].view.update(cx, |view, cx| {
                    view.queue_script(script, values, replace, cx);
                });
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
        self.session_of(account, cx)
            .is_none_or(|index| self.sessions[index].view.read(cx).signed_out())
    }

    pub(crate) fn waiting(&self, account: &AccountId) -> Option<(usize, usize)> {
        self.launches.position(account)
    }

    pub(crate) fn waiting_script(&self, account: &AccountId) -> Option<PathBuf> {
        self.launches.script(account).cloned()
    }

    pub(crate) fn unopened(&self, account: &AccountId) -> Option<SharedString> {
        self.unopened.get(account).cloned()
    }

    pub(crate) fn ended(&self, account: &AccountId) -> Option<ScriptState> {
        self.ended.get(account).cloned()
    }

    fn record_finished(&mut self, account: AccountId, entry: PathBuf, cx: &mut Context<Self>) {
        cx.global_mut::<Game>()
            .finished
            .insert((account, entry), Instant::now());
        cx.notify();
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
        match self.session_of(&id, cx) {
            Some(index) => {
                let queued = match queued_script(script, cx) {
                    Ok(queued) => queued,
                    Err(error) => {
                        self.unopened.insert(id, format!("{error:#}").into());
                        None
                    }
                };
                self.sessions[index].view.update(cx, |view, cx| {
                    view.sign_in_again(&account, cx);
                    if let Some((script, values)) = queued {
                        view.queue_script(script, values, replace, cx);
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
        let queued = queued_script(script, cx);
        let view = self.open_session(Some(account), window, cx)?;
        let (queued, unqueued) = match queued {
            Ok(queued) => (queued, None),
            Err(error) => (None, Some(error)),
        };
        view.update(cx, |view, cx| view.sign_in(account, queued, cx));
        unqueued.map_or(Ok(()), Err)
    }

    pub(crate) fn restart_script(&mut self, account: &AccountId, cx: &mut Context<Self>) {
        if let Some(index) = self.session_of(account, cx) {
            self.sessions[index]
                .view
                .update(cx, |view, cx| view.start_loaded_script(cx));
        }
    }

    pub(crate) fn stop_script(&mut self, account: &AccountId, cx: &mut Context<Self>) {
        if self.launches.remove(account) {
            cx.notify();
        }
        if let Some(index) = self.session_of(account, cx) {
            self.sessions[index]
                .view
                .update(cx, |view, cx| view.stop_script(cx));
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
        if let Some(index) = self.session_of(account, cx) {
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
        if let Some(index) = self.session_of(account, cx) {
            self.close_session(index, window, cx);
        }
    }

    fn asking(&self) -> bool {
        self.closing.as_ref().is_some_and(|closing| closing.open)
    }

    fn request_close(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get(index).filter(|_| !self.asking()) else {
            return;
        };
        let running = session
            .view
            .read(cx)
            .script
            .state(cx)
            .is_some_and(|state| state.running());
        if running && settings::confirm_close(cx) {
            self.closing = Some(Closing {
                open: true,
                view: session.view.entity_id(),
                title: format!("Close {}?", session.title(cx)).into(),
            });
            cx.notify();
        } else {
            self.close_session(index, window, cx);
        }
    }

    fn confirm_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self
            .closing
            .as_ref()
            .filter(|closing| closing.open)
            .map(|closing| closing.view)
        else {
            return;
        };
        if let Some(index) = self
            .sessions
            .iter()
            .position(|session| session.view.entity_id() == view)
        {
            self.close_session(index, window, cx);
        }
    }

    fn dismiss_close(&mut self, cx: &mut Context<Self>) {
        if let Some(closing) = &mut self.closing {
            closing.open = false;
            cx.notify();
        }
    }

    fn close_dialog(&self, cx: &mut Context<Self>) -> Option<Dialog> {
        let closing = self.closing.as_ref()?;
        let dismiss = cx.weak_entity();
        Some(
            Dialog::new("close-tab", closing.open, closing.title.clone())
                .alert()
                .description("A script is still running. Closing this tab will stop it and log out of the game.")
                .action(
                    Button::new("close-tab-cancel", "Cancel")
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_close(cx))),
                )
                .action(
                    Button::new("close-tab-confirm", "Close tab")
                        .variant(Variant::Danger)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.confirm_close(window, cx)),
                        ),
                )
                .on_close(move |_, cx| {
                    dismiss.update(cx, |this, cx| this.dismiss_close(cx)).ok();
                }),
        )
    }

    fn close_session(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.sessions.len() {
            return;
        }
        let session = self.sessions.remove(index);
        if let Some(closing) = self
            .closing
            .as_mut()
            .filter(|closing| closing.view == session.view.entity_id())
        {
            closing.open = false;
        }
        let account = session.view.read(cx).account_id();
        if let Some(account) = &account
            && self.launches.remove(account)
        {
            cx.notify();
        }
        if let Some(account) = account
            && let Some(mut last) = session.view.read(cx).script.state(cx)
        {
            if matches!(
                last.phase,
                ScriptPhase::Running | ScriptPhase::NeedsInput | ScriptPhase::Stopping
            ) {
                last.phase = ScriptPhase::Stopped;
                last.since = Some(Instant::now());
            }
            self.ended.insert(account, last);
        }
        if index < self.active || self.active == self.sessions.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.show(window, cx);
    }

    fn select(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.sessions.len() && !self.asking() {
            self.active = index;
            self.show(window, cx);
        }
    }

    pub(crate) fn set_layout(
        &mut self,
        layout: Layout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.layout == layout {
            return;
        }
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

    pub(super) fn show_status(&mut self, option: &str, on: bool, cx: &mut Context<Self>) {
        let hide = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(STATUS_LINGER).await;
            let _ = this.update(cx, |this: &mut Self, cx| {
                if let Some(status) = &mut this.status {
                    status.left = Some(Instant::now());
                }
                cx.notify();
            });
        });
        let entered = match &self.status {
            Some(status) if status.left.is_none() => status.entered,
            _ => cx.background_executor().now(),
        };
        self.status = Some(Status {
            states: [
                format!("{option}: Off").into(),
                format!("{option}: On").into(),
            ],
            on,
            entered,
            left: None,
            _hide: hide,
        });
        cx.notify();
    }

    fn status(&mut self, window: &mut Window, cx: &mut App) -> Option<impl IntoElement + use<>> {
        let layout = cx.theme().layout;
        let typography = cx.theme().typography;
        let status = self.status.as_ref()?;
        let progress = match status.left {
            None => mira_ui::motion::settling(status.entered, window, cx),
            Some(since) => match mira_ui::motion::leaving(since, window, cx) {
                Some(opacity) => opacity,
                None => {
                    self.status = None;
                    return None;
                }
            },
        };
        let mut font = window.text_style().font();
        font.weight = gpui::FontWeight::MEDIUM;
        let widest = status
            .states
            .iter()
            .map(|state| {
                let run = gpui::TextRun {
                    len: state.len(),
                    font: font.clone(),
                    color: gpui::Hsla::default(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                window
                    .text_system()
                    .shape_line(state.clone(), typography.body, &[run], None)
                    .width
            })
            .fold(Pixels::ZERO, Pixels::max);
        let text = status.states[usize::from(status.on)].clone();
        let theme = cx.theme();
        let pill = div()
            .id("hotkey-status")
            .debug_selector(|| "hotkey-status".into())
            .role(Role::Status)
            .aria_label(text.clone())
            .w(widest.ceil() + STATUS_PADDING * 2.0)
            .max_w_full()
            .px(STATUS_PADDING)
            .py(layout.space(2.0))
            .rounded(theme.radius.panel)
            .bg(theme.popover)
            .shadow(theme.popup_shadow())
            .child(theme.surface_ring(theme.radius.panel))
            .text_size(typography.body)
            .line_height(typography.body_line)
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(theme.foreground)
            .whitespace_nowrap()
            .child(div().overflow_hidden().child(text));
        Some(
            h_flex()
                .absolute()
                .top(cx.theme().layout.chrome + px(12.0))
                .left_0()
                .right_0()
                .justify_center()
                .px(layout.space(3.0))
                .child(mira_ui::motion::from_above(pill, progress)),
        )
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

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let layout = theme.layout;
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
                    glance: view.glance(cx),
                    details: view.details(cx),
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
            let tab = row.tab(&face_theme, index == active).menu({
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
                    .update(cx, |client, cx| client.request_close(index, window, cx))
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
        theme
            .sidebar_pane()
            .h_full()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(layout.space(0.5))
                    .h(cx.theme().layout.chrome)
                    .pr(layout.space(2.0))
                    .child(drag_area().flex_1())
                    .child(
                        bar_icon_button("new-session", Icon::Plus, "New tab").on_click(
                            cx.listener(|this, _, window, cx| {
                                if let Err(error) = this.open_session(None, window, cx) {
                                    tracing::error!(%error, "couldn't open a session");
                                }
                            }),
                        ),
                    )
                    .child(self.grid_toggle(cx)),
            )
            .child(div().flex_none().child(tabs))
            .child(drag_area().flex_1())
            .children(
                crate::updates::button(Placement::new(Side::Right, Align::End), cx).map(|update| {
                    h_flex()
                        .flex_none()
                        .justify_end()
                        .p(layout.space(2.0))
                        .child(update)
                }),
            )
    }

    fn sidebar_toggle(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let layout = cx.theme().layout;
        let label = match self.sidebar {
            Sidebar::Hidden => "Show sidebar",
            Sidebar::Full => "Hide sidebar",
        };
        let glyph = match self.sidebar {
            Sidebar::Hidden => Icon::Sidebar,
            Sidebar::Full => Icon::SidebarOpen,
        };
        let dot = (self.sidebar == Sidebar::Hidden)
            .then(|| crate::updates::dot(cx).map(|dot| dot.top(px(0.0)).right(px(-1.0))))
            .flatten();
        let mut toggle = bar_icon_button("sidebar-toggle", glyph, label)
            .on_click(cx.listener(|this, _, window, cx| this.toggle_sidebar(window, cx)));
        if dot.is_some() {
            toggle = toggle.tooltip("Show sidebar · an update is available");
        }
        div()
            .absolute()
            .left(traffic_lights_inset(window))
            .top((cx.theme().layout.chrome - layout.control_sm) / 2.0)
            .occlude()
            .child(toggle)
            .children(dot)
    }

    fn toggle_grid(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = match self.layout {
            Layout::Single => Layout::Grid,
            Layout::Grid => Layout::Single,
        };
        self.set_layout(next, window, cx);
    }

    fn grid_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (label, glyph) = match self.layout {
            Layout::Single => ("Grid view", Icon::LayoutGrid),
            Layout::Grid => ("Single view", Icon::Square),
        };
        bar_icon_button("grid", glyph, label)
            .on_click(cx.listener(|this, _, window, cx| this.toggle_grid(window, cx)))
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
            .border_color(theme.border)
            .when(column > 0, |tile| tile.border_l_1())
            .when(column + 1 < columns, |tile| tile.border_r_1())
            .when(row > 0, |tile| tile.border_t_1())
            .when(row + 1 < rows, |tile| tile.border_b_1())
            .child(self.sessions[index].view.clone())
            .when(index != self.active, |tile| {
                tile.child(div().absolute().inset_0().bg(gpui::black().opacity(0.4)))
            })
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
                            cell.bg(theme.background)
                        }
                    }))
            }))
    }

    fn empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let layout = theme.layout;
        let typography = theme.typography;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .items_center()
            .justify_center()
            .gap(layout.space(1.5))
            .bg(theme.background)
            .text_size(typography.body)
            .text_color(theme.muted_foreground)
            .child("No tabs are open.")
            .child(
                Button::new("open-session", "New tab")
                    .variant(Variant::Outline)
                    .mt(layout.space(2.0))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Err(error) = this.open_session(None, window, cx) {
                            tracing::error!(%error, "couldn't open a session");
                        }
                    })),
            )
    }
}

fn tab_title(
    theme: &Theme,
    label: &SharedString,
    active: bool,
    hint: Option<usize>,
) -> impl IntoElement {
    let typography = theme.typography;
    div()
        .flex()
        .items_center()
        .gap(theme.space(2.0))
        .h(typography.body_line)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .line_height(typography.body_line)
                .truncate()
                .when(active, |name| name.font_weight(gpui::FontWeight::MEDIUM))
                .child(label.clone()),
        )
        .children(hint.map(|number| Kbd::new(format!("⌘{number}"))))
}

fn tab_glance(theme: &Theme, glance: Option<Glance>) -> impl IntoElement {
    let typography = theme.typography;
    div()
        .mt(theme.space(1.0))
        .h(typography.secondary_line)
        .truncate()
        .text_caption(typography)
        .children(glance.map(|glance| {
            div()
                .truncate()
                .text_color(match glance.tone {
                    Tone::Neutral => theme.muted_foreground,
                    tone => theme.tone(tone).2,
                })
                .child(glance.label)
        }))
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

fn bar_icon_button(id: &'static str, glyph: Icon, label: &'static str) -> Button {
    Button::icon(id, glyph, label)
        .size(Size::Small)
        .icon_size(px(14.0))
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
    let layout = theme.layout;
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(layout.space(0.5))
        .h(cx.theme().layout.chrome)
        .pr(BAR_PADDING)
        .bg(theme.frame)
        .border_b_1()
        .border_color(theme.border)
}

pub(crate) fn window_bar(window: &Window, cx: &App) -> gpui::Div {
    bar(cx).child(drag_area().flex_none().w(traffic_lights_inset(window)))
}

pub(crate) fn bar_title(title: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let layout = cx.theme().layout;
    let typography = cx.theme().typography;
    drag_area()
        .flex_none()
        .flex()
        .items_center()
        .px(layout.space(2.0))
        .text_size(typography.body)
        .line_height(typography.body_line)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(title.into())
}

pub(crate) fn bar_separator(cx: &App) -> gpui::Div {
    let layout = cx.theme().layout;
    div()
        .flex_none()
        .mx(layout.space(1.0))
        .w(px(1.0))
        .h(px(16.0))
        .bg(cx.theme().border)
}

impl Render for Client {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let inset = bar_inset(window, self.sidebar.width(cx.theme()));
        for session in &self.sessions {
            if session.view.read(cx).bar_inset != inset {
                session.view.update(cx, |view, _| view.bar_inset = inset);
            }
        }
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
        self.frame(body, window, cx)
    }
}

impl Client {
    fn frame(
        &mut self,
        body: impl IntoElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let (bar_background, bar_border) = (cx.theme().frame, cx.theme().border);
        let show_empty_bar = self.sessions.is_empty();
        div()
            .window_text(cx.theme())
            .id("client")
            .role(Role::Group)
            .track_focus(&self.focus)
            .flex()
            .size_full()
            .child(mira_ui::nav::pointer_probe())
            .child(crate::quit::Overlay)
            .on_action(cx.listener(|this, _: &NewSession, window, cx| {
                if let Err(error) = this.open_session(None, window, cx) {
                    tracing::error!(%error, "couldn't open a session");
                }
            }))
            .on_action(cx.listener(|this, _: &CloseSession, window, cx| {
                if this.sessions.is_empty() {
                    window.remove_window();
                } else {
                    this.request_close(this.active, window, cx);
                }
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
            .on_action(cx.listener(|this, _: &ToggleGrid, window, cx| this.toggle_grid(window, cx)))
            .when_some(
                self.sessions
                    .get(self.active)
                    .map(|session| session.view.clone()),
                |root, view| super::game_bar::on_actions(root, view, cx.weak_entity()),
            )
            .on_action(cx.listener(|this, action: &SelectSession, window, cx| {
                if let Some(&index) = this.shortcut_tabs().get(action.0) {
                    this.select(index, window, cx)
                }
            }))
            .when(self.sidebar == Sidebar::Full, |root| {
                root.child(self.sidebar(cx))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .relative()
                    .min_w_0()
                    .h_full()
                    .when(show_empty_bar, |column| {
                        column.child(
                            drag_area()
                                .flex_none()
                                .w_full()
                                .h(cx.theme().layout.chrome)
                                .bg(bar_background)
                                .border_b_1()
                                .border_color(bar_border),
                        )
                    })
                    .child(body)
                    .children(self.status(window, cx)),
            )
            .child(self.sidebar_toggle(window, cx))
            .children(self.close_dialog(cx))
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

fn queued_script(script: Option<PathBuf>, cx: &App) -> anyhow::Result<Option<(PathBuf, Values)>> {
    script
        .map(|script| script_inputs::values(&script, cx).map(|values| (script, values)))
        .transpose()
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

    struct Shown(Entity<Client>);

    impl Render for Shown {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.0.clone())
        }
    }

    fn client_window(
        cx: &mut gpui::TestAppContext,
    ) -> (Entity<Client>, &mut gpui::VisualTestContext) {
        let folder = std::env::temp_dir().join(format!("mira-client-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        cx.update(mira_ui::init);
        let client = cx.update(|cx| {
            crate::settings::init(folder.join("settings.json"), |_| {}, cx);
            Client::empty_for_tests(&folder, cx)
        });
        let (_, cx) = cx.add_window_view(|_, _| Shown(client.clone()));
        cx.run_until_parked();
        (client, cx)
    }

    #[gpui::test]
    fn cmd_w_closes_an_empty_client_and_preserves_finished_scripts(cx: &mut gpui::TestAppContext) {
        let folder = std::env::temp_dir().join(format!("mira-finished-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        cx.update(|cx| {
            mira_ui::init(cx);
            bind_keys(cx);
            crate::settings::init(folder.join("settings.json"), |_| {}, cx);
            init(
                folder.clone(),
                folder.join("accounts.json"),
                reqwest::header::HeaderValue::from_static("test"),
                Armies::new(folder.join("armies")),
                cx,
            );
        });
        let (account, entry) = (AccountId::of("wildfox"), PathBuf::from("daily.ts"));
        let window = cx
            .update(|cx| open(Home::Setup("Daily".into()), false, cx))
            .unwrap();
        window
            .update(cx, |client, _, cx| {
                client.record_finished(account.clone(), entry.clone(), cx)
            })
            .unwrap();
        cx.simulate_keystrokes(window.into(), "cmd-w");
        assert!(!cx.windows().contains(&window.into()));
        assert!(cx.update(|cx| clients(cx).is_empty()));
        assert!(cx.update(|cx| finished_at(&account, &entry, cx)).is_some());
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[gpui::test]
    fn tools_open_for_an_open_client_and_never_open_one(cx: &mut gpui::TestAppContext) {
        use crate::ruffle_view::{
            LoadScript, OpenEnvironment, OpenLoaderGrabber, OpenPackets, OpenScripts, init_tools,
        };
        let folder = std::env::temp_dir().join(format!("mira-tools-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        cx.update(|cx| {
            mira_ui::init(cx);
            crate::settings::init(folder.join("settings.json"), |_| {}, cx);
            init(
                folder.clone(),
                folder.join("accounts.json"),
                reqwest::header::HeaderValue::from_static("test"),
                Armies::new(folder.join("armies")),
                cx,
            );
            init_tools(folder.join("scripts"), folder.join("types"), cx);
        });
        let tools: [&dyn gpui::Action; 5] = [
            &OpenPackets,
            &OpenEnvironment,
            &OpenLoaderGrabber,
            &OpenScripts,
            &LoadScript,
        ];
        for tool in tools {
            cx.update(|cx| cx.dispatch_action(tool));
            cx.run_until_parked();
            assert!(cx.windows().is_empty(), "{} opened a window", tool.name());
        }
        let client = cx
            .update(|cx| open(Home::Setup("Daily".into()), false, cx))
            .unwrap();
        cx.update(|cx| cx.dispatch_action(&OpenPackets));
        cx.run_until_parked();
        assert_eq!(cx.windows().len(), 2);
        assert!(cx.windows().contains(&client.into()));
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[gpui::test]
    fn a_status_fits_its_text_replaces_the_last_one_and_clears_itself(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let (client, cx) = client_window(cx);
        let show = |option: &str, on: bool, cx: &mut gpui::VisualTestContext| {
            client.update(cx, |client, cx| client.show_status(option, on, cx));
            cx.run_until_parked();
            cx.debug_bounds("hotkey-status")
                .map(|bounds| bounds.size.width)
        };
        let short = show("Anti-AFK", true, cx);
        cx.executor().advance_clock(STATUS_LINGER / 2);
        let long = show("Skip cutscenes", false, cx);
        assert!(short.is_some());
        assert!(long > short);
        assert_eq!(show("Skip cutscenes", true, cx), long);
        cx.executor().advance_clock(STATUS_LINGER * 3 / 4);
        cx.run_until_parked();
        assert!(cx.debug_bounds("hotkey-status").is_some());
        cx.executor().advance_clock(STATUS_LINGER / 2);
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert!(cx.debug_bounds("hotkey-status").is_none());
    }

    #[test]
    fn snapshot_hotkey_status() {
        let folder = std::env::temp_dir().join("mira-hotkey-status-snapshot");
        std::fs::create_dir_all(&folder).unwrap();
        for path in mira_ui::snapshot(
            "hotkey-status",
            gpui::size(px(640.0), px(200.0)),
            move |_, cx| {
                crate::settings::init(folder.join("settings.json"), |_| {}, cx);
                let client = Client::empty_for_tests(&folder, cx);
                client.update(cx, |client, cx| {
                    client.show_status("Infinite range", true, cx)
                });
                cx.new(|_| Shown(client))
            },
        ) {
            assert!(path.exists(), "{} was not written", path.display());
        }
    }

    #[test]
    fn snapshot_close_running_tab() {
        let folder = std::env::temp_dir().join("mira-close-tab-snapshot");
        std::fs::create_dir_all(&folder).unwrap();
        for path in mira_ui::snapshot(
            "client-close-running-tab",
            gpui::size(px(720.0), px(420.0)),
            move |_, cx| {
                crate::settings::init(folder.join("settings.json"), |_| {}, cx);
                let client = Client::empty_for_tests(&folder, cx);
                client.update(cx, |client, cx| {
                    client.closing = Some(Closing {
                        open: true,
                        view: cx.entity_id(),
                        title: "Close Player1?".into(),
                    });
                });
                cx.new(|_| Shown(client))
            },
        ) {
            assert!(path.exists(), "{} was not written", path.display());
        }
    }

    #[gpui::test]
    fn the_sidebar_offers_an_update_found_after_it_rendered(cx: &mut gpui::TestAppContext) {
        let (client, cx) = client_window(cx);
        client.update(cx, |client, cx| {
            client.sidebar = Sidebar::Full;
            cx.notify();
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("bar-update").is_none());
        cx.update(|_, cx| {
            cx.set_global(crate::updates::Updates::available_preview(
                semver::Version::new(0, 2, 0),
                None,
            ))
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("bar-update").is_some());
    }

    #[test]
    fn snapshot_bar_updates() {
        use crate::updates::Updates;
        use semver::Version;
        let update = |state: &str| {
            let next = Version::new(0, 2, 0);
            match state {
                "available" | "collapsed" => Updates::available_preview(next, None),
                "downloading" => Updates::downloading_preview(next, 0.4),
                "ready" => Updates::ready_preview(next),
                _ => Updates::available_preview(next, Some("Couldn’t download Mira 0.2.0".into())),
            }
        };
        for state in [
            "available",
            "downloading",
            "ready",
            "download-failed",
            "collapsed",
        ] {
            let folder = crate::storyboard::scratch(&format!("client-update-{state}"));
            for path in mira_ui::snapshot(
                &format!("client-update-{state}"),
                gpui::size(px(720.0), px(360.0)),
                move |_, cx| {
                    crate::settings::init(folder.join("settings.json"), |_| {}, cx);
                    cx.set_global(update(state));
                    let client = Client::empty_for_tests(&folder, cx);
                    if state != "collapsed" {
                        client.update(cx, |client, _| client.sidebar = Sidebar::Full);
                    }
                    cx.new(|_| Shown(client))
                },
            ) {
                assert!(path.exists(), "{} was not written", path.display());
            }
        }
    }

    #[test]
    fn snapshot_sidebar_update_card() {
        use crate::updates::Updates;
        let folder = crate::storyboard::scratch("client-update-card");
        for path in mira_ui::snapshot_with(
            "client-update-card",
            gpui::size(px(720.0), px(360.0)),
            move |_, cx| {
                crate::settings::init(folder.join("settings.json"), |_| {}, cx);
                cx.set_global(Updates::available_preview(
                    semver::Version::new(0, 2, 0),
                    None,
                ));
                let client = Client::empty_for_tests(&folder, cx);
                client.update(cx, |client, _| client.sidebar = Sidebar::Full);
                cx.new(|_| Shown(client))
            },
            |cx, owner| {
                for nudge in [0.0, 1.0] {
                    cx.update_window(owner, |_, window, cx| {
                        window.dispatch_event(
                            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                position: gpui::point(px(188.0 + nudge), px(340.0)),
                                ..Default::default()
                            }),
                            cx,
                        );
                        window.refresh();
                    })
                    .unwrap();
                    cx.run_until_parked();
                }
                cx.advance_clock(std::time::Duration::from_secs(1));
            },
        ) {
            assert!(path.exists(), "{} was not written", path.display());
        }
    }

    struct SessionRow;

    impl Render for SessionRow {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            let row = SessionTab {
                view: cx.entity_id(),
                number: 1,
                title: "Artix".into(),
                glance: Some(Glance {
                    label: "Twig".into(),
                    tone: Tone::Neutral,
                }),
                details: Details {
                    server: Some("Twig".into()),
                    script: Some("daily.ts".into()),
                    error: None,
                },
                hint: None,
            };
            div()
                .size_full()
                .window_text(&theme)
                .bg(gpui::black())
                .child(
                    div()
                        .w(theme.layout.sidebar)
                        .h_full()
                        .bg(theme.frame)
                        .child(
                            Tabs::new("sessions", "Tabs", 1, move |_| {
                                let (view, title) = (row.view, row.title.clone());
                                row.tab(&theme, true)
                                    .menu(move |_, _| {
                                        tool_menu(&WeakEntity::new_invalid(), view, &title)
                                    })
                                    .on_close(|_, _| {})
                            })
                            .vertical()
                            .on_reorder(|_, _, _, _| {}),
                        ),
                )
        }
    }

    struct StressRows;

    impl Render for StressRows {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            let view = cx.entity_id();
            let rows: Vec<SessionTab> = [
                ("The Legendary Hero Of Battleon And Beyond", "Swordhaven (EU) · tercessuinotlim-99999", Tone::Neutral, Some(1)),
                ("xXx_SuperLongUsernameWithoutAnySpaces_xXx", "legion-revenant-fealty-and-exalted-crown-farm.ts is waiting for you", Tone::Attention, Some(2)),
                ("a", "Couldn't join house-73675086, the room is full", Tone::Danger, None),
                ("Wwwwwwwwwwwwwwwwwwwwwwwwwwwwww", "Logging in, attempt 2 of 3", Tone::Neutral, Some(10)),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (title, label, tone, hint))| SessionTab {
                view,
                number: index + 1,
                title: title.into(),
                glance: Some(Glance { label: label.into(), tone }),
                details: Details {
                    server: Some("Swordhaven (EU)".into()),
                    script: Some("/Users/player/Documents/Mira/scripts/legion/daily-quests-and-weekly-bosses/legion-revenant-fealty-and-exalted-crown-farm.ts".into()),
                    error: Some("Couldn't join house-73675086, the room is full and the fallback room house-73675087 is full too".into()),
                },
                hint,
            })
            .collect();
            div()
                .size_full()
                .window_text(&theme)
                .bg(gpui::black())
                .child(
                    div()
                        .w(theme.layout.sidebar)
                        .h_full()
                        .bg(theme.frame)
                        .child(
                            Tabs::new("sessions", "Tabs", rows.len(), move |index| {
                                let row = &rows[index];
                                let (view, title) = (row.view, row.title.clone());
                                row.tab(&theme, index == 0)
                                    .menu(move |_, _| {
                                        tool_menu(&WeakEntity::new_invalid(), view, &title)
                                    })
                                    .on_close(|_, _| {})
                            })
                            .vertical()
                            .on_reorder(|_, _, _, _| {}),
                        ),
                )
        }
    }

    #[test]
    fn snapshot_stress_sessions() {
        mira_ui::snapshot_with(
            "stress-sessions",
            gpui::size(px(600.0), px(320.0)),
            |_, cx| cx.new(|_| StressRows),
            |cx, owner| {
                for nudge in [0.0, 1.0] {
                    cx.update_window(owner, |_, window, cx| {
                        window.dispatch_event(
                            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                position: point(px(42.0 + nudge), px(80.0)),
                                ..Default::default()
                            }),
                            cx,
                        );
                        window.refresh();
                    })
                    .unwrap();
                    cx.run_until_parked();
                }
                cx.advance_clock(std::time::Duration::from_secs(1));
            },
        );
    }

    #[test]
    fn session_hover_card_opens_beside_the_whole_row() {
        mira_ui::snapshot_with(
            "session-hover-card",
            gpui::size(px(600.0), px(150.0)),
            |_, cx| cx.new(|_| SessionRow),
            |cx, owner| {
                let before = cx.capture_screenshot(owner).unwrap();
                for nudge in [0.0, 1.0] {
                    cx.update_window(owner, |_, window, cx| {
                        window.dispatch_event(
                            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                position: point(px(42.0 + nudge), px(24.0)),
                                ..Default::default()
                            }),
                            cx,
                        );
                        window.refresh();
                    })
                    .unwrap();
                    cx.run_until_parked();
                }
                cx.advance_clock(std::time::Duration::from_secs(1));
                cx.update_window(owner, |_, window, _| window.refresh())
                    .unwrap();
                cx.run_until_parked();
                let after = cx.capture_screenshot(owner).unwrap();
                let scale = before.width() / 600;
                let point = |x, y| (x * scale, y * scale);
                let (x, y) = point(260, 17);
                assert_ne!(
                    before.get_pixel(x, y),
                    after.get_pixel(x, y),
                    "card alongside the row top"
                );
                let (x, y) = point(190, 33);
                assert_eq!(
                    before.get_pixel(x, y),
                    after.get_pixel(x, y),
                    "row stays uncovered"
                );
                let pixel = |x, y| {
                    let (x, y) = point(x, y);
                    after.get_pixel(x, y)
                };
                let edge = pixel(204, 40);
                assert_eq!(
                    pixel(300, 8),
                    edge,
                    "card edge over the stage matches its edge over the sidebar"
                );
                assert_ne!(pixel(208, 40), edge, "card edge stands out from its fill");
            },
        );
    }

    #[test]
    fn snapshot_session_tool_menu() {
        mira_ui::snapshot_with(
            "session-tool-menu",
            size(px(260.0), px(154.0)),
            |_, cx| cx.new(|_| SessionRow),
            |cx, owner| {
                cx.update_window(owner, |_, window, cx| {
                    window.dispatch_event(
                        gpui::PlatformInput::MouseDown(MouseDownEvent {
                            button: MouseButton::Right,
                            position: point(px(42.0), px(16.0)),
                            click_count: 1,
                            ..Default::default()
                        }),
                        cx,
                    );
                })
                .unwrap();
                cx.run_until_parked();
            },
        );
    }

    #[test]
    fn native_player_shows_through_the_client_while_chrome_stays_opaque() {
        struct PlayerFrame(Entity<Client>);

        impl Render for PlayerFrame {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let frame = self.0.update(cx, |client, cx| {
                    let body = div().flex_1().child(bar(cx));
                    client.frame(body, window, cx)
                });
                div().size_full().bg(gpui::rgb(0xff00ff)).child(frame)
            }
        }

        let folder = std::env::temp_dir().join("mira-player-frame-snapshot");
        std::fs::create_dir_all(&folder).unwrap();
        mira_ui::snapshot_with(
            "native-player-frame",
            gpui::size(px(960.0), px(588.0)),
            |_, cx| {
                crate::settings::init(folder.join("settings.json"), |_| {}, cx);
                let client = Client::empty_for_tests(&folder, cx);
                client.update(cx, |client, _| client.sidebar = Sidebar::Full);
                cx.new(|_| PlayerFrame(client))
            },
            |cx, window| {
                let image = cx.capture_screenshot(window).unwrap();
                let (w, h) = image.dimensions();
                let player = [255, 0, 255, 255];
                assert_eq!(image.get_pixel(w / 2, h / 2).0, player, "stage");
                assert_ne!(image.get_pixel(w / 12, h / 2).0, player, "sidebar");
                assert_ne!(image.get_pixel(w / 2, h / 32).0, player, "game bar");
            },
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
