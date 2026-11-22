mod run;
mod view;

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, EntityId, FocusHandle, Focusable as _, Global,
    KeyBinding, KeyDownEvent, ScrollHandle, SharedString, Subscription, Task, TitlebarOptions,
    Window, WindowBounds, WindowHandle, WindowOptions, actions, px, size,
};
use reqwest::header::HeaderValue;

use self::run::{Filter, Runs, Start, Stoppable, plan, setup_work, stoppable};
use super::{
    Client, ScriptState, Updated,
    account_card::Presence,
    client::{self, Home, Layout},
    script_names,
};
use mira_ui::primitives::{Input, InputEvent, text_field};

use crate::{
    account_settings,
    accounts::{Account, AccountId, Accounts, Change, Draft, Invalid, SaveError, SetupError},
    file_panel, net, script_inputs,
    setups::{Launch, Member, Setup},
    shared_objects,
};

actions!(
    mira,
    [
        OpenManager,
        AddAccount,
        NewSetup,
        StartSetup,
        ClearSelection
    ]
);

type Pick = Rc<dyn Fn(&mut Manager, Option<PathBuf>, &mut Window, &mut Context<Manager>)>;
type PickServer = Rc<dyn Fn(Option<String>, &mut Window, &mut App)>;

const CONTEXT: &str = "Manager";
const MAX_FIELD: usize = 64;
const RECENT_LIMIT: usize = 8;
const SERVERS_FRESH_FOR: Duration = Duration::from_secs(60);
const SERVERS_RECHECK_AFTER: Duration = Duration::from_secs(5);
const CLOCK: Duration = Duration::from_secs(30);
const INPUTS_UNREADABLE: &str =
    "Saved script inputs couldn't be read. Open Scripts to recover them, then start again.";

struct Open(WindowHandle<Manager>);

impl Global for Open {}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-shift-n", AddAccount, Some(CONTEXT)),
        KeyBinding::new("cmd-n", NewSetup, Some(CONTEXT)),
        KeyBinding::new("cmd-enter", StartSetup, Some(CONTEXT)),
        KeyBinding::new("cmd-a", text_field::SelectAll, Some(CONTEXT)),
        KeyBinding::new("escape", ClearSelection, Some(CONTEXT)),
    ]);
}

