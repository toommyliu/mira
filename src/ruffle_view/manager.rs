use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, AppContext as _, Bounds, ClickEvent, Context, Entity, FocusHandle, Focusable as _,
    FontWeight, Global, InteractiveElement as _, IntoElement, KeyBinding, KeyDownEvent,
    MouseButton, ParentElement as _, PathPromptOptions, Render, Role, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TitlebarOptions, Toggled,
    WeakEntity, Window, WindowBounds, WindowHandle, WindowOptions, actions, div,
    prelude::FluentBuilder as _, px, size,
};
use reqwest::header::HeaderValue;

use super::{
    Client, Login, ScriptPhase, ScriptState, Summary, Updated,
    account::file_name,
    account_card::{Face, Fix, Line, Mood, Presence, Show, face},
    client::{CloseSession, SIDEBAR_WIDTH, bar_title, drag_area, window_bar},
    form::note,
    scripts::{fit_path_to, home_relative},
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, StyledExt as _, Theme, Tone,
    components::Empty,
    h_flex,
    primitives::{
        Alert, Badge, Button, ContextMenu, Dialog, DropdownMenu, Field, Icon, Input, InputEvent,
        MenuItem, MenuList, Scrollbar, Spinner, Tab, Tabs, Tooltip,
        button::{Size, Variant},
        small_icon,
    },
    theme::{LINE_SMALL, TEXT_BODY, TEXT_SMALL, tabular},
    v_flex,
};

use crate::{
    accounts::{Account, AccountId, Accounts, Change, Draft, Invalid, SaveError, SetupError},
    net,
    setups::{Member, Setup},
};

actions!(
    mira,
    [
        OpenAccounts,
        AddAccount,
        NewSetup,
        StartSetup,
        SelectShown,
        ClearSelection,
        FindAccount
    ]
);

type Pick = Rc<dyn Fn(&mut Manager, Option<PathBuf>, &mut Context<Manager>)>;

const CONTEXT: &str = "Accounts";
const MAX_FIELD: usize = 64;
const CARD_WIDTH: f32 = 248.0;
const CARD_LINES: usize = 3;
const CARD_PATH_WIDTH: gpui::Pixels = px(CARD_WIDTH - 24.0);
const RECENT_LIMIT: usize = 8;
const SCRIPT_MENU_WIDTH: gpui::Pixels = px(260.0);
const SCRIPT_PATH_WIDTH: gpui::Pixels = px(186.0);
const CHOOSE_IN_GAME: &str = "Choose in game";
const SERVERS_FRESH_FOR: Duration = Duration::from_secs(60);
const SERVERS_RECHECK_AFTER: Duration = Duration::from_secs(5);
const SERVER_LIST_HEIGHT: gpui::Pixels = px(264.0);

struct Open(WindowHandle<Manager>);

impl Global for Open {}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-shift-n", AddAccount, Some(CONTEXT)),
        KeyBinding::new("cmd-n", NewSetup, Some(CONTEXT)),
        KeyBinding::new("cmd-enter", StartSetup, Some(CONTEXT)),
        KeyBinding::new("cmd-a", SelectShown, Some(CONTEXT)),
        KeyBinding::new("cmd-f", FindAccount, Some(CONTEXT)),
        KeyBinding::new("escape", ClearSelection, Some(CONTEXT)),
    ]);
}

pub(crate) fn open(
    game: WindowHandle<Client>,
    client: Entity<Client>,
    path: PathBuf,
    device: HeaderValue,
    cx: &mut App,
) {
    if let Some(manager) = cx.try_global::<Open>().map(|open| open.0)
        && manager
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1000.0), px(600.0)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Accounts".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(12.0))),
        }),
        app_owns_titlebar_drag: true,
        window_min_size: Some(size(px(720.0), px(420.0))),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let manager = cx.new(|cx| Manager::new(game, client, path, device, window, cx));
        let focus = manager.read(cx).focus.clone();
        window.focus(&focus, cx);
        manager
    }) {
        Ok(manager) => cx.set_global(Open(manager)),
        Err(error) => tracing::error!(%error, "couldn't open the account manager"),
    }
}

#[derive(Clone, PartialEq, Eq)]
enum View {
    All,
    Setup(String),
}

pub(crate) struct Manager {
    window: WindowHandle<Client>,
    client: Entity<Client>,
    accounts: Result<Accounts, SharedString>,
    device: HeaderValue,
    servers: Servers,
    view: View,
    show: Show,
    checked: HashSet<AccountId>,
    cursor: Option<AccountId>,
    columns: usize,
    recent: Vec<PathBuf>,
    form: Option<Form>,
    bulk: Option<Bulk>,
    renaming: Option<Renaming>,
    confirm: Option<Confirming>,
    notice: Option<SharedString>,
    filter: Entity<Input>,
    grid_scroll: ScrollHandle,
    focus: FocusHandle,
    _subscriptions: [Subscription; 4],
}

struct Servers {
    list: Vec<net::Server>,
    check: Check,
}

enum Check {
    Idle,
    Running,
    Done(Instant),
    Failed(Instant),
}

impl Servers {
    fn stale(&self) -> bool {
        match self.check {
            Check::Idle | Check::Failed(_) => true,
            Check::Running => false,
            Check::Done(at) => at.elapsed() >= SERVERS_FRESH_FOR,
        }
    }

    fn ready(&self) -> bool {
        match self.check {
            Check::Idle => true,
            Check::Running => false,
            Check::Done(at) | Check::Failed(at) => at.elapsed() >= SERVERS_RECHECK_AFTER,
        }
    }

    fn note(&self) -> SharedString {
        match self.check {
            Check::Idle | Check::Running => "Checking servers…".into(),
            Check::Done(at) => match at.elapsed().as_secs() / 60 {
                0 => "Checked just now".into(),
                minutes => format!("Checked {minutes} min ago").into(),
            },
            Check::Failed(_) => "Couldn't check the servers. Refresh to try again.".into(),
        }
    }
}

type PickServer = Rc<dyn Fn(Option<String>, &mut Window, &mut App)>;

fn server_items(
    first: &'static str,
    servers: &[net::Server],
    current: Option<Option<&str>>,
    theme: &Theme,
    pick: &PickServer,
) -> Vec<MenuItem> {
    let item = |label: String, server: Option<String>| {
        let pick = pick.clone();
        let checked = current == Some(server.as_deref());
        MenuItem::action(label, move |window, cx| pick(server.clone(), window, cx)).checked(checked)
    };
    let mut items = vec![
        item(first.to_owned(), None).pinned(),
        MenuItem::separator().pinned(),
    ];
    items.extend(servers.iter().map(|server| {
        item(server.name.clone(), Some(server.name.clone())).end_slot(server_stats(server, theme))
    }));
    if let Some(saved) = current
        .flatten()
        .filter(|saved| !servers.iter().any(|server| server.name == *saved))
    {
        items.push(item(saved.to_owned(), Some(saved.to_owned())));
    }
    items
}

fn server_stats(server: &net::Server, theme: &Theme) -> gpui::Div {
    let (players, color) = if !server.online {
        (SharedString::from("Offline"), theme.text_tertiary)
    } else {
        (
            format!("{}/{}", server.players, server.capacity).into(),
            if server.players >= server.capacity {
                theme.warning_text
            } else {
                theme.text_secondary
            },
        )
    };
    h_flex()
        .gap(px(10.0))
        .ml(px(16.0))
        .text_size(TEXT_SMALL)
        .font_features(tabular())
        .when(server.members, |stats| stats.child(Badge::new("Member")))
        .child(div().text_color(color).child(players))
        .child(
            div()
                .w(px(44.0))
                .flex()
                .justify_end()
                .text_color(theme.text_tertiary)
                .children(server.ping.map(|ping| format!("{} ms", ping.as_millis()))),
        )
}

struct Form {
    open: bool,
    editing: Option<AccountId>,
    name: Entity<Input>,
    username: Entity<Input>,
    password: Entity<Input>,
    server: Option<String>,
    error: Option<Problem>,
    _inputs: [Subscription; 3],
}

#[derive(Clone)]
enum Problem {
    Username(SharedString),
    Password(SharedString),
    Other(SharedString),
}

impl Form {
    fn draft(&self, cx: &App) -> Draft {
        let text = |input: &Entity<Input>| input.read(cx).text().to_owned();
        Draft {
            name: text(&self.name),
            username: text(&self.username),
            password: text(&self.password),
            server: self.server.clone(),
        }
    }
}

struct Bulk {
    open: bool,
    ids: HashSet<AccountId>,
    change: Change,
    error: Option<SharedString>,
}

struct Renaming {
    setup: String,
    input: Entity<Input>,
    error: Option<SharedString>,
    _subscriptions: [Subscription; 2],
}

#[derive(Clone)]
enum ScriptFor {
    Setup,
    Members(Vec<AccountId>),
}

struct Confirming {
    open: bool,
    ask: Ask,
}

