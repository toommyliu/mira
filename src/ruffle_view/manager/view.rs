use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

use gpui::{
    AnyElement, App, ClickEvent, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Toggled, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, StyledExt as _, Theme, Tone,
    components::Empty,
    h_flex,
    primitives::{
        Alert, Align, Button, ContextMenu, Dialog, DropdownMenu, Field, Flow, HoverCard, Icon,
        Marquee, MenuItem, Placement, Pose, Scrollbar, SegmentedProgress, Side, Signal, Sortable,
        Tab, Tabs,
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
        account_card::{About, Action, Card, Job, Line, Mood, Presence, Status, ago, card, status},
        client::{CloseSession, bar_title, drag_area, window_bar},
        script_names,
        scripts::{fit_path_to, home_relative},
    },
    setups::Setup,
};

const CARD_GROUP: &str = "account-card";
const NO_SERVER: &str = "Choose a server";
const SCRIPT_MENU_WIDTH: gpui::Pixels = px(224.0);
const SCRIPT_PICKER_WIDTH: gpui::Pixels = px(260.0);
const CHECK_GUTTER: gpui::Pixels = px(32.0);
const REMOVE_SLOT: gpui::Pixels = px(18.0);
const SCRIPT_ROW_INSET: gpui::Pixels = px(24.0);
const SERVER_MENU_WIDTH: gpui::Pixels = px(280.0);
const BAR_ROOM: gpui::Pixels = px(64.0);
const NARROW_BAR: gpui::Pixels = px(640.0);
const TAB_CHROME: gpui::Pixels = px(28.0);
const TAB_LIST_INSET: gpui::Pixels = px(6.0);
const BUTTON_CHROME: gpui::Pixels = px(18.0);
const START_CHROME: gpui::Pixels = px(20.0);
const START_MENU: gpui::Pixels = px(27.0);
const START_KEYS: &str = "⌘⏎";
const SCRIPT_NAME_ROOM: gpui::Pixels = px(64.0);
const FILTER_MENU_WIDTH: gpui::Pixels = px(176.0);

fn mood_color(theme: &Theme, mood: Mood) -> Hsla {
    match mood {
        Mood::Quiet => theme.muted_foreground,
        Mood::Plain => theme.foreground,
        Mood::Warning => theme.warning,
        Mood::Danger => theme.destructive,
    }
}

fn status_color(theme: &Theme, status: Status) -> Hsla {
    match status {
        Status::Failed => theme.destructive,
        Status::NeedsYou => theme.warning,
        _ => theme.muted_foreground,
    }
}

fn fact_icon(theme: &Theme, glyph: Icon, label: &'static str) -> impl IntoElement + use<> {
    div()
        .id(label)
        .role(Role::Image)
        .aria_label(label)
        .flex_none()
        .child(icon(glyph, theme.muted_foreground).size(px(12.0)))
}

fn card_height(theme: &Theme) -> gpui::Pixels {
    let typography = theme.typography;
    theme.space(3.0) * 2.0
        + px(2.0)
        + theme.space(1.0)
        + typography.body_line
        + (typography.secondary_line + px(2.0)) * 3.0
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
    let item = |server: String| {
        let pick = pick.clone();
        let checked = current == Some(Some(server.as_str()));
        MenuItem::action(server.clone(), move |window, cx| {
            pick(Some(server.clone()), window, cx)
        })
        .checked(checked)
    };
    let mut items: Vec<MenuItem> = servers
        .iter()
        .map(|server| item(server.name.clone()).end_slot(server_stats(server, theme)))
        .collect();
    if let Some(saved) = current
        .flatten()
        .filter(|saved| !servers.iter().any(|server| server.name == *saved))
    {
        items.push(item(saved.to_owned()));
    }
    if items.is_empty() {
        items.push(MenuItem::action("No servers to show", |_, _| {}).disabled(true));
    }
    items
}

