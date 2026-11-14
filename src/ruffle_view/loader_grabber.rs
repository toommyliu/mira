use std::collections::{HashMap, HashSet};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, ClipboardItem, Context, Div, Entity, FocusHandle,
    FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, actions, div,
    prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, PointerCursor as _, StyledExt as _, Theme, Tone,
    components::Empty,
    h_flex,
    primitives::{
        Align, Badge, Button, Checkbox, CheckedState, Dialog, DropdownMenu, Icon, Input,
        InputEvent, ListItem, MenuItem, Scrollbar, Select, Spinner, VirtualList,
        button::{Joined, Size, Variant},
    },
    theme::{LINE_BODY, ROW, TEXT_BODY, TEXT_TITLE, tabular},
    v_flex,
};

use super::{
    Client,
    client::CloseSession,
    form::LABEL_WIDTH,
    shown_tab::{self, ShownTab, Target, watch_changes},
};
use crate::{
    automation::{quests::Registered, register_items},
    bridge::CallError,
    domain::{
        grabber::{
            Act, Action, Entry, Group, Job, Line, Load, LoadKind, Reply, Snapshot, Source, percent,
        },
        inventory::ItemQuery,
        world::Location,
    },
    ruffle::Command,
    settings,
};

actions!(mira, [OpenLoaderGrabber]);

const WIDTH: f32 = 880.0;
const HEIGHT: f32 = 640.0;
const MIN_WIDTH: f32 = 720.0;
const MIN_HEIGHT: f32 = 420.0;
const INSPECTOR: Pixels = px(340.0);
const SEARCH: Pixels = px(200.0);
const SOURCE: Pixels = px(156.0);
const META: Pixels = px(96.0);
const ID: Pixels = px(68.0);
const MAX_SEARCH: usize = 64;

struct Open(WindowHandle<LoaderGrabber>);

impl gpui::Global for Open {}

pub(crate) fn open(client: Entity<Client>, target: Target, cx: &mut App) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0)
        && handle
            .update(cx, |grabber, window, cx| {
                window.activate_window();
                grabber.retarget(target, cx);
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
        window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
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
        let grabber = cx.new(|cx| LoaderGrabber::new(client, target, cx));
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

#[derive(Clone)]
struct Outcome {
    text: SharedString,
    failed: bool,
}

impl Outcome {
    fn done(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            failed: false,
        }
    }

    fn failed(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            failed: true,
        }
    }

    fn color(&self, theme: &Theme) -> Hsla {
        if self.failed {
            theme.danger_text
        } else {
            theme.accent_text
        }
    }
}

struct Loader {
    input: Entity<Input>,
    problem: Option<SharedString>,
    outcome: Option<Outcome>,
    loading: Option<Task<()>>,
    _events: Subscription,
}

impl Loader {
    fn new(
        kind: LoadKind,
        label: &'static str,
        source: Source,
        cx: &mut Context<LoaderGrabber>,
    ) -> Self {
        let input = cx.new(|cx| Input::new(label, cx).placeholder(kind.placeholder()));
        let events = cx.subscribe(&input, move |this, input, event, cx| match event {
            InputEvent::Changed => {
                input.update(cx, |input, cx| input.set_invalid(false, cx));
                if let Some(loader) = this.loader_mut(source) {
                    loader.problem = None;
                }
                cx.notify();
            }
            InputEvent::Submitted => this.load(source, kind, cx),
        });
        Self {
            input,
            problem: None,
            outcome: None,
            loading: None,
            _events: events,
        }
    }

    fn reset(&mut self, cx: &mut App) {
        self.problem = None;
        self.outcome = None;
        self.loading = None;
        self.input
            .update(cx, |input, cx| input.set_invalid(false, cx));
    }
}

struct ShopWarning {
    load: Load,
    quiet: bool,
}

struct Listed {
    quests: Vec<i32>,
    drops: Vec<ItemQuery>,
}

impl Listed {
    fn has(&self, act: &Act) -> bool {
        match act {
            Act::Register { quest_id } => self.quests.contains(quest_id),
            Act::RegisterDrop { name } => self.has_drop(name),
            Act::Game(_) => false,
        }
    }

    fn has_drop(&self, name: &str) -> bool {
        let query = ItemQuery::Name(name.to_owned());
        self.drops.iter().any(|known| known.same(&query))
    }
}

pub(crate) struct LoaderGrabber {
    client: Entity<Client>,
    focus: FocusHandle,
    tab: Target,
    shown: Option<ShownTab>,
    shop: Loader,
    quests: Loader,
    search: Entity<Input>,
    source: Source,
    snapshot: Option<Snapshot>,
    problem: Option<Problem>,
    selected: Option<i64>,
    drops: HashMap<i64, Drops>,
    outcome: Option<(i64, Outcome)>,
    reading: Option<Task<()>>,
    acting: Option<(Act, Task<()>)>,
    reading_drops: HashMap<i64, Task<()>>,
    opened: HashSet<i32>,
    inspector_scroll: ScrollHandle,
    located: Option<Location>,
    taken: Option<Outcome>,
    taking: Option<Task<()>>,
    shop_warning: Option<ShopWarning>,
    _subscriptions: [Subscription; 2],
}

