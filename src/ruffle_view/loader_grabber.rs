use std::collections::{HashMap, HashSet};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, ClipboardItem, Context, Div, Entity, EntityId,
    FocusHandle, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowHandle,
    WindowOptions, actions, div, prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, PointerCursor as _, StyledExt as _, Theme,
    components::Empty,
    h_flex,
    primitives::{
        Align, Badge, Button, Checkbox, CheckedState, ContextMenu, Dialog, DropdownMenu, Icon,
        Input, InputEvent, ListItem, MenuItem, Scrollbar, Spinner, Tab, Tabs, VirtualList,
        VirtualListState,
        button::{Joined, Size, Variant},
        icon,
        text_field::Find,
    },
    theme::tabular,
    v_flex,
};

use super::{
    Client,
    client::{CloseSession, refocus_root_when_lost},
    form::LABEL_WIDTH,
    shown_tab::{self, ShownTab, Target, watch_changes},
};
use crate::{
    automation::{quests::Registered, register_items},
    bridge::CallError,
    domain::{
        grabber::{
            Act, Action, Entry, Group, Heading, Job, Line, Load, LoadKind, Reply, Snapshot, Source,
            grouped, percent,
        },
        inventory::ItemQuery,
        world::Location,
    },
    file_panel,
    ruffle::Command,
    settings,
};

actions!(mira, [OpenLoaderGrabber]);

const WIDTH: f32 = 880.0;
const HEIGHT: f32 = 640.0;
const MIN_WIDTH: f32 = 720.0;
const MIN_HEIGHT: f32 = 420.0;

const SEARCH: Pixels = px(200.0);
const META: Pixels = px(96.0);
const ID: Pixels = px(68.0);
const MAX_SEARCH: usize = 64;
const LINE: &str = "grab-line";
const SETTLE: std::time::Duration = std::time::Duration::from_millis(100);

struct Open(WindowHandle<LoaderGrabber>);

impl gpui::Global for Open {}

pub(super) fn close(client: gpui::EntityId, cx: &mut App) {
    let open = cx.try_global::<Open>().map(|open| open.0);
    shown_tab::close(open, client, |grabber: &LoaderGrabber| &grabber.client, cx);
}

pub(crate) fn open(client: Entity<Client>, target: Target, cx: &mut App) {
    let open = cx.try_global::<Open>().map(|open| open.0);
    if shown_tab::reuse(
        open,
        &client,
        |grabber: &LoaderGrabber| &grabber.client,
        |grabber, _, cx| {
            grabber.retarget(target, cx);
        },
        cx,
    ) {
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
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let grabber = cx.new(|cx| LoaderGrabber::new(client, target, window, cx));
        let root = grabber.read(cx).focus.clone();
        window.focus(&root, cx);
        refocus_root_when_lost(&grabber, root, window, cx);
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
            theme.destructive
        } else {
            theme.muted_foreground
        }
    }
}

struct Loader {
    input: Entity<Input>,
    problem: Option<SharedString>,
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
        let input = cx.new(|cx| {
            Input::new(label, cx)
                .placeholder(kind.placeholder())
                .small()
        });
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
            loading: None,
            _events: events,
        }
    }

    fn reset(&mut self, cx: &mut App) {
        self.problem = None;
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
    shelved: HashMap<Source, Snapshot>,
    problem: Option<Problem>,
    sources: HashMap<(EntityId, SelectionSource), SourceNavigation>,
    drops: HashMap<i64, Drops>,
    refusal: Option<(i64, SharedString)>,
    reading: Option<Task<()>>,
    watching: Option<Task<()>>,
    acting: Option<(Act, Task<()>)>,
    reading_drops: HashMap<i64, Task<()>>,
    opened: HashSet<i32>,
    located: Option<Location>,
    status: Option<Outcome>,
    taking: Option<Task<()>>,
    shop_warning: Option<ShopWarning>,
    _subscriptions: [Subscription; 3],
}

#[derive(PartialEq, Eq, Hash)]
enum SelectionSource {
    Items(Source),
    Monsters(String),
}

#[derive(Default)]
struct SourceNavigation {
    selected: Option<i64>,
    results: VirtualListState,
    inspector: ScrollHandle,
}

