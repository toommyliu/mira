use gpui::{
    AnyElement, App, AppContext as _, Bounds, ClipboardItem, Context, Div, Entity, EntityId,
    FocusHandle, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, actions, div,
    prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, Theme, Tone,
    components::Empty,
    h_flex,
    primitives::{
        Badge, Button, Icon, Input, InputEvent, List, ListItem, Select, Spinner, VirtualList,
        button::{Size, Variant},
    },
    theme::{LINE_BODY, LINE_SMALL, TEXT_BODY, TEXT_SMALL, TEXT_TITLE, tabular},
};

use super::{
    BAR_HEIGHT, Client,
    client::{CloseSession, bar_title, drag_area, window_bar},
    shown_tab::{self, ShownTab},
};
use crate::{
    automation::{quests::Registered, register_items},
    bridge::CallError,
    domain::{
        grabber::{Act, Action, Entry, Group, Job, Line, Load, LoadKind, Reply, Snapshot, Source},
        inventory::ItemQuery,
    },
    ruffle::Command,
};

actions!(mira, [OpenLoaderGrabber]);

const WIDTH: f32 = 880.0;
const HEIGHT: f32 = 640.0;
const SIDEBAR: Pixels = px(164.0);
const INSPECTOR: Pixels = px(300.0);
const LOADER_ROW: Pixels = px(56.0);
const SEARCH_ROW: Pixels = px(48.0);
const HEADING_ROW: Pixels = px(28.0);
const FOOTER: Pixels = px(44.0);
const MAX_SEARCH: usize = 64;

struct Open(WindowHandle<LoaderGrabber>);

impl gpui::Global for Open {}

pub(crate) fn open(client: Entity<Client>, cx: &mut App) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0)
        && handle
            .update(cx, |grabber, window, cx| {
                window.activate_window();
                grabber.show_active(cx);
            })
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(WIDTH), px(HEIGHT)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Loader/grabber".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(12.0))),
        }),
        app_owns_titlebar_drag: true,
        is_minimizable: false,
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let grabber = cx.new(|cx| LoaderGrabber::new(client, cx));
        window.focus(&grabber.read(cx).focus.clone(), cx);
        grabber
    }) {
        Ok(handle) => cx.set_global(Open(handle)),
        Err(error) => tracing::error!(%error, "couldn't open the loader/grabber"),
    }
}

enum Problem {
    NotInGame,
    Failed(SharedString),
}

enum Drops {
    Reading,
    Ready(Group),
    Unavailable,
}

pub(crate) struct LoaderGrabber {
    client: Entity<Client>,
    focus: FocusHandle,
    shown: Option<ShownTab>,
    kind: LoadKind,
    ids: Entity<Input>,
    search: Entity<Input>,
    source: Source,
    snapshot: Option<Snapshot>,
    problem: Option<Problem>,
    selected: Option<i64>,
    drops: Option<(f64, Drops)>,
    notice: Option<SharedString>,
    reading: Option<Task<()>>,
    loading: Option<Task<()>>,
    acting: Option<Task<()>>,
    reading_drops: Option<Task<()>>,
    _ids_events: Subscription,
    _subscriptions: [Subscription; 2],
}

impl LoaderGrabber {
    fn new(client: Entity<Client>, cx: &mut Context<Self>) -> Self {
        let kind = LoadKind::Shop;
        let (ids, ids_events) = ids_input(kind, "", cx);
        let search = cx.new(|cx| {
            Input::new("Search", cx)
                .placeholder("Search names and IDs")
                .search()
                .max_length(MAX_SEARCH)
        });
        let subscriptions = [
            cx.subscribe(&search, |_, _, event, cx| {
                if let InputEvent::Changed = event {
                    cx.notify();
                }
            }),
            cx.observe(&client, |this, _, cx| this.show(None, cx)),
        ];
        let mut grabber = Self {
            client,
            focus: cx.focus_handle(),
            shown: None,
            kind,
            ids,
            search,
            source: Source::Shop,
            snapshot: None,
            problem: None,
            selected: None,
            drops: None,
            notice: None,
            reading: None,
            loading: None,
            acting: None,
            reading_drops: None,
            _ids_events: ids_events,
            _subscriptions: subscriptions,
        };
        grabber.show_active(cx);
        grabber
    }