impl LoaderGrabber {
    fn new(client: Entity<Client>, tab: Target, cx: &mut Context<Self>) -> Self {
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
            cx.observe(&client, |this, _, cx| this.show(cx)),
        ];
        let mut grabber = Self {
            client,
            focus: cx.focus_handle(),
            tab,
            shown: None,
            shop: Loader::new(LoadKind::Shop, "Shop ID", Source::Shop, cx),
            quests: Loader::new(LoadKind::Quests, "Quest IDs", Source::Quests, cx),
            search,
            source: Source::Shop,
            snapshot: None,
            problem: None,
            selected: None,
            drops: HashMap::new(),
            outcome: None,
            reading: None,
            acting: None,
            reading_drops: HashMap::new(),
            opened: HashSet::new(),
            inspector_scroll: ScrollHandle::new(),
            located: None,
            taken: None,
            taking: None,
            shop_warning: None,
            _subscriptions: subscriptions,
        };
        grabber.show(cx);
        grabber
    }

    fn retarget(&mut self, tab: Target, cx: &mut Context<Self>) {
        self.tab = tab;
        self.show(cx);
    }

    fn show(&mut self, cx: &mut Context<Self>) {
        let switched = shown_tab::show(
            &mut self.shown,
            &mut self.tab,
            &self.client,
            |this: &mut Self, cx| this.show(cx),
            |automation, cx| {
                vec![
                    watch_changes(automation.watch_drops(), cx),
                    watch_changes(automation.watch_quests(), cx),
                ]
            },
            cx,
        );
        let (moved, new_map) = self.follow_location(cx);
        if switched || new_map {
            self.drops.clear();
            self.reading_drops.clear();
        }
        if switched {
            self.shop.reset(cx);
            self.quests.reset(cx);
            self.outcome = None;
            self.acting = None;
            self.taken = None;
            self.taking = None;
            self.shop_warning = None;
            self.open_source(self.source, None, cx);
        } else if moved && self.source == Source::Monsters {
            self.read(cx);
        }
    }

    fn follow_location(&mut self, cx: &App) -> (bool, bool) {
        let view = self.shown.as_ref().and_then(|shown| shown.view.upgrade());
        let view = view.as_ref().map(|view| view.read(cx));
        let location = view.and_then(|view| view.bar.location());
        if location == self.located.as_ref() {
            return (false, false);
        }
        let map = |location: Option<&Location>| location.map(|location| location.map.clone());
        let new_map = map(location) != map(self.located.as_ref());
        self.located = location.cloned();
        (true, new_map)
    }

    fn loader_mut(&mut self, source: Source) -> Option<&mut Loader> {
        match source {
            Source::Shop => Some(&mut self.shop),
            Source::Quests => Some(&mut self.quests),
            _ => None,
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
        self.outcome = None;
        self.taken = None;
        self.opened.clear();
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
                        this.problem = Some(Problem::Failed(error.to_string().into()))
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
        if self.selected != Some(key) {
            self.outcome = None;
            self.opened.clear();
        }
        self.selected = Some(key);
        if let Some(entry) = self.entry(key).cloned() {
            self.follow_drops(&entry, cx);
        }
        cx.notify();
    }

    fn follow_drops(&mut self, entry: &Entry, cx: &mut Context<Self>) {
        let key = entry.key;
        if matches!(self.drops.get(&key), Some(Drops::Ready(_) | Drops::Reading)) {
            return;
        }
        let Some(mon_map_id) = entry.drops_from else {
            return;
        };
        let Some(answer) = self.send(|reply| Job::Drops { mon_map_id, reply }, cx) else {
            return;
        };
        self.drops.insert(key, Drops::Reading);
        let task = cx.spawn(async move |this, cx| {
            let result = answer.recv().await;
            let _ = this.update(cx, |this, cx| {
                this.reading_drops.remove(&key);
                let drops = match result {
                    Ok(Ok(Some(group))) => Drops::Ready(group),
                    _ => Drops::Unavailable,
                };
                this.drops.insert(key, drops);
                cx.notify();
            });
        });
        self.reading_drops.insert(key, task);
    }

    fn load(&mut self, source: Source, kind: LoadKind, cx: &mut Context<Self>) {
        let Some(loader) = self.loader_mut(source) else {
            return;
        };
        if loader.loading.is_some() {
            return;
        }
        let text = loader.input.read(cx).text().to_owned();
        if kind.takes_ids() && text.trim().is_empty() {
            return;
        }
        let load = match kind.parse(&text) {
            Ok(load) => load,
            Err(problem) => {
                loader.problem = Some(problem.message().into());
                loader
                    .input
                    .update(cx, |input, cx| input.set_invalid(true, cx));
                cx.notify();
                return;
            }
        };
        if matches!(load, Load::Shop(_) | Load::HairShop(_)) && settings::shop_warning(cx) {
            self.shop_warning = Some(ShopWarning { load, quiet: false });
            cx.notify();
            return;
        }
        self.start(source, load, cx);
    }

    fn confirm_shop_warning(&mut self, cx: &mut Context<Self>) {
        let Some(warning) = self.shop_warning.take() else {
            return;
        };
        if warning.quiet {
            settings::set_shop_warning(false, cx);
        }
        self.start(Source::Shop, warning.load, cx);
    }

    fn dismiss_shop_warning(&mut self, cx: &mut Context<Self>) {
        self.shop_warning = None;
        cx.notify();
    }

    fn start(&mut self, source: Source, load: Load, cx: &mut Context<Self>) {
        let described = describe(&load);
        let lists = matches!(load, Load::Shop(_) | Load::Quests(_));
        let first = match &load {
            Load::Quests(ids) => ids.first().map(|id| i64::from(*id)),
            _ => None,
        };
        let Some(answer) = self.send(|reply| Job::Load { load, reply }, cx) else {
            return;
        };
        let task = cx.spawn(async move |this, cx| {
            let result = answer.recv().await;
            let _ = this.update(cx, |this, cx| {
                let outcome = match result {
                    Ok(Ok(true)) if lists => Outcome::done(format!("Loaded {described}")),
                    Ok(Ok(true)) => Outcome::done(format!("Opened {described}")),
                    Ok(Ok(false)) => Outcome::failed(format!("Couldn't load {described}")),
                    Ok(Err(error)) => Outcome::failed(error.to_string()),
                    Err(_) => Outcome::failed("The tab closed"),
                };
                let refresh = lists && !outcome.failed && this.source == source;
                if let Some(loader) = this.loader_mut(source) {
                    loader.loading = None;
                    loader.outcome = Some(outcome);
                }
                if refresh {
                    this.open_source(source, first, cx);
                }
                cx.notify();
            });
        });
        if let Some(loader) = self.loader_mut(source) {
            loader.outcome = None;
            loader.loading = Some(task);
        }
        cx.notify();
    }

    fn act(&mut self, act: Act, cx: &mut Context<Self>) {
        let Some(automation) = self.shown.as_ref().map(|shown| shown.automation.clone()) else {
            return;
        };
        let action = match act {
            Act::Register { quest_id } => {
                automation.update_quests(|state| {
                    state.register([Registered {
                        quest_id,
                        reward_item_id: None,
                    }])
                });
                cx.notify();
                return;
            }
            Act::RegisterDrop { name } => {
                automation.update_drops(|state| {
                    register_items(&mut state.items, vec![ItemQuery::Name(name)])
                });
                cx.notify();
                return;
            }
            Act::Game(action) => action,
        };
        let Some(key) = self.selected else {
            return;
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
        self.outcome = None;
        let done = action.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = answer.recv().await;
            let _ = this.update(cx, |this, cx| {
                this.acting = None;
                let outcome = match result {
                    Ok(Ok(true)) => Outcome::done(done.done()),
                    Ok(Ok(false)) => Outcome::failed(done.failed()),
                    Ok(Err(error)) => Outcome::failed(error.to_string()),
                    Err(_) => Outcome::failed("The tab closed"),
                };
                this.outcome = Some((key, outcome));
                this.read(cx);
            });
        });
        self.acting = Some((Act::Game(action), task));
        cx.notify();
    }

    fn copy(&mut self, key: i64, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.outcome = Some((key, Outcome::done("Copied")));
        cx.notify();
    }

    fn take_all(&mut self, save: bool, cx: &mut Context<Self>) {
        let source = self.source;
        let Some(answer) = self.send(|reply| Job::Raw { source, reply }, cx) else {
            return;
        };
        self.taken = None;
        self.taking = Some(cx.spawn(async move |this, cx| {
            let text = match answer.recv().await {
                Ok(Ok(Some(text))) => Ok(text),
                Ok(Ok(None)) if save => Err("Nothing to export".to_owned()),
                Ok(Ok(None)) => Err("Nothing to copy".to_owned()),
                Ok(Err(error)) => Err(error.to_string()),
                Err(_) => Err("The tab closed".to_owned()),
            };
            let outcome = match (text, save) {
                (Err(problem), _) => Some(Outcome::failed(problem)),
                (Ok(text), false) => this
                    .update(cx, |_, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    })
                    .ok()
                    .map(|()| Outcome::done("Copied")),
                (Ok(text), true) => {
                    let directory = dirs::document_dir()
                        .or_else(dirs::home_dir)
                        .unwrap_or_default();
                    let name = format!("{}.json", source.name().to_lowercase().replace(' ', "-"));
                    let Ok(chosen) =
                        this.update(cx, |_, cx| cx.prompt_for_new_path(&directory, Some(&name)))
                    else {
                        return;
                    };
                    match chosen.await {
                        Ok(Ok(Some(path))) => Some(
                            match cx
                                .background_spawn(async move { std::fs::write(&path, text) })
                                .await
                            {
                                Ok(()) => Outcome::done("Saved"),
                                Err(error) => Outcome::failed(format!("Couldn't save: {error}")),
                            },
                        ),
                        Ok(Err(error)) => Some(Outcome::failed(format!("Couldn't save: {error}"))),
                        Ok(Ok(None)) | Err(_) => None,
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.taking = None;
                this.taken = outcome;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn toggle_details(&mut self, item_id: i32, cx: &mut Context<Self>) {
        if !self.opened.remove(&item_id) {
            self.opened.insert(item_id);
        }
        cx.notify();
    }

    fn listed(&self) -> Listed {
        match &self.shown {
            Some(shown) => Listed {
                quests: shown
                    .automation
                    .quests()
                    .quests
                    .iter()
                    .map(|registered| registered.quest_id)
                    .collect(),
                drops: shown.automation.drops().items,
            },
            None => Listed {
                quests: Vec::new(),
                drops: Vec::new(),
            },
        }
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

    fn toolbar(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let this = cx.weak_entity();
        let sources = Select::new(
            "grab-source",
            "Source",
            Source::ALL
                .iter()
                .map(|source| SharedString::from(source.name())),
            Source::ALL.iter().position(|source| *source == self.source),
        )
        .width(SOURCE)
        .on_select(move |index, _, cx| {
            let _ = this.update(cx, |this, cx| {
                if this.source != Source::ALL[index] {
                    this.open_source(Source::ALL[index], None, cx);
                }
            });
        });
        let loader = match self.source {
            Source::Shop => Some(&self.shop),
            Source::Quests => Some(&self.quests),
            _ => None,
        };
        let fields = loader.map(|loader| self.loader_fields(loader, theme, cx));
        let problem = loader
            .and_then(|loader| loader.problem.clone())
            .map(|problem| {
                div()
                    .text_small()
                    .text_color(theme.danger_text)
                    .child(problem)
            });
        let taking = self.taking.is_some();
        let menu_this = cx.weak_entity();
        let menu = DropdownMenu::new("grab-source-menu")
            .trigger(
                Button::icon("grab-source-menu-trigger", Icon::Ellipsis, "More actions")
                    .variant(Variant::Ghost)
                    .hide_tooltip(),
            )
            .align(Align::End)
            .items(move |_, _| {
                [("Copy all", false), ("Export all…", true)]
                    .into_iter()
                    .map(|(label, save)| {
                        let this = menu_this.clone();
                        MenuItem::action(label, move |_, cx| {
                            let _ = this.update(cx, |this, cx| this.take_all(save, cx));
                        })
                        .disabled(taking)
                    })
                    .collect()
            });
        v_flex()
            .flex_none()
            .gap(px(6.0))
            .px(px(16.0))
            .py(px(10.0))
            .border_b_1()
            .border_color(theme.hairline)
            .child(
                h_flex()
                    .gap(px(8.0))
                    .child(sources)
                    .children(fields)
                    .child(div().flex_1())
                    .children(self.taken.as_ref().map(|outcome| {
                        div()
                            .flex_none()
                            .text_small()
                            .text_color(outcome.color(theme))
                            .child(outcome.text.clone())
                    }))
                    .child(div().w(SEARCH).min_w(px(120.0)).child(self.search.clone()))
                    .child(
                        Button::icon("grab-refresh", Icon::RotateCcw, "Refresh")
                            .variant(Variant::Ghost)
                            .busy(self.reading.is_some())
                            .hide_tooltip()
                            .on_click(cx.listener(|this, _, _, cx| this.read(cx))),
                    )
                    .child(menu),
            )
            .children(problem)
    }

    fn loader_fields(&self, loader: &Loader, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let source = self.source;
        let busy = loader.loading.is_some();
        let empty = loader.input.read(cx).text().trim().is_empty();
        let load = Button::new(("load", source as usize), "Load")
            .variant(Variant::Primary)
            .busy(busy)
            .disabled(empty)
            .on_click(cx.listener(move |this, _, _, cx| {
                let kind = match source {
                    Source::Quests => LoadKind::Quests,
                    _ => LoadKind::Shop,
                };
                this.load(source, kind, cx)
            }));
        let control = if source == Source::Shop {
            let this = cx.weak_entity();
            let more = DropdownMenu::new("load-more")
                .trigger(
                    Button::icon("load-more-trigger", Icon::ChevronDown, "Other loads")
                        .variant(Variant::Primary)
                        .joined(Joined::Previous)
                        .disabled(busy)
                        .hide_tooltip(),
                )
                .items(move |_, cx| {
                    let warn = settings::shop_warning(cx);
                    [
                        ("Load as hair shop", LoadKind::HairShop, empty),
                        ("Open armor customizer", LoadKind::ArmorCustomizer, false),
                    ]
                    .into_iter()
                    .map(|(label, kind, off)| {
                        let this = this.clone();
                        MenuItem::action(label, move |_, cx| {
                            let _ = this.update(cx, |this, cx| this.load(Source::Shop, kind, cx));
                        })
                        .disabled(off)
                    })
                    .chain([
                        MenuItem::separator(),
                        MenuItem::switch("Warn before loading", warn, move |_, cx| {
                            settings::set_shop_warning(!warn, cx)
                        }),
                    ])
                    .collect()
                });
            h_flex()
                .child(load.joined(Joined::Next))
                .child(more)
                .into_any_element()
        } else {
            load.into_any_element()
        };
        h_flex()
            .gap(px(8.0))
            .min_w_0()
            .child(
                div()
                    .flex_none()
                    .w(px(if source == Source::Shop { 150.0 } else { 240.0 }))
                    .child(loader.input.clone()),
            )
            .child(control)
            .children(loader.outcome.as_ref().map(|outcome| {
                div()
                    .min_w_0()
                    .truncate()
                    .text_small()
                    .text_color(outcome.color(theme))
                    .child(outcome.text.clone())
            }))
    }

    fn shop_warning_dialog(&self, cx: &mut Context<Self>) -> Dialog {
        let warning = self.shop_warning.as_ref();
        let title = warning.map_or_else(String::new, |warning| {
            format!("Load {}?", describe(&warning.load))
        });
        let quiet = if warning.is_some_and(|warning| warning.quiet) {
            CheckedState::Checked
        } else {
            CheckedState::Unchecked
        };
        let toggling = cx.weak_entity();
        let closing = cx.weak_entity();
        Dialog::new("shop-warning", warning.is_some(), title)
            .alert()
            .description("Make sure you're in the correct map when loading this shop.")
            .footer_start_slot(
                Checkbox::new("shop-warning-quiet", "Don't warn me again", quiet).on_toggle(
                    move |on, _, cx| {
                        let _ = toggling.update(cx, |this, cx| {
                            if let Some(warning) = &mut this.shop_warning {
                                warning.quiet = on;
                            }
                            cx.notify();
                        });
                    },
                ),
            )
            .action(
                Button::new("shop-warning-cancel", "Cancel")
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss_shop_warning(cx))),
            )
            .action(
                Button::new("shop-warning-load", "Load")
                    .variant(Variant::Primary)
                    .on_click(cx.listener(|this, _, _, cx| this.confirm_shop_warning(cx))),
            )
            .on_close(move |_, cx| {
                let _ = closing.update(cx, |this, cx| this.dismiss_shop_warning(cx));
            })
    }

    fn list_column(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let entries = self.shown_entries(cx);
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
                let (lookup, keys, row_theme) = (rows.clone(), rows.clone(), theme.clone());
                let this = cx.weak_entity();
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.columns(theme))
                    .child(
                        VirtualList::new(
                            "grab-entries",
                            self.source.name(),
                            rows.len(),
                            move |index| line(&lookup[index], &row_theme),
                        )
                        .selected(selected)
                        .fill()
                        .on_select(move |index, _, cx| {
                            let key = keys[index].key;
                            let _ = this.update(cx, |this, cx| this.select(key, cx));
                        }),
                    )
                    .into_any_element()
            }
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .px(px(12.0))
            .pb(px(12.0))
            .child(body)
    }

    fn columns(&self, theme: &Theme) -> Div {
        let (heading, color) = match (&self.problem, &self.snapshot) {
            (Some(Problem::Failed(error)), _) => (
                SharedString::from(format!("Couldn't refresh: {error}")),
                theme.danger_text,
            ),
            (
                _,
                Some(Snapshot {
                    heading: Some(heading),
                    ..
                }),
            ) => (heading.clone().into(), theme.text_secondary),
            _ => ("Name".into(), theme.text_tertiary),
        };
        let [meta, id] = self.source.columns();
        h_flex()
            .flex_none()
            .h(px(28.0))
            .gap(px(8.0))
            .pl(px(13.0))
            .pr(px(9.0) + Scrollbar::GUTTER)
            .text_small()
            .text_color(theme.text_tertiary)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(color)
                    .child(heading),
            )
            .child(div().flex_none().w(META).text_right().child(meta))
            .child(div().flex_none().w(ID).text_right().child(id))
    }

    fn inspector_pane(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        div()
            .relative()
            .flex_none()
            .w(INSPECTOR)
            .min_h_0()
            .border_l_1()
            .border_color(theme.hairline)
            .child(
                div()
                    .id("grab-inspector")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.inspector_scroll)
                    .p(px(16.0))
                    .child(self.inspector(theme, cx)),
            )
            .child(Scrollbar::new(
                "grab-inspector-scrollbar",
                self.inspector_scroll.clone(),
            ))
    }

    fn inspector(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self.selected.and_then(|key| self.entry(key)).cloned() else {
            let pickable = self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| !snapshot.entries.is_empty());
            return div()
                .py(px(32.0))
                .text_center()
                .text_small()
                .text_color(theme.text_tertiary)
                .when(pickable, |note| {
                    note.child("Pick something to see its details and actions.")
                })
                .into_any_element();
        };
        let listed = self.listed();
        let more = self.more_menu(&entry, cx);
        let acting = self.acting.as_ref().map(|(act, _)| act.clone());
        let acts = entry.acts.iter().enumerate().map(|(index, offer)| {
            let act = offer.act.clone();
            let busy = acting.as_ref() == Some(&act);
            let waiting = acting.is_some() && !busy && matches!(act, Act::Game(_));
            Button::new(("grab-act", index), act.label())
                .size(Size::Small)
                .variant(if index == 0 {
                    Variant::Primary
                } else {
                    Variant::Secondary
                })
                .busy(busy)
                .disabled(!offer.enabled || waiting || listed.has(&act))
                .when_some(tooltip(&act), |button, tooltip| button.tooltip(tooltip))
                .on_click(cx.listener(move |this, _, _, cx| this.act(act.clone(), cx)))
        });
        let outcome = self
            .outcome
            .as_ref()
            .filter(|(key, _)| *key == entry.key)
            .map(|(_, outcome)| {
                div()
                    .max_w_full()
                    .truncate()
                    .text_small()
                    .text_color(outcome.color(theme))
                    .child(outcome.text.clone())
            });
        let drops = (self.source == Source::Monsters)
            .then(|| self.drops.get(&entry.key))
            .flatten();
        let unseen = drops.is_none() && entry.drops_from.is_none();
        v_flex()
            .gap(px(14.0))
            .child(
                v_flex()
                    .gap(px(6.0))
                    .child(
                        h_flex()
                            .items_start()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .pt(px(1.0))
                                    .text_size(TEXT_TITLE)
                                    .line_height(LINE_BODY)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(entry.name.clone()),
                            )
                            .child(more),
                    )
                    .when(!entry.badges.is_empty(), |header| {
                        header.child(div().flex().flex_wrap().gap(px(4.0)).children(
                            entry.badges.iter().map(|badge| {
                                Badge::new(*badge).tone(match *badge {
                                    "Equipped" => Tone::Accent,
                                    "AC" => Tone::Warning,
                                    _ => Tone::Neutral,
                                })
                            }),
                        ))
                    }),
            )
            .when(!entry.acts.is_empty() || outcome.is_some(), |inspector| {
                inspector.child(
                    h_flex()
                        .flex_wrap()
                        .gap(px(6.0))
                        .children(acts)
                        .children(outcome),
                )
            })
            .child(facts(theme, &entry.facts))
            .children(
                entry
                    .groups
                    .iter()
                    .enumerate()
                    .map(|(index, group)| self.group(theme, index, group, &listed, cx)),
            )
            .children(drops.map(|drops| {
                match drops {
                    Drops::Ready(group) => {
                        self.group(theme, entry.groups.len(), group, &listed, cx)
                    }
                    Drops::Reading => caption(theme, "Drops")
                        .child(Spinner::new("grab-drops").size(px(14.0)))
                        .into_any_element(),
                    Drops::Unavailable => caption(theme, "Drops")
                        .child(quiet(theme, "Couldn't read this monster's drops."))
                        .into_any_element(),
                }
            }))
            .when(unseen && self.source == Source::Monsters, |inspector| {
                inspector.child(quiet(theme, "Drops show for monsters in your cell."))
            })
            .into_any_element()
    }

    fn more_menu(&self, entry: &Entry, cx: &mut Context<Self>) -> DropdownMenu {
        let this = cx.weak_entity();
        let key = entry.key;
        let copies = [
            ("Copy name".to_owned(), entry.name.clone()),
            (copy_id(entry.id_label), entry.id.clone()),
            ("Copy JSON".to_owned(), entry.json.clone()),
        ];
        DropdownMenu::new("grab-more")
            .trigger(
                Button::icon("grab-more-trigger", Icon::Ellipsis, "More actions")
                    .size(Size::Small)
                    .variant(Variant::Ghost)
                    .hide_tooltip(),
            )
            .align(Align::End)
            .items(move |_, _| {
                copies
                    .iter()
                    .map(|(label, text)| {
                        let (this, text) = (this.clone(), text.clone());
                        MenuItem::action(label.clone(), move |_, cx| {
                            let _ = this.update(cx, |this, cx| this.copy(key, text.clone(), cx));
                        })
                    })
                    .collect()
            })
    }

    fn detailed_line(
        &self,
        theme: &Theme,
        id: usize,
        line: &Line,
        register: Option<Button>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = line.item_id.unwrap_or(id as i32);
        let open = self.opened.contains(&key);
        let small = |text: String, color: Hsla| {
            div()
                .flex_none()
                .text_small()
                .font_features(tabular())
                .text_color(color)
                .child(text)
        };
        let row = h_flex()
            .id(("grab-line-row", id))
            .gap(px(6.0))
            .h(ROW)
            .mx(px(-4.0))
            .px(px(4.0))
            .rounded(theme.radius.item)
            .pointer_cursor(theme)
            .hover(|style| style.bg(theme.fill))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_details(key, cx)))
            .child(
                Button::icon(
                    ("grab-line-open", id),
                    if open {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    },
                    format!("Details for {}", line.name),
                )
                .size(Size::XSmall)
                .variant(Variant::Ghost)
                .hide_tooltip()
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_details(key, cx))),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(6.0))
                    .child(div().min_w_0().truncate().child(line.name.clone()))
                    .when(line.quantity > 1, |name| {
                        let amount = if line.varies {
                            format!("×1–{}", line.quantity)
                        } else {
                            format!("×{}", line.quantity)
                        };
                        name.child(small(amount, theme.text_tertiary))
                    }),
            )
            .children(
                line.item_id
                    .map(|item_id| small(item_id.to_string(), theme.text_tertiary)),
            )
            .child(
                small(
                    line.chance.map(percent).unwrap_or_default(),
                    theme.text_secondary,
                )
                .min_w(px(32.0))
                .text_right(),
            )
            .children(register);
        v_flex()
            .id(("grab-line", id))
            .child(row)
            .when(open, |line_view| {
                line_view.child(
                    div()
                        .pl(px(26.0))
                        .pt(px(2.0))
                        .pb(px(8.0))
                        .child(facts(theme, &line.facts)),
                )
            })
            .into_any_element()
    }

    fn group(
        &self,
        theme: &Theme,
        index: usize,
        group: &Group,
        listed: &Listed,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = group.lines.iter().enumerate().map(|(line_index, line)| {
            let id = index * 1000 + line_index;
            let register = line.item_id.map(|_| {
                if listed.has_drop(&line.name) {
                    Button::icon(
                        ("grab-line-add", id),
                        Icon::Check,
                        format!("{} is a registered drop", line.name),
                    )
                    .size(Size::Small)
                    .disabled(true)
                    .hide_tooltip()
                } else {
                    let act = Act::RegisterDrop {
                        name: line.name.clone(),
                    };
                    Button::icon(
                        ("grab-line-add", id),
                        Icon::Plus,
                        format!("Register {} as a drop", line.name),
                    )
                    .size(Size::Small)
                    .tooltip("Register drop")
                    .on_click(cx.listener(move |this, _, _, cx| this.act(act.clone(), cx)))
                }
            });
            if !line.facts.is_empty() {
                return self.detailed_line(theme, id, line, register, cx);
            }
            let (amount, color) = amount(theme, line);
            h_flex()
                .id(("grab-line", id))
                .gap(px(8.0))
                .h(ROW)
                .child(div().flex_1().min_w_0().truncate().child(line.name.clone()))
                .children(detail(line).map(|text| {
                    div()
                        .flex_none()
                        .text_small()
                        .font_features(tabular())
                        .text_color(theme.text_tertiary)
                        .child(text)
                }))
                .child(
                    div()
                        .flex_none()
                        .min_w(px(32.0))
                        .text_right()
                        .text_small()
                        .font_features(tabular())
                        .text_color(color)
                        .child(amount),
                )
                .children(register)
                .into_any_element()
        });
        caption(theme, group.title)
            .child(v_flex().children(rows))
            .into_any_element()
    }
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

