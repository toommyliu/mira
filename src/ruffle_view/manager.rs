use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, FocusHandle, Global, InteractiveElement as _,
    IntoElement, ParentElement as _, PathPromptOptions, Pixels, Render, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions, actions, div, prelude::FluentBuilder as _, px, size,
};
use reqwest::header::HeaderValue;

use super::{
    Client, State, Summary,
    account::file_name,
    client::{CloseSession, TRAFFIC_LIGHTS_WIDTH, drag_area},
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, Theme, Tone,
    components::{Empty, StatusLabel},
    primitives::{
        Activity, Alert, Button, Checkbox, CheckedState, Dialog, Field, Input, InputEvent, Select,
        button::{Size, Variant},
    },
    theme::{CONTROL_MD, TEXT_BODY, TEXT_SMALL},
};

use crate::{
    accounts::{Account, AccountId, Accounts, Change, Draft, SaveError},
    net,
};

actions!(mira, [OpenAccounts]);

const MAX_FIELD: usize = 64;
const WIDE: Pixels = px(720.0);
const CHOOSE_IN_GAME: &str = "Choose in game";
const MIXED: &str = "Mixed";

struct Open(WindowHandle<Manager>);

impl Global for Open {}

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
            size(px(820.0), px(460.0)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Accounts".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(15.0))),
        }),
        app_owns_titlebar_drag: true,
        window_min_size: Some(size(px(640.0), px(360.0))),
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

pub(crate) struct Manager {
    window: WindowHandle<Client>,
    client: Entity<Client>,
    accounts: Result<Accounts, SharedString>,
    device: HeaderValue,
    servers: Servers,
    checked: HashSet<AccountId>,
    form: Option<Form>,
    bulk: Option<Bulk>,
    notice: Option<SharedString>,
    focus: FocusHandle,
    _subscriptions: [Subscription; 2],
}

enum Servers {
    Loading,
    Listed(Vec<String>),
    Unavailable,
}

impl Servers {
    fn choices(&self, saved: Option<&str>) -> Vec<String> {
        let mut names = match self {
            Servers::Listed(names) => names.clone(),
            Servers::Loading | Servers::Unavailable => Vec::new(),
        };
        if let Some(saved) = saved
            && !names.iter().any(|name| name == saved)
        {
            names.push(saved.to_owned());
        }
        names
    }

    fn note(&self) -> Option<&'static str> {
        match self {
            Servers::Loading => Some("Loading servers…"),
            Servers::Listed(_) => None,
            Servers::Unavailable => Some("Couldn't load the server list."),
        }
    }
}

struct Form {
    open: bool,
    editing: Option<AccountId>,
    name: Entity<Input>,
    username: Entity<Input>,
    password: Entity<Input>,
    server: Option<String>,
    script: Option<PathBuf>,
    error: Option<SharedString>,
    removing: bool,
    _inputs: [Subscription; 3],
}

struct Bulk {
    open: bool,
    ids: HashSet<AccountId>,
    change: Change,
    error: Option<SharedString>,
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

impl Form {
    fn draft(&self, cx: &App) -> Draft {
        let text = |input: &Entity<Input>| input.read(cx).text().to_owned();
        Draft {
            name: text(&self.name),
            username: text(&self.username),
            password: text(&self.password),
            server: self.server.clone(),
            script: self.script.clone(),
        }
    }
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

fn activity(state: State) -> Activity {
    match state {
        State::Online => Activity::Running,
        State::Busy => Activity::Waiting,
        State::Offline => Activity::Stopped,
        State::Failed => Activity::Failed,
    }
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
        let subscriptions = [
            cx.observe(&client, |_, _, cx| cx.notify()),
            cx.on_focus_lost(window, |this, window, cx| window.focus(&this.focus, cx)),
        ];
        let mut manager = Self {
            window: game,
            client,
            accounts: Accounts::open(path).map_err(Into::into),
            device,
            servers: Servers::Unavailable,
            checked: HashSet::new(),
            form: None,
            bulk: None,
            notice: None,
            focus,
            _subscriptions: subscriptions,
        };
        manager.list_servers(cx);
        manager
    }

    fn list_servers(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.servers, Servers::Unavailable) {
            return;
        }
        self.servers = Servers::Loading;
        let device = self.device.clone();
        let listed = cx.background_spawn(async move { net::servers(device) });
        cx.spawn(async move |this, cx| {
            let servers = match listed.await {
                Ok(names) => Servers::Listed(names),
                Err(error) => {
                    tracing::warn!(error = format!("{error:#}"), "couldn't list the servers");
                    Servers::Unavailable
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.servers = servers;
                cx.notify();
            });
        })
        .detach();
    }