    fn show_active(&mut self, cx: &mut Context<Self>) {
        let active = shown_tab::active(&self.client, cx);
        self.show(active, cx);
    }

    fn show(&mut self, wanted: Option<EntityId>, cx: &mut Context<Self>) {
        let switched = shown_tab::show(
            &mut self.shown,
            &self.client,
            wanted,
            |this: &mut Self, cx| this.show(None, cx),
            |_, _| Vec::new(),
            cx,
        );
        if switched {
            self.notice = None;
            self.loading = None;
            self.acting = None;
            self.open_source(self.source, None, cx);
        }
    }

    fn send<T>(
        &self,
        job: impl FnOnce(Reply<T>) -> Job,
        cx: &mut Context<Self>,
    ) -> Option<async_channel::Receiver<Result<T, CallError>>> {
        let view = self.shown.as_ref()?.view.upgrade()?;
        let (reply, answer) = async_channel::bounded(1);
        view.update(cx, |view, _| {
            view.session.command(Command::Grabber(job(reply)))
        });
        Some(answer)
    }

    fn open_source(&mut self, source: Source, selected: Option<i64>, cx: &mut Context<Self>) {
        if source != self.source {
            self.search.update(cx, |search, cx| search.set_text("", cx));
        }
        self.source = source;
        self.snapshot = None;
        self.problem = None;
        self.selected = selected;
        self.drops = None;
        self.reading_drops = None;
        self.read(cx);
    }

