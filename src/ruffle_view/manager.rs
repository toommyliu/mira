use std::{collections::HashSet, path::PathBuf};

use gpui::{
    App, AppContext as _, Bounds, Context, Div, Entity, FocusHandle, Global,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, PathPromptOptions,
    Pixels, Render, Role, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    TitlebarOptions, Toggled, Window, WindowBounds, WindowHandle, WindowOptions, actions, div,
    prelude::FluentBuilder as _, px, size,
};

use super::{
    Client, State, Summary,
    account::file_name,
    client::{CloseSession, TRAFFIC_LIGHTS_WIDTH, drag_area},
    ui::{self, Glyph},
};
use crate::accounts::{Account, AccountId, Accounts, Draft, SaveError};

actions!(mira, [OpenAccounts]);

const MAX_FIELD: usize = 64;
const WIDE: Pixels = px(720.0);

struct Open(WindowHandle<Manager>);

impl Global for Open {}

pub(crate) fn open(
    game: WindowHandle<Client>,
    client: Entity<Client>,
    path: PathBuf,
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
        window_min_size: Some(size(px(640.0), px(280.0))),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let manager = cx.new(|cx| Manager::new(game, client, path, cx));
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
    checked: HashSet<AccountId>,
    form: Option<Form>,
    notice: Option<SharedString>,
    focus: FocusHandle,
    _client: gpui::Subscription,
}

struct Form {
    editing: Option<AccountId>,
    draft: Draft,
    field: Option<Field>,
    fresh: bool,
    error: Option<SharedString>,
    removing: bool,
    focus: FocusHandle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Username,
    Password,
    Server,
}

impl Field {
    const ALL: [Field; 4] = [Field::Name, Field::Username, Field::Password, Field::Server];

    fn label(self) -> &'static str {
        match self {
            Field::Name => "Name",
            Field::Username => "Username",
            Field::Password => "Password",
            Field::Server => "Server",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Field::Name => "Same as username",
            Field::Server => "Choose in game",
            Field::Username | Field::Password => "",
        }
    }

    fn text(self, draft: &mut Draft) -> &mut String {
        match self {
            Field::Name => &mut draft.name,
            Field::Username => &mut draft.username,
            Field::Password => &mut draft.password,
            Field::Server => &mut draft.server,
        }
    }

    fn step(self, backwards: bool) -> Field {
        let index = Field::ALL.iter().position(|field| *field == self).unwrap();
        let count = Field::ALL.len();
        Field::ALL[if backwards {
            (index + count - 1) % count
        } else {
            (index + 1) % count
        }]
    }
}

