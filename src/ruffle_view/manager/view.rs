use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, Focusable as _, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Toggled, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, StyledExt as _, Theme, Tone,
    components::Empty,
    h_flex,
    primitives::{
        Alert, Align, Badge, Button, Checkbox, CheckedState, ContextMenu, Dialog, DropdownMenu,
        Field, Flow, HoverCard, Icon, MenuItem, Placement, Pose, Scrollbar, SegmentedProgress,
        Side, Signal, Sortable, Tab, Tabs, Tooltip,
        button::{Joined, Size, Variant},
        icon, small_icon, text_field,
    },
    theme::tabular,
    v_flex,
};

use super::{
    AddAccount, Ask, Check, ClearSelection, Confirming, Form, Manager, NewSetup, Pick, PickServer,
    Problem, StartSetup, View, join_names, plural,
    run::{
        Filter, ROSTER_FILTERS, RUN_FILTERS, Runs, Start, any_failed, count, setup_work, stoppable,
    },
    shared,
};
use crate::{
    accounts::{Account, AccountId},
    net,
    ruffle_view::{
        account_card::{
            Action, CHOOSE_IN_GAME, Card, Job, Line, Mood, Presence, Status, ago, card, status,
        },
        client::{CloseSession, bar_title, drag_area, window_bar},
        script_names,
        scripts::{fit_path_to, home_relative},
    },
    setups::Setup,
};

const CARD_GROUP: &str = "account-card";
const SCRIPT_MENU_WIDTH: gpui::Pixels = px(260.0);
const SCRIPT_PATH_WIDTH: gpui::Pixels = px(186.0);
const SERVER_MENU_WIDTH: gpui::Pixels = px(360.0);
const BAR_ROOM: gpui::Pixels = px(64.0);
const NARROW_BAR: gpui::Pixels = px(640.0);
const COMPACT_HEADER: gpui::Pixels = px(760.0);
const START_MIN_WIDTH: gpui::Pixels = px(112.0);
const QUICK_ACTION_ROOM: gpui::Pixels = px(44.0);

fn mood_color(theme: &Theme, mood: Mood) -> Hsla {
    match mood {
        Mood::Quiet => theme.muted_foreground,
        Mood::Plain => theme.foreground,
        Mood::Warning => theme.warning,
        Mood::Danger => theme.destructive,
    }
}

fn dot(theme: &Theme, tone: Option<Tone>) -> Div {
    let dot = div().flex_none().size(px(6.0)).rounded_full();
    match tone {
        Some(Tone::Success) => dot.bg(theme.primary),
        Some(Tone::Attention) => dot.bg(theme.warning),
        Some(Tone::Danger) => dot.bg(theme.destructive),
        Some(_) => dot.bg(theme.muted_foreground.opacity(0.6)),
        None => dot.border_1().border_color(theme.muted_foreground),
    }
}

fn attention(theme: &Theme, statuses: &[Status]) -> Option<AnyElement> {
    let (glyph, color, label) = if statuses.contains(&Status::Failed) {
        (Icon::CircleAlert, theme.destructive, "Something failed")
    } else if statuses.contains(&Status::NeedsYou) {
        (Icon::TriangleAlert, theme.warning, "Something needs you")
    } else {
        return None;
    };
    Some(
        div()
            .id(label)
            .role(Role::Image)
            .aria_label(label)
            .flex_none()
            .flex()
            .items_center()
            .child(icon(glyph, color).size(px(12.0)))
            .into_any_element(),
    )
}