fn tooltip(act: &Act) -> Option<&'static str> {
    match act {
        Act::Game(Action::FakeComplete { .. }) => Some(
            "Make this client treat it as done, so quests and maps that need it open. The server doesn't change.",
        ),
        Act::Register { .. } => {
            Some("Accept and turn it in whenever it's ready, from this tab's environment")
        }
        Act::RegisterDrop { .. } => {
            Some("Pick it up whenever it drops, from this tab's environment")
        }
        Act::Game(_) => None,
    }
}

fn copy_id(id_label: &str) -> String {
    match id_label {
        "Item ID" => "Copy item ID".into(),
        "Quest ID" => "Copy quest ID".into(),
        other => format!("Copy {other}"),
    }
}

fn line(entry: &Entry, theme: &Theme) -> ListItem {
    let tags = entry.badges.join(" · ");
    ListItem::new(entry.name.clone())
        .child(
            h_flex()
                .gap(px(8.0))
                .min_w_0()
                .child(div().min_w_0().truncate().child(entry.name.clone()))
                .when(!tags.is_empty(), |row| {
                    row.child(
                        div()
                            .flex_none()
                            .text_small()
                            .text_color(theme.text_tertiary)
                            .child(tags),
                    )
                }),
        )
        .end_slot(
            h_flex()
                .gap(px(8.0))
                .child(
                    div()
                        .w(META)
                        .text_right()
                        .truncate()
                        .when(entry.here, |meta| meta.text_color(theme.accent_text))
                        .child(entry.meta.clone()),
                )
                .child(div().w(ID).text_right().child(entry.id.clone())),
        )
}