    fn read(&mut self, cx: &mut Context<Self>) {
        let source = self.source;
        let Some(answer) = self.send(|reply| Job::Read { source, reply }, cx) else {
            self.reading = None;
            cx.notify();
            return;
        };
        self.reading = Some(cx.spawn(async move |this, cx| {
            let result = answer.recv().await;
            let _ = this.update(cx, |this, cx| {
                this.reading = None;
                match result {
                    Ok(Ok(Some(snapshot))) => this.take(snapshot, cx),
                    Ok(Ok(None)) => {
                        this.snapshot = None;
                        this.problem = Some(Problem::NotInGame);
                    }
                    Ok(Err(error)) => {
                        this.notice = Some(format!("Couldn't refresh: {error}").into());
                        this.problem = Some(Problem::Failed(error.to_string().into()));
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn take(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        self.problem = None;
        self.snapshot = Some(snapshot);
        self.drops = None;
        self.reading_drops = None;
        match self.selected.and_then(|key| self.entry(key)).cloned() {
            Some(entry) => self.follow_drops(&entry, cx),
            None => self.selected = None,
        }
    }

    fn entry(&self, key: i64) -> Option<&Entry> {
        self.snapshot
            .as_ref()?
            .entries
            .iter()
            .find(|entry| entry.key == key)
    }

    fn select(&mut self, key: i64, cx: &mut Context<Self>) {
        self.selected = Some(key);
        if let Some(entry) = self.entry(key).cloned() {
            self.follow_drops(&entry, cx);
        }
        cx.notify();
    }

    fn follow_drops(&mut self, entry: &Entry, cx: &mut Context<Self>) {
        let Some(mon_map_id) = entry.drops_from else {
            self.drops = None;
            self.reading_drops = None;
            return;
        };
        if self
            .drops
            .as_ref()
            .is_some_and(|(shown, _)| *shown == mon_map_id)
        {
            return;
        }
        let Some(answer) = self.send(|reply| Job::Drops { mon_map_id, reply }, cx) else {
            return;
        };
        self.drops = Some((mon_map_id, Drops::Reading));
        self.reading_drops = Some(cx.spawn(async move |this, cx| {
            let result = answer.recv().await;
            let _ = this.update(cx, |this, cx| {
                this.reading_drops = None;
                let drops = match result {
                    Ok(Ok(Some(group))) => Drops::Ready(group),
                    _ => Drops::Unavailable,
                };
                this.drops = Some((mon_map_id, drops));
                cx.notify();
            });
        }));
    }

    fn set_kind(&mut self, kind: LoadKind, cx: &mut Context<Self>) {
        if kind == self.kind {
            return;
        }
        let text = self.ids.read(cx).text().to_owned();
        let (ids, events) = ids_input(kind, &text, cx);
        self.kind = kind;
        self.ids = ids;
        self._ids_events = events;
        self.notice = None;
        cx.notify();
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading.is_some() {
            return;
        }
        let text = self.ids.read(cx).text().to_owned();
        let load = match self.kind.parse(&text) {
            Ok(load) => load,
            Err(problem) => {
                self.ids.update(cx, |input, cx| input.set_invalid(true, cx));
                self.notice = Some(problem.message().into());
                cx.notify();
                return;
            }
        };
        let described = describe(&load);
        let next = match &load {
            Load::Shop(_) => Some((Source::Shop, None)),
            Load::Quests(ids) => Some((Source::Quests, ids.first().map(|id| i64::from(*id)))),
            Load::HairShop(_) | Load::ArmorCustomizer => None,
        };
        let Some(answer) = self.send(|reply| Job::Load { load, reply }, cx) else {
            self.notice = Some("Open a tab first".into());
            cx.notify();
            return;
        };
        self.notice = Some(format!("Loading {described}…").into());
        self.loading = Some(cx.spawn(async move |this, cx| {
            let result = answer.recv().await;
            let _ = this.update(cx, |this, cx| {
                this.loading = None;
                let notice = match result {
                    Ok(Ok(true)) => match next {
                        Some((source, selected)) => {
                            this.open_source(source, selected, cx);
                            format!("Loaded {described}")
                        }
                        None => format!("Opened {described}"),
                    },
                    Ok(Ok(false)) => format!("Couldn't load {described}"),
                    Ok(Err(error)) => error.to_string(),
                    Err(_) => "The tab closed".into(),
                };
                this.notice = Some(notice.into());
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn act(&mut self, act: Act, cx: &mut Context<Self>) {
        let Some(automation) = self.shown.as_ref().map(|shown| shown.automation.clone()) else {
            return;
        };
        let action = match &act {
            Act::Register { quest_id } => {
                automation.update_quests(|state| {
                    state.register([Registered {
                        quest_id: *quest_id,
                        reward_item_id: None,
                    }])
                });
                self.notice =
                    Some(format!("Registered quest {quest_id} in this tab's environment").into());
                cx.notify();
                return;
            }
            Act::AddDrop { name } => {
                automation.update_drops(|state| {
                    register_items(&mut state.items, vec![ItemQuery::Name(name.clone())])
                });
                self.notice = Some(format!("Added {name} to this tab's drops").into());
                cx.notify();
                return;
            }
            Act::Game(action) => action.clone(),
        };
        if self.acting.is_some() {
            return;
        }
        let request = action.clone();
        let Some(answer) = self.send(
            |reply| Job::Act {
                action: request,
                reply,
            },
            cx,
        ) else {
            return;
        };
        self.notice = Some(format!("{}…", act.label()).into());
        self.acting = Some(cx.spawn(async move |this, cx| {
            let result = answer.recv().await;
            let _ = this.update(cx, |this, cx| {
                this.acting = None;
                let notice: SharedString = match result {
                    Ok(Ok(true)) => action.done().into(),
                    Ok(Ok(false)) => action.failed().into(),
                    Ok(Err(error)) => error.to_string().into(),
                    Err(_) => "The tab closed".into(),
                };
                this.notice = Some(notice);
                this.read(cx);
            });
        }));
        cx.notify();
    }

    fn copy(&mut self, text: String, what: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.notice = Some(format!("Copied {what}").into());
        cx.notify();
    }

    fn shown_entries(&self, cx: &App) -> Vec<Entry> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        let query = self.search.read(cx).text().trim().to_lowercase();
        snapshot
            .entries
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.name.to_lowercase().contains(&query)
                    || entry.id.contains(&query)
            })
            .cloned()
            .collect()
    }

    fn loader_row(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let this = cx.weak_entity();
        let kind = Select::new(
            "load-kind",
            "Load",
            LoadKind::ALL
                .iter()
                .map(|kind| SharedString::from(kind.name())),
            LoadKind::ALL.iter().position(|kind| *kind == self.kind),
        )
        .width(px(168.0))
        .on_select(move |index, _, cx| {
            let _ = this.update(cx, |this, cx| this.set_kind(LoadKind::ALL[index], cx));
        });
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .h(LOADER_ROW)
            .px(px(16.0))
            .border_b_1()
            .border_color(theme.hairline)
            .child(kind)
            .when(self.kind.takes_ids(), |row| {
                row.child(div().flex_1().min_w_0().child(self.ids.clone()))
            })
            .when(!self.kind.takes_ids(), |row| row.child(div().flex_1()))
            .child(
                Button::new(
                    "load",
                    if self.kind.takes_ids() {
                        "Load"
                    } else {
                        "Open"
                    },
                )
                .variant(Variant::Primary)
                .busy(self.loading.is_some())
                .disabled(self.shown.is_none())
                .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
            )
    }

    fn sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let count = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.entries.len());
        let current = self.source;
        let this = cx.weak_entity();
        div()
            .flex_none()
            .w(SIDEBAR)
            .p(px(12.0))
            .border_r_1()
            .border_color(theme.hairline)
            .child(
                List::new("grab-sources", "Sources", Source::ALL.len(), move |index| {
                    let source = Source::ALL[index];
                    let item = ListItem::new(source.name());
                    match count {
                        Some(count) if source == current => item.end_slot(count.to_string()),
                        _ => item,
                    }
                })
                .selected(Source::ALL.iter().position(|source| *source == self.source))
                .on_select(move |index, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        if this.source != Source::ALL[index] {
                            this.open_source(Source::ALL[index], None, cx);
                        }
                    });
                }),
            )
    }

    fn list_column(
        &self,
        theme: &Theme,
        entries: &[Entry],
        height: Pixels,
        cx: &mut Context<Self>,
    ) -> Div {
        let heading = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.heading.clone());
        let body = match (&self.snapshot, &self.problem) {
            (None, Some(problem)) => problem_note(problem).into_any_element(),
            (None, None) => div()
                .flex()
                .justify_center()
                .py(px(32.0))
                .child(Spinner::new("grab-reading"))
                .into_any_element(),
            (Some(snapshot), _) if snapshot.entries.is_empty() => {
                empty_note(self.source).into_any_element()
            }
            (Some(_), _) if entries.is_empty() => Empty::new(
                "No matches",
                format!(
                    "Nothing here matches “{}”.",
                    self.search.read(cx).text().trim()
                ),
            )
            .into_any_element(),
            (Some(_), _) => {
                let rows: std::rc::Rc<[Entry]> = entries.into();
                let selected = self
                    .selected
                    .and_then(|key| rows.iter().position(|entry| entry.key == key));
                let lookup = rows.clone();
                let keys = rows.clone();
                let this = cx.weak_entity();
                VirtualList::new(
                    "grab-entries",
                    self.source.name(),
                    rows.len(),
                    move |index| row(&lookup[index]),
                )
                .selected(selected)
                .height(height)
                .on_select(move |index, _, cx| {
                    let key = keys[index].key;
                    let _ = this.update(cx, |this, cx| this.select(key, cx));
                })
                .into_any_element()
            }
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .px(px(12.0))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(8.0))
                    .h(SEARCH_ROW)
                    .child(div().flex_1().min_w_0().child(self.search.clone()))
                    .child(
                        Button::icon("grab-refresh", Icon::RotateCcw, "Refresh")
                            .size(Size::Small)
                            .variant(Variant::Ghost)
                            .busy(self.reading.is_some())
                            .disabled(self.shown.is_none())
                            .tooltip("Read it again from the game")
                            .on_click(cx.listener(|this, _, _, cx| this.read(cx))),
                    ),
            )
            .when_some(heading, |column, heading| {
                column.child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .h(HEADING_ROW)
                        .truncate()
                        .text_size(TEXT_SMALL)
                        .text_color(theme.text_secondary)
                        .child(heading),
                )
            })
            .child(body)
    }

    fn inspector(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self.selected.and_then(|key| self.entry(key)).cloned() else {
            return div()
                .py(px(32.0))
                .text_center()
                .text_size(TEXT_SMALL)
                .line_height(LINE_SMALL)
                .text_color(theme.text_tertiary)
                .child("Pick something to see its details and actions.")
                .into_any_element();
        };
        let busy = self.acting.is_some();
        let acts = entry.acts.iter().enumerate().map(|(index, act)| {
            let act = act.clone();
            Button::new(("grab-act", index), act.label())
                .size(Size::Small)
                .variant(if index == 0 {
                    Variant::Primary
                } else {
                    Variant::Secondary
                })
                .disabled(busy && matches!(act, Act::Game(_)))
                .tooltip(tooltip(&act))
                .on_click(cx.listener(move |this, _, _, cx| this.act(act.clone(), cx)))
        });
        let (name, id, json) = (entry.name.clone(), entry.id.clone(), entry.json.clone());
        let id_label = entry.id_label;
        let copies = [
            Button::new("copy-name", "Copy name")
                .size(Size::Small)
                .variant(Variant::Ghost)
                .on_click(
                    cx.listener(move |this, _, _, cx| this.copy(name.clone(), "the name", cx)),
                ),
            Button::new("copy-id", format!("Copy {id_label}"))
                .size(Size::Small)
                .variant(Variant::Ghost)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.copy(id.clone(), &format!("the {id_label}"), cx)
                })),
            Button::new("copy-json", "Copy JSON")
                .size(Size::Small)
                .variant(Variant::Ghost)
                .on_click(
                    cx.listener(move |this, _, _, cx| this.copy(json.clone(), "the JSON", cx)),
                ),
        ];
        let drops = match &self.drops {
            Some((mon_map_id, drops)) if entry.drops_from == Some(*mon_map_id) => Some(drops),
            _ => None,
        };
        div()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(TEXT_TITLE)
                            .line_height(LINE_BODY)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(entry.name.clone()),
                    )
                    .when(!entry.badges.is_empty(), |header| {
                        header.child(div().flex().flex_wrap().gap(px(4.0)).children(
                            entry.badges.iter().map(|badge| {
                                Badge::new(*badge).tone(match *badge {
                                    "Equipped" | "Here" => Tone::Accent,
                                    "AC" => Tone::Warning,
                                    _ => Tone::Neutral,
                                })
                            }),
                        ))
                    }),
            )
            .when(!entry.acts.is_empty(), |inspector| {
                inspector.child(div().flex().flex_wrap().gap(px(6.0)).children(acts))
            })
            .child(facts(theme, &entry.facts))
            .children(
                entry
                    .groups
                    .iter()
                    .enumerate()
                    .map(|(index, group)| self.group(theme, index, group, cx)),
            )
            .children(drops.map(|drops| {
                match drops {
                    Drops::Ready(group) => self.group(theme, entry.groups.len(), group, cx),
                    Drops::Reading => section(theme, "Drops")
                        .child(Spinner::new("grab-drops").size(px(14.0)))
                        .into_any_element(),
                    Drops::Unavailable => section(theme, "Drops")
                        .child(quiet(theme, "Couldn't read this monster's drops."))
                        .into_any_element(),
                }
            }))
            .when(
                entry.drops_from.is_none() && self.source == Source::Monsters,
                |inspector| inspector.child(quiet(theme, "Drops show for monsters in your cell.")),
            )
            .child(div().flex().flex_wrap().gap(px(2.0)).children(copies))
            .into_any_element()
    }

    fn group(
        &self,
        theme: &Theme,
        index: usize,
        group: &Group,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        section(theme, group.title)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(theme.radius.panel)
                    .border_1()
                    .border_color(theme.hairline)
                    .bg(theme.surface)
                    .children(group.lines.iter().enumerate().map(|(line_index, line)| {
                        let add = Act::AddDrop {
                            name: line.name.clone(),
                        };
                        div()
                            .id(("grab-line", index * 1000 + line_index))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .min_h(px(32.0))
                            .pl(px(10.0))
                            .pr(px(4.0))
                            .when(line_index > 0, |row| {
                                row.border_t_1().border_color(theme.hairline)
                            })
                            .child(div().flex_1().min_w_0().truncate().child(line.name.clone()))
                            .child(amount(theme, line))
                            .child(
                                Button::icon(
                                    ("grab-line-add", index * 1000 + line_index),
                                    Icon::Plus,
                                    format!("Add {} to drops", line.name),
                                )
                                .size(Size::Small)
                                .variant(Variant::Ghost)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.act(add.clone(), cx)),
                                ),
                            )
                    })),
            )
            .into_any_element()
    }
}

