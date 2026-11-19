use std::{collections::BTreeSet, rc::Rc};

use gpui::{
    Context, Entity, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window,
    div, prelude::FluentBuilder as _, px,
};
use mira_ui::{
    primitives::{
        Checkbox, CheckedState, Column, Dialog, DropdownMenu, Field, Input, MenuItem, Select,
        Table, Toast, Toaster, button::Variant,
    },
    v_flex,
};

use super::super::{Screen, view::*};

struct Account {
    name: SharedString,
    server: &'static str,
    class: &'static str,
    setup: &'static str,
    online: bool,
    script: &'static str,
}

pub(in crate::storyboard) struct Accounts {
    sessions: Entity<Sessions>,
    accounts: Vec<Account>,
    scope: usize,
    selected: BTreeSet<usize>,
    search: Entity<Input>,
    name: Entity<Input>,
    add_open: bool,
    invalid: bool,
}

impl Accounts {
    pub(in crate::storyboard) fn new(sessions: Entity<Sessions>, cx: &mut Context<Self>) -> Self {
        let search = field("Search accounts", "Search accounts", "", true, cx);
        let name = field("Account name", "Name", "", false, cx);
        observe_input(&search, cx);
        observe_input(&name, cx);
        let accounts = [
            (
                "Aster",
                "Artix",
                "ArchPaladin",
                "Quest team",
                true,
                "Relic circuit",
            ),
            (
                "Vesper",
                "Artix",
                "LightCaster",
                "Quest team",
                true,
                "Daily route",
            ),
            (
                "Sable",
                "Yorumi",
                "Legion Revenant",
                "Farm team",
                true,
                "Essence farm",
            ),
            ("Cinder", "Yorumi", "Blaze Binder", "Farm team", false, ""),
            ("Bramble", "Alina", "StoneCrusher", "Quest team", false, ""),
            ("Rook", "Artix", "Void Highlord", "Farm team", false, ""),
            ("Fern", "Alina", "Arcana Invoker", "Quest team", false, ""),
            (
                "Wren",
                "Yorumi",
                "Eternal Inversionist",
                "Farm team",
                false,
                "",
            ),
        ]
        .into_iter()
        .map(|(name, server, class, setup, online, script)| Account {
            name: name.into(),
            server,
            class,
            setup,
            online,
            script,
        })
        .collect();
        Self {
            sessions,
            accounts,
            scope: 0,
            selected: BTreeSet::new(),
            search,
            name,
            add_open: false,
            invalid: false,
        }
    }

    fn launch(&mut self, selected: &[usize], cx: &mut Context<Self>) {
        let mut names = Vec::new();
        for &index in selected {
            let account = &mut self.accounts[index];
            account.online = true;
            names.push(account.name.clone());
        }
        self.sessions.update(cx, |sessions, cx| {
            for name in names {
                sessions.open(name);
            }
            cx.notify();
        });
        self.selected.clear();
        Toaster::push(
            Toast::new(format!(
                "Launched {} {}",
                selected.len(),
                if selected.len() == 1 {
                    "account"
                } else {
                    "accounts"
                }
            )),
            cx,
        );
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).text().trim();
        if name.is_empty()
            || self
                .accounts
                .iter()
                .any(|account| account.name.as_ref().eq_ignore_ascii_case(name))
        {
            self.invalid = true;
            self.name
                .update(cx, |input, cx| input.set_invalid(true, cx));
            return;
        }
        self.accounts.push(Account {
            name: name.to_owned().into(),
            server: "Artix",
            class: "Unassigned",
            setup: "",
            online: false,
            script: "",
        });
        self.name.update(cx, |input, cx| {
            input.set_text("", cx);
            input.set_invalid(false, cx);
        });
        self.scope = 0;
        self.add_open = false;
        self.invalid = false;
        Toaster::push(Toast::new("Account added"), cx);
    }

    fn account_table(&self, visible: Vec<usize>, cx: &Context<Self>) -> Table {
        let owner = cx.weak_entity();
        let selected = self.selected.clone();
        let rows = visible
            .iter()
            .map(|&index| {
                let account = &self.accounts[index];
                vec![
                    account.name.clone(),
                    account.class.into(),
                    account.server.into(),
                    if account.online { "Online" } else { "Offline" }.into(),
                    if account.script.is_empty() {
                        "—"
                    } else {
                        account.script
                    }
                    .into(),
                    if account.setup.is_empty() {
                        "—"
                    } else {
                        account.setup
                    }
                    .into(),
                ]
            })
            .collect::<Vec<_>>();
        let labels: Rc<[(usize, SharedString)]> = visible
            .iter()
            .map(|&index| (index, self.accounts[index].name.clone()))
            .collect::<Vec<_>>()
            .into();
        Table::new(
            "accounts",
            "Accounts",
            vec![
                Column::new("Account"),
                Column::new("Class"),
                Column::new("Server").width(px(80.0)),
                Column::new("Status").width(px(78.0)),
                Column::new("Script"),
                Column::new("Setup"),
            ],
            rows,
        )
        .leading(move |row| {
            let owner = owner.clone();
            let (label, checked) = match row {
                Some(row) => {
                    let (account, name) = &labels[row];
                    (
                        format!("Select {name}"),
                        if selected.contains(account) {
                            CheckedState::Checked
                        } else {
                            CheckedState::Unchecked
                        },
                    )
                }
                None => {
                    let count = visible
                        .iter()
                        .filter(|index| selected.contains(index))
                        .count();
                    (
                        "Select all shown accounts".to_owned(),
                        match count {
                            0 => CheckedState::Unchecked,
                            count if count == visible.len() => CheckedState::Checked,
                            _ => CheckedState::Indeterminate,
                        },
                    )
                }
            };
            let selected_rows = match row {
                Some(row) => vec![labels[row].0],
                None => visible.clone(),
            };
            Checkbox::new("account-check", label, checked)
                .hide_label()
                .on_toggle(move |on, _, cx| {
                    owner
                        .update(cx, |view, cx| {
                            for &index in &selected_rows {
                                if on {
                                    view.selected.insert(index);
                                } else {
                                    view.selected.remove(&index);
                                }
                            }
                            cx.notify();
                        })
                        .ok();
                })
                .into_any_element()
        })
    }
}