fn amount(theme: &Theme, line: &Line) -> (String, Hsla) {
    let held = line.held.total();
    if line.needed {
        let color = if held >= line.quantity {
            theme.accent_text
        } else {
            theme.text_secondary
        };
        (format!("{held}/{}", line.quantity), color)
    } else {
        (format!("×{}", line.quantity), theme.text_secondary)
    }
}

fn detail(line: &Line) -> Option<String> {
    let mut parts = Vec::new();
    if line.needed {
        if line.held.bank > 0 {
            parts.push(format!("{} in bank", line.held.bank));
        }
    } else {
        if let Some(chance) = line.chance {
            parts.push(percent(chance));
        }
        let held = line.held.total();
        if held > 0 {
            parts.push(format!("{held} owned"));
        }
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn facts(theme: &Theme, facts: &[(&'static str, String)]) -> Div {
    v_flex()
        .gap(px(4.0))
        .children(facts.iter().map(|(label, value)| {
            h_flex()
                .items_start()
                .gap(px(12.0))
                .text_small()
                .child(
                    div()
                        .flex_none()
                        .w(LABEL_WIDTH)
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

fn caption(theme: &Theme, title: &'static str) -> Div {
    v_flex().gap(px(4.0)).child(
        div()
            .text_small()
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.text_secondary)
            .child(title),
    )
}

fn quiet(theme: &Theme, text: &'static str) -> Div {
    div()
        .text_small()
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
        let header = shown_tab::header(
            "Loader/grabber",
            &self.client,
            self.shown.as_ref(),
            self.tab,
            Self::retarget,
            window,
            cx,
        );
        let body = if self.shown.is_some() {
            v_flex()
                .flex_1()
                .min_h_0()
                .child(self.toolbar(&theme, cx))
                .child(
                    h_flex()
                        .items_stretch()
                        .flex_1()
                        .min_h_0()
                        .child(self.list_column(&theme, cx))
                        .child(self.inspector_pane(&theme, cx)),
                )
        } else {
            v_flex().flex_1().child(Empty::new(
                "No tabs open",
                "Open a tab to load and grab from it.",
            ))
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
            .child(header)
            .child(body)
            .child(self.shop_warning_dialog(cx))
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Focusable as _, TestAppContext, VisualTestContext};

    use super::*;
    use crate::automation::settings::Automation;

    fn open(cx: &mut TestAppContext) -> (Entity<LoaderGrabber>, &mut VisualTestContext) {
        let folder = std::env::temp_dir().join(format!("mira-grabber-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let settings = folder.join(format!("settings-{:x}.json", u64::from_ne_bytes(random)));
        cx.update(|cx| {
            mira_ui::init(cx);
            settings::init(settings, |_| {}, cx);
        });
        let (grabber, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            let client = Client::empty_for_tests(&folder, cx);
            let mut grabber = LoaderGrabber::new(client, Target::Selected, cx);
            grabber.shown = Some(ShownTab::fake("Alt 1", Automation::default()));
            grabber
        });
        cx.run_until_parked();
        (grabber, cx)
    }

    fn press(cx: &mut VisualTestContext, keys: &'static str) {
        cx.simulate_keystrokes(keys);
        cx.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse(keys).unwrap(),
        });
        cx.run_until_parked();
    }

    fn typed(cx: &mut VisualTestContext, text: &str) {
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    fn focus_shop_field(grabber: &Entity<LoaderGrabber>, cx: &mut VisualTestContext) {
        let input = grabber.read_with(cx, |grabber, _| grabber.shop.input.clone());
        cx.update(|window, cx| window.focus(&input.focus_handle(cx), cx));
    }

    fn shop_warning(
        grabber: &Entity<LoaderGrabber>,
        cx: &mut VisualTestContext,
    ) -> Option<(String, bool)> {
        grabber.read_with(cx, |grabber, _| {
            let warning = grabber.shop_warning.as_ref()?;
            Some((describe(&warning.load), warning.quiet))
        })
    }

    fn shop_field(
        grabber: &Entity<LoaderGrabber>,
        cx: &mut VisualTestContext,
    ) -> (String, Option<String>) {
        grabber.read_with(cx, |grabber, cx| {
            (
                grabber.shop.input.read(cx).text().to_owned(),
                grabber.shop.problem.as_ref().map(ToString::to_string),
            )
        })
    }

    #[gpui::test]
    fn a_load_the_field_cant_read_keeps_the_text_and_says_why(cx: &mut TestAppContext) {
        let (grabber, cx) = open(cx);
        focus_shop_field(&grabber, cx);
        press(cx, "enter");
        assert_eq!(shop_field(&grabber, cx), (String::new(), None));

        typed(cx, "12a");
        press(cx, "enter");
        assert_eq!(
            shop_field(&grabber, cx),
            ("12a".into(), Some("IDs are whole numbers above 0".into()))
        );

        typed(cx, "4");
        assert_eq!(shop_field(&grabber, cx), ("12a4".into(), None));
    }

    #[gpui::test]
    fn loading_a_shop_warns_until_told_not_to(cx: &mut TestAppContext) {
        let (grabber, cx) = open(cx);
        focus_shop_field(&grabber, cx);
        typed(cx, "1234");
        press(cx, "enter");
        assert_eq!(
            shop_warning(&grabber, cx),
            Some(("shop 1234".into(), false))
        );

        press(cx, "escape");
        assert_eq!(shop_warning(&grabber, cx), None);
        assert!(cx.update(|_, cx| settings::shop_warning(cx)));

        press(cx, "enter");
        assert_eq!(
            shop_warning(&grabber, cx),
            Some(("shop 1234".into(), false))
        );
        press(cx, "space");
        assert_eq!(shop_warning(&grabber, cx), Some(("shop 1234".into(), true)));
        press(cx, "tab");
        press(cx, "tab");
        press(cx, "enter");
        assert_eq!(shop_warning(&grabber, cx), None);
        assert!(!cx.update(|_, cx| settings::shop_warning(cx)));

        focus_shop_field(&grabber, cx);
        press(cx, "enter");
        assert_eq!(shop_warning(&grabber, cx), None);
    }

    #[test]
    fn snapshot_shop_warning() {
        let folder = std::env::temp_dir().join(format!("mira-grabber-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let size = gpui::size(px(WIDTH), px(HEIGHT));
        for written in mira_ui::snapshot("grabber-shop-warning", size, move |_, cx| {
            cx.new(|cx| {
                let client = Client::empty_for_tests(&folder, cx);
                let mut grabber = LoaderGrabber::new(client, Target::Selected, cx);
                grabber.shown = Some(ShownTab::fake("Alt 1", Automation::default()));
                grabber.shop_warning = Some(ShopWarning {
                    load: Load::Shop(1234),
                    quiet: false,
                });
                grabber
            })
        }) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }
}