    fn edit_selected(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        self.bulk = Some(Bulk {
            open: true,
            ids: targets.iter().map(Account::id).collect(),
            change: Change {
                server: shared(&targets, |account| &account.server),
                script: shared(&targets, |account| &account.script),
            },
            error: None,
        });
        self.list_servers(cx);
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

    fn checked_accounts(&self) -> Vec<Account> {
        let Ok(accounts) = &self.accounts else {
            return Vec::new();
        };
        accounts
            .list()
            .iter()
            .filter(|account| self.checked.contains(&account.id()))
            .cloned()
            .collect()
    }

    fn act(
        &mut self,
        targets: Vec<Account>,
        cx: &mut Context<Self>,
        act: impl Fn(&mut Client, &Account, &mut Window, &mut Context<Client>) -> anyhow::Result<()>,
    ) {
        let failures = self.window.update(cx, |client, window, cx| {
            targets
                .iter()
                .filter_map(|account| act(client, account, window, cx).err())
                .map(|error| format!("{error:#}"))
                .collect::<Vec<_>>()
        });
        self.notice = match failures {
            Ok(failures) if failures.is_empty() => None,
            Ok(failures) => Some(failures.join("\n").into()),
            Err(_) => Some("The game window is closed.".into()),
        };
        cx.notify();
    }

    fn launch(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        self.act(targets, cx, |client, account, window, cx| {
            client.launch(account, window, cx)
        });
    }

    fn start(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        let client = self.client.read(cx);
        let targets = targets
            .into_iter()
            .filter(|account| {
                account.script.is_some()
                    && !client
                        .summary(&account.id(), cx)
                        .is_some_and(|summary| summary.running)
            })
            .collect();
        self.act(targets, cx, |client, account, window, cx| {
            match &account.script {
                Some(script) => client.run_script(account, script.clone(), false, window, cx),
                None => Ok(()),
            }
        });
    }

    fn stop(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        self.act(targets, cx, |client, account, _, cx| {
            client.stop_script(&account.id(), cx);
            Ok(())
        });
    }

    fn close(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        self.act(targets, cx, |client, account, window, cx| {
            client.close_account(&account.id(), window, cx);
            Ok(())
        });
    }

    fn show(&mut self, account: Account, cx: &mut Context<Self>) {
        self.act(vec![account], cx, |client, account, window, cx| {
            client.show_account(&account.id(), window, cx);
            Ok(())
        });
    }

    fn run_chosen(&mut self, targets: Vec<Account>, cx: &mut Context<Self>) {
        let chosen = choose_script(cx);
        cx.spawn(async move |this, cx| {
            let Some(script) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.act(targets, cx, |client, account, window, cx| {
                    client.run_script(account, script.clone(), true, window, cx)
                })
            });
        })
        .detach();
    }

    fn set_checked(&mut self, id: AccountId, checked: bool, cx: &mut Context<Self>) {
        if checked {
            self.checked.insert(id);
        } else {
            self.checked.remove(&id);
        }
        cx.notify();
    }

    fn set_all_checked(&mut self, checked: bool, cx: &mut Context<Self>) {
        let Ok(accounts) = &self.accounts else {
            return;
        };
        self.checked = if checked {
            accounts.list().iter().map(Account::id).collect()
        } else {
            HashSet::new()
        };
        cx.notify();
    }

    fn edit(&mut self, account: Option<&Account>, cx: &mut Context<Self>) {
        let draft = account.map(Draft::of).unwrap_or_default();
        let name = input(
            draft.name,
            |cx| Input::new("Name", cx).placeholder("Same as username"),
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
            script: draft.script,
            error: None,
            removing: false,
            _inputs: inputs,
        });
        self.list_servers(cx);
        cx.notify();
    }

    fn input_event(&mut self, _: Entity<Input>, event: &InputEvent, cx: &mut Context<Self>) {
        match event {
            InputEvent::Submitted => self.save(cx),
            InputEvent::Changed => {
                if let Some(form) = &mut self.form
                    && form.error.take().is_some()
                {
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

    fn set_removing(&mut self, removing: bool, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.form {
            form.removing = removing;
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
        if let Some(old) = renamed
            && self.client.read(cx).summary(old, cx).is_some()
        {
            form.error = Some("Close this account's tab before changing its username.".into());
            cx.notify();
            return;
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
                form.error = Some(match error {
                    SaveError::Invalid(invalid) => invalid.to_string().into(),
                    SaveError::Io(_) => error.to_string().into(),
                });
            }
        }
        cx.notify();
    }

    fn remove(&mut self, cx: &mut Context<Self>) {
        let (Some(form), Ok(accounts)) = (&mut self.form, &mut self.accounts) else {
            return;
        };
        let Some(id) = form.editing.clone() else {
            return;
        };
        match accounts.remove(&id) {
            Ok(()) => {
                self.checked.remove(&id);
                form.open = false;
            }
            Err(error) => {
                form.removing = false;
                form.error = Some(format!("couldn't save the accounts: {error}").into());
            }
        }
        cx.notify();
    }

    fn choose_script_for(&mut self, set: fn(&mut Self, Option<PathBuf>), cx: &mut Context<Self>) {
        let chosen = choose_script(cx);
        cx.spawn(async move |this, cx| {
            let Some(script) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                set(this, Some(script));
                cx.notify();
            });
        })
        .detach();
    }
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

impl Manager {
    fn toolbar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selected = self.checked_accounts();
        let any = !selected.is_empty();
        let total = self
            .accounts
            .as_ref()
            .map_or(0, |accounts| accounts.list().len());
        let all = match selected.len() {
            0 => CheckedState::Unchecked,
            count if count == total => CheckedState::Checked,
            _ => CheckedState::Indeterminate,
        };
        let count: SharedString = match selected.len() {
            0 => "Select accounts to act on them".into(),
            1 => "1 selected".into(),
            count => format!("{count} selected").into(),
        };
        let bulk = |id: &'static str,
                    label: &'static str,
                    run: fn(&mut Manager, Vec<Account>, &mut Context<Manager>),
                    cx: &mut Context<Self>| {
            let targets = selected.clone();
            Button::new(id, label)
                .disabled(!any)
                .on_click(cx.listener(move |this, _, _, cx| run(this, targets.clone(), cx)))
        };
        let check_all =
            cx.listener(|this, checked: &bool, _, cx| this.set_all_checked(*checked, cx));
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.0))
            .h(px(44.0))
            .pr(px(12.0))
            .border_b_1()
            .border_color(theme.hairline)
            .child(drag_area().flex_none().w(TRAFFIC_LIGHTS_WIDTH))
            .when(total > 0, |toolbar| {
                toolbar.child(
                    div().mr(px(4.0)).child(
                        Checkbox::new("check-all", "Select all", all)
                            .on_toggle(move |checked, window, cx| check_all(&checked, window, cx)),
                    ),
                )
            })
            .child(bulk("launch-selected", "Launch", Manager::launch, cx))
            .child(bulk("start-selected", "Start", Manager::start, cx))
            .child(bulk("stop-selected", "Stop", Manager::stop, cx))
            .child(bulk("run-selected", "Run script…", Manager::run_chosen, cx))
            .child(bulk("close-selected", "Close", Manager::close, cx))
            .child(bulk("edit-selected", "Edit…", Manager::edit_selected, cx))
            .child(
                drag_area()
                    .flex()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .px(px(6.0))
                    .truncate()
                    .text_size(TEXT_SMALL)
                    .text_color(theme.text_secondary)
                    .child(count),
            )
            .child(
                Button::new("add-account", "Add account")
                    .disabled(self.accounts.is_err())
                    .on_click(cx.listener(|this, _, _, cx| this.edit(None, cx))),
            )
    }

    fn cards(
        &self,
        theme: &Theme,
        accounts: &[Account],
        columns: u16,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let summaries: Vec<Option<Summary>> = {
            let client = self.client.read(cx);
            accounts
                .iter()
                .map(|account| client.summary(&account.id(), cx))
                .collect()
        };
        let cards: Vec<_> = accounts
            .iter()
            .zip(summaries)
            .map(|(account, summary)| self.card(theme, account, summary, cx))
            .collect();
        div()
            .id("accounts")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .grid()
                    .grid_cols(columns)
                    .gap(px(8.0))
                    .p(px(12.0))
                    .children(cards),
            )
    }

    fn card(
        &self,
        theme: &Theme,
        account: &Account,
        summary: Option<Summary>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = account.id();
        let checked = self.checked.contains(&id);
        let open = summary.is_some();
        let status_id = SharedString::from(format!("status-{}", account.username));
        let status = match &summary {
            Some(summary) => {
                StatusLabel::new(status_id, activity(summary.state), summary.login.clone())
            }
            None => StatusLabel::new(status_id, Activity::Idle, "Not open"),
        };
        let (script, script_color): (SharedString, _) = match &summary {
            Some(Summary {
                script: Some(live),
                script_failed,
                running,
                ..
            }) => (
                live.clone(),
                match (script_failed, running) {
                    (true, _) => theme.danger_text,
                    (false, true) => theme.text,
                    (false, false) => theme.text_secondary,
                },
            ),
            _ => match &account.script {
                Some(script) => (file_name(script).into(), theme.text_secondary),
                None => ("No script".into(), theme.text_tertiary),
            },
        };
        let toggle = cx.listener(move |this, checked: &bool, _, cx| {
            this.set_checked(id.clone(), *checked, cx)
        });
        let launch_target = account.clone();
        let edit_target = account.clone();
        div()
            .id(SharedString::from(format!("account-{}", account.username)))
            .flex()
            .flex_col()
            .gap(px(6.0))
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .min_w_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(
                                Checkbox::new(
                                    SharedString::from(format!("check-{}", account.username)),
                                    account.name.clone(),
                                    if checked {
                                        CheckedState::Checked
                                    } else {
                                        CheckedState::Unchecked
                                    },
                                )
                                .on_toggle(move |checked, window, cx| toggle(&checked, window, cx)),
                            ),
                    )
                    .child(
                        Button::new(
                            SharedString::from(format!("edit-{}", account.username)),
                            "Edit",
                        )
                        .variant(Variant::Ghost)
                        .size(Size::Small)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.edit(Some(&edit_target), cx)),
                        ),
                    )
                    .child(
                        Button::new(
                            SharedString::from(format!("open-{}", account.username)),
                            if open { "Show" } else { "Launch" },
                        )
                        .size(Size::Small)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if open {
                                this.show(launch_target.clone(), cx)
                            } else {
                                this.launch(vec![launch_target.clone()], cx)
                            }
                        })),
                    ),
            )
            .child(status)
            .child(
                div()
                    .min_w_0()
                    .pl(px(14.0))
                    .line_clamp(2)
                    .text_size(TEXT_SMALL)
                    .line_height(px(16.0))
                    .text_color(script_color)
                    .child(script),
            )
    }

    fn empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div().flex().flex_1().items_center().justify_center().child(
            Empty::new(
                "No accounts yet",
                "Add an account to launch it and run its script from here.",
            )
            .action(
                Button::new("add-first-account", "Add account")
                    .variant(Variant::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.edit(None, cx))),
            ),
        )
    }

    fn form(&self, theme: &Theme, form: &Form, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let body = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap(px(12.0))
                    .child(Field::new("Name", form.name.clone()))
                    .child(Field::new("Username", form.username.clone()))
                    .child(Field::new("Password", form.password.clone()))
                    .child(self.server_field(
                        "server",
                        Some(form.server.as_deref()),
                        set_form_server,
                        cx,
                    )),
            )
            .child(script_field(
                theme,
                "script",
                Some(form.script.as_deref()),
                set_form_script,
                cx,
            ))
            .children(form.error.clone().map(|error| {
                div()
                    .text_size(TEXT_SMALL)
                    .text_color(theme.danger_text)
                    .child(error)
            }));
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
        .body(body)
        .when(form.editing.is_some(), |dialog| {
            dialog.footer_start_slot(
                Button::new("remove", "Remove")
                    .variant(Variant::Danger)
                    .on_click(cx.listener(|this, _, _, cx| this.set_removing(true, cx))),
            )
        })
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
        let mixed = bulk.change.server.is_none() || bulk.change.script.is_none();
        let body = div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap(px(12.0))
                    .child(self.server_field(
                        "bulk-server",
                        bulk.change.server.as_ref().map(Option::as_deref),
                        |this, server| {
                            if let Some(bulk) = &mut this.bulk {
                                bulk.change.server = Some(server);
                            }
                        },
                        cx,
                    )),
            )
            .child(script_field(
                theme,
                "bulk-script",
                bulk.change.script.as_ref().map(Option::as_deref),
                |this, script| {
                    if let Some(bulk) = &mut this.bulk {
                        bulk.change.script = Some(script);
                    }
                },
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
            .when(mixed, |dialog| {
                dialog.description("Mixed fields keep each account's own value.")
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

    fn server_field(
        &self,
        id: &'static str,
        current: Option<Option<&str>>,
        set: fn(&mut Self, Option<String>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let names = self.servers.choices(current.flatten());
        let selected = current.map(|server| {
            server
                .and_then(|server| names.iter().position(|name| name == server))
                .map_or(0, |index| index + 1)
        });
        let options: Vec<SharedString> = std::iter::once(SharedString::from(CHOOSE_IN_GAME))
            .chain(names.iter().cloned().map(SharedString::from))
            .collect();
        let choose = cx.listener(move |this, index: &usize, _, cx| {
            set(
                this,
                index
                    .checked_sub(1)
                    .and_then(|index| names.get(index).cloned()),
            );
            cx.notify();
        });
        let field = Field::new(
            "Server",
            Select::new(id, "Server", options, selected)
                .placeholder(MIXED)
                .width(px(184.0))
                .align_item_with_trigger()
                .on_select(move |index, window, cx| choose(&index, window, cx)),
        );
        match self.servers.note() {
            Some(note) => field.description(note),
            None => field,
        }
    }

    fn remove_confirmation(&self, form: &Form, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let keep = cx.listener(|this, _: &(), _, cx| this.set_removing(false, cx));
        Dialog::new("remove-account", form.removing, "Remove this account?")
            .alert()
            .description("An open tab stays open.")
            .action(
                Button::new("keep", "Keep")
                    .on_click(cx.listener(|this, _, _, cx| this.set_removing(false, cx))),
            )
            .action(
                Button::new("confirm-remove", "Remove")
                    .variant(Variant::Danger)
                    .on_click(cx.listener(|this, _, _, cx| this.remove(cx))),
            )
            .on_close(move |window, cx| keep(&(), window, cx))
    }
}

fn set_form_server(manager: &mut Manager, server: Option<String>) {
    if let Some(form) = &mut manager.form {
        form.server = server;
    }
}

fn set_form_script(manager: &mut Manager, script: Option<PathBuf>) {
    if let Some(form) = &mut manager.form {
        form.script = script;
    }
}

fn script_field(
    theme: &Theme,
    id: &'static str,
    current: Option<Option<&Path>>,
    set: fn(&mut Manager, Option<PathBuf>),
    cx: &mut Context<Manager>,
) -> impl IntoElement + use<> {
    let (label, color): (SharedString, _) = match current {
        None => (MIXED.into(), theme.text_tertiary),
        Some(None) => ("None".into(), theme.text_tertiary),
        Some(Some(script)) => (file_name(script).into(), theme.text),
    };
    let row = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(CONTROL_MD)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(color)
                .child(label),
        )
        .when(current != Some(None), |row| {
            row.child(
                Button::new(SharedString::from(format!("{id}-clear")), "Clear").on_click(
                    cx.listener(move |this, _, _, cx| {
                        set(this, None);
                        cx.notify();
                    }),
                ),
            )
        })
        .child(
            Button::new(SharedString::from(format!("{id}-choose")), "Choose…")
                .on_click(cx.listener(move |this, _, _, cx| this.choose_script_for(set, cx))),
        );
    Field::new("Script", row)
}

impl Render for Manager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let columns = if window.viewport_size().width >= WIDE {
            3
        } else {
            2
        };
        let body = match &self.accounts {
            Err(error) => div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .px(px(24.0))
                .text_color(theme.danger_text)
                .child(format!("Mira couldn't load your accounts: {error}"))
                .into_any_element(),
            Ok(accounts) if accounts.list().is_empty() => self.empty(cx).into_any_element(),
            Ok(accounts) => {
                let list = accounts.list().to_vec();
                self.cards(&theme, &list, columns, cx).into_any_element()
            }
        };
        let form = self.form.as_ref().map(|form| self.form(&theme, form, cx));
        let bulk = self
            .bulk
            .as_ref()
            .map(|bulk| self.bulk_form(&theme, bulk, cx));
        let remove_confirmation = self
            .form
            .as_ref()
            .filter(|form| form.open)
            .map(|form| self.remove_confirmation(form, cx));
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.base)
            .text_size(TEXT_BODY)
            .text_color(theme.text)
            .key_context(FOCUS_SCOPE)
            .id("accounts")
            .role(Role::Group)
            .track_focus(&self.focus)
            .on_action(|_: &CloseSession, window, _| window.remove_window())
            .child(mira_ui::nav::pointer_probe())
            .child(self.toolbar(&theme, cx))
            .children(self.notice.clone().map(|notice| {
                div()
                    .flex_none()
                    .px(px(12.0))
                    .pt(px(12.0))
                    .child(Alert::new("notice", Tone::Danger, notice))
            }))
            .child(body)
            .children(form)
            .children(remove_confirmation)
            .children(bulk)
    }
}