fn server_items(
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
        item(CHOOSE_IN_GAME.to_owned(), None).pinned(),
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

fn server_stats(server: &net::Server, theme: &Theme) -> Div {
    let layout = theme.layout;
    let typography = theme.typography;
    let (players, color) = if !server.online {
        (SharedString::from("Offline"), theme.muted_foreground)
    } else {
        (
            format!("{}/{}", server.players, server.capacity).into(),
            if server.players >= server.capacity {
                theme.foreground
            } else {
                theme.muted_foreground
            },
        )
    };
    h_flex()
        .gap(layout.space(2.5))
        .ml(layout.space(4.0))
        .text_size(typography.secondary)
        .font_features(tabular())
        .when(server.members, |stats| stats.child(Badge::new("Member")))
        .child(div().text_color(color).child(players))
        .child(
            div()
                .w(px(44.0))
                .flex()
                .justify_end()
                .text_color(theme.muted_foreground)
                .children(server.ping.map(|ping| format!("{} ms", ping.as_millis()))),
        )
        .child(Signal::new(
            format!("{} connection", server.name),
            match server.ping.map(|ping| ping.as_millis()) {
                _ if !server.online => 0,
                Some(0..100) => 3,
                Some(100..200) => 2,
                Some(_) => 1,
                None => 0,
            },
        ))
}

fn script_items(
    this: &WeakEntity<Manager>,
    scripts: &[PathBuf],
    header: &'static str,
    current: Option<Option<&Path>>,
    window: &Window,
    cx: &App,
    run: Pick,
) -> Vec<MenuItem> {
    let theme = cx.theme();
    let item = |label: SharedString, script: Option<PathBuf>| {
        let this = this.clone();
        let run = run.clone();
        MenuItem::action(label, move |window, cx| {
            this.update(cx, |manager, cx| run(manager, script.clone(), window, cx))
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
        items.push(MenuItem::header(header));
        items.extend(listed.into_iter().map(|script| {
            let folder = script.parent().map(home_relative).unwrap_or_default();
            let hidden = script.to_path_buf();
            let refresh = this.clone();
            let row = item(script_names::name(script, cx), Some(script.to_path_buf()))
                .description(fit_path_to(&folder, SCRIPT_PATH_WIDTH, window, theme));
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

fn run_pick(targets: Vec<Account>) -> Pick {
    Rc::new(move |manager, script, window, cx| match script {
        Some(script) => manager.run(targets.clone(), script, cx),
        None => manager.run_chosen(targets.clone(), window, cx),
    })
}

type Items = Rc<dyn Fn(&mut Window, &mut App) -> Vec<MenuItem>>;

fn account_menu(
    this: WeakEntity<Manager>,
    account: Account,
    presence: Option<Presence>,
    scripts: Vec<PathBuf>,
    in_setup: bool,
) -> Items {
    Rc::new(move |window, cx| {
        let targets = [account.clone()];
        let act = |label: &'static str,
                   run: fn(&mut Manager, Vec<Account>, &mut Context<Manager>)| {
            targets_item(&this, &targets, label, move |manager, targets, _, cx| {
                run(manager, targets, cx)
            })
        };
        let open = presence.as_ref().is_some_and(Presence::opened);
        let mut items = Vec::new();
        if open {
            let shown = account.clone();
            items.push(targets_item(
                &this,
                &targets,
                "Show",
                move |manager, _, _, cx| manager.show_account(shown.clone(), cx),
            ));
        } else {
            items.push(act("Launch", Manager::launch));
        }
        items.push(MenuItem::submenu(
            "Run script",
            script_items(
                &this,
                &scripts,
                "Recent",
                None,
                window,
                cx,
                run_pick(targets.to_vec()),
            ),
        ));
        let busy = presence
            .as_ref()
            .is_some_and(|presence| presence.running().is_some() || presence.queued().is_some());
        if busy {
            items.push(act("Stop", Manager::stop));
        }
        if open {
            items.push(act("Close", Manager::close));
        }
        let edited = account.clone();
        items.extend([
            MenuItem::separator(),
            targets_item(
                &this,
                &targets,
                "Edit account…",
                move |manager, _, _, cx| manager.edit(Some(&edited), cx),
            ),
        ]);
        if !in_setup {
            let removed = account.clone();
            let mut remove = targets_item(
                &this,
                &targets,
                "Remove account…",
                move |manager, _, _, cx| {
                    manager.ask(
                        Ask::Remove {
                            account: removed.clone(),
                        },
                        cx,
                    )
                },
            )
            .danger();
            if let Some(blocked) = super::busy(&account.id(), "removing it", cx) {
                remove = remove.disabled(true).description(blocked);
            }
            items.push(remove);
        }
        items
    })
}

fn setup_menu(this: &WeakEntity<Manager>, name: &str) -> Vec<MenuItem> {
    let item = |label: &'static str,
                run: fn(&mut Manager, &str, &mut Window, &mut Context<Manager>)| {
        let this = this.clone();
        let name = name.to_owned();
        MenuItem::action(label, move |window, cx| {
            this.update(cx, |manager, cx| run(manager, &name, window, cx))
                .ok();
        })
    };
    vec![
        item("Edit", Manager::edit_setup),
        item("Duplicate", Manager::duplicate_setup),
        MenuItem::separator(),
        item("Delete…", |manager, name, _, cx| {
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

struct Frame {
    now: Instant,
    presences: Rc<HashMap<AccountId, Presence>>,
    cards: Vec<(Account, Card)>,
    scripts: Vec<PathBuf>,
}

struct SetupRow {
    name: SharedString,
    statuses: Vec<Status>,
    script: Option<SharedString>,
}

impl Manager {
    fn frame(&self, cx: &App) -> Frame {
        let now = Instant::now();
        let presences = self.presences(cx);
        let done = self
            .setup()
            .map(|setup| self.done(setup, cx))
            .unwrap_or_default();
        let cards = self
            .view_accounts()
            .into_iter()
            .map(|account| {
                let id = account.id();
                let presence = presences
                    .get(&id)
                    .cloned()
                    .unwrap_or(Presence::Closed(None));
                let card = card(&account, &presence, done.contains(&id), now);
                (account, card)
            })
            .collect();
        Frame {
            now,
            presences: Rc::new(presences),
            cards,
            scripts: self.recent_scripts(cx),
        }
    }

    fn title_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let layout = cx.theme().layout;
        window_bar(window, cx)
            .child(bar_title("Manager", cx))
            .child(drag_area().flex_1().h_full())
            .child(
                div()
                    .min_w(px(120.0))
                    .w(px(200.0))
                    .mr(layout.space(1.5))
                    .child(self.search.clone()),
            )
    }

    fn sidebar(
        &self,
        theme: &Theme,
        presences: &HashMap<AccountId, Presence>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let layout = theme.layout;
        let typography = theme.typography;
        let accounts = self.account_list();
        let setups = self.setups();
        let base = |ids: &mut dyn Iterator<Item = AccountId>| -> Vec<Status> {
            ids.map(|id| {
                presences
                    .get(&id)
                    .map_or(Status::NotOpen, |presence| status(presence, false))
            })
            .collect()
        };
        let tertiary = theme.muted_foreground;
        let label = {
            let theme = theme.clone();
            move |name: SharedString, statuses: &[Status]| {
                h_flex()
                    .min_w_0()
                    .gap(theme.space(1.5))
                    .child(div().min_w_0().truncate().child(name))
                    .children(attention(&theme, statuses))
            }
        };
        let count = move |count: usize| {
            div()
                .text_caption(typography)
                .text_color(tertiary)
                .child(count.to_string())
        };
        let all_statuses: Rc<[Status]> = base(&mut accounts.iter().map(Account::id)).into();
        let total = accounts.len();
        let all_label = label.clone();
        let all = Tabs::new("all-accounts", "Accounts", 1, move |_| {
            Tab::new("All accounts")
                .child(all_label("All accounts".into(), &all_statuses))
                .end_slot(count(total))
        })
        .page_sidebar()
        .small()
        .selected(if self.view == View::All {
            0
        } else {
            usize::MAX
        })
        .on_select({
            let this = cx.weak_entity();
            move |_, window, cx| {
                this.update(cx, |manager, cx| manager.set_view(View::All, window, cx))
                    .ok();
            }
        });
        let selected = match &self.view {
            View::All => None,
            View::Setup(name) => setups.iter().position(|setup| &setup.name == name),
        };
        let rows: Rc<[SetupRow]> = setups
            .iter()
            .map(|setup| SetupRow {
                name: setup.name.clone().into(),
                statuses: base(&mut setup.members.iter().map(|member| member.id())),
                script: setup.script.as_deref().map(home_relative),
            })
            .collect();
        let choices = rows.clone();
        let this = cx.weak_entity();
        let list = Tabs::new("setups", "Setups", setups.len(), move |index| {
            let row = &rows[index];
            let this = this.clone();
            let menu_name = row.name.clone();
            let description = row.script.clone().unwrap_or_else(|| "No script".into());
            let caption = div()
                .text_caption(typography)
                .text_color(tertiary)
                .truncate()
                .text_ellipsis_start()
                .child(description.clone());
            let tab = Tab::new(row.name.clone())
                .description(description)
                .child(label(row.name.clone(), &row.statuses))
                .detail(caption)
                .end_slot(count(row.statuses.len()))
                .menu(move |_, _| setup_menu(&this, &menu_name));
            match &row.script {
                Some(path) => tab.hover_card(
                    HoverCard::new(("setup-path", index), row.name.clone())
                        .placement(Placement::new(Side::Right, Align::Start))
                        .path_row("Script", path.clone()),
                ),
                None => tab,
            }
        })
        .page_sidebar()
        .small()
        .selected(selected.unwrap_or(usize::MAX))
        .on_select({
            let this = cx.weak_entity();
            move |index, window, cx| {
                let name = choices[index].name.to_string();
                this.update(cx, |manager, cx| {
                    manager.set_view(View::Setup(name), window, cx)
                })
                .ok();
            }
        });
        theme
            .sidebar_pane()
            .h_full()
            .child(all)
            .child(
                h_flex()
                    .flex_none()
                    .justify_between()
                    .pl(layout.space(4.0))
                    .pr(layout.space(2.0))
                    .text_caption(typography)
                    .text_color(theme.muted_foreground)
                    .child("Setups")
                    .child(
                        Button::icon("new-setup", Icon::Plus, "New setup")
                            .size(Size::Small)
                            .tooltip("New setup (⌘N)")
                            .disabled(self.accounts.is_err())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.new_setup(window, cx)),
                            ),
                    ),
            )
            .when(!setups.is_empty(), |sidebar| {
                sidebar.child(
                    div()
                        .id("setups-list")
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(list),
                )
            })
            .when(setups.is_empty() && !accounts.is_empty(), |sidebar| {
                sidebar.child(
                    div()
                        .px(layout.space(4.0))
                        .text_caption(typography)
                        .text_color(theme.muted_foreground)
                        .child("Setups run one script on a group of accounts."),
                )
            })
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

    fn filter_buttons(
        &self,
        theme: &Theme,
        filters: &[Filter],
        statuses: &[Status],
        compact: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let failed = any_failed(statuses);
        let total = statuses.len();
        filters
            .iter()
            .filter_map(|filter| {
                let count = count(statuses, *filter);
                let on = self.filter == Some(*filter);
                if count == 0 && !on {
                    return None;
                }
                let filter = *filter;
                let label = filter.label(count, total);
                Some(
                    Button::new(
                        SharedString::from(format!("filter-{filter:?}")),
                        if compact {
                            filter.short(count, total)
                        } else {
                            label.clone()
                        },
                    )
                    .variant(Variant::Ghost)
                    .size(Size::XSmall)
                    .when(compact, |button| button.tooltip(label))
                    .pressed(on)
                    .start_slot(dot(theme, filter.tone(failed)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_filter((!on).then_some(filter), cx)
                    }))
                    .into_any_element(),
                )
            })
            .collect()
    }

    fn header_row(&self, theme: &Theme) -> Div {
        let layout = theme.layout;
        h_flex()
            .flex_none()
            .min_h(layout.toolbar)
            .gap(layout.space(2.0))
            .px(layout.space(3.0))
            .pt(layout.space(2.0))
            .pb(layout.space(1.0))
    }

    fn roster_header(
        &self,
        theme: &Theme,
        statuses: &[Status],
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let layout = theme.layout;
        self.header_row(theme)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_none()
                            .pr(layout.space(1.5))
                            .font_weight(FontWeight::MEDIUM)
                            .child(plural(statuses.len(), "account")),
                    )
                    .children(self.filter_buttons(theme, &ROSTER_FILTERS, statuses, compact, cx)),
            )
            .child(
                Button::new("add-account", "Add account")
                    .size(Size::Small)
                    .disabled(self.accounts.is_err())
                    .on_click(cx.listener(|this, _, _, cx| this.edit(None, cx))),
            )
    }

    fn watch_header(
        &self,
        theme: &Theme,
        setup: &Setup,
        frame: &Frame,
        statuses: &[Status],
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let layout = theme.layout;
        let started = cx.global::<Runs>().started(&setup.name);
        let done = count(statuses, Filter::Done);
        let complete = !statuses.is_empty() && done == statuses.len();
        let when = started.map(|started| {
            let finished = setup
                .script
                .as_deref()
                .and_then(|script| {
                    setup
                        .members
                        .iter()
                        .filter_map(|member| self.finished_at(&member.id(), script, cx))
                        .max()
                })
                .filter(|_| complete);
            match finished {
                Some(at) => format!("Finished {}", ago(frame.now - at)),
                None => format!("Started {}", ago(frame.now - started)),
            }
        });
        let can_stop = !setup_work(setup, &frame.presences).is_empty();
        let failed = any_failed(statuses);
        let bar = RUN_FILTERS
            .iter()
            .filter_map(|filter| Some((count(statuses, *filter), filter.tone(failed)?)))
            .fold(
                SegmentedProgress::new(
                    "run-bar",
                    RUN_FILTERS
                        .iter()
                        .map(|filter| (count(statuses, *filter), filter))
                        .filter(|(count, _)| *count > 0)
                        .map(|(count, filter)| filter.label(count, statuses.len()))
                        .collect::<Vec<_>>()
                        .join(", "),
                    statuses.len(),
                ),
                |bar, (count, tone)| bar.part(count, tone),
            );
        v_flex()
            .flex_none()
            .child(
                self.header_row(theme)
                    .child(
                        h_flex()
                            .flex_grow(1.0)
                            .flex_shrink(1.0)
                            .min_w_0()
                            .overflow_hidden()
                            .children(self.filter_buttons(
                                theme,
                                &RUN_FILTERS,
                                statuses,
                                compact,
                                cx,
                            )),
                    )
                    .child(
                        Button::new("edit-setup", "Edit")
                            .variant(Variant::Ghost)
                            .size(Size::Small)
                            .on_click(cx.listener({
                                let name = setup.name.clone();
                                move |this, _, window, cx| this.edit_setup(&name, window, cx)
                            })),
                    )
                    .when(can_stop, |row| {
                        row.child(
                            Button::new("stop-setup", "Stop")
                                .variant(Variant::Ghost)
                                .size(Size::Small)
                                .on_click(cx.listener(|this, _, _, cx| this.stop_setup(cx))),
                        )
                    })
                    .child(self.start_controls(theme, setup, &frame.scripts, cx)),
            )
            .child(
                h_flex()
                    .gap(layout.space(3.0))
                    .px(layout.space(3.0))
                    .pt(layout.space(1.5))
                    .pb(layout.space(2.0))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_h(theme.typography.secondary_line)
                            .child(div().flex_1().child(bar)),
                    )
                    .children(when.map(|when| {
                        div()
                            .flex_none()
                            .text_caption(theme.typography)
                            .text_color(theme.muted_foreground)
                            .child(when)
                    })),
            )
    }

    fn start_controls(
        &self,
        theme: &Theme,
        setup: &Setup,
        scripts: &[PathBuf],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let this = cx.weak_entity();
        let name = setup.name.clone();
        let current = setup.script.clone();
        let launch = setup.launch;
        let started = cx.global::<Runs>().started(&setup.name).is_some();
        let scripts = scripts.to_vec();
        let empty = setup.members.is_empty();
        let items = move |window: &mut Window, cx: &mut App| {
            let mut items = Vec::new();
            if started && current.is_some() {
                let this = this.clone();
                let name = name.clone();
                items.push(MenuItem::action("Start over", move |_, cx| {
                    this.update(cx, |manager, cx| {
                        manager.start_setup(&name, Start::Fresh, None, None, cx)
                    })
                    .ok();
                }));
                items.push(MenuItem::separator());
            }
            let start_name = name.clone();
            let pick: Pick = Rc::new(move |manager, script, window, cx| match script {
                Some(script) => manager.start_with(&start_name, script, cx),
                None => manager.choose_start(start_name.clone(), window, cx),
            });
            items.extend(script_items(
                &this,
                &scripts,
                "Start with",
                Some(current.as_deref()),
                window,
                cx,
                pick,
            ));
            items.push(MenuItem::separator());
            items.push(MenuItem::header("Window"));
            let toggle = |label: &'static str, on: bool, edit: fn(&mut crate::setups::Launch)| {
                let this = this.clone();
                let name = name.clone();
                MenuItem::action(label, move |_, cx| {
                    this.update(cx, |manager, cx| manager.set_launch(&name, edit, cx))
                        .ok();
                })
                .checked(on)
                .keep_open()
            };
            items.push(toggle(
                "Open in a new window",
                launch.new_window,
                |launch| launch.new_window = !launch.new_window,
            ));
            items.push(toggle("Show in grid view", launch.grid, |launch| {
                launch.grid = !launch.grid
            }));
            items
        };
        let menu = |label: Option<&'static str>| {
            let trigger = match label {
                Some(label) => Button::new("start-with-trigger", label)
                    .variant(Variant::Primary)
                    .size(Size::Small)
                    .end_slot(small_icon(Icon::ChevronDown, theme.primary_foreground)),
                None => Button::icon(
                    "start-with-trigger",
                    Icon::ChevronDown,
                    "More ways to start",
                )
                .variant(Variant::Primary)
                .size(Size::Small)
                .joined(Joined::Previous),
            };
            DropdownMenu::new("start-with")
                .width(SCRIPT_MENU_WIDTH)
                .max_height(px(360.0))
                .align(Align::End)
                .trigger(trigger.disabled(empty))
                .items(items.clone())
        };
        let Some(script) = setup.script.as_deref() else {
            return menu(Some("Start")).into_any_element();
        };
        let name = script_names::name(script, cx);
        let done = self.done(setup, cx).len();
        let (label, start, tooltip) = match self.primary_start(setup, cx) {
            Some(Start::Fresh) => (
                format!("Start {name} again"),
                Start::Fresh,
                format!("Runs {name} on every account again"),
            ),
            _ if done > 0 => (
                format!("Continue {name}"),
                Start::Continue,
                format!("Skips the {done} already done"),
            ),
            _ => (
                format!("Start {name}"),
                Start::Continue,
                home_relative(script).to_string(),
            ),
        };
        let setup_name = setup.name.clone();
        h_flex()
            .min_w_0()
            .flex_shrink(1000.0)
            .child(
                Button::new("start-setup", label)
                    .variant(Variant::Primary)
                    .size(Size::Small)
                    .joined(Joined::Next)
                    .flex_shrink(1.0)
                    .min_w(START_MIN_WIDTH)
                    .tooltip(tooltip)
                    .disabled(empty)
                    .end_slot(
                        div()
                            .text_color(theme.primary_foreground.opacity(0.6))
                            .child("⌘⏎"),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.start_setup(&setup_name, start, None, None, cx)
                    })),
            )
            .child(menu(None))
            .into_any_element()
    }

    fn edit_header(
        &self,
        theme: &Theme,
        setup: &Setup,
        scripts: &[PathBuf],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let layout = theme.layout;
        let typography = theme.typography;
        let editing = self.editing.as_ref();
        let label = |text: &'static str| {
            div()
                .text_caption(typography)
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let this = cx.weak_entity();
        let name = setup.name.clone();
        let current = setup.script.clone();
        let scripts = scripts.to_vec();
        let script_menu = DropdownMenu::new("setup-script")
            .width(SCRIPT_MENU_WIDTH)
            .max_height(px(360.0))
            .trigger(
                Button::new(
                    "setup-script-trigger",
                    setup
                        .script
                        .as_deref()
                        .map_or_else(|| "None".into(), |script| script_names::name(script, cx)),
                )
                .variant(Variant::Outline)
                .size(Size::Small)
                .flex_shrink(1.0)
                .min_w_0()
                .when_some(setup.script.as_deref(), |button, script| {
                    button.tooltip(home_relative(script))
                })
                .end_slot(small_icon(Icon::ChevronDown, theme.muted_foreground)),
            )
            .items(move |window, cx| {
                let pick_name = name.clone();
                let none_name = name.clone();
                let this_none = this.clone();
                let pick: Pick = Rc::new(move |manager, script, window, cx| match script {
                    Some(script) => manager.set_setup_script(&pick_name, Some(script), cx),
                    None => manager.choose_setup_script(pick_name.clone(), window, cx),
                });
                let mut items = vec![
                    MenuItem::action("None", move |_, cx| {
                        this_none
                            .update(cx, |manager, cx| {
                                manager.set_setup_script(&none_name, None, cx)
                            })
                            .ok();
                    })
                    .checked(current.is_none()),
                    MenuItem::separator(),
                ];
                items.extend(script_items(
                    &this,
                    &scripts,
                    "Recent",
                    Some(current.as_deref()),
                    window,
                    cx,
                    pick,
                ));
                items
            });
        let members_menu = {
            let this = cx.weak_entity();
            let all = self.account_list();
            let setup = setup.clone();
            DropdownMenu::new("setup-accounts")
                .width(px(220.0))
                .max_height(px(360.0))
                .align(Align::End)
                .trigger(
                    Button::new("setup-accounts-trigger", "Choose accounts")
                        .variant(Variant::Outline)
                        .size(Size::Small)
                        .end_slot(small_icon(Icon::ChevronDown, theme.muted_foreground)),
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
        let error = editing.and_then(|editing| editing.error.clone());
        v_flex()
            .flex_none()
            .child(
                self.header_row(theme)
                    .items_end()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .flex_wrap()
                            .items_end()
                            .gap(layout.space(3.0))
                            .children(editing.map(|editing| {
                                v_flex()
                                    .gap(layout.space(1.0))
                                    .w(px(200.0))
                                    .child(label("Name"))
                                    .child(editing.name.clone())
                            }))
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .max_w_full()
                                    .gap(layout.space(1.0))
                                    .child(label("Script"))
                                    .child(script_menu),
                            ),
                    )
                    .child(members_menu)
                    .child(
                        Button::new("done-editing", "Done")
                            .variant(Variant::Primary)
                            .size(Size::Small)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.finish_edit(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .px(layout.space(3.0))
                    .pb(layout.space(2.0))
                    .text_caption(typography)
                    .map(|hint| match error {
                        Some(error) => hint.text_color(theme.destructive).child(error),
                        None => hint.text_color(theme.muted_foreground).child(
                            "Drag cards, or press Option and an arrow key, to reorder. Accounts \
                             launch in this order, each to its own login server.",
                        ),
                    }),
            )
    }

    fn card(
        &self,
        theme: &Theme,
        account: &Account,
        card: Card,
        frame: &Frame,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = theme.layout;
        let typography = theme.typography;
        let id = account.id();
        let checked = self.checked.contains(&id);
        let focused = self.cursor.as_ref() == Some(&id)
            && self.focus.is_focused(window)
            && mira_ui::nav::keyboard_input(window, cx);
        let in_setup = self.setup().is_some();
        let items = account_menu(
            cx.weak_entity(),
            account.clone(),
            frame.presences.get(&id).cloned(),
            frame.scripts.clone(),
            in_setup,
        );
        let more_items = items.clone();
        let more_id = SharedString::from(format!("card-more-{}", account.username));
        let surface = if checked {
            theme.background.blend(
                theme
                    .primary
                    .opacity(if theme.dark_mode { 0.1 } else { 0.05 }),
            )
        } else {
            theme.card
        };
        let more = DropdownMenu::new(more_id)
            .align(Align::End)
            .trigger(
                Button::icon(
                    SharedString::from(format!("card-more-trigger-{}", account.username)),
                    Icon::Ellipsis,
                    format!("Account actions for {}", account.name),
                )
                .size(Size::XSmall)
                .hide_tooltip(),
            )
            .items(move |window, cx| more_items(window, cx));
        let reveal = |slot: Div, shown: bool| {
            if shown {
                slot
            } else {
                slot.invisible()
                    .group_hover(CARD_GROUP, |slot| slot.visible())
            }
        };
        let badge = Badge::new(card.status.label()).tone(card.status.tone());
        let badge = if card.state.as_ref() == card.status.label() {
            badge.into_any_element()
        } else {
            div()
                .flex_none()
                .child(Tooltip::new(
                    SharedString::from(format!("card-state-{}", account.username)),
                    card.state.clone(),
                    badge,
                ))
                .into_any_element()
        };
        let row = |glyph: Icon, label: &'static str, body: AnyElement| {
            h_flex()
                .min_w_0()
                .h(px(20.0))
                .gap(layout.space(2.0))
                .child(
                    div()
                        .id(label)
                        .role(Role::Image)
                        .aria_label(label)
                        .flex_none()
                        .flex()
                        .items_center()
                        .child(icon(glyph, theme.muted_foreground).size(px(12.0))),
                )
                .child(body)
        };
        let text = |line: &Line| {
            div()
                .min_w_0()
                .truncate()
                .text_caption(typography)
                .text_color(mood_color(theme, line.mood))
                .child(line.text.clone())
        };
        let action = card.action.map(|action| {
            let target = account.clone();
            let fix = card.status.needs_you();
            let button = Button::new(
                SharedString::from(format!("card-action-{}", account.username)),
                action.label(),
            )
            .variant(if fix {
                Variant::Outline
            } else {
                Variant::Ghost
            })
            .size(Size::XSmall)
            .on_click(cx.listener(move |this, _, _, cx| match action {
                Action::EditLogin => this.edit(Some(&target), cx),
                Action::Restart => this.restart(&target, cx),
                Action::LogIn | Action::Launch => this.launch(vec![target.clone()], cx),
                Action::Show => this.show_account(target.clone(), cx),
                Action::Stop => this.stop(vec![target.clone()], cx),
            }));
            (button, fix)
        });
        let time = match &card.job {
            Job::Line(line) => line.time.clone().map(|time| {
                div()
                    .flex_none()
                    .text_caption(typography)
                    .font_features(tabular())
                    .text_color(theme.muted_foreground)
                    .child(time)
            }),
            Job::Loading(_) => None,
        };
        let end = match (action, time) {
            (Some((button, true)), _) => Some(div().flex_none().child(button)),
            (Some((button, false)), Some(time)) => Some(
                div()
                    .relative()
                    .flex_none()
                    .min_w(QUICK_ACTION_ROOM)
                    .flex()
                    .justify_end()
                    .child(if focused {
                        time.invisible()
                    } else {
                        time.group_hover(CARD_GROUP, |time| time.invisible())
                    })
                    .child(reveal(div().absolute().top(px(-2.0)).right_0(), focused).child(button)),
            ),
            (Some((button, false)), None) => Some(reveal(div().flex_none(), focused).child(button)),
            (None, time) => time,
        };
        let job = match &card.job {
            Job::Loading(Some(percent)) => h_flex()
                .flex_1()
                .min_w_0()
                .gap(layout.space(2.0))
                .child(
                    div().flex_1().child(
                        SegmentedProgress::new(
                            SharedString::from(format!("loading-{}", account.username)),
                            format!("Loading {}", account.name),
                            100,
                        )
                        .part(usize::from(*percent), Tone::Success),
                    ),
                )
                .child(
                    div()
                        .text_caption(typography)
                        .font_features(tabular())
                        .text_color(theme.muted_foreground)
                        .child(format!("{percent}%")),
                ),
            Job::Loading(None) => h_flex().flex_1().min_w_0().child(text(&Line {
                text: "Loading".into(),
                mood: Mood::Quiet,
                path: None,
                detail: None,
                time: None,
            })),
            Job::Line(line) => h_flex()
                .flex_1()
                .min_w_0()
                .gap(layout.space(1.5))
                .child({
                    let id = SharedString::from(format!("card-line-{}", account.username));
                    match (&line.detail, &line.path) {
                        (Some((title, message)), path) => HoverCard::new(id, title.clone())
                            .placement(Placement::new(Side::Bottom, Align::Start))
                            .when_some(path.clone(), |card, path| card.path_row("Script", path))
                            .row("Error", message.clone(), Tone::Danger)
                            .child({
                                let ink = mood_color(theme, line.mood);
                                text(line)
                                    .border_b_1()
                                    .border_dashed()
                                    .border_color(ink.opacity(0.5))
                                    .hover(move |style| style.border_color(ink))
                            })
                            .into_any_element(),
                        (None, Some(path)) => HoverCard::new(id, account.name.clone())
                            .placement(Placement::new(Side::Bottom, Align::Start))
                            .path_row("Script", path.clone())
                            .child(text(line).id("script-name").aria_description(path.clone()))
                            .into_any_element(),
                        (None, None) => text(line).into_any_element(),
                    }
                })
                .when(line.path.is_none() && line.detail.is_none(), |row| {
                    row.child(div().flex_1())
                }),
        }
        .children(end);
        let clicked = id.clone();
        let element = v_flex()
            .id(SharedString::from(format!("account-{}", account.username)))
            .group(CARD_GROUP)
            .relative()
            .h_full()
            .min_w_0()
            .gap(layout.space(1.5))
            .px(layout.space(3.0))
            .pt(layout.space(2.5))
            .pb(layout.space(2.0))
            .rounded(theme.radius.panel)
            .border_1()
            .map(|card| {
                if checked {
                    card.border_color(theme.primary)
                        .bg(theme
                            .primary
                            .opacity(if theme.dark_mode { 0.1 } else { 0.05 }))
                } else {
                    card.border_color(theme.surface_edge()).bg(theme.card)
                }
            })
            .role(Role::CheckBox)
            .aria_label(account.name.clone())
            .aria_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                window.focus(&this.focus, cx);
                this.toggle(clicked.clone(), cx);
            }))
            .child(
                h_flex()
                    .min_w_0()
                    .gap(layout.space(2.0))
                    .child(Checkbox::indicator(
                        if checked {
                            CheckedState::Checked
                        } else {
                            CheckedState::Unchecked
                        },
                        theme,
                    ))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(account.name.clone()),
                    )
                    .child(badge),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .child(row(Icon::FileCode, "Script", job.into_any_element()))
                    .child(row(
                        Icon::MapPin,
                        "Location",
                        text(&card.place).into_any_element(),
                    ))
                    .child(row(
                        Icon::User,
                        "Character",
                        div()
                            .min_w_0()
                            .truncate()
                            .text_caption(typography)
                            .text_color(if card.who.is_some() {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .child(card.who.clone().unwrap_or_else(|| "—".into()))
                            .into_any_element(),
                    )),
            )
            .child(
                reveal(
                    div()
                        .absolute()
                        .top(layout.space(2.5))
                        .right(layout.space(2.0))
                        .pl(layout.space(5.0))
                        .bg(gpui::linear_gradient(
                            90.0,
                            gpui::linear_color_stop(surface.opacity(0.0), 0.0),
                            gpui::linear_color_stop(surface, 0.5),
                        )),
                    focused,
                )
                .child(more),
            )
            .when(focused, |card| {
                card.child(theme.ring_around(theme.ring, theme.radius.panel, px(1.0)))
            });
        ContextMenu::new(
            SharedString::from(format!("account-menu-{}", account.username)),
            "Account actions",
        )
        .items(move |window, cx| items(window, cx))
        .child(element)
        .into_any_element()
    }

    fn edit_grid(
        &self,
        theme: &Theme,
        setup: &Setup,
        frame: &Frame,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = theme.layout;
        let members = self.members(setup);
        if members.is_empty() {
            return div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(theme.muted_foreground)
                .child("No accounts in this setup yet. Use Choose accounts to pick some.")
                .into_any_element();
        }
        let columns = self.columns.max(1);
        let inset = layout.space(3.0);
        let gap = layout.space(2.0);
        let width = window.viewport_size().width - layout.sidebar - inset * 2.0;
        let card_width = (width - gap * (columns as f32 - 1.0)) / columns as f32;
        let cards: Rc<Vec<(Account, Card)>> = Rc::new(
            members
                .iter()
                .map(|account| {
                    let card = frame
                        .cards
                        .iter()
                        .find(|(shown, _)| shown.id() == account.id())
                        .map(|(_, card)| card.clone())
                        .unwrap_or_else(|| {
                            card(account, &Presence::Closed(None), false, frame.now)
                        });
                    (account.clone(), card)
                })
                .collect(),
        );
        let theme_for = theme.clone();
        let cursor = self
            .cursor
            .clone()
            .filter(|_| self.focus.is_focused(window) && mira_ui::nav::keyboard_input(window, cx));
        let keys: Vec<SharedString> = members
            .iter()
            .map(|account| SharedString::from(format!("edit-{}", account.username)))
            .collect();
        let count = members.len();
        let this = cx.weak_entity();
        let name = setup.name.clone();
        let sortable = Sortable::new(
            div()
                .id("edit-grid")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_wrap()
                .content_start()
                .gap(gap)
                .px(inset)
                .pt(layout.space(1.0))
                .pb(layout.space(3.0)),
            Flow::Wrap,
            keys,
            move |(index, pose), _, _| {
                let theme = &theme_for;
                let typography = theme.typography;
                let (account, card) = &cards[index];
                let current = cursor.as_ref() == Some(&account.id());
                let line = |glyph: Icon, label: &'static str, value: SharedString, quiet: bool| {
                    h_flex()
                        .min_w_0()
                        .h(px(20.0))
                        .gap(theme.space(2.0))
                        .child(
                            div()
                                .id(label)
                                .role(Role::Image)
                                .aria_label(label)
                                .flex_none()
                                .child(icon(glyph, theme.muted_foreground).size(px(12.0))),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_caption(typography)
                                .text_color(if quiet {
                                    theme.muted_foreground
                                } else {
                                    theme.foreground
                                })
                                .child(value),
                        )
                };
                v_flex()
                    .id(index)
                    .relative()
                    .w(card_width)
                    .h_full()
                    .gap(theme.space(1.5))
                    .px(theme.space(3.0))
                    .pt(theme.space(2.5))
                    .pb(theme.space(2.0))
                    .rounded(theme.radius.panel)
                    .bg(theme.card)
                    .cursor_grab()
                    .when(pose == Pose::Lifted, |card| {
                        card.shadow(theme.popup_shadow())
                    })
                    .child(theme.surface_ring(theme.radius.panel))
                    .role(Role::ListItem)
                    .aria_label(format!("{}, {} of {count}", account.name, index + 1))
                    .child(
                        h_flex()
                            .min_w_0()
                            .h(px(20.0))
                            .gap(theme.space(2.0))
                            .child(
                                div()
                                    .flex_none()
                                    .size(theme.layout.check)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon(Icon::GripVertical, theme.muted_foreground)),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(account.name.clone()),
                            ),
                    )
                    .child(
                        v_flex()
                            .child(line(
                                Icon::Server,
                                "Login server",
                                account
                                    .server
                                    .clone()
                                    .unwrap_or_else(|| CHOOSE_IN_GAME.into())
                                    .into(),
                                true,
                            ))
                            .child(line(
                                Icon::User,
                                "Character",
                                card.who.clone().unwrap_or_else(|| "—".into()),
                                card.who.is_none(),
                            )),
                    )
                    .when(current, |card| {
                        card.child(theme.ring_around(theme.ring, theme.radius.panel, px(1.0)))
                    })
                    .into_any_element()
            },
            move |from, to, _, cx| {
                this.update(cx, |manager, cx| manager.move_member(&name, from, to, cx))
                    .ok();
            },
        )
        .scrolls(&self.grid_scroll);
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(sortable)
            .child(Scrollbar::new("edit-scrollbar", self.grid_scroll.clone()))
            .into_any_element()
    }

    fn selection_bar(
        &self,
        theme: &Theme,
        selected: Vec<Account>,
        frame: &Frame,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let layout = theme.layout;
        let narrow = window.viewport_size().width - layout.sidebar < NARROW_BAR;
        let separator = || {
            div()
                .flex_none()
                .mx(layout.space(1.0))
                .w(px(1.0))
                .h(px(16.0))
                .bg(theme.border)
        };
        let presences = frame.presences.clone();
        let presence = |account: &Account| presences.get(&account.id());
        let bulk = |id: &'static str,
                    label: &'static str,
                    run: fn(&mut Manager, Vec<Account>, &mut Context<Manager>),
                    cx: &mut Context<Self>| {
            let targets = selected.clone();
            Button::new(id, label)
                .variant(Variant::Ghost)
                .size(Size::Small)
                .on_click(cx.listener(move |this, _, _, cx| run(this, targets.clone(), cx)))
        };
        let menu_button = |id: &'static str, label: &'static str| {
            Button::new(id, label)
                .variant(Variant::Ghost)
                .size(Size::Small)
                .end_slot(small_icon(Icon::ChevronDown, theme.muted_foreground))
        };
        let this = cx.weak_entity();
        let run = run_pick(selected.clone());
        let scripts = frame.scripts.clone();
        let run_menu = DropdownMenu::new("run-selected")
            .width(SCRIPT_MENU_WIDTH)
            .max_height(px(320.0))
            .trigger(menu_button("run-selected-trigger", "Run script"))
            .items(move |window, cx| {
                script_items(&this, &scripts, "Recent", None, window, cx, run.clone())
            });
        let launchable = selected
            .iter()
            .any(|account| !presence(account).is_some_and(Presence::opened));
        let stoppable_any = !stoppable(
            selected
                .iter()
                .filter_map(|account| Some((account.id(), presence(account)?))),
            None,
        )
        .is_empty();
        let closable = selected
            .iter()
            .any(|account| presence(account).is_some_and(Presence::opened));
        let ids: HashSet<AccountId> = selected.iter().map(Account::id).collect();
        let current = shared(&selected, |account| &account.server);
        let servers = self.servers.list.clone();
        let this = cx.weak_entity();
        let server_items: Items = Rc::new(move |_, cx| {
            let this = this.clone();
            let ids = ids.clone();
            let pick: PickServer = Rc::new(move |server, _, cx| {
                this.update(cx, |manager, cx| {
                    manager.set_servers(ids.clone(), server, cx)
                })
                .ok();
            });
            server_items(
                &servers,
                current.as_ref().map(Option::as_deref),
                cx.theme(),
                &pick,
            )
        });
        let add_items: Option<Items> = (self.view == View::All).then(|| {
            let setups: Vec<String> = self.setups().into_iter().map(|setup| setup.name).collect();
            let this = cx.weak_entity();
            let targets = selected.clone();
            Rc::new(move |_: &mut Window, _: &mut App| {
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
                    |manager, targets, window, cx| manager.new_setup_from(targets, window, cx),
                ));
                items
            }) as Items
        });
        let settings: Vec<AnyElement> = if narrow {
            let add = add_items.clone();
            vec![
                DropdownMenu::new("selected-more")
                    .width(px(200.0))
                    .align(Align::End)
                    .trigger(menu_button("selected-more-trigger", "More"))
                    .items(move |window, cx| {
                        let mut items =
                            vec![MenuItem::submenu("Login server", server_items(window, cx))];
                        if let Some(add) = &add {
                            items.push(MenuItem::submenu("Add to setup", add(window, cx)));
                        }
                        items
                    })
                    .into_any_element(),
            ]
        } else {
            let mut menus = vec![
                DropdownMenu::new("selected-server")
                    .width(SERVER_MENU_WIDTH)
                    .max_height(px(360.0))
                    .trigger(menu_button("selected-server-trigger", "Login server"))
                    .items(move |window, cx| server_items(window, cx))
                    .into_any_element(),
            ];
            if let Some(add) = add_items {
                menus.push(
                    DropdownMenu::new("add-to-setup")
                        .width(px(200.0))
                        .max_height(px(320.0))
                        .trigger(menu_button("add-to-setup-trigger", "Add to setup"))
                        .items(move |window, cx| add(window, cx))
                        .into_any_element(),
                );
            }
            menus
        };
        h_flex()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .justify_center()
            .px(layout.space(3.0))
            .pt(layout.space(6.0))
            .pb(layout.space(3.0))
            .bg(gpui::linear_gradient(
                180.0,
                gpui::linear_color_stop(theme.background.opacity(0.0), 0.0),
                gpui::linear_color_stop(theme.background, 0.45),
            ))
            .child(
                theme
                    .floating_bar()
                    .id("selection-bar")
                    .role(Role::Toolbar)
                    .aria_label("Selected accounts")
                    .max_w_full()
                    .child(
                        div()
                            .pr(px(2.0))
                            .font_weight(FontWeight::MEDIUM)
                            .font_features(tabular())
                            .whitespace_nowrap()
                            .child(format!("{} selected", selected.len())),
                    )
                    .child(
                        Button::icon("clear-selection", Icon::X, "Clear selection")
                            .size(Size::Small)
                            .on_click(cx.listener(|this, _, _, cx| this.clear_selection(cx))),
                    )
                    .child(separator())
                    .when(launchable, |bar| {
                        bar.child(bulk("launch-selected", "Launch", Manager::launch, cx))
                    })
                    .child(run_menu)
                    .when(stoppable_any, |bar| {
                        bar.child(bulk("stop-selected", "Stop", Manager::stop, cx))
                    })
                    .when(closable, |bar| {
                        bar.child(bulk("close-selected", "Close", Manager::close, cx))
                    })
                    .child(separator())
                    .children(settings),
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
        let layout = theme.layout;
        let typography = theme.typography;
        let (username, password, other) = match form.error.clone() {
            Some(Problem::Username(message)) => (Some(message), None, None),
            Some(Problem::Password(message)) => (None, Some(message), None),
            Some(Problem::Other(message)) => (None, None, Some(message)),
            None => (None, None, None),
        };
        let this = cx.weak_entity();
        let pick: PickServer = Rc::new(move |server, _, cx| {
            this.update(cx, |manager, cx| {
                if let Some(form) = &mut manager.form {
                    form.server = server;
                }
                cx.notify();
            })
            .ok();
        });
        let servers = self.servers.list.clone();
        let current = form.server.clone();
        let stats = current.as_deref().and_then(|name| {
            servers
                .iter()
                .find(|server| server.name == name)
                .map(|server| server_stats(server, theme))
        });
        let server = DropdownMenu::new("account-server")
            .max_height(px(320.0))
            .width(SERVER_MENU_WIDTH)
            .trigger(
                Button::new(
                    "account-server-trigger",
                    current.clone().unwrap_or_else(|| CHOOSE_IN_GAME.into()),
                )
                .variant(Variant::Outline)
                .w_full()
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(layout.space(1.5))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .child(current.clone().unwrap_or_else(|| CHOOSE_IN_GAME.into())),
                        )
                        .child(div().flex_1())
                        .children(stats)
                        .child(small_icon(Icon::ChevronDown, theme.muted_foreground)),
                ),
            )
            .items(move |_, cx| {
                server_items(&servers, Some(current.as_deref()), cx.theme(), &pick)
            });
        let checking = matches!(self.servers.check, Check::Idle | Check::Running);
        let refresh = Button::icon("refresh-servers", Icon::RefreshCw, "Refresh servers")
            .variant(Variant::Ghost)
            .size(Size::XSmall)
            .icon_size(px(14.0))
            .busy(checking)
            .disabled(!checking && !self.servers.ready())
            .on_click(cx.listener(|this, _, _, cx| this.check_servers(cx)));
        let body =
            v_flex()
                .gap(layout.space(4.0))
                .child(
                    h_flex()
                        .items_start()
                        .gap(layout.space(3.0))
                        .child(
                            div().flex_1().min_w_0().child(
                                Field::new("Username", form.username.clone()).error(username),
                            ),
                        )
                        .child(
                            div().flex_1().min_w_0().child(
                                Field::new("Password", form.password.clone()).error(password),
                            ),
                        ),
                )
                .child(
                    Field::new("Display name", form.name.clone()).end_slot(
                        div()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(theme.muted_foreground)
                            .child("Optional"),
                    ),
                )
                .child(
                    Field::new("Login server", server)
                        .description(self.servers.note())
                        .end_slot(h_flex().h(typography.secondary_line).child(refresh)),
                )
                .children(other.map(|error| {
                    div()
                        .text_size(typography.secondary)
                        .text_color(theme.destructive)
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

    fn confirmation(
        &self,
        confirm: &Confirming,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let dismiss = cx.listener(|this, _: &(), _, cx| this.dismiss(cx));
        let cancel = || {
            Button::new("confirm-cancel", "Cancel")
                .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx)))
                .into_any_element()
        };
        let choose = |id: &'static str, label: String, variant: Variant, choice: bool| {
            Button::new(id, label)
                .variant(variant)
                .on_click(cx.listener(move |this, _, window, cx| this.confirm(choice, window, cx)))
                .into_any_element()
        };
        let (title, description, actions): (String, String, Vec<AnyElement>) = match &confirm.ask {
            Ask::Start {
                setup, conflicts, ..
            } => {
                let one = conflicts.len() == 1;
                (
                    format!("Start {setup}?"),
                    match conflicts.as_slice() {
                        [one] => format!("{one} is already running another script."),
                        many => format!("{} are already running other scripts.", join_names(many)),
                    },
                    vec![
                        choose(
                            "confirm-leave",
                            (if one {
                                "Leave it running"
                            } else {
                                "Leave them running"
                            })
                            .into(),
                            Variant::Outline,
                            false,
                        ),
                        choose(
                            "confirm-replace",
                            (if one {
                                "Replace script"
                            } else {
                                "Replace scripts"
                            })
                            .into(),
                            Variant::Primary,
                            true,
                        ),
                    ],
                )
            }
            Ask::Run {
                script, conflicts, ..
            } => {
                let one = conflicts.len() == 1;
                (
                    format!("Run {}?", script_names::name(script, cx)),
                    match conflicts.as_slice() {
                        [(name, running)] => format!("{name} is already running {running}."),
                        many => format!(
                            "{} are already running other scripts.",
                            join_names(
                                &many
                                    .iter()
                                    .map(|(name, _)| name.clone())
                                    .collect::<Vec<_>>()
                            )
                        ),
                    },
                    vec![
                        cancel(),
                        choose(
                            "confirm-replace",
                            (if one {
                                "Replace script"
                            } else {
                                "Replace scripts"
                            })
                            .into(),
                            Variant::Primary,
                            true,
                        ),
                    ],
                )
            }
            Ask::Stop {
                title,
                description,
                many,
                ..
            } => (
                title.clone(),
                description.clone(),
                vec![
                    cancel(),
                    choose(
                        "confirm-stop",
                        (if *many { "Stop scripts" } else { "Stop script" }).into(),
                        Variant::Danger,
                        true,
                    ),
                ],
            ),
            Ask::Close { targets, running } => (
                match targets.as_slice() {
                    [one] => format!("Close {}?", one.name),
                    many => format!("Close {} accounts?", many.len()),
                },
                match (targets.len(), running) {
                    (1, _) => "This stops its running script and logs it out.".to_owned(),
                    (_, 1) => "This stops 1 running script and logs them all out.".to_owned(),
                    (_, count) => {
                        format!("This stops {count} running scripts and logs them all out.")
                    }
                },
                vec![
                    cancel(),
                    choose(
                        "confirm-close",
                        (if targets.len() == 1 {
                            "Close account"
                        } else {
                            "Close accounts"
                        })
                        .into(),
                        Variant::Danger,
                        true,
                    ),
                ],
            ),
            Ask::Remove { account } => (
                format!("Remove {}?", account.name),
                format!(
                    "This deletes the saved login and settings for {} and takes it out of any \
                     setups.",
                    account.name
                ),
                vec![
                    cancel(),
                    choose(
                        "confirm-remove",
                        "Remove account".into(),
                        Variant::Danger,
                        true,
                    ),
                ],
            ),
            Ask::Delete { setup } => (
                format!("Delete {setup}?"),
                format!("The accounts in {setup} stay in Mira. Only the setup is deleted."),
                vec![
                    cancel(),
                    choose(
                        "confirm-delete",
                        "Delete setup".into(),
                        Variant::Danger,
                        true,
                    ),
                ],
            ),
        };
        actions
            .into_iter()
            .fold(
                Dialog::new("confirm", confirm.open, title)
                    .alert()
                    .description(description),
                |dialog, action| dialog.action(action),
            )
            .on_close(move |window, cx| dismiss(&(), window, cx))
    }
}