impl Manager {
    fn new(
        window: WindowHandle<Client>,
        client: Entity<Client>,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let observed = cx.observe(&client, |_, _, cx| cx.notify());
        Self {
            window,
            client,
            accounts: Accounts::open(path).map_err(Into::into),
            checked: HashSet::new(),
            form: None,
            notice: None,
            focus,
            _client: observed,
        }
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

    fn toggle(&mut self, id: AccountId, cx: &mut Context<Self>) {
        if !self.checked.remove(&id) {
            self.checked.insert(id);
        }
        cx.notify();
    }

    fn toggle_all(&mut self, cx: &mut Context<Self>) {
        let Ok(accounts) = &self.accounts else {
            return;
        };
        if self.checked.len() == accounts.list().len() {
            self.checked.clear();
        } else {
            self.checked = accounts.list().iter().map(Account::id).collect();
        }
        cx.notify();
    }

    fn edit(&mut self, account: Option<&Account>, window: &mut Window, cx: &mut Context<Self>) {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.form = Some(Form {
            editing: account.map(Account::id),
            draft: account.map(Draft::of).unwrap_or_default(),
            field: Some(if account.is_some() {
                Field::Name
            } else {
                Field::Username
            }),
            fresh: true,
            error: None,
            removing: false,
            focus,
        });
        cx.notify();
    }

    fn close_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(form), Ok(accounts)) = (&mut self.form, &mut self.accounts) else {
            return;
        };
        let renamed = form
            .editing
            .as_ref()
            .filter(|editing| **editing != AccountId::of(form.draft.username.trim()));
        if let Some(old) = renamed
            && self.client.read(cx).summary(old, cx).is_some()
        {
            form.error = Some("Close this account's tab before changing its username.".into());
            cx.notify();
            return;
        }
        match accounts.save(form.editing.as_ref(), form.draft.clone()) {
            Ok(id) => {
                if let Some(old) = form.editing.take()
                    && self.checked.remove(&old)
                {
                    self.checked.insert(id);
                }
                self.close_form(window, cx);
            }
            Err(error) => {
                form.error = Some(match error {
                    SaveError::Invalid(invalid) => invalid.to_string().into(),
                    SaveError::Io(_) => error.to_string().into(),
                });
                cx.notify();
            }
        }
    }

    fn remove(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(form), Ok(accounts)) = (&mut self.form, &mut self.accounts) else {
            return;
        };
        let Some(id) = form.editing.clone() else {
            return;
        };
        match accounts.remove(&id) {
            Ok(()) => {
                self.checked.remove(&id);
                self.close_form(window, cx);
            }
            Err(error) => {
                form.error = Some(format!("couldn't save the accounts: {error}").into());
                cx.notify();
            }
        }
    }

    fn choose_form_script(&mut self, cx: &mut Context<Self>) {
        let chosen = choose_script(cx);
        cx.spawn(async move |this, cx| {
            let Some(script) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(form) = &mut this.form {
                    form.draft.script = Some(script);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn form_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.form else {
            return;
        };
        let keystroke = &event.keystroke;
        let command = keystroke.modifiers.platform;
        match keystroke.key.as_str() {
            "escape" => return self.close_form(window, cx),
            "enter" => return self.save(window, cx),
            "tab" => {
                form.field = Some(
                    form.field
                        .map_or(Field::Name, |field| field.step(keystroke.modifiers.shift)),
                );
                form.fresh = true;
            }
            "a" if command => form.fresh = true,
            "v" if command => {
                let pasted = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .unwrap_or_default();
                let pasted: String = pasted.chars().filter(|c| !c.is_control()).collect();
                form.type_text(&pasted);
            }
            "backspace" => {
                let Some(field) = form.field else {
                    return;
                };
                let text = field.text(&mut form.draft);
                if form.fresh || command {
                    text.clear();
                } else {
                    text.pop();
                }
                form.fresh = false;
            }
            _ => {
                let Some(typed) = keystroke.key_char.as_deref() else {
                    return;
                };
                if command || keystroke.modifiers.control {
                    return;
                }
                form.type_text(typed);
            }
        }
        form.error = None;
        cx.stop_propagation();
        cx.notify();
    }
}

impl Form {
    fn type_text(&mut self, typed: &str) {
        let Some(field) = self.field else {
            return;
        };
        let text = field.text(&mut self.draft);
        if self.fresh {
            text.clear();
            self.fresh = false;
        }
        let room = MAX_FIELD.saturating_sub(text.chars().count());
        text.extend(typed.chars().take(room));
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

fn danger() -> gpui::Hsla {
    gpui::hsla(0.0, 0.75, 0.66, 1.0)
}

fn button(id: impl Into<gpui::ElementId>, label: &'static str, enabled: bool) -> Stateful<Div> {
    let button = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .h(px(26.0))
        .px(px(10.0))
        .rounded(ui::CONTROL_RADIUS)
        .border_1()
        .border_color(ui::hairline())
        .role(Role::Button)
        .aria_label(label)
        .child(label);
    if enabled {
        ui::pressable(button)
    } else {
        button.text_color(ui::tertiary_text())
    }
}

fn checkbox(
    id: impl Into<gpui::ElementId>,
    label: SharedString,
    on: Option<bool>,
) -> Stateful<Div> {
    let filled = on != Some(false);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(16.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(if filled {
            ui::accent()
        } else {
            ui::secondary_text()
        })
        .when(filled, |box_| box_.bg(ui::accent()))
        .cursor_pointer()
        .role(Role::CheckBox)
        .aria_label(label)
        .aria_toggled(match on {
            Some(true) => Toggled::True,
            Some(false) => Toggled::False,
            None => Toggled::Mixed,
        })
        .map(|box_| match on {
            Some(true) => box_.child(ui::glyph(Glyph::Check, ui::text())),
            None => box_.child(div().w(px(8.0)).h(px(1.5)).bg(ui::text())),
            Some(false) => box_,
        })
}

fn small(button: Stateful<Div>) -> Stateful<Div> {
    button
        .h(px(22.0))
        .px(px(8.0))
        .text_size(ui::SMALL_TEXT_SIZE)
}

fn state_color(state: State) -> gpui::Hsla {
    match state {
        State::Online => gpui::hsla(142.0 / 360.0, 0.6, 0.52, 1.0),
        State::Busy => ui::tertiary_text(),
        State::Offline => gpui::hsla(38.0 / 360.0, 0.9, 0.58, 1.0),
        State::Failed => danger(),
    }
}

impl Manager {
    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selected = self.checked_accounts();
        let any = !selected.is_empty();
        let total = self
            .accounts
            .as_ref()
            .map_or(0, |accounts| accounts.list().len());
        let all = match selected.len() {
            0 => Some(false),
            count if count == total => Some(true),
            _ => None,
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
            button(id, label, any).when(any, |button| {
                button.on_click(cx.listener(move |this, _, _, cx| run(this, targets.clone(), cx)))
            })
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.0))
            .h(px(44.0))
            .pr(px(12.0))
            .border_b_1()
            .border_color(ui::hairline())
            .child(drag_area().flex_none().w(TRAFFIC_LIGHTS_WIDTH))
            .when(total > 0, |toolbar| {
                toolbar.child(
                    checkbox("check-all", "Select all accounts".into(), all)
                        .mr(px(4.0))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_all(cx))),
                )
            })
            .child(bulk("launch-selected", "Launch", Manager::launch, cx))
            .child(bulk("start-selected", "Start", Manager::start, cx))
            .child(bulk("stop-selected", "Stop", Manager::stop, cx))
            .child(bulk("run-selected", "Run script…", Manager::run_chosen, cx))
            .child(bulk("close-selected", "Close", Manager::close, cx))
            .child(
                drag_area()
                    .flex()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .px(px(6.0))
                    .truncate()
                    .text_size(ui::SMALL_TEXT_SIZE)
                    .text_color(ui::secondary_text())
                    .child(count),
            )
            .child(
                button("add-account", "Add account", self.accounts.is_ok()).when(
                    self.accounts.is_ok(),
                    |button| {
                        button.on_click(
                            cx.listener(|this, _, window, cx| this.edit(None, window, cx)),
                        )
                    },
                ),
            )
    }

    fn cards(
        &self,
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
            .map(|(account, summary)| self.card(account, summary, cx))
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
        account: &Account,
        summary: Option<Summary>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = account.id();
        let checked = self.checked.contains(&id);
        let open = summary.is_some();
        let (status, status_color, dot) = match &summary {
            Some(summary) => (
                summary.login.clone(),
                if summary.state == State::Failed {
                    danger()
                } else {
                    ui::text()
                },
                Some(state_color(summary.state)),
            ),
            None => ("Not open".into(), ui::tertiary_text(), None),
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
                    (true, _) => danger(),
                    (false, true) => ui::text(),
                    (false, false) => ui::secondary_text(),
                },
            ),
            _ => match &account.script {
                Some(script) => (file_name(script).into(), ui::secondary_text()),
                None => ("No script".into(), ui::tertiary_text()),
            },
        };
        let name = account.name.clone();
        let toggle_id = id.clone();
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
                    card.border_color(ui::accent())
                        .bg(ui::accent().opacity(0.08))
                } else {
                    card.border_color(ui::hairline())
                        .bg(gpui::hsla(0.0, 0.0, 1.0, 0.035))
                }
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .min_w_0()
                    .child(
                        checkbox(
                            SharedString::from(format!("check-{}", account.username)),
                            format!("Select {name}").into(),
                            Some(checked),
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.toggle(toggle_id.clone(), cx)),
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(name),
                    )
                    .child(
                        small(button(
                            SharedString::from(format!("edit-{}", account.username)),
                            "Edit",
                            true,
                        ))
                        .border_color(gpui::transparent_black())
                        .text_color(ui::secondary_text())
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.edit(Some(&edit_target), window, cx),
                        )),
                    )
                    .child(
                        small(button(
                            SharedString::from(format!("open-{}", account.username)),
                            if open { "Show" } else { "Launch" },
                            true,
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if open {
                                this.show(launch_target.clone(), cx)
                            } else {
                                this.launch(vec![launch_target.clone()], cx)
                            }
                        })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(7.0))
                    .min_w_0()
                    .text_size(ui::SMALL_TEXT_SIZE)
                    .line_height(px(16.0))
                    .child(
                        div()
                            .flex_none()
                            .mt(px(4.5))
                            .size(px(7.0))
                            .rounded_full()
                            .map(|dot_| match dot {
                                Some(color) => dot_.bg(color),
                                None => dot_.border_1().border_color(ui::tertiary_text()),
                            }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .line_clamp(2)
                            .text_color(status_color)
                            .child(status),
                    ),
            )
            .child(
                div()
                    .min_w_0()
                    .pl(px(14.0))
                    .line_clamp(2)
                    .text_size(ui::SMALL_TEXT_SIZE)
                    .line_height(px(16.0))
                    .text_color(script_color)
                    .child(script),
            )
    }

    fn empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .text_color(ui::secondary_text())
            .child("No accounts yet.")
            .child(
                button("add-first-account", "Add account", true)
                    .mt(px(8.0))
                    .text_color(ui::text())
                    .on_click(cx.listener(|this, _, window, cx| this.edit(None, window, cx))),
            )
    }

    fn form(&self, form: &Form, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let fields: Vec<_> = Field::ALL
            .iter()
            .map(|&field| {
                let editing = form.field == Some(field);
                let value = match field {
                    Field::Name => form.draft.name.clone(),
                    Field::Username => form.draft.username.clone(),
                    Field::Password => "•".repeat(form.draft.password.chars().count()),
                    Field::Server => form.draft.server.clone(),
                };
                let input = ui::text_field(
                    SharedString::from(format!("field-{}", field.label())),
                    value,
                    field.placeholder(),
                    editing,
                    editing && form.fresh,
                )
                .w(px(260.0))
                .role(if field == Field::Password {
                    Role::PasswordInput
                } else {
                    Role::TextInput
                })
                .aria_label(field.label())
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(form) = &mut this.form {
                        form.field = Some(field);
                        form.fresh = true;
                        window.focus(&form.focus, cx);
                        cx.notify();
                    }
                }));
                form_row(field.label(), input)
            })
            .collect();
        let script: SharedString = form
            .draft
            .script
            .as_deref()
            .map_or_else(|| "None".into(), |script| file_name(script).into());
        let has_script = form.draft.script.is_some();
        let script_row = form_row(
            "Script",
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .w(px(260.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(if has_script {
                            ui::text()
                        } else {
                            ui::tertiary_text()
                        })
                        .child(script),
                )
                .when(has_script, |row| {
                    row.child(button("clear-script", "Clear", true).on_click(cx.listener(
                        |this, _, _, cx| {
                            if let Some(form) = &mut this.form {
                                form.draft.script = None;
                                cx.notify();
                            }
                        },
                    )))
                })
                .child(
                    button("choose-script", "Choose…", true)
                        .on_click(cx.listener(|this, _, _, cx| this.choose_form_script(cx))),
                ),
        );
        let title = if form.editing.is_some() {
            "Edit account"
        } else {
            "Add account"
        };
        let footer = if form.removing {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .flex_1()
                        .text_size(ui::SMALL_TEXT_SIZE)
                        .text_color(ui::secondary_text())
                        .child("Remove this account? An open tab stays open."),
                )
                .child(
                    button("keep", "Keep", true).on_click(cx.listener(|this, _, _, cx| {
                        if let Some(form) = &mut this.form {
                            form.removing = false;
                            cx.notify();
                        }
                    })),
                )
                .child(
                    button("confirm-remove", "Remove", true)
                        .text_color(danger())
                        .on_click(cx.listener(|this, _, window, cx| this.remove(window, cx))),
                )
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .when(form.editing.is_some(), |footer| {
                    footer.child(
                        button("remove", "Remove", true)
                            .text_color(danger())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(form) = &mut this.form {
                                    form.removing = true;
                                    cx.notify();
                                }
                            })),
                    )
                })
                .child(div().flex_1())
                .child(
                    button("cancel", "Cancel", true)
                        .on_click(cx.listener(|this, _, window, cx| this.close_form(window, cx))),
                )
                .child(
                    button("save", "Save", true)
                        .bg(ui::accent())
                        .border_color(ui::accent())
                        .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                )
        };
        div()
            .id("form-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::hsla(0.0, 0.0, 0.0, 0.45))
            .child(
                ui::panel("account-form", px(400.0))
                    .p(px(16.0))
                    .gap(px(10.0))
                    .role(Role::Dialog)
                    .aria_label(title)
                    .track_focus(&form.focus)
                    .on_key_down(cx.listener(Self::form_key_down))
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .children(fields)
                    .child(script_row)
                    .children(form.error.clone().map(|error| {
                        div()
                            .text_size(ui::SMALL_TEXT_SIZE)
                            .text_color(danger())
                            .child(error)
                    }))
                    .child(footer.mt(px(6.0))),
            )
    }
}

fn form_row(label: &'static str, input: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(div().text_color(ui::secondary_text()).child(label))
        .child(input)
}

impl Render for Manager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                .text_color(danger())
                .child(format!("Mira couldn't load your accounts: {error}"))
                .into_any_element(),
            Ok(accounts) if accounts.list().is_empty() => self.empty(cx).into_any_element(),
            Ok(accounts) => {
                let list = accounts.list().to_vec();
                self.cards(&list, columns, cx).into_any_element()
            }
        };
        let form = self.form.as_ref().map(|form| self.form(form, cx));
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(ui::bar_background())
            .text_size(ui::TEXT_SIZE)
            .text_color(ui::text())
            .track_focus(&self.focus)
            .on_action(|_: &CloseSession, window, _| window.remove_window())
            .child(self.toolbar(cx))
            .children(self.notice.clone().map(|notice| {
                div()
                    .flex_none()
                    .px(px(12.0))
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(ui::hairline())
                    .text_size(ui::SMALL_TEXT_SIZE)
                    .text_color(danger())
                    .child(notice)
            }))
            .child(body)
            .children(form)
    }
}