pub(crate) fn open(
    path: PathBuf,
    device: HeaderValue,
    cx: &mut App,
) -> Option<WindowHandle<Manager>> {
    if let Some(manager) = cx.try_global::<Open>().map(|open| open.0)
        && manager
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return Some(manager);
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1000.0), px(600.0)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Manager".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(12.0))),
        }),
        app_owns_titlebar_drag: true,
        window_min_size: Some(size(px(720.0), px(420.0))),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        crate::settings::follow_system(window);
        let manager = cx.new(|cx| {
            let mut manager = Manager::new(path, device, window, cx);
            manager.check_servers(cx);
            manager
        });
        let focus = manager.read(cx).focus.clone();
        window.focus(&focus, cx);
        manager
    }) {
        Ok(manager) => {
            cx.set_global(Open(manager));
            Some(manager)
        }
        Err(error) => {
            tracing::error!(%error, "couldn't open the manager");
            None
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum View {
    All,
    Setup(String),
}

pub(crate) struct Manager {
    accounts: Result<Accounts, SharedString>,
    device: HeaderValue,
    servers: Servers,
    view: View,
    filter: Option<Filter>,
    editing: Option<Editing>,
    checked: HashSet<AccountId>,
    cursor: Option<AccountId>,
    columns: usize,
    recent: Vec<PathBuf>,
    form: Option<Form>,
    confirm: Option<Confirming>,
    notice: Option<SharedString>,
    search: Entity<Input>,
    grid_scroll: ScrollHandle,
    focus: FocusHandle,
    clock: Option<Task<()>>,
    #[cfg(test)]
    fake: Option<Fake>,
    watching: HashMap<EntityId, [Subscription; 2]>,
    _subscriptions: [Subscription; 4],
}

#[cfg(test)]
struct Fake {
    presences: HashMap<AccountId, Presence>,
    finished: HashMap<AccountId, Instant>,
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

struct Editing {
    setup: String,
    name: Entity<Input>,
    error: Option<SharedString>,
    _subscriptions: [Subscription; 1],
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

struct Confirming {
    open: bool,
    ask: Ask,
}

#[derive(Clone)]
enum Ask {
    Start {
        setup: String,
        start: Start,
        with: Option<PathBuf>,
        conflicts: Vec<String>,
    },
    Run {
        targets: Vec<Account>,
        script: PathBuf,
        conflicts: Vec<(String, String)>,
    },
    Stop {
        targets: Vec<AccountId>,
        title: String,
        description: String,
        many: bool,
    },
    Close {
        targets: Vec<Account>,
        running: usize,
    },
    Remove {
        account: Account,
    },
    Delete {
        setup: String,
    },
}

fn busy(account: &AccountId, action: &str, cx: &App) -> Option<String> {
    let clients = games(cx);
    if clients
        .iter()
        .any(|client| client.uses_settings_of(account, cx))
    {
        Some(format!("Close this account's tab before {action}."))
    } else if clients
        .iter()
        .any(|client| client.waiting(account).is_some())
    {
        Some(format!("Stop this account's launch before {action}."))
    } else {
        None
    }
}

fn games(cx: &App) -> Vec<&Client> {
    client::clients(cx)
        .into_iter()
        .map(|client| client.read(cx))
        .collect()
}

fn window_for(
    account: &AccountId,
    home: Option<&Home>,
    cx: &mut App,
) -> Option<anyhow::Result<WindowHandle<Client>>> {
    match client::holding(account, cx) {
        Some(game) => Some(Ok(game)),
        None => home.map(|home| client::open(home.clone(), false, cx)),
    }
}

fn watch(client: &Entity<Client>, cx: &mut Context<Manager>) -> [Subscription; 2] {
    [
        cx.observe(client, |_, _, cx| cx.notify()),
        cx.subscribe(client, |_, _, _: &Updated, cx| cx.notify()),
    ]
}

fn unavailable(error: &anyhow::Error) -> SharedString {
    tracing::error!(%error, "couldn't open the client");
    "Couldn't open the game window.".into()
}

fn presence_across(clients: &[&Client], account: &AccountId, cx: &App) -> Presence {
    if let Some(client) = clients.iter().find(|client| client.holds(account, cx)) {
        return presence(client, account, cx);
    }
    if let Some(error) = clients.iter().find_map(|client| client.unopened(account)) {
        return Presence::Unopened(error);
    }
    Presence::Closed(clients.iter().find_map(|client| client.ended(account)))
}

fn presence(client: &Client, account: &AccountId, cx: &App) -> Presence {
    if let Some((place, _)) = client.waiting(account) {
        return Presence::Waiting {
            place,
            script: client
                .waiting_script(account)
                .map(|script| ScriptState::queued(&script, cx)),
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
        Presence::Open(summary) => summary.script.as_ref().map(|script| script.entry.as_path()),
        Presence::Closed(Some(script)) => Some(&script.entry),
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

fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn plural(count: usize, word: &str) -> String {
    match count {
        1 => format!("1 {word}"),
        count => format!("{count} {word}s"),
    }
}

impl Manager {
    fn new(
        path: PathBuf,
        device: HeaderValue,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe_global::<crate::updates::Updates>(|_, cx| cx.notify())
            .detach();
        cx.observe_global::<script_names::Names>(|_, cx| cx.notify())
            .detach();
        let focus = cx.focus_handle();
        let search = cx.new(|cx| {
            Input::new("Filter accounts", cx)
                .placeholder("Filter accounts")
                .search()
                .small()
        });
        let manager = cx.weak_entity();
        let closed = manager.clone();
        let subscriptions = [
            cx.on_window_closed(move |cx, _| {
                closed.update(cx, |_, cx| cx.notify()).ok();
            }),
            cx.observe_new(move |_: &mut Client, _, cx| {
                let client = cx.entity();
                manager
                    .update(cx, |manager, cx| manager.watch(&client, cx))
                    .ok();
            }),
            cx.on_focus_lost(window, |this, window, cx| window.focus(&this.focus, cx)),
            cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Changed) {
                    this.checked.clear();
                    cx.notify();
                }
            }),
        ];
        cx.default_global::<Runs>();
        let watching = client::clients(cx)
            .iter()
            .map(|client| (client.entity_id(), watch(client, cx)))
            .collect();
        Self {
            accounts: Accounts::open(path).map_err(Into::into),
            device,
            servers: Servers {
                list: Vec::new(),
                check: Check::Idle,
            },
            view: View::All,
            filter: None,
            editing: None,
            checked: HashSet::new(),
            cursor: None,
            columns: 1,
            recent: Vec::new(),
            form: None,
            confirm: None,
            notice: None,
            search,
            grid_scroll: ScrollHandle::new(),
            focus,
            clock: None,
            #[cfg(test)]
            fake: None,
            watching,
            _subscriptions: subscriptions,
        }
    }

    fn watch(&mut self, client: &Entity<Client>, cx: &mut Context<Self>) {
        let live: HashSet<EntityId> = client::clients(cx).iter().map(Entity::entity_id).collect();
        self.watching.retain(|id, _| live.contains(id));
        self.watching.insert(client.entity_id(), watch(client, cx));
        cx.notify();
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

    fn setups(&self) -> Vec<Setup> {
        self.accounts
            .as_ref()
            .map(|accounts| accounts.setups().to_vec())
            .unwrap_or_default()
    }

    fn setup_named(&self, name: &str) -> Option<&Setup> {
        self.accounts.as_ref().ok()?.setup(name)
    }

    fn setup(&self) -> Option<&Setup> {
        let View::Setup(name) = &self.view else {
            return None;
        };
        self.setup_named(name)
    }

    fn members(&self, setup: &Setup) -> Vec<Account> {
        let accounts = self.account_list();
        setup
            .members
            .iter()
            .filter_map(|member| {
                accounts
                    .iter()
                    .find(|account| account.id() == member.id())
                    .cloned()
            })
            .collect()
    }

    fn view_accounts(&self) -> Vec<Account> {
        match self.setup() {
            Some(setup) => self.members(setup),
            None => self.account_list(),
        }
    }

    fn presences(&self, cx: &App) -> HashMap<AccountId, Presence> {
        #[cfg(test)]
        if let Some(fake) = &self.fake {
            return self
                .account_list()
                .iter()
                .map(|account| {
                    let id = account.id();
                    let presence = fake
                        .presences
                        .get(&id)
                        .cloned()
                        .unwrap_or(Presence::Closed(None));
                    (id, presence)
                })
                .collect();
        }
        let clients = games(cx);
        self.account_list()
            .iter()
            .map(|account| {
                let id = account.id();
                let presence = presence_across(&clients, &id, cx);
                (id, presence)
            })
            .collect()
    }

    fn finished_at(&self, account: &AccountId, script: &Path, cx: &App) -> Option<Instant> {
        #[cfg(test)]
        if let Some(fake) = &self.fake {
            return fake.finished.get(account).copied();
        }
        client::finished_at(account, script, cx)
    }

    fn done(&self, setup: &Setup, cx: &App) -> HashSet<AccountId> {
        let (Some(script), Some(started)) = (
            setup.script.as_deref(),
            cx.global::<Runs>().started(&setup.name),
        ) else {
            return HashSet::new();
        };
        setup
            .members
            .iter()
            .map(Member::id)
            .filter(|id| {
                self.finished_at(id, script, cx)
                    .is_some_and(|at| at >= started)
            })
            .collect()
    }

    fn checked_accounts(&self) -> Vec<Account> {
        self.account_list()
            .into_iter()
            .filter(|account| self.checked.contains(&account.id()))
            .collect()
    }

    fn named(&self, ids: &[AccountId]) -> Vec<Account> {
        let accounts = self.account_list();
        ids.iter()
            .filter_map(|id| accounts.iter().find(|account| &account.id() == id).cloned())
            .collect()
    }

    fn act(
        &mut self,
        targets: Vec<Account>,
        home: Option<Home>,
        cx: &mut Context<Self>,
        act: impl Fn(&mut Client, &Account, &mut Window, &mut Context<Client>),
    ) {
        let mut failed = None;
        for account in &targets {
            let Some(game) = window_for(&account.id(), home.as_ref(), cx) else {
                continue;
            };
            let done = game.and_then(|game| {
                game.update(cx, |client, window, cx| act(client, account, window, cx))
            });
            if let Err(error) = done {
                failed = Some(error);
            }
        }
        self.notice = failed.map(|error| unavailable(&error));
        cx.notify();
    }

    fn launch(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        self.act(
            targets,
            Some(Home::Shared),
            cx,
            |client, account, window, cx| client.launch(account, None, window, cx),
        );
    }

    fn remember(&mut self, script: &Path, cx: &mut Context<Self>) {
        self.recent.retain(|recent| recent != script);
        self.recent.insert(0, script.to_path_buf());
        crate::settings::show_recent_script(script, cx);
    }

    fn run(&mut self, targets: Vec<Account>, script: PathBuf, cx: &mut Context<Self>) {
        let presences = self.presences(cx);
        let mut plan = Vec::new();
        let mut conflicts = Vec::new();
        for account in targets {
            match presences.get(&account.id()).and_then(Presence::running) {
                Some(running) if running == script => {}
                Some(running) => {
                    conflicts.push((account.name.clone(), script_names::name(running, cx).into()));
                    plan.push(account);
                }
                None => plan.push(account),
            }
        }
        if conflicts.is_empty() {
            self.run_now(plan, script, cx);
        } else {
            self.ask(
                Ask::Run {
                    targets: plan,
                    script,
                    conflicts,
                },
                cx,
            );
        }
    }

    fn run_now(&mut self, targets: Vec<Account>, script: PathBuf, cx: &mut Context<Self>) {
        self.remember(&script, cx);
        if script_inputs::unreadable(cx) {
            self.notice = Some(INPUTS_UNREADABLE.into());
            cx.notify();
            return;
        }
        self.act(
            targets,
            Some(Home::Shared),
            cx,
            |client, account, window, cx| {
                client.run_script(account, script.clone(), true, None, window, cx)
            },
        );
    }

    fn run_chosen(&mut self, targets: Vec<Account>, window: &Window, cx: &mut Context<Self>) {
        let chosen = file_panel::open(file_panel::Pick::File, "Choose", window, cx);
        cx.spawn(async move |this, cx| {
            let Some(script) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.run(targets, script, cx));
        })
        .detach();
    }

    fn stop(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        let presences = self.presences(cx);
        let found = stoppable(
            targets
                .iter()
                .filter_map(|account| Some((account.id(), presences.get(&account.id())?))),
            None,
        );
        self.ask_stop(found, None, &presences, cx);
    }

    fn stop_setup(&mut self, cx: &mut Context<Self>) {
        let Some(setup) = self.setup().cloned() else {
            return;
        };
        let presences = self.presences(cx);
        let found = setup_work(&setup, &presences);
        self.ask_stop(found, Some(&setup), &presences, cx);
    }

    fn ask_stop(
        &mut self,
        found: Stoppable,
        setup: Option<&Setup>,
        presences: &HashMap<AccountId, Presence>,
        cx: &mut Context<Self>,
    ) {
        if found.is_empty() {
            return;
        }
        let targets = found.all();
        let names: Vec<String> = self
            .named(&targets)
            .into_iter()
            .map(|account| account.name)
            .collect();
        let scripts: HashSet<&Path> = targets
            .iter()
            .filter_map(|id| {
                let presence = presences.get(id)?;
                presence.running().or_else(|| presence.queued())
            })
            .collect();
        let script = (scripts.len() == 1)
            .then(|| {
                scripts
                    .iter()
                    .next()
                    .map(|script| script_names::name(script, cx))
            })
            .flatten();
        let one = (names.len() == 1).then(|| names[0].clone());
        let title = match (setup, &one, &script) {
            (Some(setup), _, _) => format!("Stop {}?", setup.name),
            (None, Some(name), Some(script)) => format!("Stop {script} on {name}?"),
            (None, None, Some(script)) => format!("Stop {script} on {} accounts?", names.len()),
            (None, _, None) => format!("Stop {}?", plural(targets.len(), "script")),
        };
        let mut description = String::new();
        if setup.is_some() {
            let (running, waiting) = (found.running.len(), found.waiting.len());
            description.push_str(&match (running, waiting) {
                (0, waiting) => format!(
                    "This stops {} that haven't started. ",
                    plural(waiting, "script")
                ),
                (running, 0) => format!("This stops {}. ", plural(running, "running script")),
                (running, waiting) => format!(
                    "This stops {} and {waiting} that haven't started. ",
                    plural(running, "running script")
                ),
            });
        }
        description.push_str(&match &one {
            Some(name) => format!("{name} stays logged in."),
            None => "The accounts stay logged in.".to_owned(),
        });
        if let Some(setup) = setup {
            let done = self.done(setup, cx).len();
            if done > 0 {
                description.push_str(&format!(
                    " Starting {} again later skips the {done} already done.",
                    setup.name
                ));
            }
        }
        self.ask(
            Ask::Stop {
                targets,
                title,
                description,
                many: one.is_none(),
            },
            cx,
        );
    }

    fn stop_now(&mut self, targets: Vec<AccountId>, cx: &mut Context<Self>) {
        let targets = self.named(&targets);
        self.act(targets, None, cx, |client, account, _, cx| {
            client.stop_script(&account.id(), cx)
        });
    }

    fn close(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        let presences = self.presences(cx);
        let running = targets
            .iter()
            .filter(|account| {
                presences
                    .get(&account.id())
                    .is_some_and(|presence| presence.running().is_some())
            })
            .count();
        if running == 0 || !crate::settings::confirm_close(cx) {
            self.close_now(targets, cx);
        } else {
            self.ask(Ask::Close { targets, running }, cx);
        }
    }

    fn close_now(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        self.act(targets, None, cx, |client, account, window, cx| {
            client.close_account(&account.id(), window, cx)
        });
    }

    fn show_account(&mut self, account: Account, cx: &mut Context<Self>) {
        self.act(vec![account], None, cx, |client, account, window, cx| {
            client.show_account(&account.id(), window, cx)
        });
    }

    fn open_account(&mut self, account: Account, cx: &mut Context<Self>) {
        let open = self
            .presences(cx)
            .get(&account.id())
            .is_some_and(Presence::opened);
        if open {
            self.show_account(account, cx);
        } else {
            self.launch(vec![account], cx);
        }
    }

    fn restart(&mut self, account: &Account, cx: &mut Context<Self>) {
        let id = account.id();
        if let Some(game) = client::holding(&id, cx) {
            game.update(cx, |client, _, cx| client.restart_script(&id, cx))
                .ok();
        }
    }

    fn recent_scripts(&self, cx: &App) -> Vec<PathBuf> {
        let from_setups = self.setups().into_iter().filter_map(|setup| setup.script);
        let hidden = crate::settings::hidden_recent_scripts(cx);
        let mut scripts: Vec<PathBuf> = Vec::new();
        for script in self
            .recent
            .iter()
            .cloned()
            .chain(
                games(cx)
                    .iter()
                    .flat_map(|client| client.loaded_scripts(cx)),
            )
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
            self.check_stale_servers(cx);
        }
        self.cursor = Some(id);
        cx.notify();
    }

    fn set_filter(&mut self, filter: Option<Filter>, cx: &mut Context<Self>) {
        self.filter = filter;
        self.checked.clear();
        cx.notify();
    }

    fn set_view(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        if self.view != view {
            self.finish_edit(window, cx);
            self.view = view;
            self.filter = None;
            self.checked.clear();
            self.cursor = None;
        }
        cx.notify();
    }

    fn shown(&self, cx: &App) -> Vec<Account> {
        let accounts = self.view_accounts();
        let presences = self.presences(cx);
        let done = self
            .setup()
            .map(|setup| self.done(setup, cx))
            .unwrap_or_default();
        let query = self.search.read(cx).text().to_owned();
        let now = Instant::now();
        accounts
            .into_iter()
            .filter(|account| {
                let id = account.id();
                let presence = presences
                    .get(&id)
                    .cloned()
                    .unwrap_or(Presence::Closed(None));
                let card = super::account_card::card(account, &presence, done.contains(&id), now);
                self.filter.is_none_or(|filter| filter.admits(card.status))
                    && card.matches(account, &query)
            })
            .collect()
    }

    fn select_shown(&mut self, cx: &mut Context<Self>) {
        self.checked = self.shown(cx).iter().map(Account::id).collect();
        self.check_stale_servers(cx);
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
        if let Some(editing) = &self.editing {
            let setup = editing.setup.clone();
            if !modifiers.platform && !modifiers.control {
                self.reorder_key(&setup, event, modifiers.alt, cx);
            }
            return;
        }
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

    fn reorder_key(&mut self, name: &str, event: &KeyDownEvent, alt: bool, cx: &mut Context<Self>) {
        let Some(setup) = self.setup_named(name) else {
            return;
        };
        let ids: Vec<AccountId> = self.members(setup).iter().map(Account::id).collect();
        if ids.is_empty() {
            return;
        }
        let at = self
            .cursor
            .as_ref()
            .and_then(|cursor| ids.iter().position(|id| id == cursor));
        let columns = self.columns.max(1);
        let last = ids.len() - 1;
        let to = match (event.keystroke.key.as_str(), at) {
            ("left" | "right" | "up" | "down", None) => {
                self.cursor = Some(ids[0].clone());
                self.grid_scroll.scroll_to_item(0);
                cx.stop_propagation();
                cx.notify();
                return;
            }
            ("left", Some(at)) => at.saturating_sub(1),
            ("right", Some(at)) => (at + 1).min(last),
            ("up", Some(at)) => at.saturating_sub(columns),
            ("down", Some(at)) => (at + columns).min(last),
            _ => return,
        };
        let Some(at) = at else {
            return;
        };
        if alt {
            self.move_member(name, at, to, cx);
        } else {
            self.cursor = Some(ids[to].clone());
        }
        self.grid_scroll.scroll_to_item(to);
        cx.stop_propagation();
        cx.notify();
    }

    fn dialog_open(&self) -> bool {
        self.form.as_ref().is_some_and(|form| form.open)
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

    fn confirm(&mut self, choice: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(confirm) = self.confirm.as_mut().filter(|confirm| confirm.open) else {
            return;
        };
        confirm.open = false;
        match confirm.ask.clone() {
            Ask::Start {
                setup, start, with, ..
            } => self.start_setup(&setup, start, Some(choice), with, cx),
            Ask::Run {
                targets, script, ..
            } => self.run_now(targets, script, cx),
            Ask::Stop { targets, .. } => self.stop_now(targets, cx),
            Ask::Close { targets, .. } => self.close_now(targets, cx),
            Ask::Delete { setup } => self.delete_setup(&setup, window, cx),
            Ask::Remove { account } => self.remove(&account.id(), cx),
        }
        cx.notify();
    }

    fn primary_start(&self, setup: &Setup, cx: &App) -> Option<Start> {
        setup.script.as_ref()?;
        let done = self.done(setup, cx);
        let complete = !setup.members.is_empty()
            && setup
                .members
                .iter()
                .all(|member| done.contains(&member.id()));
        Some(if complete {
            Start::Fresh
        } else {
            Start::Continue
        })
    }

    fn start_current(&mut self, cx: &mut Context<Self>) {
        let Some(setup) = self.setup().cloned() else {
            return;
        };
        if self.editing.is_some() {
            return;
        }
        if let Some(start) = self.primary_start(&setup, cx) {
            self.start_setup(&setup.name, start, None, None, cx);
        }
    }

    fn start_with(&mut self, name: &str, script: PathBuf, cx: &mut Context<Self>) {
        self.start_setup(name, Start::Fresh, None, Some(script), cx);
    }

    fn start_setup(
        &mut self,
        name: &str,
        start: Start,
        replace: Option<bool>,
        with: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(mut setup) = self.setup_named(name).cloned() else {
            return;
        };
        if let Some(script) = &with {
            setup.script = Some(script.clone());
        }
        let Some(script) = setup.script.clone() else {
            return;
        };
        let presences = self.presences(cx);
        let done = self.done(&setup, cx);
        let plan = plan(&setup, &presences, &done, start);
        if !plan.conflicts.is_empty() && replace.is_none() {
            let conflicts = self
                .named(&plan.conflicts)
                .into_iter()
                .map(|account| account.name)
                .collect();
            self.ask(
                Ask::Start {
                    setup: setup.name,
                    start,
                    with,
                    conflicts,
                },
                cx,
            );
            return;
        }
        let runs: Vec<(Account, bool)> = self
            .named(&plan.start)
            .into_iter()
            .map(|account| (account, false))
            .chain(
                self.named(&plan.conflicts)
                    .into_iter()
                    .filter(|_| replace == Some(true))
                    .map(|account| (account, true)),
            )
            .collect();
        let logins = self.named(&plan.login);
        if !runs.is_empty() && script_inputs::unreadable(cx) {
            self.notice = Some(INPUTS_UNREADABLE.into());
            cx.notify();
            return;
        }
        if let Some(script) = with {
            self.remember(&script, cx);
            if !self.change_setup(name, cx, |setup| setup.script = Some(script)) {
                return;
            }
        }
        if start == Start::Fresh || cx.global::<Runs>().started(&setup.name).is_none() {
            cx.global_mut::<Runs>().start(&setup.name, Instant::now());
        }
        let home = if setup.launch.new_window {
            Home::Setup(setup.name.clone())
        } else {
            Home::Shared
        };
        let grid = setup.launch.grid;
        let work = runs
            .into_iter()
            .map(|(account, replace)| (account, Some(replace)))
            .chain(logins.into_iter().map(|account| (account, None)));
        let mut failed = None;
        for (account, replace) in work {
            let Some(game) = window_for(&account.id(), Some(&home), cx) else {
                continue;
            };
            let done = game.and_then(|game| {
                game.update(cx, |client, window, cx| {
                    match replace {
                        Some(replace) => {
                            client.run_script(&account, script.clone(), replace, None, window, cx)
                        }
                        None => client.launch(&account, None, window, cx),
                    }
                    if grid {
                        client.set_layout(Layout::Grid, window, cx);
                    }
                })
            });
            if let Err(error) = done {
                failed = Some(error);
            }
        }
        self.notice = failed.map(|error| unavailable(&error));
        cx.notify();
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
        if let Some(blocked) = renamed.and_then(|old| busy(old, "changing its username", cx)) {
            form.error = Some(Problem::Username(blocked.into()));
            cx.notify();
            return;
        }
        match accounts.save(form.editing.as_ref(), draft) {
            Ok(id) => {
                if let Some(old) = renamed {
                    account_settings::rename(old, &id, cx);
                    shared_objects::rename(old, &id, cx);
                }
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
        if let Some(blocked) = busy(id, "removing it", cx) {
            self.notice = Some(blocked.into());
            cx.notify();
            return;
        }
        match accounts.remove(id) {
            Ok(()) => {
                account_settings::forget(id, cx);
                shared_objects::forget(id, cx);
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

    fn set_servers(
        &mut self,
        ids: HashSet<AccountId>,
        server: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Ok(accounts) = &mut self.accounts else {
            return;
        };
        if let Err(error) = accounts.change(
            &ids,
            Change {
                server: Some(server),
            },
        ) {
            self.notice = Some(format!("couldn't save the accounts: {error}").into());
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
        let presences = self.presences(cx);
        let scripts: Vec<PathBuf> = targets
            .iter()
            .filter_map(|account| last_entry(presences.get(&account.id())?))
            .map(Path::to_path_buf)
            .collect();
        let members = targets
            .into_iter()
            .map(|account| Member {
                username: account.username,
            })
            .collect();
        self.create_setup(common_script(&scripts), members, window, cx);
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
            Ok(name) => self.edit_setup(&name, window, cx),
            Err(error) => self.notice = Some(format!("couldn't save the setups: {error}").into()),
        }
        cx.notify();
    }

    fn edit_setup(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.set_view(View::Setup(name.to_owned()), window, cx);
        let input = input(
            name.to_owned(),
            |cx| Input::new("Setup name", cx).small(),
            cx,
        );
        let subscriptions = [cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Submitted => this.finish_edit(window, cx),
                InputEvent::Changed => {
                    if let Some(editing) = &mut this.editing
                        && editing.error.take().is_some()
                    {
                        cx.notify();
                    }
                }
            },
        )];
        let focus = input.focus_handle(cx);
        window.focus(&focus, cx);
        self.editing = Some(Editing {
            setup: name.to_owned(),
            name: input,
            error: None,
            _subscriptions: subscriptions,
        });
        self.checked.clear();
        self.cursor = None;
        cx.notify();
    }

    fn finish_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(editing), Ok(accounts)) = (&mut self.editing, &mut self.accounts) else {
            return;
        };
        let name = editing.name.read(cx).text().to_owned();
        if name.trim() != editing.setup {
            match accounts.rename_setup(&editing.setup, &name) {
                Ok(name) => {
                    cx.global_mut::<Runs>().rename(&editing.setup, &name);
                    client::rename_setup(&editing.setup, &name, cx);
                    if self.view == View::Setup(editing.setup.clone()) {
                        self.view = View::Setup(name);
                    }
                }
                Err(SetupError::Invalid(invalid)) => {
                    editing.error = Some(invalid.to_string().into());
                    cx.notify();
                    return;
                }
                Err(SetupError::Io(error)) => {
                    self.notice = Some(format!("couldn't save the setups: {error}").into());
                }
            }
        }
        self.editing = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn change_setup(
        &mut self,
        name: &str,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Setup),
    ) -> bool {
        let Ok(accounts) = &mut self.accounts else {
            return false;
        };
        let saved = accounts.edit_setup(name, edit);
        if let Err(error) = &saved {
            self.notice = Some(format!("couldn't save the setups: {error}").into());
        }
        cx.notify();
        saved.is_ok()
    }

    fn set_setup_script(&mut self, name: &str, script: Option<PathBuf>, cx: &mut Context<Self>) {
        if let Some(script) = &script {
            self.remember(script, cx);
        }
        self.change_setup(name, cx, |setup| setup.script = script);
    }

    fn choose_setup_script(&mut self, name: String, window: &Window, cx: &mut Context<Self>) {
        let chosen = file_panel::open(file_panel::Pick::File, "Choose", window, cx);
        cx.spawn(async move |this, cx| {
            let Some(script) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.set_setup_script(&name, Some(script), cx)
            });
        })
        .detach();
    }

    fn choose_start(&mut self, name: String, window: &Window, cx: &mut Context<Self>) {
        let chosen = file_panel::open(file_panel::Pick::File, "Choose", window, cx);
        cx.spawn(async move |this, cx| {
            let Some(script) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.start_with(&name, script, cx));
        })
        .detach();
    }

    fn set_launch(&mut self, name: &str, edit: impl FnOnce(&mut Launch), cx: &mut Context<Self>) {
        self.change_setup(name, cx, |setup| edit(&mut setup.launch));
    }

    fn toggle_member(&mut self, name: &str, account: &Account, cx: &mut Context<Self>) {
        let id = account.id();
        let member = self
            .setup_named(name)
            .is_some_and(|setup| setup.member(&id).is_some());
        if member {
            self.checked.remove(&id);
            self.change_setup(name, cx, |setup| setup.remove(&[id]));
        } else {
            self.add_to_setup(name, vec![account.clone()], cx);
        }
    }

    fn add_to_setup(&mut self, name: &str, targets: Vec<Account>, cx: &mut Context<Self>) {
        self.change_setup(name, cx, |setup| {
            setup.add(targets.into_iter().map(|account| account.username));
        });
    }

    fn move_member(&mut self, name: &str, from: usize, to: usize, cx: &mut Context<Self>) {
        self.change_setup(name, cx, |setup| setup.move_member(from, to));
    }

    fn duplicate_setup(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(accounts) = &mut self.accounts else {
            return;
        };
        match accounts.duplicate_setup(name) {
            Ok(Some(copy)) => self.set_view(View::Setup(copy), window, cx),
            Ok(None) => {}
            Err(error) => self.notice = Some(format!("couldn't save the setups: {error}").into()),
        }
        cx.notify();
    }

    fn delete_setup(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(accounts) = &mut self.accounts else {
            return;
        };
        match accounts.remove_setup(name) {
            Ok(()) => {
                cx.global_mut::<Runs>().forget(name);
                if self
                    .editing
                    .as_ref()
                    .is_some_and(|editing| editing.setup == name)
                {
                    self.editing = None;
                }
                if self.view == View::Setup(name.to_owned()) {
                    self.set_view(View::All, window, cx);
                }
            }
            Err(error) => self.notice = Some(format!("couldn't save the setups: {error}").into()),
        }
        cx.notify();
    }

    fn keep_clock(&mut self, ticking: bool, cx: &mut Context<Self>) {
        if !ticking {
            self.clock = None;
        } else if self.clock.is_none() {
            self.clock = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(CLOCK).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
        }
    }
}

#[cfg(test)]
mod tests;