#[derive(Clone)]
enum Ask {
    Close {
        targets: Vec<Account>,
        running: usize,
    },
    Start {
        setup: String,
        scope: Scope,
        conflicts: Vec<String>,
    },
    Delete {
        setup: String,
    },
    Remove {
        account: Account,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Everyone,
    Failed,
}

#[derive(Default, Debug, PartialEq, Eq)]
struct Plan {
    start: Vec<(AccountId, PathBuf)>,
    conflicts: Vec<(AccountId, PathBuf)>,
    login: Vec<AccountId>,
    unscripted: Vec<AccountId>,
}

fn live(presence: &Presence) -> Option<&Summary> {
    match presence {
        Presence::Open(summary)
        | Presence::Waiting {
            open: Some(summary),
            ..
        } => Some(summary),
        _ => None,
    }
}

fn running_entry(presence: &Presence) -> Option<&Path> {
    match live(presence)?.script.as_ref()? {
        ScriptState {
            entry,
            phase: ScriptPhase::Running | ScriptPhase::NeedsInput,
            ..
        } => Some(entry),
        _ => None,
    }
}

fn plan(setup: &Setup, presences: &HashMap<AccountId, Presence>, scope: Scope) -> Plan {
    let mut plan = Plan::default();
    for member in &setup.members {
        let id = member.id();
        let Some(presence) = presences.get(&id) else {
            continue;
        };
        if scope == Scope::Failed && !retryable(presence) {
            continue;
        }
        let Some(script) = setup.script_for(&id) else {
            plan.unscripted.push(id);
            continue;
        };
        match running_entry(presence) {
            Some(running) if running == script => {
                if logged_out(presence) {
                    plan.login.push(id);
                }
            }
            Some(_) => plan.conflicts.push((id, script.to_path_buf())),
            None => plan.start.push((id, script.to_path_buf())),
        }
    }
    plan
}

fn logged_out(presence: &Presence) -> bool {
    live(presence)
        .is_some_and(|summary| matches!(summary.login, Login::Disconnected | Login::Failed(_)))
}

fn retryable(presence: &Presence) -> bool {
    match presence {
        Presence::Unopened(_) => true,
        Presence::Open(summary) => match summary.login {
            Login::Failed(_) | Login::Disconnected => true,
            Login::Online => matches!(
                summary.script,
                Some(ScriptState {
                    phase: ScriptPhase::Failed(_),
                    ..
                })
            ),
            _ => false,
        },
        Presence::Closed(Some(ScriptState {
            phase: ScriptPhase::Failed(_),
            ..
        })) => true,
        Presence::Waiting { .. } | Presence::Closed(_) => false,
    }
}

fn presence(client: &Client, account: &AccountId, cx: &App) -> Presence {
    if let Some((place, of)) = client.waiting(account) {
        return Presence::Waiting {
            place,
            of,
            open: client.summary(account, cx),
        };
    }
    if let Some(summary) = client.summary(account, cx) {
        return Presence::Open(summary);
    }
    match client.unopened(account) {
        Some(error) => Presence::Unopened(error),
        None => Presence::Closed(client.ended(account)),
    }
}

fn last_entry(presence: &Presence) -> Option<&Path> {
    match presence {
        Presence::Open(Summary {
            script: Some(script),
            ..
        })
        | Presence::Closed(Some(script)) => Some(&script.entry),
        _ => None,
    }
}

fn shared<T: Clone + PartialEq>(
    accounts: &[Account],
    field: impl Fn(&Account) -> &Option<T>,
) -> Option<Option<T>> {
    let first = field(accounts.first()?);
    accounts
        .iter()
        .all(|account| field(account) == first)
        .then(|| first.clone())
}

fn input(
    text: String,
    build: impl FnOnce(&mut Context<Input>) -> Input,
    cx: &mut Context<Manager>,
) -> Entity<Input> {
    cx.new(|cx| {
        let mut input = build(cx).max_length(MAX_FIELD);
        input.set_text(text, cx);
        input
    })
}

fn choose_script(cx: &mut Context<Manager>) -> impl Future<Output = Option<PathBuf>> + use<> {
    let chosen = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Choose".into()),
    });
    async move {
        let Ok(Ok(Some(paths))) = chosen.await else {
            return None;
        };
        paths.into_iter().next()
    }
}

fn common_script(scripts: &[PathBuf]) -> Option<PathBuf> {
    let mut counts: HashMap<&PathBuf, usize> = HashMap::new();
    for script in scripts {
        *counts.entry(script).or_default() += 1;
    }
    scripts
        .iter()
        .fold(None, |best: Option<&PathBuf>, script| match best {
            Some(best) if counts[best] >= counts[script] => Some(best),
            _ => Some(script),
        })
        .cloned()
}