fn server_stats(server: &net::Server, theme: &Theme) -> Div {
    let layout = theme.layout;
    let typography = theme.typography;
    let (players, color) = if !server.online {
        (SharedString::from("Offline"), theme.muted_foreground)
    } else if server.players >= server.capacity {
        ("Full".into(), theme.foreground)
    } else {
        (server.players.to_string().into(), theme.muted_foreground)
    };
    let ping = server.ping.map(|ping| ping.as_millis());
    h_flex()
        .flex_none()
        .gap(layout.space(3.0))
        .ml(layout.space(4.0))
        .text_size(typography.secondary)
        .font_features(tabular())
        .text_color(theme.muted_foreground)
        .when(server.members, |stats| stats.child("Members"))
        .child(div().text_color(color).child(players))
        .child(
            div()
                .w(typography.secondary * 3.5)
                .flex()
                .justify_end()
                .whitespace_nowrap()
                .children(ping.map(|ping| format!("{ping} ms"))),
        )
        .child(Signal::new(
            format!("{} connection", server.name),
            match ping {
                _ if !server.online => 0,
                Some(0..100) => 3,
                Some(100..200) => 2,
                Some(_) => 1,
                None => 0,
            },
        ))
}

type ShownFilters = Rc<[(Option<Filter>, usize)]>;

fn filter_name(filter: Option<Filter>) -> &'static str {
    filter.map_or("All", Filter::name)
}

struct Measure<'a> {
    window: &'a Window,
    theme: &'a Theme,
}

impl Measure<'_> {
    fn text(&self, text: &str, size: gpui::Pixels, weight: FontWeight) -> gpui::Pixels {
        let run = gpui::TextRun {
            len: text.len(),
            font: gpui::Font {
                weight,
                ..gpui::font(self.theme.font_family.clone())
            },
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        self.window
            .text_system()
            .shape_line(text.to_owned().into(), size, &[run], None)
            .width
            .ceil()
    }

    fn label(&self, text: &str) -> gpui::Pixels {
        self.text(text, self.theme.typography.body, FontWeight::MEDIUM)
    }

    fn button(&self, label: &str) -> gpui::Pixels {
        self.label(label) + BUTTON_CHROME
    }

    fn tabs(&self, shown: &[(Option<Filter>, usize)]) -> gpui::Pixels {
        let typography = self.theme.typography;
        shown
            .iter()
            .map(|(filter, count)| {
                self.label(filter_name(*filter))
                    + self.text(&count.to_string(), typography.secondary, FontWeight::NORMAL)
                    + TAB_CHROME
            })
            .fold(TAB_LIST_INSET, |total, tab| total + tab)
    }

    fn filter_button(
        &self,
        shown: &[(Option<Filter>, usize)],
        current: Option<Filter>,
    ) -> gpui::Pixels {
        let total = shown.first().map_or(0, |(_, count)| *count);
        let label = current.map_or_else(
            || plural(total, "account"),
            |filter| filter.name().to_owned(),
        );
        self.text(&label, self.theme.typography.body, FontWeight::NORMAL) + px(36.0)
    }
}

struct StartLabel {
    verb: &'static str,
    name: SharedString,
    again: bool,
    start: Start,
    tooltip: String,
}

impl StartLabel {
    fn full(&self) -> String {
        let Self { verb, name, .. } = self;
        if self.again {
            format!("{verb} {name} again")
        } else {
            format!("{verb} {name}")
        }
    }

    fn short(&self) -> String {
        if self.again {
            format!("{} again", self.verb)
        } else {
            self.verb.to_owned()
        }
    }
}

#[derive(Clone, Copy)]
struct StartFit {
    compact: bool,
    named: bool,
    min_width: gpui::Pixels,
}

#[derive(Clone, Copy)]
enum Recents<'a> {
    Removable,
    Listed,
    Checked(Option<&'a Path>),
}