fn ids_input(
    kind: LoadKind,
    text: &str,
    cx: &mut Context<LoaderGrabber>,
) -> (Entity<Input>, Subscription) {
    let input = cx.new(|cx| {
        let mut input = Input::new(kind.placeholder(), cx).placeholder(kind.placeholder());
        input.set_text(text.to_owned(), cx);
        input
    });
    let events = cx.subscribe(&input, |this, input, event, cx| match event {
        InputEvent::Changed => input.update(cx, |input, cx| input.set_invalid(false, cx)),
        InputEvent::Submitted => this.load(cx),
    });
    (input, events)
}

fn describe(load: &Load) -> String {
    match load {
        Load::Shop(id) => format!("shop {id}"),
        Load::Quests(ids) if ids.len() == 1 => format!("quest {}", ids[0]),
        Load::Quests(ids) => format!("{} quests", ids.len()),
        Load::HairShop(id) => format!("hair shop {id}"),
        Load::ArmorCustomizer => "the armor customizer".into(),
    }
}

fn tooltip(act: &Act) -> &'static str {
    match act {
        Act::Game(Action::Buy { .. }) => "Buy one purchase of it",
        Act::Game(Action::Accept { .. }) => "Accept it now",
        Act::Game(Action::FakeComplete { .. }) => {
            "Make this client treat it as done, so quests and maps that need it open. The server doesn't change."
        }
        Act::Game(Action::Equip { .. }) => "Equip it",
        Act::Game(Action::Deposit { .. }) => "Move it to the bank",
        Act::Game(Action::Withdraw { .. }) => "Move it to the inventory",
        Act::Game(Action::Jump { .. }) => "Jump to this monster's cell",
        Act::Register { .. } => {
            "Accept and turn it in whenever it's ready, from this tab's environment"
        }
        Act::AddDrop { .. } => "Pick it up whenever it drops, from this tab's environment",
    }
}