impl Manager {
    fn new(
        game: WindowHandle<Client>,
        client: Entity<Client>,
        path: PathBuf,
        device: HeaderValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let filter = cx.new(|cx| {
            Input::new("Filter accounts", cx)
                .placeholder("Filter accounts")
                .search()
                .small()
        });
        let subscriptions = [
            cx.observe(&client, |_, _, cx| cx.notify()),
            cx.subscribe(&client, |_, _, _: &Updated, cx| cx.notify()),
            cx.on_focus_lost(window, |this, window, cx| window.focus(&this.focus, cx)),
            cx.subscribe(&filter, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Changed) {
                    this.checked.clear();
                    cx.notify();
                }
            }),
        ];
        let mut manager = Self {
            window: game,
            client,
            accounts: Accounts::open(path).map_err(Into::into),
            device,
            servers: Servers {
                list: Vec::new(),
                check: Check::Idle,
            },
            view: View::All,
            show: Show::All,
            checked: HashSet::new(),
            cursor: None,
            columns: 1,
            recent: Vec::new(),
            form: None,
            bulk: None,
            renaming: None,
            confirm: None,
            notice: None,
            filter,
            grid_scroll: ScrollHandle::new(),
            focus,
            _subscriptions: subscriptions,
        };
        manager.check_servers(cx);
        manager
    }

    fn check_servers(&mut self, cx: &mut Context<Self>) {
        if !self.servers.ready() {
            return;
        }
        self.servers.check = Check::Running;
        let device = self.device.clone();
        let checked = cx.background_spawn(async move { net::servers(device) });
        cx.spawn(async move |this, cx| {
            let checked = checked.await;
            let updated = this.update(cx, |this, cx| {
                match checked {
                    Ok(list) => {
                        this.servers.list = list;
                        this.servers.check = Check::Done(Instant::now());
                    }
                    Err(error) => {
                        tracing::warn!(error = format!("{error:#}"), "couldn't check the servers");
                        this.servers.check = Check::Failed(Instant::now());
                    }
                }
                cx.notify();
            });
            if updated.is_ok() {
                cx.background_executor().timer(SERVERS_RECHECK_AFTER).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();
        cx.notify();
    }

    fn check_stale_servers(&mut self, cx: &mut Context<Self>) {
        if self.servers.stale() {
            self.check_servers(cx);
        }
    }

    fn account_list(&self) -> Vec<Account> {
        self.accounts
            .as_ref()
            .map(|accounts| accounts.list().to_vec())
            .unwrap_or_default()
    }

    fn setup(&self) -> Option<&Setup> {
        let View::Setup(name) = &self.view else {
            return None;
        };
        self.accounts.as_ref().ok()?.setup(name)
    }

    fn view_accounts(&self) -> Vec<Account> {
        let accounts = self.account_list();
        match self.setup() {
            Some(setup) => setup
                .members
                .iter()
                .filter_map(|member| {
                    accounts
                        .iter()
                        .find(|account| account.id() == member.id())
                        .cloned()
                })
                .collect(),
            None => accounts,
        }
    }

    fn presences(&self, accounts: &[Account], cx: &App) -> Vec<Presence> {
        let client = self.client.read(cx);
        accounts
            .iter()
            .map(|account| presence(client, &account.id(), cx))
            .collect()
    }

    fn shown(&self, cx: &App) -> Vec<Account> {
        let accounts = self.view_accounts();
        let query = self.filter.read(cx).text().to_owned();
        let presences = self.presences(&accounts, cx);
        accounts
            .into_iter()
            .zip(presences)
            .filter(|(account, presence)| {
                let face = face(account, presence);
                face.shown_in(self.show) && face.matches(account, &query)
            })
            .map(|(account, _)| account)
            .collect()
    }

    fn checked_accounts(&self) -> Vec<Account> {
        self.account_list()
            .into_iter()
            .filter(|account| self.checked.contains(&account.id()))
            .collect()
    }

    fn act(
        &mut self,
        targets: Vec<Account>,
        cx: &mut Context<Self>,
        act: impl Fn(&mut Client, &Account, &mut Window, &mut Context<Client>),
    ) {
        let done = self.window.update(cx, |client, window, cx| {
            for account in &targets {
                act(client, account, window, cx);
            }
        });
        self.notice = done.is_err().then(|| "The game window is closed.".into());
        cx.notify();
    }

    fn launch(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        self.act(targets, cx, |client, account, window, cx| {
            client.launch(account, None, window, cx)
        });
    }

    fn run(&mut self, targets: Vec<Account>, script: PathBuf, cx: &mut Context<Self>) {
        self.recent.retain(|recent| recent != &script);
        self.recent.insert(0, script.clone());
        crate::settings::show_recent_script(&script, cx);
        self.act(targets, cx, |client, account, window, cx| {
            client.run_script(account, script.clone(), true, None, window, cx)
        });
    }

    fn run_chosen(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        let chosen = choose_script(cx);
        cx.spawn(async move |this, cx| {
            let Some(script) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.run(targets, script, cx));
        })
        .detach();
    }

    fn stop(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        self.act(targets, cx, |client, account, _, cx| {
            client.stop_script(&account.id(), cx)
        });
    }

    fn close(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        let presences = self.presences(&targets, cx);
        let running = presences
            .iter()
            .filter(|presence| running_entry(presence).is_some())
            .count();
        if running == 0 {
            self.close_now(targets, cx);
        } else {
            self.ask(Ask::Close { targets, running }, cx);
        }
    }

    fn close_now(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        self.act(targets, cx, |client, account, window, cx| {
            client.close_account(&account.id(), window, cx)
        });
    }

    fn show_account(&mut self, account: Account, cx: &mut Context<Self>) {
        self.act(vec![account], cx, |client, account, window, cx| {
            client.show_account(&account.id(), window, cx)
        });
    }

    fn open_account(&mut self, account: Account, cx: &mut Context<Self>) {
        let open = self.client.read(cx).summary(&account.id(), cx).is_some();
        if open {
            self.show_account(account, cx);
        } else {
            self.launch(vec![account], cx);
        }
    }

    fn restart(&mut self, account: &Account, cx: &mut Context<Self>) {
        let id = account.id();
        self.client
            .update(cx, |client, cx| client.restart_script(&id, cx));
    }

    fn recent_scripts(&self, cx: &App) -> Vec<PathBuf> {
        let setups = self
            .accounts
            .as_ref()
            .map(|accounts| accounts.setups().to_vec())
            .unwrap_or_default();
        let from_setups = setups.into_iter().flat_map(|setup| {
            setup
                .script
                .into_iter()
                .chain(setup.members.into_iter().filter_map(|member| member.script))
        });
        let hidden = crate::settings::hidden_recent_scripts(cx);
        let mut scripts: Vec<PathBuf> = Vec::new();
        for script in self
            .recent
            .iter()
            .cloned()
            .chain(self.client.read(cx).loaded_scripts(cx))
            .chain(from_setups)
        {
            if !scripts.contains(&script) && !hidden.contains(&script) {
                scripts.push(script);
            }
        }
        scripts.truncate(RECENT_LIMIT);
        scripts
    }

    fn toggle(&mut self, id: AccountId, cx: &mut Context<Self>) {
        if !self.checked.remove(&id) {
            self.checked.insert(id.clone());
        }
        self.cursor = Some(id);
        cx.notify();
    }

    fn set_show(&mut self, show: Show, cx: &mut Context<Self>) {
        self.show = show;
        self.checked.clear();
        cx.notify();
    }

    fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        if self.view != view {
            self.view = view;
            self.checked.clear();
            self.cursor = None;
        }
        cx.notify();
    }

    fn select_shown(&mut self, cx: &mut Context<Self>) {
        self.checked = self.shown(cx).iter().map(Account::id).collect();
        cx.notify();
    }

    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.checked.clear();
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) || self.dialog_open() {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }
        let shown = self.shown(cx);
        if shown.is_empty() {
            return;
        }
        let at = self
            .cursor
            .as_ref()
            .and_then(|cursor| shown.iter().position(|account| &account.id() == cursor));
        let columns = self.columns.max(1);
        let last = shown.len() - 1;
        let moved = match (event.keystroke.key.as_str(), at) {
            ("left" | "right" | "up" | "down", None) => Some(0),
            ("left", Some(at)) => Some(at.saturating_sub(1)),
            ("right", Some(at)) => Some((at + 1).min(last)),
            ("up", Some(at)) => Some(at.saturating_sub(columns)),
            ("down", Some(at)) => Some((at + columns).min(last)),
            ("space", Some(at)) => {
                self.toggle(shown[at].id(), cx);
                cx.stop_propagation();
                return;
            }
            ("enter", Some(at)) => {
                self.open_account(shown[at].clone(), cx);
                cx.stop_propagation();
                return;
            }
            _ => None,
        };
        if let Some(index) = moved {
            self.grid_scroll.scroll_to_item(index);
            let id = shown[index].id();
            if modifiers.shift {
                self.checked.insert(id.clone());
            }
            self.cursor = Some(id);
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn dialog_open(&self) -> bool {
        self.form.as_ref().is_some_and(|form| form.open)
            || self.bulk.as_ref().is_some_and(|bulk| bulk.open)
            || self.confirm.as_ref().is_some_and(|confirm| confirm.open)
    }

    fn ask(&mut self, ask: Ask, cx: &mut Context<Self>) {
        self.confirm = Some(Confirming { open: true, ask });
        cx.notify();
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if let Some(confirm) = &mut self.confirm {
            confirm.open = false;
            cx.notify();
        }
    }

    fn confirm(&mut self, choice: bool, cx: &mut Context<Self>) {
        let Some(confirm) = self.confirm.as_mut().filter(|confirm| confirm.open) else {
            return;
        };
        confirm.open = false;
        match confirm.ask.clone() {
            Ask::Close { targets, .. } => self.close_now(targets, cx),
            Ask::Start { setup, scope, .. } => self.start_setup(&setup, scope, Some(choice), cx),
            Ask::Delete { setup } => self.delete_setup(&setup, cx),
            Ask::Remove { account } => self.remove(&account.id(), cx),
        }
        cx.notify();
    }

    fn start_current(&mut self, cx: &mut Context<Self>) {
        if let View::Setup(name) = self.view.clone() {
            self.start_setup(&name, Scope::Everyone, None, cx);
        }
    }

    fn start_setup(
        &mut self,
        name: &str,
        scope: Scope,
        replace: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let Some(setup) = self
            .accounts
            .as_ref()
            .ok()
            .and_then(|accounts| accounts.setup(name))
            .cloned()
        else {
            return;
        };
        let accounts = self.account_list();
        let presences: HashMap<_, _> = {
            let client = self.client.read(cx);
            accounts
                .iter()
                .map(|account| (account.id(), presence(client, &account.id(), cx)))
                .collect()
        };
        let plan = plan(&setup, &presences, scope);
        let named = |id: &AccountId| accounts.iter().find(|account| &account.id() == id).cloned();
        if !plan.conflicts.is_empty() && replace.is_none() {
            let conflicts = plan
                .conflicts
                .iter()
                .filter_map(|(id, _)| named(id).map(|account| account.name))
                .collect();
            self.ask(
                Ask::Start {
                    setup: setup.name,
                    scope,
                    conflicts,
                },
                cx,
            );
            return;
        }
        let runs: Vec<(Account, PathBuf, bool)> = plan
            .start
            .iter()
            .map(|(id, script)| (id, script, false))
            .chain(
                plan.conflicts
                    .iter()
                    .filter(|_| replace == Some(true))
                    .map(|(id, script)| (id, script, true)),
            )
            .filter_map(|(id, script, replace)| {
                named(id).map(|account| (account, script.clone(), replace))
            })
            .collect();
        let logins: Vec<Account> = plan.login.iter().filter_map(&named).collect();
        let server = setup.server.clone();
        let done = self.window.update(cx, |client, window, cx| {
            for (account, script, replace) in &runs {
                client.run_script(
                    account,
                    script.clone(),
                    *replace,
                    server.clone(),
                    window,
                    cx,
                );
            }
            for account in &logins {
                client.launch(account, server.clone(), window, cx);
            }
        });
        let skipped: Vec<String> = plan
            .unscripted
            .iter()
            .filter_map(&named)
            .map(|account| format!("{} has no script in this setup.", account.name))
            .collect();
        self.notice = match done {
            Err(_) => Some("The game window is closed.".into()),
            Ok(()) if skipped.is_empty() => None,
            Ok(()) => Some(skipped.join("\n").into()),
        };
        cx.notify();
    }

    fn retry_failed(&mut self, cx: &mut Context<Self>) {
        if let View::Setup(name) = self.view.clone() {
            self.start_setup(&name, Scope::Failed, None, cx);
        }
    }

    fn edit(&mut self, account: Option<&Account>, cx: &mut Context<Self>) {
        let draft = account.map(Draft::of).unwrap_or_default();
        let placeholder = draft.username.trim().to_owned();
        let name = input(
            draft.name,
            |cx| Input::new("Display name", cx).placeholder(placeholder),
            cx,
        );
        let username = input(draft.username, |cx| Input::new("Username", cx), cx);
        let password = input(
            draft.password,
            |cx| Input::new("Password", cx).password(),
            cx,
        );
        let inputs =
            [&name, &username, &password].map(|input| cx.subscribe(input, Self::input_event));
        self.form = Some(Form {
            open: true,
            editing: account.map(Account::id),
            name,
            username,
            password,
            server: draft.server,
            error: None,
            _inputs: inputs,
        });
        self.check_stale_servers(cx);
        cx.notify();
    }

    fn input_event(&mut self, input: Entity<Input>, event: &InputEvent, cx: &mut Context<Self>) {
        match event {
            InputEvent::Submitted => self.save(cx),
            InputEvent::Changed => {
                let Some(form) = &mut self.form else {
                    return;
                };
                if input == form.username {
                    let username = form.username.read(cx).text().trim().to_owned();
                    form.name
                        .update(cx, |name, cx| name.set_placeholder(username, cx));
                }
                if form.error.take().is_some() {
                    cx.notify();
                }
            }
        }
    }

    fn close_form(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.form {
            form.open = false;
            cx.notify();
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let (Some(form), Ok(accounts)) = (&mut self.form, &mut self.accounts) else {
            return;
        };
        let draft = form.draft(cx);
        let renamed = form
            .editing
            .as_ref()
            .filter(|editing| **editing != AccountId::of(draft.username.trim()));
        if let Some(old) = renamed {
            let client = self.client.read(cx);
            let blocked = if client.summary(old, cx).is_some() {
                Some("Close this account's tab before changing its username.")
            } else if client.waiting(old).is_some() {
                Some("Stop this account's launch before changing its username.")
            } else {
                None
            };
            if let Some(blocked) = blocked {
                form.error = Some(Problem::Username(blocked.into()));
                cx.notify();
                return;
            }
        }
        match accounts.save(form.editing.as_ref(), draft) {
            Ok(id) => {
                if let Some(old) = &form.editing
                    && self.checked.remove(old)
                {
                    self.checked.insert(id);
                }
                form.open = false;
            }
            Err(error) => {
                let message = SharedString::from(error.to_string());
                form.error = Some(match error {
                    SaveError::Invalid(Invalid::NoUsername | Invalid::Taken(_)) => {
                        Problem::Username(message)
                    }
                    SaveError::Invalid(Invalid::NoPassword) => Problem::Password(message),
                    SaveError::Io(_) => Problem::Other(message),
                });
            }
        }
        cx.notify();
    }

    fn remove(&mut self, id: &AccountId, cx: &mut Context<Self>) {
        let Ok(accounts) = &mut self.accounts else {
            return;
        };
        match accounts.remove(id) {
            Ok(()) => {
                self.checked.remove(id);
                if let Some(form) = &mut self.form
                    && form.editing.as_ref() == Some(id)
                {
                    form.open = false;
                }
            }
            Err(error) => self.notice = Some(format!("couldn't save the accounts: {error}").into()),
        }
        cx.notify();
    }

    fn edit_selected(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        self.bulk = Some(Bulk {
            open: true,
            ids: targets.iter().map(Account::id).collect(),
            change: Change {
                server: shared(&targets, |account| &account.server),
            },
            error: None,
        });
        self.check_stale_servers(cx);
        cx.notify();
    }

    fn close_bulk(&mut self, cx: &mut Context<Self>) {
        if let Some(bulk) = &mut self.bulk {
            bulk.open = false;
            cx.notify();
        }
    }

    fn save_bulk(&mut self, cx: &mut Context<Self>) {
        let (Some(bulk), Ok(accounts)) = (&mut self.bulk, &mut self.accounts) else {
            return;
        };
        match accounts.change(&bulk.ids, bulk.change.clone()) {
            Ok(()) => bulk.open = false,
            Err(error) => bulk.error = Some(format!("couldn't save the accounts: {error}").into()),
        }
        cx.notify();
    }

    fn new_setup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.create_setup(None, Vec::new(), window, cx);
    }

    fn new_setup_from(
        &mut self,
        targets: Vec<Account>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let presences = self.presences(&targets, cx);
        let entries: Vec<Option<PathBuf>> = presences
            .iter()
            .map(|presence| last_entry(presence).map(Path::to_path_buf))
            .collect();
        let scripts: Vec<PathBuf> = entries.iter().flatten().cloned().collect();
        let common = common_script(&scripts);
        let members = targets
            .into_iter()
            .zip(entries)
            .map(|(account, entry)| Member {
                username: account.username,
                script: entry.filter(|entry| Some(entry) != common.as_ref()),
            })
            .collect();
        self.create_setup(common, members, window, cx);
    }

    fn create_setup(
        &mut self,
        script: Option<PathBuf>,
        members: Vec<Member>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(accounts) = &mut self.accounts else {
            return;
        };
        match accounts.create_setup(script, members) {
            Ok(name) => {
                self.view = View::Setup(name.clone());
                self.checked.clear();
                self.start_rename(&name, window, cx);
            }
            Err(error) => self.notice = Some(format!("couldn't save the setups: {error}").into()),
        }
        cx.notify();
    }

    fn start_rename(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.set_view(View::Setup(name.to_owned()), cx);
        let input = input(
            name.to_owned(),
            |cx| Input::new("Setup name", cx).bare(),
            cx,
        );
        let focus = input.focus_handle(cx);
        let subscriptions =
            [
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::Submitted => this.finish_rename(true, window, cx),
                        InputEvent::Changed => {
                            if let Some(renaming) = &mut this.renaming
                                && renaming.error.take().is_some()
                            {
                                cx.notify();
                            }
                        }
                    }
                }),
                cx.on_blur(&focus, window, |this, window, cx| {
                    this.finish_rename(false, window, cx)
                }),
            ];
        window.focus(&focus, cx);
        self.renaming = Some(Renaming {
            setup: name.to_owned(),
            input,
            error: None,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    fn finish_rename(&mut self, submitted: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(renaming), Ok(accounts)) = (&mut self.renaming, &mut self.accounts) else {
            return;
        };
        let name = renaming.input.read(cx).text().to_owned();
        match accounts.rename_setup(&renaming.setup, &name) {
            Ok(name) => {
                if self.view == View::Setup(renaming.setup.clone()) {
                    self.view = View::Setup(name);
                }
                self.end_rename(submitted, window, cx);
            }
            Err(SetupError::Invalid(invalid)) if submitted => {
                renaming.error = Some(invalid.to_string().into());
                cx.notify();
            }
            Err(SetupError::Invalid(_)) => self.end_rename(false, window, cx),
            Err(SetupError::Io(error)) => {
                self.notice = Some(format!("couldn't save the setups: {error}").into());
                self.end_rename(submitted, window, cx);
            }
        }
    }

    fn end_rename(&mut self, refocus: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = None;
        if refocus {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    fn change_setup(&mut self, name: &str, cx: &mut Context<Self>, edit: impl FnOnce(&mut Setup)) {
        let Ok(accounts) = &mut self.accounts else {
            return;
        };
        if let Err(error) = accounts.edit_setup(name, edit) {
            self.notice = Some(format!("couldn't save the setups: {error}").into());
        }
        cx.notify();
    }

    fn set_setup_server(&mut self, setup: &str, server: Option<String>, cx: &mut Context<Self>) {
        self.change_setup(setup, cx, |setup| setup.server = server);
    }

    fn toggle_member(&mut self, setup: &str, account: &Account, cx: &mut Context<Self>) {
        let member = self
            .accounts
            .as_ref()
            .ok()
            .and_then(|accounts| accounts.setup(setup))
            .is_some_and(|setup| setup.member(&account.id()).is_some());
        if member {
            self.remove_from_setup(setup, std::slice::from_ref(account), cx);
        } else {
            self.add_to_setup(setup, vec![account.clone()], cx);
        }
    }

    fn add_to_setup(&mut self, setup: &str, targets: Vec<Account>, cx: &mut Context<Self>) {
        self.change_setup(setup, cx, |setup| {
            setup.add(targets.into_iter().map(|account| account.username));
        });
    }

    fn remove_from_setup(&mut self, setup: &str, targets: &[Account], cx: &mut Context<Self>) {
        let ids: Vec<AccountId> = targets.iter().map(Account::id).collect();
        self.checked.retain(|id| !ids.contains(id));
        self.change_setup(setup, cx, |setup| setup.remove(&ids));
    }

    fn set_script(
        &mut self,
        setup: &str,
        target: ScriptFor,
        script: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if let Some(script) = &script {
            crate::settings::show_recent_script(script, cx);
        }
        self.change_setup(setup, cx, |setup| match target {
            ScriptFor::Setup => setup.script = script,
            ScriptFor::Members(ids) => setup.pick(&ids, script),
        });
    }

    fn choose_script_for(&mut self, setup: String, target: ScriptFor, cx: &mut Context<Self>) {
        let chosen = choose_script(cx);
        cx.spawn(async move |this, cx| {
            let Some(script) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.set_script(&setup, target, Some(script), cx)
            });
        })
        .detach();
    }

    fn duplicate_setup(&mut self, name: &str, cx: &mut Context<Self>) {
        let Ok(accounts) = &mut self.accounts else {
            return;
        };
        match accounts.duplicate_setup(name) {
            Ok(Some(copy)) => self.view = View::Setup(copy),
            Ok(None) => {}
            Err(error) => self.notice = Some(format!("couldn't save the setups: {error}").into()),
        }
        cx.notify();
    }

    fn delete_setup(&mut self, name: &str, cx: &mut Context<Self>) {
        let Ok(accounts) = &mut self.accounts else {
            return;
        };
        match accounts.remove_setup(name) {
            Ok(()) => {
                if self.view == View::Setup(name.to_owned()) {
                    self.view = View::All;
                }
            }
            Err(error) => self.notice = Some(format!("couldn't save the setups: {error}").into()),
        }
        cx.notify();
    }

    fn set_server(&mut self, server: Option<String>, bulk: bool) {
        if bulk {
            if let Some(bulk) = &mut self.bulk {
                bulk.change.server = Some(server);
            }
        } else if let Some(form) = &mut self.form {
            form.server = server;
        }
    }
}