fn script_items(
    this: &WeakEntity<Manager>,
    scripts: &[PathBuf],
    header: &'static str,
    recents: Recents,
    window: &Window,
    cx: &App,
    run: Pick,
) -> Vec<MenuItem> {
    let theme = cx.theme();
    let room = match recents {
        Recents::Removable => SCRIPT_MENU_WIDTH - REMOVE_SLOT,
        Recents::Listed => SCRIPT_MENU_WIDTH - CHECK_GUTTER,
        Recents::Checked(_) => SCRIPT_PICKER_WIDTH - CHECK_GUTTER,
    } - SCRIPT_ROW_INSET;
    let item = |label: SharedString, script: Option<PathBuf>| {
        let this = this.clone();
        let run = run.clone();
        MenuItem::action(label, move |window, cx| {
            this.update(cx, |manager, cx| run(manager, script.clone(), window, cx))
                .ok();
        })
    };
    let mut listed: Vec<&Path> = scripts.iter().map(PathBuf::as_path).collect();
    if let Recents::Checked(Some(current)) = recents
        && !listed.contains(&current)
    {
        listed.insert(0, current);
    }
    let mut items = Vec::new();
    if !listed.is_empty() {
        items.push(MenuItem::header(header));
        items.extend(listed.into_iter().map(|script| {
            let folder = script.parent().map(home_relative).unwrap_or_default();
            let row = item(script_names::name(script, cx), Some(script.to_path_buf()))
                .description(fit_path_to(&folder, room, window, theme));
            match recents {
                Recents::Removable => {
                    let hidden = script.to_path_buf();
                    let refresh = this.clone();
                    row.dismiss("Remove from recent", move |_, cx| {
                        crate::settings::hide_recent_script(hidden.clone(), cx);
                        refresh.update(cx, |_, cx| cx.notify()).ok();
                    })
                }
                Recents::Listed => row,
                Recents::Checked(current) => row.checked(current == Some(script)),
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
                Recents::Removable,
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
            .when(self.accounts.is_ok(), |bar| {
                bar.child(
                    div()
                        .min_w(px(120.0))
                        .w(px(200.0))
                        .mr(layout.space(1.5))
                        .child(self.search.clone()),
                )
            })
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
        let marked = {
            let theme = theme.clone();
            move |tab: Tab, statuses: &[Status]| match attention(&theme, statuses) {
                Some(mark) => tab.end_slot(mark),
                None => tab,
            }
        };
        let all_statuses: Rc<[Status]> = base(&mut accounts.iter().map(Account::id)).into();
        let all_marked = marked.clone();
        let all = Tabs::new("all-accounts", "Accounts", 1, move |_| {
            all_marked(Tab::new("All accounts"), &all_statuses)
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
                .child(Marquee::new(("setup-script", index), description.clone()));
            marked(
                Tab::new(row.name.clone())
                    .description(description)
                    .child(div().truncate().child(row.name.clone()))
                    .detail(caption)
                    .menu(move |_, _| setup_menu(&this, &menu_name)),
                &row.statuses,
            )
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
                    .pr(layout.space(2.5))
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

    fn shown_filters(&self, filters: &'static [Filter], statuses: &[Status]) -> ShownFilters {
        std::iter::once((None, statuses.len()))
            .chain(
                filters
                    .iter()
                    .map(|filter| (Some(*filter), count(statuses, *filter)))
                    .filter(|(filter, count)| *count > 0 || self.filter == *filter),
            )
            .collect()
    }

    fn filters(
        &self,
        theme: &Theme,
        shown: ShownFilters,
        statuses: &[Status],
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ink = status_color(
            theme,
            if any_failed(statuses) {
                Status::Failed
            } else {
                Status::NeedsYou
            },
        );
        let quiet = theme.muted_foreground;
        let selected = shown
            .iter()
            .position(|(filter, _)| *filter == self.filter)
            .unwrap_or(0);
        let tally = move |filter: Option<Filter>, count: usize| {
            div()
                .font_features(tabular())
                .text_color(if filter == Some(Filter::Needs) {
                    ink
                } else {
                    quiet
                })
                .child(count.to_string())
        };
        let this = cx.weak_entity();
        if compact {
            let (current, total) = (shown[selected].0, statuses.len());
            let all = move |filter: Option<Filter>| filter.map_or("All accounts", Filter::name);
            return DropdownMenu::new("status-filter")
                .width(FILTER_MENU_WIDTH)
                .trigger(
                    Button::new(
                        "status-filter-trigger",
                        current.map_or_else(
                            || plural(total, "account"),
                            |filter| filter.name().to_owned(),
                        ),
                    )
                    .variant(Variant::Outline)
                    .shows_value(),
                )
                .items(move |_, _| {
                    shown
                        .iter()
                        .map(|(filter, count)| {
                            let this = this.clone();
                            let filter = *filter;
                            MenuItem::action(all(filter), move |_, cx| {
                                this.update(cx, |manager, cx| manager.set_filter(filter, cx))
                                    .ok();
                            })
                            .end_slot(tally(filter, *count))
                            .checked(filter == current)
                        })
                        .collect()
                })
                .into_any_element();
        }
        let tabs = shown.clone();
        Tabs::new("status-filter", "Show", shown.len(), move |index| {
            let (filter, count) = tabs[index];
            Tab::new(filter_name(filter)).end_slot(tally(filter, count))
        })
        .selected(selected)
        .on_select(move |index, _, cx| {
            let filter = shown[index].0;
            this.update(cx, |manager, cx| manager.set_filter(filter, cx))
                .ok();
        })
        .into_any_element()
    }

    fn filter_slot(&self, filters: AnyElement) -> Div {
        div()
            .flex_none()
            .debug_selector(|| "toolbar-filters".into())
            .child(filters)
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
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let layout = theme.layout;
        let measure = Measure { window, theme };
        let shown = self.shown_filters(&ROSTER_FILTERS, statuses);
        let add = measure.button("Add account") + px(16.0);
        let compact =
            self.toolbar_room(window, theme) < measure.tabs(&shown) + layout.space(2.0) + add;
        self.header_row(theme)
            .child(self.filter_slot(self.filters(theme, shown, statuses, compact, cx)))
            .child(
                div()
                    .flex_none()
                    .ml_auto()
                    .debug_selector(|| "toolbar-end".into())
                    .child(
                        Button::new("add-account", "Add account")
                            .start_slot(icon(Icon::Plus, theme.secondary_foreground).size(px(14.0)))
                            .disabled(self.accounts.is_err())
                            .on_click(cx.listener(|this, _, _, cx| this.edit(None, cx))),
                    ),
            )
    }

    fn toolbar_room(&self, window: &Window, theme: &Theme) -> gpui::Pixels {
        let layout = theme.layout;
        window.viewport_size().width - layout.sidebar - layout.space(3.0) * 2.0
    }

    fn watch_header(
        &self,
        theme: &Theme,
        setup: &Setup,
        frame: &Frame,
        statuses: &[Status],
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let started = cx.global::<Runs>().started(&setup.name);
        let complete = !statuses.is_empty() && count(statuses, Filter::Done) == statuses.len();
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
        let layout = theme.layout;
        let measure = Measure { window, theme };
        let shown = self.shown_filters(&RUN_FILTERS, statuses);
        let gap = layout.space(2.0);
        let start = self.start_label(setup, cx);
        let short = start
            .as_ref()
            .map_or_else(|| "Start".to_owned(), StartLabel::short);
        let min_width = measure.label(&short)
            + if start.is_some() {
                layout.space(1.0) + measure.label(START_KEYS) + START_CHROME
            } else {
                px(36.0)
            };
        let commands = gap * 2.0
            + measure.button("Edit")
            + if can_stop {
                gap + measure.button("Stop")
            } else {
                px(0.0)
            }
            + if start.is_some() { START_MENU } else { px(0.0) };
        let room = self.toolbar_room(window, theme) - commands;
        let compact = !statuses.is_empty() && room - measure.tabs(&shown) < min_width;
        let filters = if statuses.is_empty() {
            px(0.0)
        } else if compact {
            measure.filter_button(&shown, self.filter)
        } else {
            measure.tabs(&shown)
        };
        let named = start.as_ref().is_some_and(|start| {
            let name = measure.label(&start.name) + measure.label(" ");
            room - filters - min_width >= name.min(SCRIPT_NAME_ROOM)
        });
        let fit = StartFit {
            compact,
            named,
            min_width,
        };
        self.header_row(theme)
            .when(!statuses.is_empty(), |row| {
                row.child(self.filter_slot(self.filters(theme, shown, statuses, fit.compact, cx)))
            })
            .child(
                h_flex()
                    .ml_auto()
                    .min_w_0()
                    .gap(gap)
                    .debug_selector(|| "toolbar-end".into())
                    .child(
                        Button::new("edit-setup", "Edit")
                            .variant(Variant::Ghost)
                            .on_click(cx.listener({
                                let name = setup.name.clone();
                                move |this, _, window, cx| this.edit_setup(&name, window, cx)
                            })),
                    )
                    .when(can_stop, |row| {
                        row.child(
                            Button::new("stop-setup", "Stop")
                                .variant(Variant::Ghost)
                                .on_click(cx.listener(|this, _, _, cx| this.stop_setup(cx))),
                        )
                    })
                    .child(self.start_controls(setup, &frame.scripts, when, start, fit, cx)),
            )
    }

    fn start_label(&self, setup: &Setup, cx: &App) -> Option<StartLabel> {
        let script = setup.script.as_deref()?;
        let name = script_names::name(script, cx);
        let done = self.done(setup, cx).len();
        Some(match self.primary_start(setup, cx) {
            Some(Start::Fresh) => StartLabel {
                verb: "Start",
                again: true,
                start: Start::Fresh,
                tooltip: format!("Runs {name} on every account again"),
                name,
            },
            _ if done > 0 => StartLabel {
                verb: "Continue",
                again: false,
                start: Start::Continue,
                tooltip: format!("Skips the {done} already done"),
                name,
            },
            _ => StartLabel {
                verb: "Start",
                again: false,
                start: Start::Continue,
                tooltip: home_relative(script).to_string(),
                name,
            },
        })
    }

    fn start_controls(
        &self,
        setup: &Setup,
        scripts: &[PathBuf],
        when: Option<String>,
        label: Option<StartLabel>,
        fit: StartFit,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let this = cx.weak_entity();
        let name = setup.name.clone();
        let current = setup.script.clone();
        let launch = setup.launch;
        let started = cx.global::<Runs>().started(&setup.name).is_some();
        let scripts = scripts.to_vec();
        let empty = setup.members.is_empty();
        let items = move |window: &mut Window, cx: &mut App| {
            let mut items = Vec::new();
            if let Some(when) = &when {
                items.push(MenuItem::header(when.clone()));
            }
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
            if current.is_none() {
                let start_name = name.clone();
                let pick: Pick = Rc::new(move |manager, script, window, cx| match script {
                    Some(script) => manager.start_with(&start_name, script, cx),
                    None => manager.choose_start(start_name.clone(), window, cx),
                });
                if scripts.is_empty() {
                    items.push(MenuItem::header("Start with"));
                }
                items.extend(script_items(
                    &this,
                    &scripts,
                    "Start with",
                    Recents::Listed,
                    window,
                    cx,
                    pick,
                ));
                items.push(MenuItem::separator());
            }
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
            items.push(MenuItem::header("Window"));
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
                    .end_slot(small_icon(Icon::ChevronDown, theme.primary_foreground)),
                None => Button::icon(
                    "start-with-trigger",
                    Icon::ChevronDown,
                    "More ways to start",
                )
                .variant(Variant::Primary)
                .joined(Joined::Previous),
            };
            DropdownMenu::new("start-with")
                .width(SCRIPT_MENU_WIDTH)
                .max_height(px(360.0))
                .align(Align::End)
                .trigger(trigger.disabled(empty))
                .items(items.clone())
        };
        let Some(label) = label else {
            return menu(Some("Start")).into_any_element();
        };
        let start = label.start;
        let setup_name = setup.name.clone();
        h_flex()
            .min_w_0()
            .flex_shrink(1000.0)
            .child(
                Button::new("start-setup", label.full())
                    .variant(Variant::Primary)
                    .joined(Joined::Next)
                    .flex_shrink(1.0)
                    .min_w(fit.min_width)
                    .tooltip(label.tooltip.clone())
                    .disabled(empty)
                    .child(div().truncate().child(if fit.named {
                        label.full()
                    } else {
                        label.short()
                    }))
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
            .width(SCRIPT_PICKER_WIDTH)
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
                    Recents::Checked(current.as_deref()),
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
        let presence = frame.presences.get(&id);
        let open = presence.is_some_and(Presence::opened);
        let items = account_menu(
            cx.weak_entity(),
            account.clone(),
            presence.cloned(),
            frame.scripts.clone(),
            self.setup().is_some(),
        );
        let more_items = items.clone();
        let tint = theme
            .primary
            .opacity(if theme.dark_mode { 0.1 } else { 0.05 });
        let surface = if checked {
            theme.background.blend(tint)
        } else {
            theme.card
        };
        let more = DropdownMenu::new(SharedString::from(format!(
            "card-more-{}",
            account.username
        )))
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
        let reveal = |slot: Div| {
            if focused {
                slot
            } else {
                slot.invisible()
                    .group_hover(CARD_GROUP, |slot| slot.visible())
            }
        };
        let fix = card.status.needs_you();
        let action = card.action.map(|action| {
            let target = account.clone();
            Button::new(
                SharedString::from(format!("card-action-{}", account.username)),
                action.label(),
            )
            .variant(if fix {
                Variant::Outline
            } else {
                Variant::Ghost
            })
            .size(Size::XSmall)
            .on_click(cx.listener(move |this, _, _, cx| this.perform(action, &target, cx)))
        });
        let (pinned, quick) = if fix { (action, None) } else { (None, action) };
        let text = |line: &Line| {
            div()
                .min_w_0()
                .truncate()
                .text_color(mood_color(theme, line.mood))
                .child(line.text.clone())
        };
        let aside = |text: SharedString| {
            div()
                .flex_none()
                .font_features(tabular())
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let job = match &card.job {
            Job::Loading(Some(percent)) => h_flex()
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
                .child(aside(format!("{percent}%").into())),
            Job::Loading(None) => h_flex().child(aside("Loading".into())),
            Job::Line(line) => h_flex()
                .gap(layout.space(2.0))
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
                })
                .children(line.time.clone().map(aside)),
        };
        let status = div()
            .flex_none()
            .text_caption(typography)
            .text_color(status_color(theme, card.status))
            .map(|status| {
                if focused {
                    status.invisible()
                } else {
                    status.group_hover(CARD_GROUP, |status| status.invisible())
                }
            })
            .child(card.status.label());
        let line = typography.secondary_line + px(2.0);
        let blank =
            matches!(&card.job, Job::Line(line) if line.text.is_empty() && line.time.is_none());
        let fact = |glyph: Icon, label: &'static str, body: Div| {
            h_flex()
                .min_w_0()
                .h(line)
                .gap(layout.space(1.5))
                .child(fact_icon(theme, glyph, label))
                .child(body)
        };
        let clicked = id.clone();
        let element = v_flex()
            .id(SharedString::from(format!("account-{}", account.username)))
            .group(CARD_GROUP)
            .relative()
            .h_full()
            .min_w_0()
            .min_h(card_height(theme))
            .gap(layout.space(1.0))
            .p(layout.space(3.0))
            .rounded(theme.radius.control)
            .border_1()
            .map(|card| {
                if checked {
                    card.border_color(theme.ring).bg(tint)
                } else {
                    card.border_color(theme.surface_edge()).bg(theme.card)
                }
            })
            .role(Role::CheckBox)
            .aria_label(account.name.clone())
            .aria_description(card.state.clone())
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
                    .h(typography.body_line)
                    .gap(layout.space(2.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .when(!open, |name| name.text_color(theme.muted_foreground))
                            .child(account.name.clone()),
                    )
                    .child(status),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .text_size(typography.secondary)
                    .line_height(line)
                    .flex_1()
                    .when(!blank, |facts| {
                        let (glyph, label) = match card.about() {
                            About::Script => (Icon::FileCode, "Script"),
                            About::Login => (Icon::LogIn, "Login"),
                        };
                        facts.child(fact(glyph, label, job.flex_1().min_w_0()))
                    })
                    .when(!card.place.text.is_empty(), |facts| {
                        facts.child(fact(Icon::MapPin, "Location", text(&card.place)))
                    })
                    .when(card.who.is_some() || pinned.is_some(), |facts| {
                        facts.child(
                            h_flex()
                                .min_w_0()
                                .h(line)
                                .gap(layout.space(2.0))
                                .when(card.who.is_none(), |row| row.mt_auto())
                                .child(div().flex_1().min_w_0().children(card.who.clone().map(
                                    |who| {
                                        fact(
                                            Icon::User,
                                            "Character",
                                            div().min_w_0().truncate().child(who),
                                        )
                                    },
                                )))
                                .children(pinned),
                        )
                    }),
            )
            .child(
                reveal(
                    h_flex()
                        .absolute()
                        .top(layout.space(3.0))
                        .right(layout.space(2.0))
                        .gap(px(2.0))
                        .pl(layout.space(5.0))
                        .bg(gpui::linear_gradient(
                            90.0,
                            gpui::linear_color_stop(surface.opacity(0.0), 0.0),
                            gpui::linear_color_stop(surface, 0.4),
                        )),
                )
                .children(quick)
                .child(more),
            )
            .when(focused, |card| {
                card.child(theme.ring_around(theme.ring, theme.radius.control, px(1.0)))
            });
        ContextMenu::new(
            SharedString::from(format!("account-menu-{}", account.username)),
            "Account actions",
        )
        .items(move |window, cx| items(window, cx))
        .child(element)
        .into_any_element()
    }

    fn perform(&mut self, action: Action, target: &Account, cx: &mut Context<Self>) {
        match action {
            Action::EditLogin => self.edit(Some(target), cx),
            Action::Restart => self.restart(target, cx),
            Action::LogIn | Action::Launch => self.launch(vec![target.clone()], cx),
            Action::Show => self.show_account(target.clone(), cx),
            Action::Stop => self.stop(vec![target.clone()], cx),
        }
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
                .child(Empty::new(
                    "No accounts in this setup",
                    "Use Choose accounts to add some.",
                ))
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
                let height = typography.secondary_line + px(2.0);
                let line = |glyph: Icon, label: &'static str, value: SharedString, quiet: bool| {
                    h_flex()
                        .min_w_0()
                        .h(height)
                        .gap(theme.space(1.5))
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
                                .text_size(typography.secondary)
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
                    .min_h(
                        theme.space(3.0) * 2.0
                            + theme.space(1.0)
                            + typography.body_line
                            + height * 2.0,
                    )
                    .gap(theme.space(1.0))
                    .p(theme.space(3.0))
                    .rounded(theme.radius.control)
                    .bg(theme.card)
                    .cursor_grab()
                    .when(pose == Pose::Lifted, |card| {
                        card.shadow(theme.popup_shadow())
                    })
                    .child(theme.surface_ring(theme.radius.control))
                    .role(Role::ListItem)
                    .aria_label(format!("{}, {} of {count}", account.name, index + 1))
                    .child(
                        h_flex()
                            .min_w_0()
                            .h(typography.body_line)
                            .gap(theme.space(1.5))
                            .child(div().flex_none().child(
                                icon(Icon::GripVertical, theme.muted_foreground).size(px(12.0)),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(account.name.clone()),
                            ),
                    )
                    .child(
                        v_flex()
                            .children(account.server.clone().map(|server| {
                                line(Icon::Server, "Login server", server.into(), true)
                            }))
                            .children(
                                card.who
                                    .clone()
                                    .map(|who| line(Icon::User, "Character", who, false)),
                            ),
                    )
                    .when(current, |card| {
                        card.child(theme.ring_around(theme.ring, theme.radius.control, px(1.0)))
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
                script_items(
                    &this,
                    &scripts,
                    "Recent",
                    Recents::Removable,
                    window,
                    cx,
                    run.clone(),
                )
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
            .trigger(
                Button::new(
                    "account-server-trigger",
                    current.clone().unwrap_or_else(|| NO_SERVER.into()),
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
                                .when(current.is_none(), |label| {
                                    label
                                        .font_weight(FontWeight::NORMAL)
                                        .text_color(theme.muted_foreground)
                                })
                                .child(current.clone().unwrap_or_else(|| NO_SERVER.into())),
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
                    Field::new("Login server", server).end_slot(
                        h_flex()
                            .h(typography.secondary_line)
                            .gap(layout.space(1.5))
                            .child(
                                div()
                                    .font_weight(FontWeight::NORMAL)
                                    .text_color(theme.muted_foreground)
                                    .child(self.servers.note()),
                            )
                            .child(refresh),
                    ),
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
        let sidebar = self
            .accounts
            .is_ok()
            .then(|| self.sidebar(&theme, &frame.presences, cx));
        let selected = if editing {
            Vec::new()
        } else {
            self.checked_accounts()
        };
        let content = match &self.accounts {
            Err(unloaded) => {
                let path = unloaded.path.clone();
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .child(
                        Empty::new("Couldn't load your accounts", unloaded.error.clone())
                            .action(
                                Button::new("accounts-retry", "Try again")
                                    .variant(Variant::Primary)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.reload_accounts(cx)),
                                    ),
                            )
                            .action(
                                Button::new("accounts-show", "Show in Finder")
                                    .on_click(move |_, _, cx| cx.reveal_path(&path)),
                            ),
                    )
                    .into_any_element()
            }
            Ok(all) if all.list().is_empty() => self.empty(cx).into_any_element(),
            Ok(_) => {
                let header = match &setup {
                    Some(setup) if editing => self
                        .edit_header(&theme, setup, &frame.scripts, cx)
                        .into_any_element(),
                    Some(setup) => self
                        .watch_header(&theme, setup, &frame, &statuses, window, cx)
                        .into_any_element(),
                    None => self
                        .roster_header(&theme, &statuses, window, cx)
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
                            let unfilled = setup.as_ref().filter(|setup| setup.members.is_empty());
                            let empty =
                                match (query.trim(), unfilled) {
                                    ("", Some(setup)) => Empty::new(
                                        "No accounts in this setup",
                                        "Edit the setup to choose its accounts.",
                                    )
                                    .action(Button::new("edit-empty-setup", "Edit").on_click(
                                        cx.listener({
                                            let name = setup.name.clone();
                                            move |this, _, window, cx| {
                                                this.edit_setup(&name, window, cx)
                                            }
                                        }),
                                    )),
                                    ("", None) => Empty::new(
                                        "No accounts here",
                                        "No account has this status right now.",
                                    ),
                                    _ => Empty::new("No accounts match", "Try a different search."),
                                };
                            div()
                                .flex()
                                .flex_1()
                                .items_center()
                                .justify_center()
                                .child(empty)
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
            .when(self.accounts.is_ok() && !self.dialog_open(), |root| {
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
                    .on_action(text_field::Find::in_search(&self.search))
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
                    .children(sidebar)
                    .child(content),
            )
            .children(form)
            .children(confirmation)
    }
}