impl LoaderGrabber {
    fn new(
        client: Entity<Client>,
        tab: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.read(cx);
                }
            }),
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
            shelved: HashMap::new(),
            problem: None,
            sources: HashMap::new(),
            drops: HashMap::new(),
            refusal: None,
            reading: None,
            watching: None,
            acting: None,
            reading_drops: HashMap::new(),
            opened: HashSet::new(),
            located: None,
            status: None,
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
        let location = self
            .shown
            .as_ref()
            .and_then(|shown| shown.view.upgrade())
            .and_then(|view| view.read(cx).bar.location().cloned());
        if !switched {
            self.follow(location, cx);
            return;
        }
        self.located = location;
        self.shelved.clear();
        self.drops.clear();
        self.reading_drops.clear();
        self.shop.reset(cx);
        self.quests.reset(cx);
        self.refusal = None;
        self.acting = None;
        self.status = None;
        self.taking = None;
        self.shop_warning = None;
        self.start_over(cx);
    }

    fn follow(&mut self, location: Option<Location>, cx: &mut Context<Self>) {
        if location == self.located {
            return;
        }
        let map = |location: &Option<Location>| location.as_ref().map(|at| at.map.clone());
        let new_map = map(&location) != map(&self.located);
        let entered_or_left = location.is_some() != self.located.is_some();
        self.located = location;
        if new_map {
            self.shelved.remove(&Source::Monsters);
            self.drops.clear();
            self.reading_drops.clear();
        }
        if entered_or_left {
            self.shelved.clear();
        }
        if entered_or_left || (new_map && self.source == Source::Monsters) {
            self.start_over(cx);
        } else if self.source == Source::Monsters {
            self.read(cx);
        }
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

    fn start_over(&mut self, cx: &mut Context<Self>) {
        self.snapshot = None;
        self.open_source(self.source, cx);
    }

    fn open_source(&mut self, source: Source, cx: &mut Context<Self>) {
        if source != self.source {
            self.search.update(cx, |search, cx| search.set_text("", cx));
            if let Some(left) = self.snapshot.take() {
                self.shelved.insert(self.source, left);
            }
            self.snapshot = self.shelved.remove(&source);
        }
        self.source = source;
        self.problem = None;
        self.refusal = None;
        self.status = None;
        self.opened.clear();
        self.read(cx);
        self.watch(cx);
    }

    fn watch(&mut self, cx: &mut Context<Self>) {
        self.watching = None;
        let Some(view) = self.shown.as_ref().and_then(|shown| shown.view.upgrade()) else {
            return;
        };
        let (changed, changes) = async_channel::bounded(1);
        let source = self.source;
        view.update(cx, |view, _| {
            view.session
                .command(Command::Grabber(Job::Watch { source, changed }))
        });
        self.watching = Some(cx.spawn(async move |this, cx| {
            while changes.recv().await.is_ok() {
                cx.background_executor().timer(SETTLE).await;
                let _ = changes.try_recv();
                if this.update(cx, |this, cx| this.read(cx)).is_err() {
                    return;
                }
            }
        }));
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
        let selected = self
            .selection_key()
            .and_then(|scope| self.sources.entry(scope).or_default().selected);
        if let Some(entry) = selected.and_then(|key| self.entry(key)).cloned() {
            self.follow_drops(&entry, cx);
        }
    }

    fn selected(&self) -> Option<i64> {
        self.navigation()?.selected
    }

    fn navigation(&self) -> Option<&SourceNavigation> {
        self.sources.get(&self.selection_key()?)
    }

    fn selection_key(&self) -> Option<(EntityId, SelectionSource)> {
        let target = self.shown.as_ref()?.id;
        let source = match self.source {
            Source::Monsters => {
                let map = self.snapshot.as_ref()?.map.as_ref()?;
                if map != &self.located.as_ref()?.map {
                    return None;
                }
                SelectionSource::Monsters(map.clone())
            }
            source => SelectionSource::Items(source),
        };
        Some((target, source))
    }

    fn entry(&self, key: i64) -> Option<&Entry> {
        self.snapshot
            .as_ref()?
            .entries
            .iter()
            .find(|entry| entry.key == key)
    }

    fn select(&mut self, key: i64, cx: &mut Context<Self>) {
        let Some(scope) = self.selection_key() else {
            return;
        };
        let navigation = self.sources.entry(scope).or_default();
        if navigation.selected.replace(key) != Some(key) {
            navigation.inspector = ScrollHandle::new();
            self.refusal = None;
            self.opened.clear();
        }
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
        if matches!(load, Load::Shop(_)) && settings::shop_warning(cx) {
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
                let problem = match result {
                    Ok(Ok(true)) => None,
                    Ok(Ok(false)) => Some(format!("Couldn't load {described}")),
                    Ok(Err(error)) => Some(error.to_string()),
                    Err(_) => Some("The tab closed".to_owned()),
                };
                let refresh = lists && problem.is_none() && this.source == source;
                if let Some(loader) = this.loader_mut(source) {
                    loader.loading = None;
                    loader.problem = problem.map(SharedString::from);
                }
                if refresh {
                    if let Some(scope) = this.selection_key() {
                        if let Some(key) = first {
                            this.sources.insert(
                                scope,
                                SourceNavigation {
                                    selected: Some(key),
                                    ..Default::default()
                                },
                            );
                        } else {
                            this.sources.remove(&scope);
                        }
                    }
                    this.open_source(source, cx);
                }
                cx.notify();
            });
        });
        if let Some(loader) = self.loader_mut(source) {
            loader.problem = None;
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
        let Some(key) = self.selected().filter(|key| self.entry(*key).is_some()) else {
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
        self.refusal = None;
        let attempted = action.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = answer.recv().await;
            let _ = this.update(cx, |this, cx| {
                this.acting = None;
                let refusal = match result {
                    Ok(Ok(true)) => None,
                    Ok(Ok(false)) => Some(attempted.failed().into()),
                    Ok(Err(error)) => Some(error.to_string().into()),
                    Err(_) => Some("The tab closed".into()),
                };
                this.refusal = refusal.map(|text| (key, text));
                this.read(cx);
            });
        });
        self.acting = Some((Act::Game(action), task));
        cx.notify();
    }

    fn copy(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.status = Some(Outcome::done("Copied"));
        cx.notify();
    }

    fn take_all(&mut self, save: bool, window: &Window, cx: &mut Context<Self>) {
        let source = self.source;
        let Some(answer) = self.send(|reply| Job::Raw { source, reply }, cx) else {
            return;
        };
        self.status = None;
        self.taking = Some(cx.spawn_in(window, async move |this, cx| {
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
                        cx.update(|window, cx| file_panel::save(&directory, &name, window, cx))
                    else {
                        return;
                    };
                    match chosen.await {
                        Some(path) => Some(
                            match cx
                                .background_spawn(async move { std::fs::write(&path, text) })
                                .await
                            {
                                Ok(()) => Outcome::done("Saved"),
                                Err(error) => Outcome::failed(format!("Couldn't save: {error}")),
                            },
                        ),
                        None => None,
                    }
                }
            };
            let _ = this.update(cx, |this, cx| {
                this.taking = None;
                this.status = outcome;
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
        let selected = Source::ALL
            .iter()
            .position(|source| *source == self.source)
            .unwrap_or_default();
        let sources = Tabs::new("grab-sources", "Sources", Source::ALL.len(), |index| {
            Tab::new(Source::ALL[index].name())
        })
        .line()
        .selected(selected)
        .on_select(move |index, _, cx| {
            let _ = this.update(cx, |this, cx| {
                if this.source != Source::ALL[index] {
                    this.open_source(Source::ALL[index], cx);
                }
            });
        });
        theme
            .toolbar()
            .child(div().flex_none().child(sources))
            .child(div().flex_1())
            .child(div().w(SEARCH).min_w(px(120.0)).child(self.search.clone()))
    }

    fn source_header(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let loader = match self.source {
            Source::Shop => &self.shop,
            Source::Quests => &self.quests,
            _ => return None,
        };
        let layout = theme.layout;
        let typography = theme.typography;
        let (name, facts) = match (self.source, &self.snapshot) {
            (
                Source::Shop,
                Some(Snapshot {
                    heading: Some(Heading { name, facts }),
                    ..
                }),
            ) => (Some(name.clone()), Some(facts.clone())),
            (Source::Quests, Some(snapshot)) => match snapshot.entries.len() {
                0 => (None, None),
                1 => (Some("1 quest".to_owned()), None),
                count => (Some(format!("{count} quests")), None),
            },
            _ => (None, None),
        };
        let detail = match &loader.problem {
            Some(problem) => div().text_color(theme.destructive).child(problem.clone()),
            None => div()
                .truncate()
                .text_color(theme.muted_foreground)
                .children(facts),
        };
        Some(
            h_flex()
                .flex_none()
                .gap(layout.space(2.0))
                .pl(layout.space(5.0))
                .pr(layout.space(3.0))
                .pt(layout.space(3.0))
                .pb(layout.space(2.0))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .children(name.map(|name| {
                            div()
                                .id("grab-source-title")
                                .role(gpui::Role::Heading)
                                .aria_level(1)
                                .truncate()
                                .text_title(typography)
                                .child(name)
                        }))
                        .child(detail.text_caption(typography)),
                )
                .child(self.loader_fields(loader, theme, cx)),
        )
    }

    fn loader_fields(&self, loader: &Loader, theme: &Theme, cx: &mut Context<Self>) -> Div {
        let layout = theme.layout;
        let source = self.source;
        let busy = loader.loading.is_some();
        let empty = loader.input.read(cx).text().trim().is_empty();
        let load = Button::new(("load", source as usize), "Load")
            .size(Size::Small)
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
                        .variant(Variant::Secondary)
                        .size(Size::Small)
                        .joined(Joined::Previous)
                        .disabled(busy)
                        .hide_tooltip(),
                )
                .align(Align::End)
                .items(move |_, _| {
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
            .flex_none()
            .gap(layout.space(2.0))
            .child(
                div()
                    .w(px(if source == Source::Shop { 120.0 } else { 160.0 }))
                    .child(loader.input.clone()),
            )
            .child(control)
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

    fn list_column(
        &self,
        theme: &Theme,
        navigation: Option<&SourceNavigation>,
        cx: &mut Context<Self>,
    ) -> Div {
        let layout = theme.layout;
        let entries = self.shown_entries(cx);
        let body = match (&self.snapshot, &self.problem) {
            (None, Some(problem)) => centered(problem_note(problem)),
            (None, None) => div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(Spinner::new("grab-reading").delayed())
                .into_any_element(),
            (Some(snapshot), _) if snapshot.entries.is_empty() => centered(empty_note(self.source)),
            (Some(_), _) if entries.is_empty() => centered(Empty::new(
                "No matches",
                format!(
                    "Nothing here matches “{}”.",
                    self.search.read(cx).text().trim()
                ),
            )),
            (Some(_), _) => {
                let rows: std::rc::Rc<[Entry]> = entries.into();
                let selected = navigation
                    .and_then(|navigation| navigation.selected)
                    .and_then(|key| rows.iter().position(|entry| entry.key == key));
                let (keys, row_theme) = (rows.clone(), theme.clone());
                let this = cx.weak_entity();
                let menu_this = cx.weak_entity();
                let (source, taking) = (self.source, self.taking.is_some());
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.columns(theme))
                    .child(
                        VirtualList::new(
                            "grab-entries",
                            self.source.name(),
                            rows.len(),
                            move |index| {
                                let (rows, this) = (rows.clone(), menu_this.clone());
                                line(&rows[index], &row_theme).context_menu(move |_, _| {
                                    record_menu(&this, &rows[index], source, taking)
                                })
                            },
                        )
                        .selected(selected)
                        .when_some(navigation, |list, navigation| {
                            list.state(&navigation.results)
                        })
                        .flush()
                        .fill()
                        .on_select(move |index, _, cx| {
                            let key = keys[index].key;
                            let _ = this.update(cx, |this, cx| this.select(key, cx));
                        }),
                    )
                    .into_any_element()
            }
        };
        let header = self.source_header(theme, cx);
        v_flex()
            .flex_1()
            .min_w_0()
            .when(header.is_none(), |column| column.pt(layout.space(2.0)))
            .children(header)
            .child(body)
    }

    fn columns(&self, theme: &Theme) -> Div {
        let layout = theme.layout;
        let typography = theme.typography;
        let (heading, color) = match &self.problem {
            Some(Problem::Failed(error)) => (
                SharedString::from(format!("Couldn't refresh: {error}")),
                theme.destructive,
            ),
            _ => ("Name".into(), theme.muted_foreground),
        };
        let [meta, id] = self.source.columns();
        h_flex()
            .flex_none()
            .h(layout.space(6.0))
            .gap(layout.space(2.0))
            .px(layout.space(5.0))
            .text_caption(typography)
            .text_color(theme.muted_foreground)
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

    fn inspector_pane(
        &self,
        theme: &Theme,
        navigation: Option<&SourceNavigation>,
        cx: &mut Context<Self>,
    ) -> Div {
        let layout = theme.layout;
        let entry = navigation.and_then(|navigation| self.entry(navigation.selected?));
        let scroll = entry
            .and(navigation)
            .map(|navigation| &navigation.inspector);
        theme
            .inspector_pane()
            .relative()
            .h_full()
            .child(
                div()
                    .id("grab-inspector")
                    .size_full()
                    .overflow_y_scroll()
                    .when_some(scroll, |pane, scroll| pane.track_scroll(scroll))
                    .p(layout.space(4.0))
                    .pt(layout.space(3.0))
                    .child(self.inspector(theme, entry, cx)),
            )
            .children(
                scroll.map(|scroll| Scrollbar::new("grab-inspector-scrollbar", scroll.clone())),
            )
    }

    fn inspector(
        &self,
        theme: &Theme,
        entry: Option<&Entry>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = theme.layout;
        let typography = theme.typography;
        let Some(entry) = entry else {
            let pickable = self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| !snapshot.entries.is_empty());
            return div()
                .py(layout.space(8.0))
                .text_center()
                .text_caption(typography)
                .text_color(theme.muted_foreground)
                .when(pickable, |note| {
                    note.child("Pick something to see its details and actions.")
                })
                .into_any_element();
        };
        let listed = self.listed();
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
        let refusal = self
            .refusal
            .as_ref()
            .filter(|(key, _)| *key == entry.key)
            .map(|(_, text)| {
                div()
                    .max_w_full()
                    .truncate()
                    .text_caption(typography)
                    .text_color(theme.destructive)
                    .child(text.clone())
            });
        let drops = (self.source == Source::Monsters)
            .then(|| self.drops.get(&entry.key))
            .flatten();
        let unseen = drops.is_none() && entry.drops_from.is_none();
        v_flex()
            .gap(theme.space(4.0))
            .child(
                v_flex()
                    .gap(layout.space(1.5))
                    .child(
                        div()
                            .text_size(typography.body)
                            .line_height(typography.body_line)
                            .font_weight(FontWeight::MEDIUM)
                            .child(entry.name.clone()),
                    )
                    .when(!entry.badges.is_empty(), |header| {
                        header.child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(layout.space(1.0))
                                .children(entry.badges.iter().map(|badge| Badge::new(*badge))),
                        )
                    }),
            )
            .when(!entry.acts.is_empty() || refusal.is_some(), |inspector| {
                inspector.child(
                    h_flex()
                        .flex_wrap()
                        .gap(layout.space(1.5))
                        .children(acts)
                        .children(refusal),
                )
            })
            .child(self.facts(theme, "entry", &entry.facts, cx))
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
                        .child(Spinner::new("grab-drops").size(px(14.0)).delayed())
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

    fn facts(
        &self,
        theme: &Theme,
        scope: impl Into<SharedString>,
        facts: &[(&'static str, String)],
        cx: &mut Context<Self>,
    ) -> Div {
        let layout = theme.layout;
        let typography = theme.typography;
        let scope = scope.into();
        let all: std::rc::Rc<str> = facts
            .iter()
            .map(|(label, value)| format!("{label}: {value}"))
            .collect::<Vec<_>>()
            .join("\n")
            .into();
        v_flex().children(facts.iter().enumerate().map(|(index, (label, value))| {
            let selector = format!("{scope}-fact-{index}");
            let this = cx.weak_entity();
            let copies = [
                (format!("Copy {label}"), value.clone()),
                ("Copy all facts".to_owned(), all.to_string()),
            ];
            ContextMenu::new((scope.clone(), index), *label)
                .items(move |_, _| {
                    copies
                        .iter()
                        .map(|(item, text)| {
                            let (this, text) = (this.clone(), text.clone());
                            MenuItem::action(item.clone(), move |_, cx| {
                                let _ = this.update(cx, |this, cx| this.copy(text.clone(), cx));
                            })
                        })
                        .collect()
                })
                .child(
                    h_flex()
                        .debug_selector(move || selector.clone())
                        .items_start()
                        .gap(layout.space(3.0))
                        .py(px(2.0))
                        .text_body(typography)
                        .child(
                            div()
                                .flex_none()
                                .w(LABEL_WIDTH)
                                .text_color(theme.muted_foreground)
                                .child(*label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_features(tabular())
                                .child(value.clone()),
                        ),
                )
        }))
    }

    fn detailed_line(
        &self,
        theme: &Theme,
        id: usize,
        line: &Line,
        register: Option<AnyElement>,
        registered: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = theme.layout;
        let typography = theme.typography;
        let key = line.item_id.unwrap_or(id as i32);
        let open = self.opened.contains(&key);
        let small = |text: String, color: Hsla| {
            div()
                .flex_none()
                .text_caption(typography)
                .font_features(tabular())
                .text_color(color)
                .child(text)
        };
        let row = h_flex()
            .id(("grab-line-row", id))
            .debug_selector(move || format!("grab-line-{id}"))
            .group(LINE)
            .gap(layout.space(1.5))
            .h(layout.control)
            .mx(layout.space(-1.0))
            .px(layout.space(1.0))
            .rounded(theme.radius.control)
            .pointer_cursor(theme)
            .hover(|style| style.bg(theme.muted))
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
                    .gap(layout.space(1.5))
                    .child(div().min_w_0().truncate().child(line.name.clone()))
                    .when(line.quantity > 1, |name| {
                        let amount = if line.varies {
                            format!("×1–{}", grouped(line.quantity))
                        } else {
                            format!("×{}", grouped(line.quantity))
                        };
                        name.child(small(amount, theme.muted_foreground))
                    }),
            )
            .children(
                line.item_id
                    .map(|item_id| small(item_id.to_string(), theme.muted_foreground)),
            )
            .child(
                small(
                    line.chance.map(percent).unwrap_or_default(),
                    theme.muted_foreground,
                )
                .min_w(px(32.0))
                .text_right(),
            )
            .children(register);
        v_flex()
            .id(("grab-line", id))
            .child(self.line_menu(id, line, registered, row, cx))
            .when(open, |line_view| {
                line_view.child(
                    div()
                        .pl(px(26.0))
                        .pt(layout.space(0.5))
                        .pb(layout.space(2.0))
                        .child(self.facts(theme, format!("line-{id}"), &line.facts, cx)),
                )
            })
            .into_any_element()
    }

    fn line_menu(
        &self,
        id: usize,
        line: &Line,
        registered: bool,
        row: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> ContextMenu {
        let this = cx.weak_entity();
        let mut copies = vec![("Copy name", line.name.clone())];
        copies.extend(
            line.item_id
                .map(|item_id| ("Copy item ID", item_id.to_string())),
        );
        let register = (line.item_id.is_some() && !registered).then(|| Act::RegisterDrop {
            name: line.name.clone(),
        });
        ContextMenu::new(("grab-line-menu", id), line.name.clone())
            .items(move |_, _| {
                let copy = copies.iter().map(|(label, text)| {
                    let (this, text) = (this.clone(), text.clone());
                    MenuItem::action(*label, move |_, cx| {
                        let _ = this.update(cx, |this, cx| this.copy(text.clone(), cx));
                    })
                });
                let register = register.iter().flat_map(|act| {
                    let (this, act) = (this.clone(), act.clone());
                    [
                        MenuItem::separator(),
                        MenuItem::action("Register drop", move |_, cx| {
                            let _ = this.update(cx, |this, cx| this.act(act.clone(), cx));
                        }),
                    ]
                });
                copy.chain(register).collect()
            })
            .child(row)
    }

    fn group(
        &self,
        theme: &Theme,
        index: usize,
        group: &Group,
        listed: &Listed,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = theme.layout;
        let typography = theme.typography;
        let rows = group.lines.iter().enumerate().map(|(line_index, line)| {
            let id = index * 1000 + line_index;
            let registered = listed.has_drop(&line.name);
            let register = line.item_id.map(|_| {
                let slot = h_flex()
                    .flex_none()
                    .size(layout.control_xs)
                    .justify_center();
                if registered {
                    return slot
                        .id(("grab-line-registered", id))
                        .aria_label(format!("{} is a registered drop", line.name))
                        .child(icon(Icon::Check, theme.muted_foreground).size(px(12.0)))
                        .into_any_element();
                }
                let act = Act::RegisterDrop {
                    name: line.name.clone(),
                };
                slot.child(
                    Button::icon(
                        ("grab-line-add", id),
                        Icon::Plus,
                        format!("Register {} as a drop", line.name),
                    )
                    .size(Size::XSmall)
                    .icon_size(Size::Small.icon_size())
                    .revealed_by(LINE)
                    .tooltip("Register drop")
                    .on_click(cx.listener(move |this, _, _, cx| this.act(act.clone(), cx))),
                )
                .into_any_element()
            });
            if !line.facts.is_empty() {
                return self.detailed_line(theme, id, line, register, registered, cx);
            }
            let (amount, color) = amount(theme, line);
            let row = h_flex()
                .id(("grab-line", id))
                .debug_selector(move || format!("grab-line-{id}"))
                .group(LINE)
                .gap(layout.space(2.0))
                .h(layout.control)
                .child(div().flex_1().min_w_0().truncate().child(line.name.clone()))
                .children(detail(line).map(|text| {
                    div()
                        .flex_none()
                        .text_caption(typography)
                        .font_features(tabular())
                        .text_color(theme.muted_foreground)
                        .child(text)
                }))
                .child(
                    div()
                        .flex_none()
                        .min_w(px(32.0))
                        .text_right()
                        .text_caption(typography)
                        .font_features(tabular())
                        .text_color(color)
                        .child(amount),
                )
                .children(register);
            self.line_menu(id, line, registered, row, cx)
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
        Act::Game(Action::FakeComplete { .. }) => Some("Marks it done on this client only"),
        Act::Register { .. } | Act::RegisterDrop { .. } | Act::Game(_) => None,
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
    let layout = theme.layout;
    ListItem::new(entry.name.clone()).end_slot(
        h_flex()
            .gap(layout.space(2.0))
            .child(
                div()
                    .w(META)
                    .text_right()
                    .truncate()
                    .when(entry.here, |meta| meta.text_color(theme.foreground))
                    .child(entry.meta.clone()),
            )
            .child(div().w(ID).text_right().child(entry.id.clone())),
    )
}

fn record_menu(
    this: &WeakEntity<LoaderGrabber>,
    entry: &Entry,
    source: Source,
    taking: bool,
) -> Vec<MenuItem> {
    let copies = [
        ("Copy name".to_owned(), entry.name.clone()),
        (copy_id(entry.id_label), entry.id.clone()),
        ("Copy JSON".to_owned(), entry.json.clone()),
    ];
    let whole = source.name().to_lowercase();
    copies
        .into_iter()
        .map(|(label, text)| {
            let this = this.clone();
            MenuItem::action(label, move |_, cx| {
                let _ = this.update(cx, |this, cx| this.copy(text.clone(), cx));
            })
        })
        .chain([MenuItem::separator()])
        .chain(
            [
                (format!("Copy {whole} JSON"), false),
                (format!("Export {whole} JSON…"), true),
            ]
            .into_iter()
            .map(|(label, save)| {
                let this = this.clone();
                MenuItem::action(label, move |window, cx| {
                    let _ = this.update(cx, |this, cx| this.take_all(save, window, cx));
                })
                .disabled(taking)
            }),
        )
        .collect()
}

fn amount(theme: &Theme, line: &Line) -> (String, Hsla) {
    let held = line.held.total();
    if line.needed {
        let color = if held >= line.quantity {
            theme.foreground
        } else {
            theme.muted_foreground
        };
        (
            format!("{}/{}", grouped(held), grouped(line.quantity)),
            color,
        )
    } else {
        (
            format!("×{}", grouped(line.quantity)),
            theme.muted_foreground,
        )
    }
}

fn detail(line: &Line) -> Option<String> {
    let mut parts = Vec::new();
    if line.needed {
        if line.held.bank > 0 {
            parts.push(format!("{} in bank", grouped(line.held.bank)));
        }
    } else {
        if let Some(chance) = line.chance {
            parts.push(percent(chance));
        }
        let held = line.held.total();
        if held > 0 {
            parts.push(format!("{} owned", grouped(held)));
        }
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn caption(theme: &Theme, title: &'static str) -> Div {
    let layout = theme.layout;
    let typography = theme.typography;
    v_flex().gap(layout.space(1.0)).child(
        div()
            .text_caption(typography)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.muted_foreground)
            .child(title),
    )
}

fn quiet(theme: &Theme, text: &'static str) -> Div {
    let typography = theme.typography;
    div()
        .text_caption(typography)
        .text_color(theme.muted_foreground)
        .child(text)
}

fn centered(empty: Empty) -> AnyElement {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .child(empty)
        .into_any_element()
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
        let layout = theme.layout;
        let typography = theme.typography;
        let header = shown_tab::header(
            "Loader/grabber",
            &self.client,
            self.shown.as_ref(),
            self.tab,
            Self::retarget,
            window,
            cx,
        )
        .children(self.status.as_ref().map(|outcome| {
            div()
                .id("grab-status")
                .role(gpui::Role::Status)
                .aria_label(outcome.text.clone())
                .min_w_0()
                .max_w(px(360.0))
                .pr(layout.space(4.0))
                .truncate()
                .text_body(typography)
                .text_color(outcome.color(&theme))
                .child(outcome.text.clone())
        }));
        let has_tab = self.shown.is_some();
        let body = if has_tab {
            let navigation = self.navigation();
            let empty = match &self.snapshot {
                Some(snapshot) => snapshot.entries.is_empty(),
                None => self.problem.is_some(),
            };
            v_flex()
                .flex_1()
                .min_h_0()
                .child(self.toolbar(&theme, cx))
                .child(
                    h_flex()
                        .items_stretch()
                        .flex_1()
                        .min_h_0()
                        .child(self.list_column(&theme, navigation, cx))
                        .when(!empty, |panes| {
                            panes.child(self.inspector_pane(&theme, navigation, cx))
                        }),
                )
        } else {
            v_flex().flex_1().child(centered(Empty::new(
                "No tabs open",
                "Open a tab to load and grab from it.",
            )))
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .window_surface(&theme)
            .key_context(FOCUS_SCOPE)
            .id("loader-grabber")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseSession, window, _| window.remove_window()))
            .when(has_tab, |root| {
                root.on_action(Find::in_search(&self.search))
            })
            .child(mira_ui::nav::pointer_probe())
            .child(crate::quit::Overlay)
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
            let mut grabber = LoaderGrabber::new(client, Target::Selected, window, cx);
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

    fn results(key: i64, name: &str) -> Snapshot {
        Snapshot {
            heading: None,
            map: None,
            entries: vec![Entry {
                key,
                name: name.into(),
                id_label: "Item ID",
                id: key.to_string(),
                meta: String::new(),
                badges: Vec::new(),
                facts: Vec::new(),
                groups: Vec::new(),
                acts: Vec::new(),
                here: false,
                drops_from: None,
                json: "{}".into(),
            }],
        }
    }

    fn tall_shop() -> Snapshot {
        let mut shop = results(12, "Potion");
        shop.heading = Some(Heading {
            name: "Blacksmith Shop".into(),
            facts: "Shop ID 1234 · 53 items · Merge".into(),
        });
        shop.entries[0].facts = (0..140)
            .map(|i| ("Ingredient", format!("Ingredient {i}")))
            .collect();
        shop.entries
            .extend((13..65).flat_map(|key| results(key, &format!("Item {key}")).entries));
        shop
    }

    #[gpui::test]
    fn returning_to_a_source_restores_its_record_and_scroll_positions(cx: &mut TestAppContext) {
        let (grabber, cx) = open(cx);
        grabber.update(cx, |grabber, cx| {
            grabber.take(tall_shop(), cx);
            cx.notify();
        });
        cx.run_until_parked();
        let pick = |cx: &mut VisualTestContext| {
            let row = cx
                .debug_bounds("row-0")
                .expect("the source shows its record");
            cx.simulate_click(row.center(), gpui::Modifiers::none());
            cx.run_until_parked();
        };
        let shown = |cx: &mut VisualTestContext| {
            grabber.read_with(cx, |grabber, _| {
                grabber
                    .selected()
                    .and_then(|key| grabber.entry(key))
                    .map(|entry| entry.name.clone())
            })
        };
        pick(cx);
        assert_eq!(shown(cx).as_deref(), Some("Potion"));
        let position = cx.debug_bounds("row-0").unwrap().center();
        cx.simulate_event(gpui::ScrollEvent {
            position,
            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-260.0))),
            ..Default::default()
        });
        let offset = gpui::point(px(0.0), px(-120.0));
        grabber.update(cx, |grabber, cx| {
            let scroll = &grabber.navigation().unwrap().inspector;
            assert!(scroll.max_offset().y >= -offset.y);
            scroll.set_offset(offset);
            cx.notify();
        });
        cx.run_until_parked();
        let place = cx.debug_bounds("row-12").unwrap();
        assert!(cx.debug_bounds("row-0").is_none());

        grabber.update(cx, |grabber, cx| {
            grabber.open_source(Source::Inventory, cx);
            grabber.take(results(12, "Sword"), cx);
        });
        cx.run_until_parked();
        assert_eq!(shown(cx), None);
        pick(cx);
        assert_eq!(shown(cx).as_deref(), Some("Sword"));

        grabber.update(cx, |grabber, cx| grabber.open_source(Source::Shop, cx));
        cx.run_until_parked();
        assert_eq!(shown(cx).as_deref(), Some("Potion"));
        assert_eq!(cx.debug_bounds("row-12"), Some(place));
        grabber.read_with(cx, |grabber, _| {
            assert_eq!(grabber.navigation().unwrap().inspector.offset(), offset)
        });

        let location = |map: &str| Location {
            map: map.into(),
            room: None,
            cell: "Enter".into(),
            pad: "Spawn".into(),
            cells: Vec::new(),
            cell_pads: Vec::new(),
        };
        grabber.update(cx, |grabber, cx| {
            grabber.located = Some(location("forest"));
            grabber.open_source(Source::Monsters, cx);
            grabber.take(
                Snapshot {
                    map: Some("forest".into()),
                    ..results(12, "Wolf")
                },
                cx,
            );
        });
        cx.run_until_parked();
        pick(cx);
        assert_eq!(shown(cx).as_deref(), Some("Wolf"));

        grabber.update(cx, |grabber, cx| {
            grabber.take(
                Snapshot {
                    map: Some("graveyard".into()),
                    ..results(12, "Skeleton")
                },
                cx,
            );
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(shown(cx), None);
    }

    fn located(map: &str) -> Location {
        Location {
            map: map.into(),
            room: None,
            cell: "Enter".into(),
            pad: "Spawn".into(),
            cells: Vec::new(),
            cell_pads: Vec::new(),
        }
    }

    #[gpui::test]
    fn logging_in_or_out_reads_the_shown_source_again(cx: &mut TestAppContext) {
        let (grabber, cx) = open(cx);
        grabber.update(cx, |grabber, cx| {
            grabber.problem = Some(Problem::NotInGame);
            grabber.follow(Some(located("battleon")), cx);
        });
        assert!(grabber.read_with(cx, |grabber, _| grabber.problem.is_none()));

        grabber.update(cx, |grabber, cx| {
            grabber.take(blacksmith(), cx);
            grabber.follow(None, cx);
        });
        assert!(grabber.read_with(cx, |grabber, _| grabber.snapshot.is_none()));
    }

    #[gpui::test]
    fn loading_a_shop_warns_until_told_not_to(cx: &mut TestAppContext) {
        let (grabber, cx) = open(cx);
        focus_shop_field(&grabber, cx);
        press(cx, "cmd-f");
        typed(cx, "chest");
        assert_eq!(
            grabber.read_with(cx, |grabber, cx| grabber.search.read(cx).text().to_owned()),
            "chest"
        );

        focus_shop_field(&grabber, cx);
        typed(cx, "1234");
        grabber.update(cx, |grabber, cx| {
            grabber.load(Source::Shop, LoadKind::HairShop, cx)
        });
        assert_eq!(shop_warning(&grabber, cx), None);
        press(cx, "enter");
        assert_eq!(
            shop_warning(&grabber, cx),
            Some(("shop 1234".into(), false))
        );

        let dialog_focus = cx.update(|window, cx| window.focused(cx).unwrap());
        press(cx, "cmd-f");
        assert!(cx.update(|window, _| dialog_focus.is_focused(window)));
        press(cx, "escape");
        assert_eq!(shop_warning(&grabber, cx), None);
        assert!(cx.update(|_, cx| settings::shop_warning(cx)));

        press(cx, "enter");
        assert_eq!(
            shop_warning(&grabber, cx),
            Some(("shop 1234".into(), false))
        );
        press(cx, "shift-tab");
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
    fn snapshot_scrolled_source() {
        let folder = crate::storyboard::scratch("grabber-scrolled");
        for written in mira_ui::snapshot_with(
            "grabber-scrolled",
            gpui::size(px(WIDTH), px(HEIGHT)),
            |window, cx| {
                cx.new(|cx| {
                    let client = Client::empty_for_tests(&folder, cx);
                    let mut grabber = LoaderGrabber::new(client, Target::Selected, window, cx);
                    grabber.shown = Some(ShownTab::fake("Alt 1", Automation::default()));
                    grabber.take(tall_shop(), cx);
                    grabber.select(12, cx);
                    grabber
                })
            },
            |cx, window| {
                cx.update_window(window, |root, window, cx| {
                    root.downcast::<LoaderGrabber>()
                        .unwrap()
                        .update(cx, |grabber, _| {
                            grabber
                                .navigation()
                                .unwrap()
                                .inspector
                                .set_offset(gpui::point(px(0.0), px(-120.0)));
                        });
                    window.dispatch_event(
                        gpui::PlatformInput::Scroll(gpui::ScrollEvent {
                            position: gpui::point(px(100.0), px(180.0)),
                            delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-260.0))),
                            ..Default::default()
                        }),
                        cx,
                    );
                })
                .unwrap();
            },
        ) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }

    fn blacksmith() -> Snapshot {
        let item = |key: i32, name: &str, price: &str, badges: Vec<&'static str>| Entry {
            key: i64::from(key),
            name: name.into(),
            id_label: "Item ID",
            id: (8800 + key).to_string(),
            meta: price.into(),
            badges,
            facts: vec![
                ("Item ID", (8800 + key).to_string()),
                ("Shop item ID", (44100 + key).to_string()),
                ("Category", "Sword".into()),
                ("Price", price.into()),
            ],
            groups: Vec::new(),
            acts: vec![
                Act::Game(Action::Buy {
                    shop_item_id: 44100 + key,
                })
                .into(),
            ],
            here: false,
            drops_from: None,
            json: "{}".into(),
        };
        Snapshot {
            heading: Some(Heading {
                name: "Blacksmith Shop".into(),
                facts: "Shop ID 1234 · 6 items · Merge".into(),
            }),
            map: None,
            entries: vec![
                item(1, "Blade of Awe", "120,000 gold", Vec::new()),
                item(2, "Dragon Blade", "40 AC", vec!["Member"]),
                item(3, "Necrotic Sword of Doom", "250 AC", vec!["Member"]),
                item(4, "Iron Helm", "2,500 gold", Vec::new()),
                item(5, "Steel Cape", "9,000 gold", Vec::new()),
                item(6, "Rusty Dagger", "10 gold", Vec::new()),
            ],
        }
    }

    #[gpui::test]
    fn a_record_copies_from_its_row_menu_and_its_facts(cx: &mut TestAppContext) {
        use gpui::{Modifiers, MouseButton};
        let (grabber, cx) = open(cx);
        grabber.update(cx, |grabber, cx| {
            grabber.take(blacksmith(), cx);
            grabber.select(2, cx);
        });
        cx.run_until_parked();
        let clipboard =
            |cx: &mut VisualTestContext| cx.read_from_clipboard().and_then(|item| item.text());

        let third = cx.debug_bounds("row-2").unwrap().center();
        cx.simulate_mouse_down(third, MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_up(third, MouseButton::Right, Modifiers::none());
        cx.run_until_parked();
        press(cx, "down");
        press(cx, "down");
        press(cx, "enter");
        assert_eq!(clipboard(cx).as_deref(), Some("8803"));

        let fact = cx.debug_bounds("entry-fact-1").unwrap().center();
        let copy_from_fact = |cx: &mut VisualTestContext, item: usize| {
            cx.simulate_mouse_down(fact, MouseButton::Right, Modifiers::none());
            cx.simulate_mouse_up(fact, MouseButton::Right, Modifiers::none());
            cx.run_until_parked();
            for _ in 0..=item {
                press(cx, "down");
            }
            press(cx, "enter");
        };
        copy_from_fact(cx, 0);
        assert_eq!(clipboard(cx).as_deref(), Some("44102"));
        copy_from_fact(cx, 1);
        assert_eq!(
            clipboard(cx).as_deref(),
            Some("Item ID: 8802\nShop item ID: 44102\nCategory: Sword\nPrice: 40 AC")
        );
    }

    #[test]
    fn snapshot_loaded_shop() {
        let folder = crate::storyboard::scratch("grabber-shop");
        for written in mira_ui::snapshot_with(
            "grabber-shop",
            gpui::size(px(WIDTH), px(HEIGHT)),
            |window, cx| {
                cx.new(|cx| {
                    let client = Client::empty_for_tests(&folder, cx);
                    let mut grabber = LoaderGrabber::new(client, Target::Selected, window, cx);
                    grabber.shown = Some(ShownTab::fake("Alt 1", Automation::default()));
                    grabber.take(blacksmith(), cx);
                    grabber.select(2, cx);
                    grabber
                })
            },
            right_click_at(690.0, 205.0),
        ) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }

    fn line(item_id: i32, name: &str, quantity: i32) -> Line {
        Line {
            item_id: Some(item_id),
            name: name.into(),
            quantity,
            chance: None,
            needed: false,
            held: Default::default(),
            varies: false,
            facts: Vec::new(),
        }
    }

    fn record(key: i64, name: &str, id_label: &'static str) -> Entry {
        Entry {
            id_label,
            ..results(key, name).entries.remove(0)
        }
    }

    fn right_click_at(
        x: f32,
        y: f32,
    ) -> impl Fn(&mut gpui::HeadlessAppContext, gpui::AnyWindowHandle) {
        move |cx, window| {
            let (position, button) = (gpui::point(px(x), px(y)), gpui::MouseButton::Right);
            cx.update_window(window, |_, window, cx| {
                for event in [
                    gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                        button,
                        position,
                        click_count: 1,
                        ..Default::default()
                    }),
                    gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                        button,
                        position,
                        click_count: 1,
                        ..Default::default()
                    }),
                ] {
                    window.dispatch_event(event, cx);
                }
                window.refresh();
            })
            .unwrap();
            cx.run_until_parked();
        }
    }

    fn frogzard() -> Entry {
        Entry {
            meta: "Enter".into(),
            here: true,
            drops_from: Some(3.0),
            facts: vec![
                ("MonMapID", "3".into()),
                ("Monster ID", "2045".into()),
                ("Cell", "Enter".into()),
                ("Level", "25".into()),
            ],
            ..record(3, "Frogzard", "MonMapID")
        }
    }

    fn frogzard_drops() -> Group {
        Group {
            title: "Drops",
            lines: vec![
                Line {
                    chance: Some(2.5),
                    facts: vec![("Category", "Helm".into()), ("Rarity", "Rare".into())],
                    ..line(12, "Frog Hat", 1)
                },
                Line {
                    chance: Some(50.0),
                    varies: true,
                    facts: vec![("Category", "Item".into())],
                    ..line(13, "Frogzard Scale", 4)
                },
            ],
        }
    }

    #[gpui::test]
    fn a_drop_row_copies_and_registers_from_its_menu(cx: &mut TestAppContext) {
        use gpui::{Modifiers, MouseButton};
        let (grabber, cx) = open(cx);
        grabber.update(cx, |grabber, cx| {
            grabber.source = Source::Monsters;
            grabber.located = Some(located("battleontown"));
            grabber.take(
                Snapshot {
                    heading: None,
                    map: Some("battleontown".into()),
                    entries: vec![frogzard()],
                },
                cx,
            );
            grabber.select(3, cx);
            grabber.drops.insert(3, Drops::Ready(frogzard_drops()));
            cx.notify();
        });
        cx.run_until_parked();
        let scale = cx.debug_bounds("grab-line-1").unwrap().center();
        let pick = |cx: &mut VisualTestContext, item: usize| {
            cx.simulate_mouse_down(scale, MouseButton::Right, Modifiers::none());
            cx.simulate_mouse_up(scale, MouseButton::Right, Modifiers::none());
            cx.run_until_parked();
            for _ in 0..=item {
                press(cx, "down");
            }
            press(cx, "enter");
        };

        pick(cx, 1);
        let copied = cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(copied.as_deref(), Some("13"));

        pick(cx, 2);
        assert!(grabber.read_with(cx, |grabber, _| grabber.listed().has_drop("Frogzard Scale")));
    }

    #[test]
    fn snapshot_quest_and_drops() {
        const LINE_Y: f32 = 300.0;
        let folder = crate::storyboard::scratch("grabber-records");
        let tab = || {
            let automation = Automation::default();
            automation.update_drops(|state| {
                register_items(&mut state.items, vec![ItemQuery::Name("Frog Hat".into())])
            });
            ShownTab::fake("Alt 1", automation)
        };
        let quest = Entry {
            meta: "In progress".into(),
            badges: vec!["Daily"],
            facts: vec![
                ("Quest ID", "4570".into()),
                ("Status", "In progress".into()),
            ],
            groups: vec![
                Group {
                    title: "Requirements",
                    lines: vec![
                        Line {
                            needed: true,
                            held: crate::domain::grabber::Holding {
                                inventory: 3,
                                temp: 0,
                                bank: 4,
                                ..Default::default()
                            },
                            ..line(1, "Bone Dust", 10)
                        },
                        Line {
                            needed: true,
                            held: crate::domain::grabber::Holding {
                                inventory: 0,
                                temp: 5,
                                bank: 0,
                                ..Default::default()
                            },
                            ..line(2, "Slime Sample", 5)
                        },
                    ],
                },
                Group {
                    title: "Rewards",
                    lines: vec![
                        Line {
                            chance: Some(25.0),
                            ..line(11, "Dragon Blade", 1)
                        },
                        line(12, "Frog Hat", 1),
                    ],
                },
            ],
            acts: vec![
                Act::Register { quest_id: 4570 }.into(),
                Act::Game(Action::FakeComplete { quest_id: 4570 }).into(),
            ],
            ..record(4570, "Gather Bone Dust", "Quest ID")
        };
        let (monster, drops) = (frogzard(), frogzard_drops());
        let size = gpui::size(px(WIDTH), px(HEIGHT));
        let hover_line = |cx: &mut gpui::HeadlessAppContext, window: gpui::AnyWindowHandle| {
            for nudge in [0.0, 1.0] {
                cx.update_window(window, |_, window, cx| {
                    window.dispatch_event(
                        gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                            position: gpui::point(px(640.0 + nudge), px(LINE_Y)),
                            ..Default::default()
                        }),
                        cx,
                    );
                    window.refresh();
                })
                .unwrap();
                cx.run_until_parked();
            }
        };
        for written in mira_ui::snapshot_with(
            "grabber-quest",
            size,
            |window, cx| {
                let quest = quest.clone();
                cx.new(|cx| {
                    let client = Client::empty_for_tests(&folder, cx);
                    let mut grabber = LoaderGrabber::new(client, Target::Selected, window, cx);
                    grabber.shown = Some(tab());
                    grabber.source = Source::Quests;
                    grabber.take(
                        Snapshot {
                            heading: None,
                            map: None,
                            entries: vec![quest],
                        },
                        cx,
                    );
                    grabber.select(4570, cx);
                    grabber
                })
            },
            hover_line,
        )
        .into_iter()
        .chain(mira_ui::snapshot_with(
            "grabber-drops",
            size,
            |window, cx| {
                let (monster, drops) = (monster.clone(), drops.clone());
                cx.new(|cx| {
                    let client = Client::empty_for_tests(&folder, cx);
                    let mut grabber = LoaderGrabber::new(client, Target::Selected, window, cx);
                    grabber.shown = Some(tab());
                    grabber.source = Source::Monsters;
                    grabber.located = Some(located("battleontown"));
                    grabber.take(
                        Snapshot {
                            heading: None,
                            map: Some("battleontown".into()),
                            entries: vec![monster],
                        },
                        cx,
                    );
                    grabber.select(3, cx);
                    grabber.drops.insert(3, Drops::Ready(drops));
                    grabber.opened.insert(12);
                    grabber
                })
            },
            right_click_at(700.0, 359.0),
        )) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }

    #[test]
    fn snapshot_narrow_toolbar() {
        let folder = std::env::temp_dir().join(format!("mira-grabber-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let size = gpui::size(px(MIN_WIDTH), px(MIN_HEIGHT));
        for written in mira_ui::snapshot("grabber-narrow", size, move |window, cx| {
            cx.new(|cx| {
                let client = Client::empty_for_tests(&folder, cx);
                let mut grabber = LoaderGrabber::new(client, Target::Selected, window, cx);
                grabber.shown = Some(ShownTab::fake("Alt 1", Automation::default()));
                grabber.source = Source::Quests;
                let quest = |key: i64, name: &str, status: &str| Entry {
                    id_label: "Quest ID",
                    meta: status.into(),
                    ..results(key, name).entries.remove(0)
                };
                grabber.take(
                    Snapshot {
                        heading: None,
                        map: None,
                        entries: vec![
                            quest(4567, "The Dragon's Hoard of Unreasonably Long Names", ""),
                            quest(4570, "Gather Bone Dust", "In progress"),
                            quest(4571, "Slay the Lich", "Done"),
                        ],
                    },
                    cx,
                );
                grabber.select(4570, cx);
                grabber.quests.problem = Some("IDs are whole numbers above 0".into());
                grabber.status = Some(Outcome {
                    text: "Couldn't save: the volume is read-only".into(),
                    failed: true,
                });
                grabber
            })
        }) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }

    #[test]
    fn a_quick_read_shows_no_spinner_and_a_slow_one_waits_before_showing_it() {
        use std::time::Duration;
        let ms = Duration::from_millis;
        let folder = crate::storyboard::scratch("grabber-spinner");
        let recording = mira_ui::record(
            "grabber-spinner",
            gpui::size(px(WIDTH), px(HEIGHT)),
            ms(1500),
            |window, cx| {
                cx.new(|cx| {
                    let client = Client::empty_for_tests(&folder, cx);
                    let mut grabber = LoaderGrabber::new(client, Target::Selected, window, cx);
                    grabber.shown = Some(ShownTab::fake("Alt 1", Automation::default()));
                    grabber.take(blacksmith(), cx);
                    grabber
                })
            },
            |cx, window| {
                cx.update_window(window, |root, _, cx| {
                    let grabber = root.downcast::<LoaderGrabber>().unwrap();
                    grabber.update(cx, |grabber, cx| {
                        grabber.open_source(Source::Inventory, cx);
                        cx.spawn(async move |this, cx| {
                            let mut at = Duration::ZERO;
                            for (when, step) in [
                                (80, Step::Rows(results(1, "Bone Dust"))),
                                (300, Step::Open(Source::Bank)),
                                (900, Step::Rows(results(4, "Bone Dust"))),
                                (1000, Step::Open(Source::TempInventory)),
                            ] {
                                cx.background_executor().timer(ms(when) - at).await;
                                at = ms(when);
                                let _ = this.update(cx, |grabber, cx| match step {
                                    Step::Rows(rows) => grabber.take(rows, cx),
                                    Step::Open(source) => grabber.open_source(source, cx),
                                });
                            }
                        })
                        .detach();
                    });
                })
                .unwrap();
            },
        );
        let between = |from: u64, to: u64| {
            recording
                .frames
                .iter()
                .filter(move |frame| frame.at > ms(from) && frame.at < ms(to))
        };
        let still = |from, to| between(from, to).all(|frame| frame.changed == 0);
        let spinning = |from, to| between(from, to).all(|frame| frame.changed > 0);
        assert!(still(20, 75), "a quick read drew something while it waited");
        assert!(still(320, 595), "a slow read showed its spinner too soon");
        assert!(spinning(620, 890), "a slow read never showed its spinner");
        assert!(
            still(1020, 1295),
            "the next slow read showed its spinner at once"
        );
        assert!(
            spinning(1320, 1500),
            "the next slow read never showed its spinner"
        );
    }

    enum Step {
        Rows(Snapshot),
        Open(Source),
    }

    #[test]
    fn snapshot_shop_warning() {
        let folder = std::env::temp_dir().join(format!("mira-grabber-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let size = gpui::size(px(WIDTH), px(HEIGHT));
        for written in mira_ui::snapshot("grabber-shop-warning", size, move |window, cx| {
            cx.new(|cx| {
                let client = Client::empty_for_tests(&folder, cx);
                let mut grabber = LoaderGrabber::new(client, Target::Selected, window, cx);
                grabber.shown = Some(ShownTab::fake("Alt 1", Automation::default()));
                grabber.take(
                    Snapshot {
                        heading: None,
                        map: None,
                        entries: Vec::new(),
                    },
                    cx,
                );
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