fn row(entry: &Entry) -> ListItem {
    ListItem::new(entry.name.clone())
        .description(format!("{} {}", entry.id_label, entry.id))
        .end_slot(
            h_flex()
                .gap(px(4.0))
                .children(entry.badges.iter().map(|badge| Badge::new(*badge)))
                .when(!entry.meta.is_empty(), |end| end.child(entry.meta.clone())),
        )
}

fn amount(theme: &Theme, line: &Line) -> Div {
    let held = line.held.total();
    let (text, color) = if line.needed {
        let color = if held >= line.quantity {
            theme.accent_text
        } else {
            theme.text_secondary
        };
        (format!("{held}/{}", line.quantity), color)
    } else {
        let mut text = format!("×{}", line.quantity);
        if let Some(chance) = line.chance {
            text.push_str(&format!(" · {}%", (chance * 100.0).round() / 100.0));
        }
        if held > 0 {
            text.push_str(&format!(" · have {held}"));
        }
        (text, theme.text_secondary)
    };
    let in_bank = (line.held.bank > 0).then(|| format!("{} in bank", line.held.bank));
    div()
        .flex()
        .flex_none()
        .flex_col()
        .items_end()
        .text_size(TEXT_SMALL)
        .font_features(tabular())
        .text_color(color)
        .child(text)
        .children(in_bank.map(|text| div().text_color(theme.text_tertiary).child(text)))
}