impl Render for Manager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let layout = theme.layout;
        if let View::Setup(name) = &self.view
            && self.setup().is_none()
        {
            tracing::debug!(setup = name, "the shown setup is gone");
            self.view = View::All;
            self.editing = None;
        }
        let frame = self.frame(cx);
        let setup = self.setup().cloned();
        let editing = setup.as_ref().is_some_and(|setup| {
            self.editing
                .as_ref()
                .is_some_and(|editing| editing.setup == setup.name)
        });
        let statuses: Vec<Status> = frame.cards.iter().map(|(_, card)| card.status).collect();
        let compact = window.viewport_size().width - layout.sidebar < COMPACT_HEADER;
        let ticking = setup
            .as_ref()
            .is_some_and(|setup| cx.global::<Runs>().started(&setup.name).is_some())
            || frame.presences.values().any(|presence| {
                match presence {
                    Presence::Closed(last) => last.as_ref(),
                    other => other.live().and_then(|summary| summary.script.as_ref()),
                }
                .is_some_and(|script| script.since.is_some())
            });
        self.keep_clock(ticking, cx);
        self.columns = theme.account_columns(window.viewport_size().width - layout.sidebar);
        mira_ui::nav::set_scroll_root(&self.grid_scroll, window, cx);
        let title_bar = self.title_bar(window, cx);
        let sidebar = self.sidebar(&theme, &frame.presences, cx);
        let selected = if editing {
            Vec::new()
        } else {
            self.checked_accounts()
        };
        let content = match &self.accounts {
            Err(error) => div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .px(layout.space(6.0))
                .text_color(theme.destructive)
                .child(format!("Mira couldn't load your accounts: {error}"))
                .into_any_element(),
            Ok(all) if all.list().is_empty() => self.empty(cx).into_any_element(),
            Ok(_) => {
                let header = match &setup {
                    Some(setup) if editing => self
                        .edit_header(&theme, setup, &frame.scripts, cx)
                        .into_any_element(),
                    Some(setup) => self
                        .watch_header(&theme, setup, &frame, &statuses, compact, cx)
                        .into_any_element(),
                    None => self
                        .roster_header(&theme, &statuses, compact, cx)
                        .into_any_element(),
                };
                let grid = match &setup {
                    Some(setup) if editing => self.edit_grid(&theme, setup, &frame, window, cx),
                    _ => {
                        let query = self.search.read(cx).text().to_owned();
                        let shown: Vec<AnyElement> = frame
                            .cards
                            .iter()
                            .filter(|(account, card)| {
                                self.filter.is_none_or(|filter| filter.admits(card.status))
                                    && card.matches(account, &query)
                            })
                            .map(|(account, card)| {
                                self.card(&theme, account, card.clone(), &frame, window, cx)
                            })
                            .collect();
                        if shown.is_empty() {
                            div()
                                .flex()
                                .flex_1()
                                .items_center()
                                .justify_center()
                                .text_color(theme.muted_foreground)
                                .child(match query.trim() {
                                    "" if setup.as_ref().is_some_and(|setup| setup.members.is_empty()) => {
                                        "No accounts in this setup yet. Use Choose accounts in Edit."
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
                                        .gap(layout.space(2.0))
                                        .px(layout.space(3.0))
                                        .pt(layout.space(1.0))
                                        .pb(if selected.is_empty() {
                                            layout.space(3.0)
                                        } else {
                                            BAR_ROOM
                                        })
                                        .children(shown),
                                )
                                .child(Scrollbar::new(
                                    "accounts-scrollbar",
                                    self.grid_scroll.clone(),
                                ))
                                .into_any_element()
                        }
                    }
                };
                v_flex()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(header)
                    .children(self.notice.clone().map(|notice| {
                        div()
                            .flex_none()
                            .px(layout.space(3.0))
                            .pb(layout.space(2.0))
                            .child(Alert::new("notice", Tone::Danger, notice))
                    }))
                    .child(grid)
                    .when(!selected.is_empty(), |content| {
                        content.child(self.selection_bar(&theme, selected, &frame, window, cx))
                    })
                    .into_any_element()
            }
        };
        let form = self.form.as_ref().map(|form| self.form(&theme, form, cx));
        let confirmation = self
            .confirm
            .as_ref()
            .map(|confirm| self.confirmation(confirm, cx));
        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .window_surface(&theme)
            .key_context(format!("{FOCUS_SCOPE} {}", super::CONTEXT).as_str())
            .id("manager")
            .role(Role::Group)
            .track_focus(&self.focus)
            .on_action(|_: &CloseSession, window, _| window.remove_window())
            .when(!self.dialog_open(), |root| {
                root.on_action(cx.listener(|this, _: &AddAccount, _, cx| this.edit(None, cx)))
                    .on_action(
                        cx.listener(|this, _: &NewSetup, window, cx| this.new_setup(window, cx)),
                    )
                    .on_action(cx.listener(|this, _: &StartSetup, _, cx| this.start_current(cx)))
                    .on_action(
                        cx.listener(|this, _: &text_field::SelectAll, _, cx| this.select_shown(cx)),
                    )
                    .on_action(cx.listener(|this, _: &ClearSelection, window, cx| {
                        if this.editing.is_some() {
                            this.finish_edit(window, cx);
                        } else {
                            this.clear_selection(cx);
                        }
                    }))
                    .on_action(cx.listener(|this, _: &crate::app_menu::Find, window, cx| {
                        this.search.read(cx).focus_handle(cx).focus(window, cx)
                    }))
            })
            .on_key_down(cx.listener(Self::key_down))
            .child(mira_ui::nav::pointer_probe())
            .child(crate::quit::Overlay)
            .child(title_bar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar)
                    .child(content),
            )
            .children(form)
            .children(confirmation)
    }
}