impl Render for Accounts {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).text().trim().to_lowercase();
        let setups = ["All accounts", "Quest team", "Farm team"];
        let visible = self
            .accounts
            .iter()
            .enumerate()
            .filter_map(|(index, account)| {
                ((self.scope == 0 || account.setup == setups[self.scope])
                    && account.name.to_lowercase().contains(&query))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        let selected = visible
            .iter()
            .filter(|index| self.selected.contains(index))
            .copied()
            .collect::<Vec<_>>();
        let selection = selected.len();
        let setup_rows = selected.clone();
        let owner = cx.weak_entity();
        let online = visible
            .iter()
            .filter(|&&index| self.accounts[index].online)
            .count();
        let tools = toolbar(cx)
            .child(
                Select::new("setup", "Setup", setups, Some(self.scope))
                    .width(px(144.0))
                    .on_select(choose(cx, |view, scope, _| view.scope = scope)),
            )
            .child(div().w(px(184.0)).child(self.search.clone()))
            .child(spring())
            .map(|bar| {
                if selection == 0 {
                    bar.child(command(
                        "add",
                        "Add account",
                        Variant::Primary,
                        change(cx, |view, _| view.add_open = true),
                    ))
                } else {
                    bar.child(meta(format!("{selection} selected"), cx))
                        .child(
                            DropdownMenu::new("assign-setup")
                                .trigger(menu_button("assign", "Set setup", cx))
                                .items(move |_, _| {
                                    setups[1..]
                                        .iter()
                                        .map(|&setup| {
                                            let rows = setup_rows.clone();
                                            let owner = owner.clone();
                                            MenuItem::action(setup, move |_, cx| {
                                                owner
                                                    .update(cx, |view, cx| {
                                                        for &index in &rows {
                                                            view.accounts[index].setup = setup;
                                                        }
                                                        view.selected.clear();
                                                        cx.notify();
                                                    })
                                                    .ok();
                                            })
                                        })
                                        .collect()
                                }),
                        )
                        .child(command(
                            "clear",
                            "Clear selection",
                            Variant::Ghost,
                            change(cx, |view, _| {
                                view.selected.clear();
                            }),
                        ))
                        .child(command(
                            "launch",
                            "Launch",
                            Variant::Primary,
                            change(cx, move |view, cx| view.launch(&selected, cx)),
                        ))
                }
            });
        let body = if visible.is_empty() {
            empty(
                "No matching accounts",
                "Try a different search or setup.",
                cx,
            )
            .into_any_element()
        } else {
            scroll("account-rows")
                .px(px(16.0))
                .pt(px(8.0))
                .child(self.account_table(visible.clone(), cx))
                .into_any_element()
        };
        let footer = footer(cx)
            .child(meta(
                format!("{} shown · {online} online", visible.len()),
                cx,
            ))
            .child(spring());
        let close = change(cx, |view, cx| {
            view.add_open = false;
            view.invalid = false;
            view.name
                .update(cx, |input, cx| input.set_invalid(false, cx));
        });
        let dialog = Dialog::new("add-account", self.add_open, "Add account")
            .width(px(384.0))
            .description("Add an account to your local list.")
            .body(
                v_flex().child(
                    Field::new("Account name", self.name.clone()).error(
                        self.invalid
                            .then(|| "Enter a name that isn't already in the list.".into()),
                    ),
                ),
            )
            .on_close(close)
            .action(command(
                "cancel-add",
                "Cancel",
                Variant::Outline,
                change(cx, |view, cx| {
                    view.add_open = false;
                    view.invalid = false;
                    view.name
                        .update(cx, |input, cx| input.set_invalid(false, cx));
                }),
            ))
            .action(command(
                "confirm-add",
                "Add account",
                Variant::Primary,
                change(cx, Self::add),
            ));
        frame(tool_chrome(Screen::Accounts, None, cx), cx)
            .child(tools)
            .child(body)
            .child(footer)
            .child(dialog)
    }
}