fn facts(theme: &Theme, facts: &[(&'static str, String)]) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .children(facts.iter().map(|(label, value)| {
            div()
                .flex()
                .gap(px(12.0))
                .text_size(TEXT_SMALL)
                .line_height(LINE_SMALL)
                .child(
                    div()
                        .flex_none()
                        .w(px(96.0))
                        .text_color(theme.text_tertiary)
                        .child(*label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_features(tabular())
                        .child(value.clone()),
                )
        }))
}

fn section(theme: &Theme, title: &'static str) -> Div {
    div().flex().flex_col().gap(px(6.0)).child(
        div()
            .text_size(TEXT_SMALL)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.text_secondary)
            .child(title),
    )
}

fn quiet(theme: &Theme, text: &'static str) -> Div {
    div()
        .text_size(TEXT_SMALL)
        .line_height(LINE_SMALL)
        .text_color(theme.text_tertiary)
        .child(text)
}

fn problem_note(problem: &Problem) -> Empty {
    match problem {
        Problem::NotInGame => Empty::new("Not in game", "Log this tab in to read the game."),
        Problem::Failed(error) => Empty::new("Couldn't read the game", error.clone()),
    }
}

fn empty_note(source: Source) -> Empty {
    match source {
        Source::Shop => Empty::new("No shop open", "Load a shop above to see what it sells."),
        Source::Quests => Empty::new(
            "No quests loaded",
            "Load quests above, or open them in game.",
        ),
        Source::Inventory => Empty::new("Inventory empty", "Nothing in your inventory."),
        Source::TempInventory => {
            Empty::new("No temporary items", "Temporary quest items show here.")
        }
        Source::Bank => Empty::new("Bank empty", "Nothing in your bank."),
        Source::Monsters => Empty::new("No monsters", "This map has no monsters."),
    }
}