fn selection_box(theme: &Theme, checked: bool) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(14.0))
        .rounded(theme.radius.tag)
        .border_1()
        .map(|mark| {
            if checked {
                mark.border_color(theme.inverse)
                    .bg(theme.inverse)
                    .child(small_icon(Icon::Check, theme.on_inverse))
            } else {
                mark.border_color(theme.border).bg(theme.fill)
            }
        })
}

fn plan_line(setup: &Setup, id: &AccountId, presence: &Presence) -> Option<Line> {
    let current = match presence {
        Presence::Open(Summary {
            script: Some(script),
            ..
        }) => Some(script.entry.as_path()),
        _ => None,
    };
    match setup.script_for(id) {
        Some(planned) if Some(planned) != current => {
            Some(Line::new(file_name(planned), Mood::Quiet).path(planned))
        }
        Some(_) => None,
        None => Some(Line::new("No script in this setup", Mood::Quiet)),
    }
}

fn color(theme: &Theme, mood: Mood) -> gpui::Hsla {
    match mood {
        Mood::Quiet => theme.text_tertiary,
        Mood::Muted => theme.text_secondary,
        Mood::Plain => theme.text,
        Mood::Accent => theme.accent_text,
        Mood::Warning => theme.warning_text,
        Mood::Danger => theme.danger_text,
    }
}

fn targets_item(
    this: &WeakEntity<Manager>,
    targets: &[Account],
    label: impl Into<SharedString>,
    run: impl Fn(&mut Manager, Vec<Account>, &mut Window, &mut Context<Manager>) + 'static,
) -> MenuItem {
    let this = this.clone();
    let targets = targets.to_vec();
    MenuItem::action(label, move |window, cx| {
        this.update(cx, |manager, cx| run(manager, targets.clone(), window, cx))
            .ok();
    })
}

fn script_pick(setup: String, target: ScriptFor) -> Pick {
    Rc::new(move |manager, script, cx| match script {
        Some(script) => manager.set_script(&setup, target.clone(), Some(script), cx),
        None => manager.choose_script_for(setup.clone(), target.clone(), cx),
    })
}

fn member_script_items(
    this: &WeakEntity<Manager>,
    scripts: &[PathBuf],
    setup: &str,
    ids: Vec<AccountId>,
    own: Option<Option<&Path>>,
    window: &Window,
) -> Vec<MenuItem> {
    let same = {
        let this = this.clone();
        let setup = setup.to_owned();
        let ids = ids.clone();
        MenuItem::action("Same as the setup", move |_, cx| {
            this.update(cx, |manager, cx| {
                manager.set_script(&setup, ScriptFor::Members(ids.clone()), None, cx)
            })
            .ok();
        })
        .checked(own == Some(None))
    };
    let mut items = vec![same, MenuItem::separator()];
    items.extend(script_items(
        this,
        scripts,
        own,
        window,
        script_pick(setup.to_owned(), ScriptFor::Members(ids)),
    ));
    items
}

fn script_items(
    this: &WeakEntity<Manager>,
    scripts: &[PathBuf],
    current: Option<Option<&Path>>,
    window: &Window,
    run: Pick,
) -> Vec<MenuItem> {
    let item = |label: SharedString, script: Option<PathBuf>| {
        let this = this.clone();
        let run = run.clone();
        MenuItem::action(label, move |_, cx| {
            this.update(cx, |manager, cx| run(manager, script.clone(), cx))
                .ok();
        })
    };
    let mut listed: Vec<&Path> = scripts.iter().map(PathBuf::as_path).collect();
    if let Some(Some(current)) = current
        && !listed.contains(&current)
    {
        listed.insert(0, current);
    }
    let mut items = Vec::new();
    if !listed.is_empty() {
        items.push(MenuItem::header("Recent"));
        items.extend(listed.into_iter().map(|script| {
            let folder = script.parent().map(home_relative).unwrap_or_default();
            let hidden = script.to_path_buf();
            let refresh = this.clone();
            let row = item(file_name(script).into(), Some(script.to_path_buf()))
                .description(fit_path_to(&folder, SCRIPT_PATH_WIDTH, window));
            let row = if current == Some(Some(script)) {
                row
            } else {
                row.dismiss("Remove from recent", move |_, cx| {
                    crate::settings::hide_recent_script(hidden.clone(), cx);
                    refresh.update(cx, |_, cx| cx.notify()).ok();
                })
            };
            match current {
                Some(current) => row.checked(current == Some(script)),
                None => row,
            }
        }));
        items.push(MenuItem::separator());
    }
    items.push(item("Choose a file…".into(), None));
    items
}

fn account_menu(
    this: WeakEntity<Manager>,
    account: Account,
    targets: Vec<Account>,
    open: bool,
    scripts: Vec<PathBuf>,
    setup: Option<(String, Option<PathBuf>)>,
) -> impl Fn(&mut Window, &mut App) -> Vec<MenuItem> + 'static {
    move |window, _| {
        let many = targets.len() > 1;
        let act = |label: &'static str,
                   run: fn(&mut Manager, Vec<Account>, &mut Context<Manager>)| {
            targets_item(&this, &targets, label, move |manager, targets, _, cx| {
                run(manager, targets, cx)
            })
        };
        let run_targets = targets.clone();
        let run: Pick = Rc::new(move |manager, script, cx| match script {
            Some(script) => manager.run(run_targets.clone(), script, cx),
            None => manager.run_chosen(run_targets.clone(), cx),
        });
        let mut items = Vec::new();
        if many {
            items.push(MenuItem::header(format!("{} accounts", targets.len())));
        }
        items.push(if open && !many {
            act("Show", |manager, targets, cx| {
                if let Some(account) = targets.into_iter().next() {
                    manager.show_account(account, cx);
                }
            })
        } else {
            act("Launch", Manager::launch)
        });
        items.extend([
            MenuItem::submenu(
                "Run script",
                script_items(&this, &scripts, None, window, run),
            ),
            act("Stop", Manager::stop),
            act("Close", Manager::close),
            MenuItem::separator(),
        ]);
        if let Some((name, own)) = &setup {
            let ids = targets.iter().map(Account::id).collect();
            let removed = name.clone();
            items.extend([
                MenuItem::header(name.clone()),
                MenuItem::submenu(
                    "Script",
                    member_script_items(
                        &this,
                        &scripts,
                        name,
                        ids,
                        (!many).then_some(own.as_deref()),
                        window,
                    ),
                ),
                targets_item(&this, &targets, "Remove", move |manager, targets, _, cx| {
                    manager.remove_from_setup(&removed, &targets, cx)
                }),
                MenuItem::separator(),
            ]);
        }
        if many {
            items.push(act("Edit server…", Manager::edit_selected));
        } else {
            let edited = account.clone();
            let removed = account.clone();
            items.extend([
                targets_item(&this, &targets, "Edit…", move |manager, _, _, cx| {
                    manager.edit(Some(&edited), cx)
                }),
                targets_item(&this, &targets, "Remove…", move |manager, _, _, cx| {
                    manager.ask(
                        Ask::Remove {
                            account: removed.clone(),
                        },
                        cx,
                    )
                })
                .danger(),
            ]);
        }
        items
    }
}