impl Render for LoaderGrabber {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let picker = shown_tab::picker(
            "grabber-tab",
            &self.client,
            self.shown.as_ref(),
            |this: &mut Self, id, cx| this.show(Some(id), cx),
            cx,
        );
        let entries = self.shown_entries(cx);
        let heading = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.heading.is_some());
        let list_height = (window.viewport_size().height
            - BAR_HEIGHT
            - LOADER_ROW
            - SEARCH_ROW
            - if heading { HEADING_ROW } else { px(0.0) }
            - FOOTER
            - px(12.0))
        .max(px(120.0));
        let body = if self.shown.is_some() {
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(self.sidebar(&theme, cx))
                .child(self.list_column(&theme, &entries, list_height, cx))
                .child(
                    div()
                        .id("grab-inspector")
                        .flex_none()
                        .w(INSPECTOR)
                        .min_h_0()
                        .overflow_y_scroll()
                        .p(px(16.0))
                        .border_l_1()
                        .border_color(theme.hairline)
                        .child(self.inspector(&theme, cx)),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .child(Empty::new(
                    "No tabs open",
                    "Open a tab to load and grab from it.",
                ))
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.base)
            .text_size(TEXT_BODY)
            .text_color(theme.text)
            .key_context(FOCUS_SCOPE)
            .id("loader-grabber")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseSession, window, _| window.remove_window()))
            .child(mira_ui::nav::pointer_probe())
            .child(
                window_bar(cx)
                    .child(bar_title("Loader/grabber", cx))
                    .child(drag_area().flex_1())
                    .child(picker),
            )
            .child(self.loader_row(&theme, cx))
            .child(body)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(FOOTER)
                    .px(px(16.0))
                    .border_t_1()
                    .border_color(theme.hairline)
                    .text_size(TEXT_SMALL)
                    .text_color(theme.text_secondary)
                    .truncate()
                    .children(self.notice.clone()),
            )
    }
}