impl Manager {
    fn title_bar(
        &self,
        faces: &[Face],
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let count = |show: Show| faces.iter().filter(|face| face.shown_in(show)).count();
        let problems = count(Show::Problems);
        let pills = [
            (Show::All, "show-all", "All", Tone::Neutral),
            (Show::Running, "show-running", "Running", Tone::Neutral),
            (
                Show::Problems,
                "show-problems",
                "Problems",
                if problems > 0 {
                    Tone::Danger
                } else {
                    Tone::Neutral
                },
            ),
            (Show::NotOpen, "show-not-open", "Not open", Tone::Neutral),
        ]
        .map(|(show, id, label, tone)| {
            Button::new(id, label)
                .variant(Variant::Ghost)
                .size(Size::Small)
                .text_size(TEXT_SMALL)
                .pressed(self.show == show)
                .end_slot(Badge::new(count(show).to_string()).tone(tone))
                .on_click(cx.listener(move |this, _, _, cx| this.set_show(show, cx)))
        });
        window_bar(window, cx)
            .child(bar_title("Accounts", cx))
            .child(drag_area().flex_1().h_full())
            .child(
                h_flex()
                    .flex_none()
                    .gap(px(2.0))
                    .mr(px(6.0))
                    .children(pills),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(200.0))
                    .mr(px(6.0))
                    .child(self.filter.clone()),
            )
    }

    fn sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let accounts = self.account_list();
        let setups = self
            .accounts
            .as_ref()
            .map(|accounts| accounts.setups().to_vec())
            .unwrap_or_default();
        let rows: Vec<(Setup, usize)> = {
            let client = self.client.read(cx);
            setups
                .into_iter()
                .map(|setup| {
                    let running = setup
                        .members
                        .iter()
                        .filter(|member| {
                            running_entry(&presence(client, &member.id(), cx)).is_some()
                        })
                        .count();
                    (setup, running)
                })
                .collect()
        };
        let selected = match &self.view {
            View::All => None,
            View::Setup(name) => rows.iter().position(|(setup, _)| &setup.name == name),
        };
        let tertiary = theme.text_tertiary;
        let total = accounts.len();
        let all = Tabs::new("all-accounts", "Accounts", 1, move |_| {
            Tab::new("All accounts").end_slot(
                div()
                    .text_small()
                    .text_color(tertiary)
                    .child(total.to_string()),
            )
        })
        .vertical()
        .selected(if self.view == View::All {
            0
        } else {
            usize::MAX
        })
        .on_select({
            let this = cx.weak_entity();
            move |_, _, cx| {
                this.update(cx, |manager, cx| manager.set_view(View::All, cx))
                    .ok();
            }
        });
        let this = cx.weak_entity();
        let names: Rc<[String]> = rows.iter().map(|(setup, _)| setup.name.clone()).collect();
        let labels: Rc<[(SharedString, SharedString)]> = rows
            .iter()
            .map(|(setup, running)| {
                (
                    setup.name.clone().into(),
                    format!("{running}/{}", setup.members.len()).into(),
                )
            })
            .collect();
        let menu_names = names.clone();
        let list = Tabs::new("setups", "Setups", rows.len(), move |index| {
            let (title, count) = &labels[index];
            let this = this.clone();
            let name = menu_names[index].clone();
            Tab::new(title.clone())
                .end_slot(div().text_small().text_color(tertiary).child(count.clone()))
                .menu(move |_, _| setup_menu(&this, &name))
        })
        .vertical()
        .selected(selected.unwrap_or(usize::MAX))
        .on_select({
            let this = cx.weak_entity();
            move |index, _, cx| {
                let name = names[index].clone();
                this.update(cx, |manager, cx| manager.set_view(View::Setup(name), cx))
                    .ok();
            }
        });
        let empty = rows.is_empty();
        v_flex()
            .flex_none()
            .w(SIDEBAR_WIDTH)
            .h_full()
            .bg(theme.well)
            .border_r_1()
            .border_color(theme.hairline)
            .text_size(TEXT_BODY)
            .text_color(theme.text)
            .pt(px(8.0))
            .child(all)
            .child(
                div()
                    .flex_none()
                    .h(px(28.0))
                    .mt(px(8.0))
                    .px(px(14.0))
                    .flex()
                    .items_center()
                    .text_small()
                    .text_color(theme.text_tertiary)
                    .child("Setups"),
            )
            .child(
                div()
                    .id("setups-list")
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .when(empty, |sidebar| {
                sidebar.child(
                    div()
                        .px(px(14.0))
                        .text_small()
                        .text_color(theme.text_tertiary)
                        .child("None yet."),
                )
            })
            .child(drag_area().flex_1())
            .child(
                div().flex_none().p(px(8.0)).child(
                    Button::new("new-setup", "New setup")
                        .variant(Variant::Ghost)
                        .w_full()
                        .disabled(self.accounts.is_err())
                        .child(
                            h_flex()
                                .flex_1()
                                .justify_between()
                                .child("New setup")
                                .child(div().text_color(theme.text_tertiary).child("⌘N")),
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.new_setup(window, cx))),
                ),
            )
    }

    fn header(
        &self,
        theme: &Theme,
        accounts: &[Account],
        presences: &[Presence],
        scripts: &[PathBuf],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (title, summary, actions) = match self.setup().cloned() {
            None => (
                div()
                    .truncate()
                    .text_body()
                    .font_weight(FontWeight::MEDIUM)
                    .child("All accounts")
                    .into_any_element(),
                note(
                    "Select accounts to launch them or run a script.",
                    true,
                    theme,
                )
                .truncate()
                .into_any_element(),
                vec![
                    Button::new("add-account", "Add account")
                        .size(Size::Small)
                        .end_slot(div().text_color(theme.text_tertiary).child("⇧⌘N"))
                        .disabled(self.accounts.is_err())
                        .on_click(cx.listener(|this, _, _, cx| this.edit(None, cx)))
                        .into_any_element(),
                ],
            ),
            Some(setup) => {
                let faces: Vec<Face> = accounts
                    .iter()
                    .zip(presences)
                    .map(|(account, presence)| face(account, presence))
                    .collect();
                let count = |show: Show| faces.iter().filter(|face| face.shown_in(show)).count();
                let mut parts = vec![format!(
                    "{}/{} running",
                    count(Show::Running),
                    accounts.len()
                )];
                match count(Show::Problems) {
                    0 => {}
                    1 => parts.push("1 problem".to_owned()),
                    problems => parts.push(format!("{problems} problems")),
                }
                match count(Show::NotOpen) {
                    0 => {}
                    closed => parts.push(format!("{closed} not open")),
                }
                let renaming = self
                    .renaming
                    .as_ref()
                    .filter(|renaming| renaming.setup == setup.name);
                let name_slot = match renaming {
                    Some(renaming) => div()
                        .flex_none()
                        .w(px(280.0))
                        .font_weight(FontWeight::MEDIUM)
                        .on_action(cx.listener(|this, _: &ClearSelection, window, cx| {
                            this.end_rename(true, window, cx)
                        }))
                        .child(renaming.input.clone())
                        .into_any_element(),
                    None => {
                        let name = setup.name.clone();
                        div()
                            .id("setup-title")
                            .truncate()
                            .text_body()
                            .font_weight(FontWeight::MEDIUM)
                            .cursor(gpui::CursorStyle::IBeam)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_rename(&name, window, cx)
                            }))
                            .child(setup.name.clone())
                            .into_any_element()
                    }
                };
                let title = h_flex()
                    .min_w_0()
                    .gap(px(8.0))
                    .child(name_slot)
                    .child(note(parts.join(" · "), true, theme).flex_none())
                    .into_any_element();
                let summary = match renaming.and_then(|renaming| renaming.error.clone()) {
                    Some(error) => div()
                        .text_small()
                        .text_color(theme.danger_text)
                        .child(error)
                        .into_any_element(),
                    None => {
                        let this = cx.weak_entity();
                        let scripts = scripts.to_vec();
                        let current = setup.script.clone();
                        let name = setup.name.clone();
                        let label = current
                            .as_deref()
                            .map_or_else(|| "Choose a script".to_owned(), file_name);
                        let script_menu = DropdownMenu::new("setup-script")
                            .width(SCRIPT_MENU_WIDTH)
                            .max_height(px(320.0))
                            .trigger(
                                Button::new("setup-script-trigger", label)
                                    .variant(Variant::Ghost)
                                    .size(Size::XSmall)
                                    .text_size(TEXT_SMALL)
                                    .text_color(theme.text)
                                    .when_some(setup.script.as_deref(), |button, script| {
                                        button.tooltip(home_relative(script))
                                    })
                                    .end_slot(small_icon(Icon::ChevronDown, theme.text_tertiary)),
                            )
                            .items(move |window, _| {
                                script_items(
                                    &this,
                                    &scripts,
                                    Some(current.as_deref()),
                                    window,
                                    script_pick(name.clone(), ScriptFor::Setup),
                                )
                            });
                        h_flex()
                            .min_w_0()
                            .gap(px(2.0))
                            .child(note("Runs", true, theme).flex_none())
                            .child(h_flex().h(LINE_SMALL).child(script_menu))
                            .child(note("on", true, theme).flex_none())
                            .child(
                                h_flex()
                                    .h(LINE_SMALL)
                                    .child(self.setup_server_menu(&setup, cx)),
                            )
                            .into_any_element()
                    }
                };
                let members_menu = {
                    let this = cx.weak_entity();
                    let all = self.account_list();
                    let setup = setup.clone();
                    DropdownMenu::new("setup-accounts")
                        .width(px(220.0))
                        .max_height(px(360.0))
                        .align(mira_ui::primitives::Align::End)
                        .trigger(
                            Button::new("setup-accounts-trigger", "Accounts")
                                .variant(Variant::Ghost)
                                .size(Size::Small)
                                .text_size(TEXT_BODY)
                                .end_slot(small_icon(Icon::ChevronDown, theme.text_tertiary)),
                        )
                        .items(move |_, _| {
                            all.iter()
                                .map(|account| {
                                    let this = this.clone();
                                    let toggled = account.clone();
                                    let name = setup.name.clone();
                                    MenuItem::action(account.name.clone(), move |_, cx| {
                                        this.update(cx, |manager, cx| {
                                            manager.toggle_member(&name, &toggled, cx)
                                        })
                                        .ok();
                                    })
                                    .checked(setup.member(&account.id()).is_some())
                                    .keep_open()
                                })
                                .collect()
                        })
                };
                let mut actions = vec![members_menu.into_any_element()];
                if presences.iter().any(retryable) {
                    actions.push(
                        Button::new("retry-failed", "Retry failed")
                            .variant(Variant::Ghost)
                            .size(Size::Small)
                            .text_size(TEXT_BODY)
                            .on_click(cx.listener(|this, _, _, cx| this.retry_failed(cx)))
                            .into_any_element(),
                    );
                }
                actions.push(
                    Button::new("start-setup", "Start setup")
                        .size(Size::Small)
                        .end_slot(div().text_color(theme.text_tertiary).child("⌘⏎"))
                        .disabled(setup.members.is_empty())
                        .on_click(cx.listener(|this, _, _, cx| this.start_current(cx)))
                        .into_any_element(),
                );
                (title, summary, actions)
            }
        };
        h_flex()
            .flex_none()
            .gap(px(8.0))
            .px(px(12.0))
            .pt(px(12.0))
            .pb(px(4.0))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.0))
                    .child(title)
                    .child(summary),
            )
            .children(actions)
    }

    fn setup_server_menu(&self, setup: &Setup, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let this = cx.weak_entity();
        let name = setup.name.clone();
        let current = setup.server.clone();
        let servers = self.servers.list.clone();
        let label = current
            .clone()
            .unwrap_or_else(|| "their servers".to_owned());
        let pick: PickServer = Rc::new(move |server, _, cx| {
            this.update(cx, |manager, cx| {
                manager.set_setup_server(&name, server, cx)
            })
            .ok();
        });
        DropdownMenu::new("setup-server")
            .width(px(340.0))
            .max_height(px(360.0))
            .trigger(
                Button::new("setup-server-trigger", label)
                    .variant(Variant::Ghost)
                    .size(Size::XSmall)
                    .text_size(TEXT_SMALL)
                    .text_color(theme.text)
                    .end_slot(small_icon(Icon::ChevronDown, theme.text_tertiary)),
            )
            .items(move |_, _| {
                server_items(
                    "Their own servers",
                    &servers,
                    Some(current.as_deref()),
                    &theme,
                    &pick,
                )
            })
    }

    fn card(
        &self,
        theme: &Theme,
        account: &Account,
        face: Face,
        scripts: &[PathBuf],
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = account.id();
        let checked = self.checked.contains(&id);
        let focused = self.cursor.as_ref() == Some(&id)
            && self.focus.is_focused(window)
            && mira_ui::nav::keyboard_input(window, cx);
        let line = |line: &Line| {
            div()
                .min_w_0()
                .truncate()
                .text_small()
                .text_color(color(theme, line.mood))
                .child(line.text.clone())
        };
        let open = face.open;
        let clicked = account.clone();
        let toggled = id.clone();
        let fix = face.fix.map(|fix| {
            let target = account.clone();
            div()
                .id(SharedString::from(format!("fix-{}", account.username)))
                .flex_none()
                .text_small()
                .text_color(theme.text)
                .underline()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| match fix {
                    Fix::EditLogin => this.edit(Some(&target), cx),
                    Fix::Restart => this.restart(&target, cx),
                }))
                .child(match fix {
                    Fix::EditLogin => "Edit login",
                    Fix::Restart => "Restart",
                })
        });
        let card = v_flex()
            .id(SharedString::from(format!("account-{}", account.username)))
            .relative()
            .h_full()
            .gap(px(3.0))
            .min_w_0()
            .px(px(12.0))
            .pt(px(10.0))
            .pb(px(11.0))
            .rounded(px(8.0))
            .border_1()
            .map(|card| {
                if checked {
                    card.border_color(theme.accent)
                        .bg(theme.accent.opacity(0.08))
                } else {
                    card.border_color(theme.hairline).bg(theme.surface)
                }
            })
            .role(Role::CheckBox)
            .aria_label(account.name.clone())
            .aria_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                window.focus(&this.focus, cx);
                if event.click_count() < 2 {
                    this.toggle(toggled.clone(), cx);
                } else if open {
                    this.show_account(clicked.clone(), cx);
                } else {
                    this.launch(vec![clicked.clone()], cx);
                }
            }))
            .child(
                h_flex()
                    .gap(px(8.0))
                    .min_w_0()
                    .mb(px(2.0))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(8.0))
                            .child(selection_box(theme, checked))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(account.name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_small()
                            .text_color(color(theme, face.state.mood))
                            .child(face.state.text.clone()),
                    ),
            )
            .child(line(&face.place))
            .children(face.who.clone().map(|who| {
                div()
                    .min_w_0()
                    .truncate()
                    .text_small()
                    .text_color(theme.text_secondary)
                    .child(who)
            }))
            .children(face.note().map(|note| {
                match (fix, note.path.clone()) {
                    (None, Some(path)) => h_flex()
                        .min_w_0()
                        .child(
                            Tooltip::new(
                                SharedString::from(format!("note-{}", account.username)),
                                fit_path_to(&path, CARD_PATH_WIDTH, window),
                                line(note),
                            )
                            .align_start(),
                        )
                        .into_any_element(),
                    (fix, _) => h_flex()
                        .min_w_0()
                        .gap(px(6.0))
                        .child(line(note).flex_shrink(1.0))
                        .children(fix)
                        .into_any_element(),
                }
            }))
            .children((face.lines()..CARD_LINES).map(|_| div().h(LINE_SMALL)))
            .when(focused, |card| {
                card.child(theme.ring_around(theme.focus, px(8.0), px(1.0)))
            });
        ContextMenu::new(
            SharedString::from(format!("account-menu-{}", account.username)),
            "Account actions",
        )
        .items(account_menu(
            cx.weak_entity(),
            account.clone(),
            if checked && self.checked.len() > 1 {
                self.checked_accounts()
            } else {
                vec![account.clone()]
            },
            open,
            scripts.to_vec(),
            self.setup().map(|setup| {
                (
                    setup.name.clone(),
                    setup.member(&id).and_then(|member| member.script.clone()),
                )
            }),
        ))
        .child(card)
    }

    fn selection_bar(
        &self,
        theme: &Theme,
        selected: Vec<Account>,
        scripts: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let count = selected.len();
        let separator = || {
            div()
                .flex_none()
                .mx(px(4.0))
                .w(px(1.0))
                .h(px(16.0))
                .bg(theme.hairline)
        };
        let bulk = |id: &'static str,
                    label: &'static str,
                    run: fn(&mut Manager, Vec<Account>, &mut Context<Manager>),
                    cx: &mut Context<Self>| {
            let targets = selected.clone();
            Button::new(id, label)
                .variant(Variant::Ghost)
                .size(Size::Small)
                .text_size(TEXT_BODY)
                .on_click(cx.listener(move |this, _, _, cx| run(this, targets.clone(), cx)))
        };
        let this = cx.weak_entity();
        let targets = selected.clone();
        let run: Pick = Rc::new(move |manager, script, cx| match script {
            Some(script) => manager.run(targets.clone(), script, cx),
            None => manager.run_chosen(targets.clone(), cx),
        });
        let run_menu = DropdownMenu::new("run-selected")
            .width(SCRIPT_MENU_WIDTH)
            .max_height(px(320.0))
            .trigger(
                Button::new("run-selected-trigger", "Run script…")
                    .variant(Variant::Ghost)
                    .size(Size::Small)
                    .text_size(TEXT_BODY),
            )
            .items(move |window, _| script_items(&this, &scripts, None, window, run.clone()));
        let this = cx.weak_entity();
        let menu_button = |id: &'static str, label: &'static str| {
            Button::new(id, label)
                .variant(Variant::Ghost)
                .size(Size::Small)
                .text_size(TEXT_BODY)
                .end_slot(small_icon(Icon::ChevronDown, theme.text_tertiary))
        };
        let targets = selected.clone();
        let setup_controls: Vec<gpui::AnyElement> = match self.view.clone() {
            View::All => {
                let setups: Vec<String> = self
                    .accounts
                    .as_ref()
                    .map(|accounts| {
                        accounts
                            .setups()
                            .iter()
                            .map(|setup| setup.name.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                let add = DropdownMenu::new("add-to-setup")
                    .width(px(200.0))
                    .max_height(px(320.0))
                    .trigger(menu_button("add-to-setup-trigger", "Add to setup"))
                    .items(move |_, _| {
                        let mut items: Vec<MenuItem> = setups
                            .iter()
                            .map(|setup| {
                                let added = setup.clone();
                                targets_item(
                                    &this,
                                    &targets,
                                    setup.clone(),
                                    move |manager, targets, _, cx| {
                                        manager.add_to_setup(&added, targets, cx)
                                    },
                                )
                            })
                            .collect();
                        if !items.is_empty() {
                            items.push(MenuItem::separator());
                        }
                        items.push(targets_item(
                            &this,
                            &targets,
                            "New setup",
                            |manager, targets, window, cx| {
                                manager.new_setup_from(targets, window, cx)
                            },
                        ));
                        items
                    });
                vec![add.into_any_element()]
            }
            View::Setup(setup) => {
                let scripts = self.recent_scripts(cx);
                let ids: Vec<AccountId> = selected.iter().map(Account::id).collect();
                let picked = setup.clone();
                let script = DropdownMenu::new("selection-script")
                    .width(SCRIPT_MENU_WIDTH)
                    .max_height(px(320.0))
                    .trigger(menu_button("selection-script-trigger", "Setup script"))
                    .items(move |window, _| {
                        member_script_items(&this, &scripts, &picked, ids.clone(), None, window)
                    });
                vec![script.into_any_element()]
            }
        };
        div()
            .absolute()
            .bottom(px(14.0))
            .left(SIDEBAR_WIDTH)
            .right_0()
            .px(px(12.0))
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_center()
                    .gap_x(px(4.0))
                    .gap_y(px(2.0))
                    .p(px(4.0))
                    .rounded(theme.radius.panel)
                    .bg(theme.overlay)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(theme.floating())
                    .child(
                        h_flex()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .pl(px(8.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(format!("{count} selected")),
                            )
                            .child(
                                Button::icon("clear-selection", Icon::X, "Clear selection")
                                    .size(Size::Small)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.clear_selection(cx)),
                                    ),
                            )
                            .child(separator())
                            .child(bulk("launch-selected", "Launch", Manager::launch, cx))
                            .child(run_menu)
                            .child(bulk("stop-selected", "Stop", Manager::stop, cx))
                            .child(bulk("close-selected", "Close", Manager::close, cx)),
                    )
                    .child(h_flex().gap(px(2.0)).children(setup_controls)),
            )
    }

    fn empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div().flex().flex_1().items_center().justify_center().child(
            Empty::new("No accounts yet", "Add an account to launch it from here.").action(
                Button::new("add-first-account", "Add account")
                    .variant(Variant::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.edit(None, cx))),
            ),
        )
    }

    fn form(&self, theme: &Theme, form: &Form, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (username, password, other) = match form.error.clone() {
            Some(Problem::Username(message)) => (Some(message), None, None),
            Some(Problem::Password(message)) => (None, Some(message), None),
            Some(Problem::Other(message)) => (None, None, Some(message)),
            None => (None, None, None),
        };
        let login = v_flex()
            .flex_none()
            .w(px(232.0))
            .gap(px(16.0))
            .child(Field::new("Username", form.username.clone()).error(username))
            .child(Field::new("Password", form.password.clone()).error(password))
            .child(
                Field::new("Display name", form.name.clone()).end_slot(
                    div()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(theme.text_tertiary)
                        .child("Optional"),
                ),
            )
            .children(other.map(|error| {
                div()
                    .text_size(TEXT_SMALL)
                    .text_color(theme.danger_text)
                    .child(error)
            }));
        let body = h_flex().items_start().gap(px(24.0)).child(login).child(
            div().flex_1().min_w_0().child(self.server_list(
                "server",
                Some(form.server.as_deref()),
                false,
                cx,
            )),
        );
        let close = cx.listener(|this, _: &(), _, cx| this.close_form(cx));
        Dialog::new(
            "account-form",
            form.open,
            if form.editing.is_some() {
                "Edit account"
            } else {
                "Add account"
            },
        )
        .width(px(640.0))
        .body(body)
        .action(
            Button::new("cancel", "Cancel")
                .on_click(cx.listener(|this, _, _, cx| this.close_form(cx))),
        )
        .action(
            Button::new("save", "Save")
                .variant(Variant::Primary)
                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
        )
        .on_close(move |window, cx| close(&(), window, cx))
    }

    fn bulk_form(
        &self,
        theme: &Theme,
        bulk: &Bulk,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let title = match bulk.ids.len() {
            1 => "Edit 1 account".to_owned(),
            count => format!("Edit {count} accounts"),
        };
        let body = v_flex()
            .gap(px(12.0))
            .child(self.server_list(
                "bulk-server",
                bulk.change.server.as_ref().map(Option::as_deref),
                true,
                cx,
            ))
            .children(bulk.error.clone().map(|error| {
                div()
                    .text_size(TEXT_SMALL)
                    .text_color(theme.danger_text)
                    .child(error)
            }));
        let close = cx.listener(|this, _: &(), _, cx| this.close_bulk(cx));
        Dialog::new("bulk-form", bulk.open, title)
            .when(bulk.change.server.is_none(), |dialog| {
                dialog.description("Mixed keeps each account's own server.")
            })
            .body(body)
            .action(
                Button::new("bulk-cancel", "Cancel")
                    .on_click(cx.listener(|this, _, _, cx| this.close_bulk(cx))),
            )
            .action(
                Button::new("bulk-save", "Save")
                    .variant(Variant::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.save_bulk(cx))),
            )
            .on_close(move |window, cx| close(&(), window, cx))
    }

    fn server_list(
        &self,
        id: &'static str,
        current: Option<Option<&str>>,
        bulk: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.weak_entity();
        let pick: PickServer = Rc::new(move |server, _, cx| {
            this.update(cx, |manager, cx| {
                manager.set_server(server, bulk);
                cx.notify();
            })
            .ok();
        });
        let items = server_items(
            CHOOSE_IN_GAME,
            &self.servers.list,
            current,
            cx.theme(),
            &pick,
        );
        Field::new(
            "Server",
            MenuList::new(id, "Server", items).height(SERVER_LIST_HEIGHT),
        )
        .description(self.servers.note())
        .end_slot(
            h_flex().h(LINE_SMALL).child(match self.servers.check {
                Check::Idle | Check::Running => div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(mira_ui::theme::CONTROL_XS)
                    .child(Spinner::new("refresh-servers").color(cx.theme().text_secondary))
                    .into_any_element(),
                _ => Button::icon("refresh-servers", Icon::RotateCcw, "Refresh servers")
                    .variant(Variant::Ghost)
                    .size(Size::XSmall)
                    .disabled(!self.servers.ready())
                    .on_click(cx.listener(|this, _, _, cx| this.check_servers(cx)))
                    .into_any_element(),
            }),
        )
    }

    fn confirmation(
        &self,
        confirm: &Confirming,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let dismiss = cx.listener(|this, _: &(), _, cx| this.dismiss(cx));
        let (title, description, actions): (String, String, Vec<gpui::AnyElement>) =
            match &confirm.ask {
                Ask::Close { targets, running } => (
                    match targets.len() {
                        1 => format!("Close {}?", targets[0].name),
                        count => format!("Close {count} accounts?"),
                    },
                    match (targets.len(), running) {
                        (1, _) => "This stops its running script and logs it out.".to_owned(),
                        (_, 1) => "This stops 1 running script and logs them all out.".to_owned(),
                        (_, count) => {
                            format!("This stops {count} running scripts and logs them all out.")
                        }
                    },
                    vec![
                        Button::new(
                            "confirm-close",
                            if targets.len() == 1 {
                                "Close account"
                            } else {
                                "Close accounts"
                            },
                        )
                        .variant(Variant::Danger)
                        .on_click(cx.listener(|this, _, _, cx| this.confirm(true, cx)))
                        .into_any_element(),
                    ],
                ),
                Ask::Start {
                    setup, conflicts, ..
                } => (
                    format!("Start {setup}?"),
                    match conflicts.as_slice() {
                        [one] => format!("{one} is already running another script."),
                        many => format!("{} are already running other scripts.", join_names(many)),
                    },
                    vec![
                        Button::new(
                            "confirm-skip",
                            if conflicts.len() == 1 {
                                "Leave it running"
                            } else {
                                "Leave them running"
                            },
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.confirm(false, cx)))
                        .into_any_element(),
                        Button::new(
                            "confirm-replace",
                            if conflicts.len() == 1 {
                                "Replace script"
                            } else {
                                "Replace scripts"
                            },
                        )
                        .variant(Variant::Primary)
                        .on_click(cx.listener(|this, _, _, cx| this.confirm(true, cx)))
                        .into_any_element(),
                    ],
                ),
                Ask::Remove { account } => (
                    format!("Remove {}?", account.name),
                    format!(
                        "This deletes the saved login for {} and takes it out of any setups. If \
                         its tab is open, the tab stays open.",
                        account.name
                    ),
                    vec![
                        Button::new("confirm-remove", "Remove account")
                            .variant(Variant::Danger)
                            .on_click(cx.listener(|this, _, _, cx| this.confirm(true, cx)))
                            .into_any_element(),
                    ],
                ),
                Ask::Delete { setup } => (
                    format!("Delete {setup}?"),
                    format!("The accounts in {setup} stay in Mira. Only the setup is deleted."),
                    vec![
                        Button::new("confirm-delete", "Delete setup")
                            .variant(Variant::Danger)
                            .on_click(cx.listener(|this, _, _, cx| this.confirm(true, cx)))
                            .into_any_element(),
                    ],
                ),
            };
        actions
            .into_iter()
            .fold(
                Dialog::new("confirm", confirm.open, title)
                    .alert()
                    .description(description)
                    .action(
                        Button::new("confirm-cancel", "Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx))),
                    ),
                |dialog, action| dialog.action(action),
            )
            .on_close(move |window, cx| dismiss(&(), window, cx))
    }
}

fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn setup_menu(this: &WeakEntity<Manager>, name: &str) -> Vec<MenuItem> {
    let item = |label: &'static str, run: fn(&mut Manager, &str, &mut Context<Manager>)| {
        let this = this.clone();
        let name = name.to_owned();
        MenuItem::action(label, move |_, cx| {
            this.update(cx, |manager, cx| run(manager, &name, cx)).ok();
        })
    };
    vec![
        {
            let this = this.clone();
            let name = name.to_owned();
            MenuItem::action("Rename", move |window, cx| {
                this.update(cx, |manager, cx| manager.start_rename(&name, window, cx))
                    .ok();
            })
        },
        item("Duplicate", |manager, name, cx| {
            manager.duplicate_setup(name, cx)
        }),
        MenuItem::separator(),
        item("Delete…", |manager, name, cx| {
            manager.ask(
                Ask::Delete {
                    setup: name.to_owned(),
                },
                cx,
            )
        })
        .danger(),
    ]
}

impl Render for Manager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if let View::Setup(name) = &self.view
            && self.setup().is_none()
        {
            let missing = name.clone();
            tracing::debug!(setup = missing, "the shown setup is gone");
            self.view = View::All;
        }
        let accounts = self.view_accounts();
        let presences = self.presences(&accounts, cx);
        let faces: Vec<Face> = accounts
            .iter()
            .zip(&presences)
            .map(|(account, presence)| face(account, presence))
            .collect();
        let title_bar = self.title_bar(&faces, window, cx);
        let sidebar = self.sidebar(&theme, cx);
        let selected = self.checked_accounts();
        let scripts = self.recent_scripts(cx);
        let width = f32::from(window.viewport_size().width) - f32::from(SIDEBAR_WIDTH);
        self.columns = ((width - 16.0) / CARD_WIDTH).floor().clamp(1.0, 6.0) as usize;
        mira_ui::nav::set_scroll_root(&self.grid_scroll, window, cx);
        let content = match &self.accounts {
            Err(error) => div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .px(px(24.0))
                .text_color(theme.danger_text)
                .child(format!("Mira couldn't load your accounts: {error}"))
                .into_any_element(),
            Ok(all) if all.list().is_empty() => self.empty(cx).into_any_element(),
            Ok(_) => {
                let header = self.header(&theme, &accounts, &presences, &scripts, cx);
                let query = self.filter.read(cx).text().to_owned();
                let setup = self.setup().cloned();
                let cards: Vec<_> = accounts
                    .iter()
                    .zip(presences.iter().zip(faces))
                    .filter(|(account, (_, face))| {
                        face.shown_in(self.show) && face.matches(account, &query)
                    })
                    .map(|(account, (presence, face))| {
                        let plan = setup
                            .as_ref()
                            .and_then(|setup| plan_line(setup, &account.id(), presence));
                        let place = match setup.as_ref().and_then(|setup| setup.server.clone()) {
                            Some(server) if !face.open => Line::new(server, Mood::Quiet),
                            _ => face.place,
                        };
                        let face = Face {
                            plan,
                            place,
                            ..face
                        };
                        self.card(&theme, account, face, &scripts, window, cx)
                            .into_any_element()
                    })
                    .collect();
                let grid = if cards.is_empty() {
                    div()
                        .flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .text_color(theme.text_secondary)
                        .child(match query.trim() {
                            "" if setup.as_ref().is_some_and(|setup| setup.members.is_empty()) => {
                                "No accounts in this setup yet. Add some from the Accounts menu."
                                    .to_owned()
                            }
                            "" => "No accounts here.".to_owned(),
                            query => format!("No accounts match \u{201c}{query}\u{201d}."),
                        })
                        .into_any_element()
                } else {
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .child(
                            div()
                                .id("accounts-grid")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .track_scroll(&self.grid_scroll)
                                .grid()
                                .grid_cols(self.columns as u16)
                                .content_start()
                                .gap(px(8.0))
                                .px(px(12.0))
                                .pt(px(8.0))
                                .pb(px(if selected.is_empty() { 12.0 } else { 64.0 }))
                                .children(cards),
                        )
                        .child(Scrollbar::new(
                            "accounts-scrollbar",
                            self.grid_scroll.clone(),
                        ))
                        .into_any_element()
                };
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(header)
                    .children(self.notice.clone().map(|notice| {
                        div().flex_none().px(px(12.0)).pt(px(8.0)).child(Alert::new(
                            "notice",
                            Tone::Danger,
                            notice,
                        ))
                    }))
                    .child(grid)
                    .into_any_element()
            }
        };
        let selection_bar = (!selected.is_empty())
            .then(|| self.selection_bar(&theme, selected, scripts.clone(), cx));
        let form = self.form.as_ref().map(|form| self.form(&theme, form, cx));
        let bulk = self
            .bulk
            .as_ref()
            .map(|bulk| self.bulk_form(&theme, bulk, cx));
        let confirmation = self
            .confirm
            .as_ref()
            .map(|confirm| self.confirmation(confirm, cx));
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.base)
            .text_size(TEXT_BODY)
            .text_color(theme.text)
            .key_context(format!("{FOCUS_SCOPE} {CONTEXT}").as_str())
            .id("accounts")
            .role(Role::Group)
            .track_focus(&self.focus)
            .on_action(|_: &CloseSession, window, _| window.remove_window())
            .when(!self.dialog_open(), |root| {
                root.on_action(cx.listener(|this, _: &AddAccount, _, cx| this.edit(None, cx)))
                    .on_action(
                        cx.listener(|this, _: &NewSetup, window, cx| this.new_setup(window, cx)),
                    )
                    .on_action(cx.listener(|this, _: &StartSetup, _, cx| this.start_current(cx)))
                    .on_action(cx.listener(|this, _: &SelectShown, _, cx| this.select_shown(cx)))
                    .on_action(
                        cx.listener(|this, _: &ClearSelection, _, cx| this.clear_selection(cx)),
                    )
                    .on_action(cx.listener(|this, _: &FindAccount, window, cx| {
                        this.filter.read(cx).focus_handle(cx).focus(window, cx)
                    }))
            })
            .on_key_down(cx.listener(Self::key_down))
            .child(mira_ui::nav::pointer_probe())
            .child(title_bar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar)
                    .child(content),
            )
            .children(selection_bar)
            .children(form)
            .children(bulk)
            .children(confirmation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_script(entry: &str, phase: ScriptPhase) -> Presence {
        Presence::Open(Summary {
            login: Login::Online,
            place: None,
            character: None,
            script: Some(ScriptState {
                entry: entry.into(),
                name: file_name(Path::new(entry)).into(),
                phase,
            }),
        })
    }

    fn online(entry: &str, phase: ScriptPhase) -> Summary {
        let Presence::Open(summary) = with_script(entry, phase) else {
            unreachable!()
        };
        summary
    }

    fn disconnected(entry: &str) -> Summary {
        Summary {
            login: Login::Disconnected,
            ..online(entry, ScriptPhase::Running)
        }
    }

    fn waiting(open: Option<Summary>) -> Presence {
        Presence::Waiting {
            place: 2,
            of: 3,
            open,
        }
    }

    fn member(username: &str, script: Option<&str>) -> Member {
        Member {
            username: username.into(),
            script: script.map(Into::into),
        }
    }

    fn id(username: &str) -> AccountId {
        AccountId::of(username)
    }

    #[test]
    fn a_card_names_the_setups_script_unless_another_script_is_busy_or_failed() {
        let setup = Setup {
            name: "Warlord farm".into(),
            script: Some("/s/warlord.ts".into()),
            server: None,
            members: vec![member("main", None)],
        };
        let account = Account {
            name: "Main".into(),
            username: "main".into(),
            password: String::new(),
            server: None,
        };
        let note = |presence: Presence| {
            let face = Face {
                plan: plan_line(&setup, &account.id(), &presence),
                ..face(&account, &presence)
            };
            face.note()
                .map(|line| (line.text.to_string(), line.path.clone()))
        };
        let planned = Some(("warlord.ts".to_owned(), Some("/s/warlord.ts".into())));
        assert_eq!(note(Presence::Closed(None)), planned);
        assert_eq!(
            note(with_script("/s/hunt.ts", ScriptPhase::Finished)),
            planned
        );
        assert_eq!(
            note(with_script("/s/hunt.ts", ScriptPhase::Running)),
            Some(("hunt.ts".to_owned(), Some("/s/hunt.ts".into())))
        );
        assert_eq!(
            note(with_script("/s/warlord.ts", ScriptPhase::Finished)),
            Some((
                "warlord.ts finished".to_owned(),
                Some("/s/warlord.ts".into())
            ))
        );
        assert_eq!(
            note(Presence::Unopened("No such file".into())),
            Some(("No such file".to_owned(), None))
        );
    }

    #[test]
    fn starting_a_setup_leaves_matching_runs_alone_and_flags_other_scripts() {
        let setup = Setup {
            name: "Warlord farm".into(),
            script: Some("/s/warlord.ts".into()),
            server: None,
            members: vec![
                member("main", None),
                member("alt1", None),
                member("alt2", None),
                member("alt3", Some("/s/follower.ts")),
                member("alt4", None),
                member("alt5", None),
                member("alt6", None),
                member("alt7", None),
                member("gone", None),
            ],
        };
        let presences = HashMap::from([
            (
                id("main"),
                with_script("/s/warlord.ts", ScriptPhase::Running),
            ),
            (
                id("alt1"),
                with_script("/s/hunt.ts", ScriptPhase::NeedsInput),
            ),
            (id("alt2"), with_script("/s/hunt.ts", ScriptPhase::Stopping)),
            (id("alt3"), Presence::Closed(None)),
            (
                id("alt4"),
                with_script("/s/warlord.ts", ScriptPhase::Failed("x".into())),
            ),
            (
                id("alt5"),
                Presence::Open(Summary {
                    login: Login::Disconnected,
                    place: None,
                    character: None,
                    script: Some(ScriptState {
                        entry: "/s/warlord.ts".into(),
                        name: "warlord.ts".into(),
                        phase: ScriptPhase::Running,
                    }),
                }),
            ),
            (id("alt6"), waiting(Some(disconnected("/s/warlord.ts")))),
            (
                id("alt7"),
                waiting(Some(online("/s/hunt.ts", ScriptPhase::Running))),
            ),
        ]);
        assert_eq!(
            plan(&setup, &presences, Scope::Everyone),
            Plan {
                start: vec![
                    (id("alt2"), "/s/warlord.ts".into()),
                    (id("alt3"), "/s/follower.ts".into()),
                    (id("alt4"), "/s/warlord.ts".into()),
                ],
                conflicts: vec![
                    (id("alt1"), "/s/warlord.ts".into()),
                    (id("alt7"), "/s/warlord.ts".into()),
                ],
                login: vec![id("alt5"), id("alt6")],
                unscripted: Vec::new(),
            }
        );
    }

    #[test]
    fn a_member_without_a_script_is_reported_not_started() {
        let setup = Setup {
            name: "Mules".into(),
            script: None,
            server: None,
            members: vec![member("bank", None), member("alt", Some("/s/sell.ts"))],
        };
        let presences = HashMap::from([
            (id("bank"), Presence::Closed(None)),
            (id("alt"), Presence::Closed(None)),
        ]);
        let plan = plan(&setup, &presences, Scope::Everyone);
        assert_eq!(plan.unscripted, [id("bank")]);
        assert_eq!(plan.start, [(id("alt"), "/s/sell.ts".into())]);
    }

    #[test]
    fn retrying_a_setup_only_touches_failed_members_without_doubling_scripts() {
        let setup = Setup {
            name: "Dailies".into(),
            script: Some("/s/daily.ts".into()),
            server: None,
            members: ["main", "alt1", "alt2", "alt3", "alt4"]
                .into_iter()
                .map(|name| member(name, None))
                .collect(),
        };
        let presences = HashMap::from([
            (
                id("main"),
                with_script("/s/daily.ts", ScriptPhase::Finished),
            ),
            (id("alt1"), with_script("/s/daily.ts", ScriptPhase::Running)),
            (
                id("alt2"),
                with_script("/s/daily.ts", ScriptPhase::Failed("x".into())),
            ),
            (id("alt3"), Presence::Open(disconnected("/s/daily.ts"))),
            (id("alt4"), Presence::Closed(None)),
        ]);
        assert_eq!(
            plan(&setup, &presences, Scope::Failed),
            Plan {
                start: vec![(id("alt2"), "/s/daily.ts".into())],
                login: vec![id("alt3")],
                ..Plan::default()
            }
        );
    }

    #[test]
    fn retry_failed_picks_up_logins_and_scripts_that_failed() {
        let failed_login = Presence::Open(Summary {
            login: Login::Failed("Wrong password".into()),
            place: None,
            character: None,
            script: None,
        });
        assert!(retryable(&failed_login));
        assert!(retryable(&Presence::Unopened("no player".into())));
        assert!(retryable(&with_script(
            "/s/a.ts",
            ScriptPhase::Failed("x".into())
        )));
        assert!(!retryable(&with_script("/s/a.ts", ScriptPhase::Finished)));
        assert!(!retryable(&with_script("/s/a.ts", ScriptPhase::NeedsInput)));
        assert!(!retryable(&Presence::Closed(None)));
        assert!(retryable(&Presence::Closed(Some(ScriptState {
            entry: "/s/a.ts".into(),
            name: "a.ts".into(),
            phase: ScriptPhase::Failed("x".into()),
        }))));
    }
}
