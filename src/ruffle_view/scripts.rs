use mira_ui::StyledExt as _;
use std::{
    collections::{HashMap, HashSet},
    fs, io, iter,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Div, Entity, FocusHandle, Focusable as _,
    FontWeight, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _,
    Pixels, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, TextRun, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowHandle,
    WindowOptions, actions, div, linear_color_stop, linear_gradient, prelude::FluentBuilder as _,
    px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, Theme, Tone,
    components::{Disclosure, Empty, StatusLabel},
    h_flex,
    primitives::{
        Activity, Alert, Align, Button, Checkbox, CheckedState, DropdownMenu, HoverCard, Icon,
        Input, InputEvent, List, ListItem, MenuItem, Placement, Popover, Scrollbar, Select, Side,
        Switch, Tab, Tabs, Toaster, VirtualList,
        button::{Size, Variant},
        icon,
    },
    theme::RING_ROOM,
    v_flex,
};

use crate::file_panel::{self, Pick};

use super::{
    Changed, Client, RuffleView, ScriptPhase, Updated,
    account::file_name,
    client::{CloseSession, drag_area},
    core_bots::CoreBotsPage,
    form::{note, row, row_label},
    package_dialogs::{PackageDialogs, PackageEvent, Pending},
    queue::{self, EntryId, Queue},
    script::failure,
    script_form::{Draft, FieldDraft, ROOM_NOTE, drafts, error, room_text, rows, starting, values},
    script_names,
    shown_tab::{self, ShownTab, Target},
};
use crate::{
    account_settings::{self, Scripts as ScriptOptions},
    accounts::AccountId,
    domain::rooms::{PrivateRoom, Rooms},
    packages::{self, Manifest, ManifestError, Origin, Status},
    script_inputs,
    scripting::{
        self, Declared, Definition, ExtractError, Kind, Launch, Meta, Outcome, Supplied, Values,
    },
    settings, shortcuts,
};

actions!(mira, [OpenScripts, LoadScript, AddToQueue]);

const CONTEXT: &str = "Scripts";

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([gpui::KeyBinding::new(
        "cmd-enter",
        AddToQueue,
        Some(CONTEXT),
    )]);
}

const WIDTH: f32 = 880.0;
const HEIGHT: f32 = 560.0;
const MIN_WIDTH: f32 = 800.0;
const MIN_HEIGHT: f32 = 400.0;
const PADDING: Pixels = px(16.0);
const QUEUE_RUN_WIDTH: Pixels = px(104.0);
const PATIENCE: Duration = Duration::from_millis(150);
const SETTLE: Duration = Duration::from_millis(150);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    path: PathBuf,
    name: SharedString,
    description: Option<SharedString>,
    tags: Arc<[SharedString]>,
    folder: Option<SharedString>,
    source: Option<usize>,
    searched: SharedString,
}

impl Entry {
    fn new(
        path: PathBuf,
        name: SharedString,
        folder: Option<SharedString>,
        description: Option<SharedString>,
        tags: Arc<[SharedString]>,
    ) -> Self {
        let file = file_name(&path);
        let searched = [name.as_ref(), file.as_str()]
            .into_iter()
            .chain(folder.as_deref())
            .chain(description.as_deref())
            .chain(tags.iter().map(AsRef::as_ref))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase()
            .into();
        Self {
            path,
            name,
            description,
            tags,
            folder,
            source: None,
            searched,
        }
    }

    fn matches(&self, query: &str) -> bool {
        self.searched.contains(query)
    }
}

struct About {
    name: SharedString,
    description: Option<SharedString>,
    tags: Arc<[SharedString]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SourceKind {
    Folder,
    Package {
        version: Option<SharedString>,
        description: Option<SharedString>,
        origin: Option<Origin>,
    },
    Skua {
        origin: Option<Origin>,
    },
}

impl SourceKind {
    fn origin(&self) -> Option<&Origin> {
        match self {
            SourceKind::Folder => None,
            SourceKind::Package { origin, .. } | SourceKind::Skua { origin } => origin.as_ref(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    root: PathBuf,
    name: SharedString,
    kind: SourceKind,
    problem: Option<SharedString>,
    scripts: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Library {
    entries: Vec<Entry>,
    sources: Vec<Source>,
}

impl Library {
    fn add(&mut self, mut source: Source, scanned: io::Result<Vec<Entry>>) {
        let index = Some(self.sources.len());
        match scanned {
            Ok(entries) => {
                source.scripts = entries.len();
                self.entries.extend(entries.into_iter().map(|entry| Entry {
                    source: index,
                    ..entry
                }));
            }
            Err(error) => {
                tracing::warn!(%error, path = %source.root.display(), "couldn't read a script source");
                source.problem.get_or_insert("Can't read".into());
            }
        }
        self.sources.push(source);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    Source(usize),
    Script(Entry),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    All,
    BuiltIn,
    Source(usize),
}

fn library_rows(library: &Library, query: &str, scope: Scope) -> Vec<Row> {
    let scripts = |source: Option<usize>| {
        library
            .entries
            .iter()
            .filter(move |entry| entry.source == source && entry.matches(query))
            .cloned()
            .map(Row::Script)
    };
    match scope {
        Scope::BuiltIn => return scripts(None).collect(),
        Scope::Source(index) => return scripts(Some(index)).collect(),
        Scope::All => {}
    }
    let mut rows: Vec<Row> = scripts(None).collect();
    for (index, source) in library.sources.iter().enumerate() {
        let named = source.name.to_lowercase().contains(query);
        let scripts: Vec<Row> = library
            .entries
            .iter()
            .filter(|entry| entry.source == Some(index) && (named || entry.matches(query)))
            .cloned()
            .map(Row::Script)
            .collect();
        if !scripts.is_empty() {
            rows.push(Row::Source(index));
            rows.extend(scripts);
        }
    }
    rows
}

fn scan_all(built_in: &Path, added: &[PathBuf], packages: &Path, skua: &Path) -> Library {
    let mut library = Library::default();
    match scan(built_in) {
        Ok(entries) => library.entries = entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(%error, path = %built_in.display(), "couldn't read the scripts folder");
        }
    }
    for root in added {
        if let Some((source, scripts)) = package(root.clone()) {
            library.add(
                Source {
                    kind: SourceKind::Folder,
                    ..source
                },
                scripts,
            );
            continue;
        }
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let source = Source {
            root: root.clone(),
            name: name.into(),
            kind: SourceKind::Folder,
            problem: None,
            scripts: 0,
        };
        library.add(source, scan(root));
    }
    for (source, scripts) in package_roots(packages).into_iter().filter_map(package) {
        library.add(source, scripts);
    }
    if let Some((source, scripts)) = skua_package(skua.to_owned()) {
        library.add(source, scripts);
    }
    library
}

fn skua_package(root: PathBuf) -> Option<(Source, io::Result<Vec<Entry>>)> {
    let (origin, problem) = match packages::origin(&root) {
        Ok(None) => return None,
        Ok(origin) => (origin, None),
        Err(problem) => (None, Some(SharedString::from(problem))),
    };
    let scripts = skua_entries(&root);
    let source = Source {
        root,
        name: packages::SKUA_NAME.into(),
        kind: SourceKind::Skua { origin },
        problem,
        scripts: 0,
    };
    Some((source, scripts))
}

fn skua_entries(root: &Path) -> io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = packages::skua_listed(root)?
        .into_iter()
        .filter_map(|listed| {
            let (folder, _) = listed.path.split_last()?;
            Some(Entry::new(
                root.join(listed.path.as_str()),
                listed.name.into(),
                (!folder.is_root()).then(|| folder.to_string().into()),
                listed.description.map(Into::into),
                listed.tags.into_iter().map(SharedString::from).collect(),
            ))
        })
        .collect();
    entries.sort_by_cached_key(|entry| {
        (
            entry.folder.as_deref().unwrap_or_default().to_lowercase(),
            entry.name.to_lowercase(),
        )
    });
    Ok(entries)
}

fn package_roots(packages: &Path) -> Vec<PathBuf> {
    let Ok(items) = fs::read_dir(packages) else {
        return Vec::new();
    };
    let mut roots: Vec<PathBuf> = items
        .filter_map(Result::ok)
        .map(|item| item.path())
        .filter(|path| {
            path.join("package.json").is_file() || path.join(packages::RECEIPT).is_file()
        })
        .collect();
    roots.sort();
    roots
}

fn package(root: PathBuf) -> Option<(Source, io::Result<Vec<Entry>>)> {
    let (origin, problem) = match packages::origin(&root) {
        Ok(origin) => (origin, None),
        Err(problem) => (None, Some(SharedString::from(problem))),
    };
    let installed = origin.is_some() || problem.is_some();
    let mut source = Source {
        name: root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
            .into(),
        root,
        kind: SourceKind::Package {
            version: None,
            description: None,
            origin: origin.clone(),
        },
        problem,
        scripts: 0,
    };
    let bytes = match fs::read(source.root.join("package.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound && !installed => return None,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            source.problem = Some("Its package.json is missing.".into());
            return Some((source, Ok(Vec::new())));
        }
        Err(error) => return Some((source, Err(error))),
    };
    let manifest = match Manifest::parse(&bytes) {
        Ok(manifest) => manifest,
        Err(ManifestError::NotAPackage) if !installed => return None,
        Err(error) => {
            source.problem = Some(error.to_string().into());
            return Some((source, Ok(Vec::new())));
        }
    };
    let scripts = match scan(&source.root.join("scripts")) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        scanned => scanned,
    };
    source.name = manifest.name.into();
    source.kind = SourceKind::Package {
        version: manifest.version.map(Into::into),
        description: manifest.description.map(Into::into),
        origin,
    };
    Some((source, scripts))
}

fn scan(root: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    visit(root, root, &mut entries)?;
    entries.sort_by_cached_key(|entry| {
        (
            entry.folder.as_deref().unwrap_or_default().to_lowercase(),
            entry.name.to_lowercase(),
        )
    });
    Ok(entries)
}

fn visit(root: &Path, dir: &Path, entries: &mut Vec<Entry>) -> io::Result<()> {
    for item in fs::read_dir(dir)? {
        let item = item?;
        let name = item.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let path = item.path();
        if item.file_type()?.is_dir() {
            if let Err(error) = visit(root, &path, entries) {
                tracing::warn!(%error, path = %path.display(), "couldn't read a script folder");
            }
        } else if scripting::runnable(name) || scripting::is_skua(&path) {
            let text = fs::read_to_string(&path).ok();
            let skua = scripting::is_skua(&path);
            if skua
                && !text
                    .as_deref()
                    .is_some_and(|text| text.contains("ScriptMain("))
            {
                continue;
            }
            let folder = dir
                .strip_prefix(root)
                .ok()
                .filter(|folder| !folder.as_os_str().is_empty())
                .map(|folder| folder.display().to_string().into());
            let meta = text
                .filter(|_| !skua)
                .and_then(|text| scripting::read_meta(&path, &text))
                .unwrap_or_default();
            entries.push(Entry::new(
                path,
                meta.name.unwrap_or_else(|| name.to_owned()).into(),
                folder,
                meta.description.map(Into::into),
                Arc::default(),
            ));
        }
    }
    Ok(())
}

pub(super) fn home_relative(path: &Path) -> SharedString {
    dirs::home_dir()
        .and_then(|home| path.strip_prefix(home).ok())
        .map_or_else(
            || path.display().to_string(),
            |rest| Path::new("~").join(rest).display().to_string(),
        )
        .into()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Page {
    Library(Option<PathBuf>),
    CoreBots,
    Queue,
    Log,
}

const PAGES: [Page; 3] = [Page::Library(None), Page::Queue, Page::Log];
const SKUA_PAGES: [&str; 2] = ["Scripts", "CoreBots"];
const SKUA_ROOMS: &str = "Skua scripts take their rooms from CoreBots options.";

#[derive(Clone)]
enum SourceState {
    Problem(SharedString),
    UpdateAvailable,
    UpdateFailed(SharedString),
}

impl SourceState {
    fn description(&self) -> SharedString {
        match self {
            SourceState::Problem(text) | SourceState::UpdateFailed(text) => text.clone(),
            SourceState::UpdateAvailable => "Update available".into(),
        }
    }
}

struct SourceRow {
    root: PathBuf,
    name: SharedString,
    qualifier: Option<SharedString>,
    description: Option<SharedString>,
    path: SharedString,
    scripts: Option<usize>,
    release: Option<(&'static str, SharedString)>,
    state: Option<SourceState>,
}

fn release(kind: &SourceKind) -> Option<(&'static str, SharedString)> {
    match kind {
        SourceKind::Package {
            version: Some(version),
            ..
        } => Some(("Version", version.clone())),
        _ => kind
            .origin()
            .map(|origin| ("Commit", origin.commit.clone().into())),
    }
}

fn qualifiers(rows: &[(SharedString, Vec<String>)]) -> Vec<Option<SharedString>> {
    let suffix = |segments: &[String], depth: usize| {
        let mut taken: Vec<&str> = segments.iter().take(depth).map(String::as_str).collect();
        taken.reverse();
        taken.join("/")
    };
    rows.iter()
        .map(|(name, segments)| {
            let rivals: Vec<&[String]> = rows
                .iter()
                .filter(|(other, _)| other == name)
                .map(|(_, segments)| segments.as_slice())
                .collect();
            if rivals.len() < 2 {
                return None;
            }
            let deepest = rivals.iter().map(|segments| segments.len()).max()?;
            let depth = (1..=deepest)
                .find(|&depth| {
                    let mine = suffix(segments, depth);
                    rivals
                        .iter()
                        .filter(|other| suffix(other, depth) == mine)
                        .count()
                        == 1
                })
                .unwrap_or(deepest);
            Some(suffix(segments, depth).into())
        })
        .collect()
}

fn parent_segments(root: &Path) -> Vec<String> {
    root.parent()
        .map(|parent| {
            home_relative(parent)
                .split('/')
                .rev()
                .filter(|segment| !segment.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn dir_name(root: &Path) -> String {
    root.file_name()
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn source_mark(theme: &Theme, state: &SourceState) -> Option<impl IntoElement> {
    let (glyph, label, tone) = match state {
        SourceState::Problem(_) => (Icon::CircleAlert, "Problem", Tone::Danger),
        SourceState::UpdateFailed(_) => (Icon::TriangleAlert, "Update problem", Tone::Warning),
        SourceState::UpdateAvailable => return None,
    };
    Some(
        div()
            .id(label)
            .role(gpui::Role::Image)
            .aria_label(label)
            .flex_none()
            .flex()
            .items_center()
            .child(icon(glyph, theme.tone(tone).0).size(px(12.0))),
    )
}

struct Open(WindowHandle<ScriptsWindow>);

impl gpui::Global for Open {}

pub(super) fn close(client: gpui::EntityId, cx: &mut App) {
    let open = cx.try_global::<Open>().map(|open| open.0);
    shown_tab::close(open, client, |scripts: &ScriptsWindow| &scripts.client, cx);
}

pub(crate) fn open(
    client: Entity<Client>,
    folder: PathBuf,
    types: PathBuf,
    target: Target,
    cx: &mut App,
) {
    let open = cx.try_global::<Open>().map(|open| open.0);
    if shown_tab::reuse(
        open,
        &client,
        |scripts: &ScriptsWindow| &scripts.client,
        |scripts, window, cx| {
            scripts.retarget(target, cx);
            scripts.rescan(cx);
            window.focus(&scripts.search.focus_handle(cx), cx);
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
            title: Some("Scripts".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(12.0))),
        }),
        app_owns_titlebar_drag: true,
        is_minimizable: false,
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let scripts = cx.new(|cx| ScriptsWindow::new(client, folder, types, target, window, cx));
        let search = scripts.read(cx).search.focus_handle(cx);
        window.focus(&search, cx);
        scripts
    }) {
        Ok(handle) => cx.set_global(Open(handle)),
        Err(error) => tracing::error!(%error, "couldn't open scripts"),
    }
}

pub(crate) fn load(client: Entity<Client>, folder: PathBuf, types: PathBuf, cx: &mut App) {
    open(client, folder, types, Target::Selected, cx);
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0) {
        let _ = handle.update(cx, |scripts, window, cx| {
            if scripts.shown.is_some() && !scripts.running(cx) {
                scripts.choose_file(window, cx);
            }
        });
    }
}

pub(super) struct ScriptsWindow {
    client: Entity<Client>,
    focus: FocusHandle,
    tab: Target,
    shown: Option<ShownTab>,
    folder: PathBuf,
    packages: PathBuf,
    skua: PathBuf,
    types: PathBuf,
    library: Option<Library>,
    scanning: Option<Task<()>>,
    search: Entity<Input>,
    room: Entity<Input>,
    selected: usize,
    page: Page,
    folders_open: bool,
    packages_open: bool,
    queued: usize,
    chosen: Option<PathBuf>,
    forms: HashMap<FormKey, Form>,
    generation: u64,
    loading: HashMap<FormKey, Task<()>>,
    describing: Arc<AtomicU64>,
    urgent: Arc<AtomicUsize>,
    preloading: Option<(PathBuf, Arc<AtomicBool>)>,
    visible: Range<usize>,
    settled: bool,
    settle: Option<Task<()>>,
    probes: HashMap<EntryId, Arc<AtomicBool>>,
    waiting: Option<(PathBuf, Instant, Task<()>)>,
    displayed: Option<Entry>,
    detail_scroll: ScrollHandle,
    error_cursor: Option<usize>,
    source_scroll: ScrollHandle,
    package_dialogs: Entity<PackageDialogs>,
    core_bots: Entity<CoreBotsPage>,
    toasts: Entity<Toaster>,
    _view_changes: Option<Vec<Subscription>>,
    _subscriptions: [Subscription; 8],
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum FormKey {
    Script(PathBuf),
    Entry(EntryId),
}

struct Form {
    generation: u64,
    meta: Meta,
    edited: bool,
    state: FormState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Load {
    Shown,
    Ahead,
}

enum Stop {
    Superseded {
        turn: u64,
        latest: Arc<AtomicU64>,
    },
    Ahead {
        cancelled: Arc<AtomicBool>,
        urgent: Arc<AtomicUsize>,
    },
    Removed(Arc<AtomicBool>),
}

impl Stop {
    fn stopped(&self) -> bool {
        match self {
            Stop::Superseded { turn, latest } => latest.load(Ordering::SeqCst) != *turn,
            Stop::Ahead { cancelled, urgent } => {
                cancelled.load(Ordering::SeqCst) || urgent.load(Ordering::SeqCst) > 0
            }
            Stop::Removed(removed) => removed.load(Ordering::SeqCst),
        }
    }
}

enum FormState {
    Broken {
        title: &'static str,
        message: SharedString,
    },
    Plain {
        unread: Option<SharedString>,
    },
    Fields {
        definition: Definition,
        drafts: Vec<FieldDraft>,
        texts: Vec<Option<Entity<Input>>>,
        _changes: Vec<Subscription>,
    },
}

impl ScriptsWindow {
    fn new(
        client: Entity<Client>,
        folder: PathBuf,
        types: PathBuf,
        tab: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            Input::new("Search scripts", cx)
                .placeholder("Search")
                .search()
        });
        let room = cx.new(|cx| Input::new("Room", cx).placeholder("Random"));
        let toasts = Toaster::new(window, cx);
        let package_dialogs = cx.new(|cx| PackageDialogs::new(toasts.clone(), cx));
        let skua = packages::packages(cx).read(cx).skua_scripts();
        let core_bots = cx.new(|cx| CoreBotsPage::new(skua.clone(), window, cx));
        let subscriptions = [
            cx.observe(&packages::packages(cx), |_, _, cx| cx.notify()),
            cx.subscribe(&package_dialogs, |this, _, event: &PackageEvent, cx| {
                if let PackageEvent::Installed(root) = event {
                    this.set_page(Page::Library(Some(root.clone())), cx);
                }
                this.rescan(cx);
            }),
            cx.observe(&client, |this, _, cx| this.show(cx)),
            cx.subscribe(&search, |this, _, event: &InputEvent, cx| match event {
                InputEvent::Changed => {
                    this.chosen = None;
                    this.selected = first_script(&this.rows(cx));
                    cx.notify();
                }
                InputEvent::Submitted => this.run_selected(cx),
            }),
            cx.subscribe_in(&room, window, |this, room, event, window, cx| {
                let text = room.read(cx).text().trim().to_owned();
                let parsed = PrivateRoom::parse(&text);
                let valid = text.is_empty() || parsed.is_some();
                let private = matches!(
                    this.options(cx).map(|options| options.rooms),
                    Some(Rooms::Private { .. })
                );
                match event {
                    InputEvent::Changed if valid && private => {
                        this.change_options(cx, |options| {
                            options.rooms = Rooms::Private { room: parsed };
                        });
                    }
                    InputEvent::Changed => {}
                    InputEvent::Submitted if valid => window.blur(cx),
                    InputEvent::Submitted => {}
                }
                room.update(cx, |input, cx| {
                    input.set_invalid(matches!(event, InputEvent::Submitted) && !valid, cx);
                });
            }),
            cx.on_blur(&room.focus_handle(cx), window, |this, _, cx| {
                this.show_saved_room(cx);
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.rescan(cx);
                }
            }),
            cx.observe_global::<script_names::Names>(|_, cx| cx.notify()),
        ];
        let mut scripts = Self {
            client,
            focus: cx.focus_handle(),
            tab,
            shown: None,
            packages: folder.with_file_name("packages"),
            skua,
            folder,
            types,
            library: None,
            scanning: None,
            search,
            room,
            selected: 0,
            page: Page::Library(None),
            folders_open: true,
            packages_open: true,
            queued: 0,
            chosen: None,
            forms: HashMap::new(),
            generation: 0,
            loading: HashMap::new(),
            describing: Arc::default(),
            urgent: Arc::default(),
            preloading: None,
            visible: 0..0,
            settled: false,
            settle: None,
            probes: HashMap::new(),
            waiting: None,
            displayed: None,
            detail_scroll: ScrollHandle::new(),
            error_cursor: None,
            source_scroll: ScrollHandle::new(),
            package_dialogs,
            core_bots,
            toasts,
            _view_changes: None,
            _subscriptions: subscriptions,
        };
        scripts.show(cx);
        scripts.rescan(cx);
        scripts
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
            |_, _| Vec::new(),
            cx,
        );
        if switched {
            self.switched(cx);
        }
    }

    fn switched(&mut self, cx: &mut Context<Self>) {
        self._view_changes = self.view().map(|view| {
            vec![
                cx.subscribe(&view, |this, _, _: &Changed, cx| {
                    this.refresh_core_bots(cx);
                    cx.notify();
                }),
                cx.subscribe(&view, |this, _, _: &Updated, cx| this.refresh_core_bots(cx)),
            ]
        });
        self.queued = 0;
        self.chosen = None;
        self.show_saved_room(cx);
        self.select_last_run(cx);
        if self.page == Page::CoreBots {
            self.show_core_bots(cx);
        }
    }

    fn refresh_core_bots(&mut self, cx: &mut Context<Self>) {
        if self.page == Page::CoreBots {
            let account = self.account(cx);
            let view = self.view();
            self.core_bots
                .update(cx, |page, cx| page.refresh(account, view, cx));
        }
    }

    fn show_core_bots(&mut self, cx: &mut Context<Self>) {
        let account = self.account(cx);
        let view = self.view();
        self.core_bots
            .update(cx, |page, cx| page.show(account, view, cx));
    }

    fn view(&self) -> Option<Entity<RuffleView>> {
        self.shown.as_ref()?.view.upgrade()
    }

    fn running(&self, cx: &App) -> bool {
        self.view()
            .is_some_and(|view| view.read(cx).script.stoppable())
    }

    fn account(&self, cx: &App) -> Option<AccountId> {
        self.view()?.read(cx).settings_account.clone()
    }

    fn run_status(&self, cx: &App) -> Option<RunStatus> {
        let script = &self.view()?.read(cx).script;
        let state = script.state(cx)?;
        let activity = match state.phase {
            ScriptPhase::Running => Activity::Running,
            ScriptPhase::NeedsInput => Activity::NeedsYou,
            ScriptPhase::Queued | ScriptPhase::Stopping => Activity::Waiting,
            ScriptPhase::Finished => Activity::Idle,
            ScriptPhase::Stopped => Activity::Stopped,
            ScriptPhase::Failed(_) => Activity::Failed,
        };
        let failure = match &state.phase {
            ScriptPhase::Failed(message) => Some(Failure {
                title: format!("{} failed", state.name).into(),
                script: home_relative(&state.entry),
                message: message.clone(),
            }),
            _ => None,
        };
        Some(RunStatus {
            text: script.status(cx)?.into(),
            activity,
            failure,
            stop: (script.stoppable() && state.phase != ScriptPhase::Stopping)
                .then_some(state.name),
        })
    }

    fn set_private_rooms(&mut self, on: bool, cx: &mut Context<Self>) {
        let rooms = if on {
            Rooms::Private {
                room: PrivateRoom::parse(self.room.read(cx).text()),
            }
        } else {
            Rooms::Public
        };
        self.change_options(cx, |options| options.rooms = rooms);
    }

    fn show_saved_room(&mut self, cx: &mut Context<Self>) {
        let text = room_text(self.options(cx).unwrap_or_default().rooms);
        self.room.update(cx, |input, cx| {
            input.set_text(text, cx);
            input.set_invalid(false, cx);
        });
        cx.notify();
    }

    fn options(&self, cx: &App) -> Option<ScriptOptions> {
        let account = self.account(cx)?;
        Some(account_settings::get(&account, cx).scripts)
    }

    fn change_options(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut ScriptOptions)) {
        if let Some(view) = self.view() {
            view.update(cx, |view, cx| view.change_script_options(edit, cx));
        }
        cx.notify();
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let built_in = self.folder.clone();
        let added = settings::script_folders(cx).to_vec();
        let packages = self.packages.clone();
        let skua = self.skua.clone();
        let types = self.types.clone();
        let scanned = cx.background_spawn(async move {
            let library = scan_all(&built_in, &added, &packages, &skua);
            let packages = library
                .sources
                .iter()
                .filter(|source| matches!(source.kind, SourceKind::Package { .. }))
                .map(|source| &source.root);
            let folders = iter::once(&built_in).chain(&added).chain(packages);
            scripting::link_types(folders.map(PathBuf::as_path), &types);
            library
        });
        self.scanning = Some(cx.spawn(async move |this, cx| {
            let scanned = scanned.await;
            let _ = this.update(cx, |this, cx| {
                let first = this.library.is_none();
                let kept = match this.rows(cx).get(this.selected) {
                    Some(Row::Script(entry)) => Some(entry.path.clone()),
                    _ => None,
                };
                script_names::learn(
                    scanned
                        .entries
                        .iter()
                        .map(|entry| (entry.path.clone(), entry.name.clone())),
                    cx,
                );
                this.library = Some(scanned);
                if this.scope(cx).is_none() {
                    this.page = Page::Library(None);
                }
                if first {
                    this.select_last_run(cx);
                }
                let rows = this.rows(cx);
                if let Some(index) = kept.and_then(|path| {
                    rows.iter()
                        .position(|row| matches!(row, Row::Script(entry) if entry.path == path))
                }) {
                    this.selected = index;
                } else if !matches!(rows.get(this.selected), Some(Row::Script(_))) {
                    this.selected = first_script(&rows);
                }
                cx.notify();
            });
        }));
    }

    fn select_last_run(&mut self, cx: &App) {
        let last = self
            .view()
            .and_then(|view| view.read(cx).script.entry().cloned());
        if let Some(index) = self
            .rows(cx)
            .iter()
            .position(|row| matches!(row, Row::Script(entry) if Some(&entry.path) == last.as_ref()))
        {
            self.selected = index;
        }
    }

    fn rows(&self, cx: &App) -> Rc<[Row]> {
        let (Some(library), Some(scope)) = (&self.library, self.scope(cx)) else {
            return Rc::new([]);
        };
        let query = self.search.read(cx).text().trim().to_lowercase();
        library_rows(library, &query, scope).into()
    }

    fn searching(&self, cx: &App) -> bool {
        !self.search.read(cx).text().trim().is_empty()
    }

    fn shown_page(&self, cx: &App) -> Page {
        if self.searching(cx) {
            Page::Library(None)
        } else {
            self.page.clone()
        }
    }

    fn source_root(&self, cx: &App) -> Option<PathBuf> {
        match self.shown_page(cx) {
            Page::Library(root) => root,
            Page::CoreBots => Some(self.skua.clone()),
            Page::Queue | Page::Log => None,
        }
    }

    fn scope(&self, cx: &App) -> Option<Scope> {
        let Some(root) = self.source_root(cx) else {
            return Some(Scope::All);
        };
        if root == self.folder {
            return Some(Scope::BuiltIn);
        }
        self.library
            .as_ref()?
            .sources
            .iter()
            .position(|source| source.root == root)
            .map(Scope::Source)
    }

    fn set_page(&mut self, page: Page, cx: &mut Context<Self>) {
        let searching = self.searching(cx);
        if page == self.page && !searching {
            return;
        }
        if searching {
            self.search.update(cx, |search, cx| search.set_text("", cx));
        }
        self.page = page;
        self.chosen = None;
        self.selected = first_script(&self.rows(cx));
        if self.page == Page::CoreBots {
            self.show_core_bots(cx);
        }
        cx.notify();
    }

    fn skua_selected(&self, cx: &App) -> bool {
        let script = match self.shown_page(cx) {
            Page::Library(_) => self.target(cx).map(|entry| entry.path),
            Page::Queue => self
                .queue(cx)
                .0
                .get(self.queued)
                .map(|entry| entry.script.clone()),
            Page::CoreBots | Page::Log => None,
        };
        script.is_some_and(|script| scripting::is_skua(&script))
    }

    fn run_selected(&mut self, cx: &mut Context<Self>) {
        if self.running(cx) {
            return;
        }
        let (Some(view), Some((entry, values))) = (self.view(), self.launch(cx)) else {
            return;
        };
        let launch = Launch {
            entry,
            inputs: Supplied::Values(values),
        };
        view.update(cx, |view, cx| view.start_script(launch, cx));
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.view() {
            view.update(cx, |view, cx| view.stop_script(cx));
        }
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let mut index = self.selected;
        while let Some(next) = index
            .checked_add_signed(by)
            .filter(|next| *next < rows.len())
        {
            index = next;
            if matches!(rows[index], Row::Script(_)) {
                self.selected = index;
                self.chosen = None;
                cx.notify();
                return;
            }
        }
    }

    fn choose_file(&mut self, window: &Window, cx: &mut Context<Self>) {
        let chosen = file_panel::open(Pick::File, "Choose", window, cx);
        cx.spawn(async move |this, cx| {
            let Some(path) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if !matches!(this.page, Page::Library(_)) {
                    this.set_page(Page::Library(None), cx);
                }
                this.chosen = Some(path);
                cx.notify();
            });
        })
        .detach();
    }

    fn show_folder(&mut self, folder: &Path, cx: &mut Context<Self>) {
        if folder == self.folder
            && let Err(error) = fs::create_dir_all(folder)
        {
            tracing::warn!(%error, path = %folder.display(), "couldn't create the scripts folder");
            return;
        }
        cx.open_with_system(folder);
    }

    fn add_folder(&mut self, window: &Window, cx: &mut Context<Self>) {
        let chosen = file_panel::open(Pick::Folder, "Add", window, cx);
        cx.spawn(async move |this, cx| {
            let Some(folder) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if folder != this.folder {
                    settings::add_script_folder(folder, cx);
                    this.rescan(cx);
                }
            });
        })
        .detach();
    }

    fn remove_folder(&mut self, folder: &Path, cx: &mut Context<Self>) {
        settings::remove_script_folder(folder, cx);
        self.rescan(cx);
    }

    fn locate_folder(&mut self, old: PathBuf, window: &Window, cx: &mut Context<Self>) {
        let chosen = file_panel::open(Pick::Folder, "Choose", window, cx);
        cx.spawn(async move |this, cx| {
            let Some(folder) = chosen.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if folder != this.folder {
                    settings::replace_script_folder(&old, folder, cx);
                    this.rescan(cx);
                }
            });
        })
        .detach();
    }

    fn source_rows(&self, cx: &App) -> (Vec<SourceRow>, Vec<SourceRow>) {
        let library = self.library.as_ref();
        let built_in = SourceRow {
            root: self.folder.clone(),
            name: dir_name(&self.folder).into(),
            qualifier: None,
            description: None,
            path: home_relative(&self.folder),
            scripts: library.map(|library| {
                library
                    .entries
                    .iter()
                    .filter(|entry| entry.source.is_none())
                    .count()
            }),
            release: None,
            state: None,
        };
        let packages = packages::packages(cx).read(cx);
        let (folders, installed): (Vec<&Source>, Vec<&Source>) = library
            .map_or(&[][..], |library| library.sources.as_slice())
            .iter()
            .partition(|source| source.kind == SourceKind::Folder);
        let row = |source: &Source| {
            let status = match source.kind {
                SourceKind::Folder => None,
                SourceKind::Package { .. } | SourceKind::Skua { .. } => {
                    packages.status(&source.root)
                }
            };
            let state = match (&source.problem, status) {
                (Some(problem), _) => Some(SourceState::Problem(problem.clone())),
                (None, Some(Status::Available(_))) => Some(SourceState::UpdateAvailable),
                (None, Some(Status::Failed(message))) => {
                    Some(SourceState::UpdateFailed(message.clone().into()))
                }
                (None, None) => None,
            };
            SourceRow {
                root: source.root.clone(),
                name: source.name.clone(),
                qualifier: None,
                description: match &source.kind {
                    SourceKind::Package { description, .. } => description.clone(),
                    SourceKind::Folder | SourceKind::Skua { .. } => None,
                },
                path: home_relative(&source.root),
                scripts: source.problem.is_none().then_some(source.scripts),
                release: release(&source.kind),
                state,
            }
        };
        let qualify = |mut rows: Vec<SourceRow>, segments: fn(&Path) -> Vec<String>| {
            let keys: Vec<(SharedString, Vec<String>)> = rows
                .iter()
                .map(|row| (row.name.clone(), segments(&row.root)))
                .collect();
            for (row, qualifier) in rows.iter_mut().zip(qualifiers(&keys)) {
                row.qualifier = qualifier;
            }
            rows
        };
        (
            qualify(
                iter::once(built_in)
                    .chain(folders.into_iter().map(row))
                    .collect(),
                parent_segments,
            ),
            qualify(installed.into_iter().map(row).collect(), |root| {
                vec![dir_name(root)]
            }),
        )
    }

    fn source_actions(&self, root: &Path, this: &WeakEntity<Self>, cx: &App) -> Vec<MenuItem> {
        let source = self
            .library
            .as_ref()
            .and_then(|library| library.sources.iter().find(|source| source.root == root));
        let Some(source) = source else {
            return Vec::new();
        };
        if source.kind == SourceKind::Folder {
            let this = this.clone();
            let root = root.to_owned();
            return vec![MenuItem::action("Remove from library", move |_, cx| {
                let _ = this.update(cx, |this, cx| this.remove_folder(&root, cx));
            })];
        }
        let mut items = Vec::new();
        if let Some(origin) = source.kind.origin() {
            let url = origin.url.clone();
            items.push(MenuItem::action("Open on GitHub", move |_, cx| {
                cx.open_url(&url);
            }));
        }
        let dialogs = self.package_dialogs.clone();
        let (root, name) = (source.root.clone(), source.name.clone());
        items.push(
            MenuItem::action("Uninstall…", move |_, cx| {
                dialogs.update(cx, |dialogs, cx| {
                    dialogs.open_uninstall(root.clone(), name.clone(), cx)
                });
            })
            .disabled(packages::packages(cx).read(cx).activity().is_some()),
        );
        items
    }

    fn open_file(&self, theme: &Theme, cx: &mut Context<Self>) -> Button {
        let label = "Open file…";
        Button::new("open-file", label)
            .variant(Variant::Ghost)
            .w_full()
            .disabled(self.shown.is_none() || self.running(cx))
            .child(
                h_flex().flex_1().justify_between().child(label).children(
                    shortcuts::keys_for(&LoadScript, cx)
                        .map(|keys| div().text_color(theme.muted_foreground).child(keys)),
                ),
            )
            .on_click(cx.listener(|this, _, window, cx| this.choose_file(window, cx)))
    }

    fn pending_updates(&self, cx: &App) -> Vec<Pending> {
        let state = packages::packages(cx).read(cx);
        self.library
            .iter()
            .flat_map(|library| &library.sources)
            .filter_map(|source| {
                let origin = source.kind.origin()?;
                let Some(Status::Available(to)) = state.status(&source.root) else {
                    return None;
                };
                Some(Pending {
                    root: source.root.clone(),
                    name: source.name.clone(),
                    from: origin.commit.clone().into(),
                    to: to.short().to_owned().into(),
                })
            })
            .collect()
    }

    fn check_for_updates(&self, cx: &App) -> AnyElement {
        let state = packages::packages(cx).read(cx);
        let working = matches!(
            state.activity(),
            Some(packages::Activity::Checking | packages::Activity::Updating(_))
        );
        let pending = self.pending_updates(cx);
        if !pending.is_empty() {
            let title = match pending.len() {
                1 => SharedString::from("1 update available"),
                count => format!("{count} updates available").into(),
            };
            let card = pending.iter().fold(
                HoverCard::new("updates-card", title)
                    .placement(Placement::new(Side::Right, Align::Start)),
                |card, pending| {
                    card.row(
                        pending.name.clone(),
                        format!("{} → {}", pending.from, pending.to),
                        Tone::Neutral,
                    )
                },
            );
            let dialogs = self.package_dialogs.clone();
            return card
                .child(
                    Button::icon("review-updates", Icon::Download, "Review updates")
                        .size(Size::Small)
                        .hide_tooltip()
                        .busy(working)
                        .disabled(state.activity().is_some())
                        .on_click(move |_, _, cx| {
                            dialogs.update(cx, |dialogs, cx| {
                                dialogs.review_updates(pending.clone(), cx)
                            })
                        }),
                )
                .into_any_element();
        }
        let installed = self.library.as_ref().is_some_and(|library| {
            library
                .sources
                .iter()
                .any(|source| source.kind.origin().is_some())
        });
        let dialogs = self.package_dialogs.clone();
        Button::icon("check-updates", Icon::RefreshCw, "Check for updates")
            .size(Size::Small)
            .busy(working)
            .disabled(state.activity().is_some() || !installed)
            .on_click(move |_, _, cx| dialogs.update(cx, |dialogs, cx| dialogs.check(cx)))
            .into_any_element()
    }

    fn sidebar(&self, theme: &Theme, unfinished: usize, cx: &mut Context<Self>) -> Div {
        let layout = theme.layout;
        let current = (!self.searching(cx)).then_some(&self.page);
        let total = self.library.as_ref().map(|library| library.entries.len());
        let this = cx.weak_entity();
        let pages = Tabs::new("pages", "Views", PAGES.len(), move |index| {
            let (label, count) = match PAGES[index] {
                Page::Library(_) | Page::CoreBots => ("Library", total),
                Page::Queue => ("Queue", (unfinished > 0).then_some(unfinished)),
                Page::Log => ("Log", None),
            };
            match count {
                Some(count) => Tab::new(label).end_slot(SharedString::from(count.to_string())),
                None => Tab::new(label),
            }
        })
        .page_sidebar()
        .small()
        .selected(
            current
                .and_then(|page| PAGES.iter().position(|shown| shown == page))
                .unwrap_or(usize::MAX),
        )
        .on_select(move |index, _, cx| {
            let _ = this.update(cx, |this, cx| this.set_page(PAGES[index].clone(), cx));
        });
        let (folders, installed) = self.source_rows(cx);
        let toggles = cx.weak_entity();
        let caption =
            |id: &'static str, label: &'static str, open: bool, set: fn(&mut Self, bool)| {
                let this = toggles.clone();
                h_flex()
                    .flex_none()
                    .h(layout.control)
                    .justify_between()
                    .pl(layout.space(4.0))
                    .pr(layout.space(2.0))
                    .child(Disclosure::new(id, label, open).caption().on_toggle(
                        move |open, _, cx| {
                            let _ = this.update(cx, |this, cx| {
                                set(this, open);
                                cx.notify();
                            });
                        },
                    ))
            };
        let folders = self
            .folders_open
            .then(|| self.source_list("folders", "Folders", folders, theme, cx));
        let installed = self
            .packages_open
            .then(|| self.source_list("packages", "Packages", installed, theme, cx));
        let busy = packages::packages(cx).read(cx).activity().is_some();
        let installing = matches!(
            packages::packages(cx).read(cx).activity(),
            Some(packages::Activity::Installing)
        );
        let dialogs = self.package_dialogs.clone();
        let sources = v_flex()
            .child(
                caption(
                    "folders-toggle",
                    "Folders",
                    self.folders_open,
                    |this, open| this.folders_open = open,
                )
                .child(
                    Button::icon("add-folder", Icon::Plus, "Add folder")
                        .size(Size::Small)
                        .on_click(cx.listener(|this, _, window, cx| this.add_folder(window, cx))),
                ),
            )
            .children(folders)
            .child(
                caption(
                    "packages-toggle",
                    "Packages",
                    self.packages_open,
                    |this, open| this.packages_open = open,
                )
                .child(
                    h_flex()
                        .gap(layout.space(1.0))
                        .child(self.check_for_updates(cx))
                        .child(
                            Button::icon("install-package", Icon::Plus, "Install package")
                                .size(Size::Small)
                                .busy(installing)
                                .disabled(busy)
                                .on_click(move |_, window, cx| {
                                    dialogs
                                        .update(cx, |dialogs, cx| dialogs.open_install(window, cx))
                                }),
                        ),
                ),
            )
            .children(installed);
        theme
            .sidebar_pane()
            .h_full()
            .child(
                div()
                    .flex_none()
                    .p(layout.space(2.0))
                    .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        let by = match event.keystroke.key.as_str() {
                            "up" => -1,
                            "down" => 1,
                            _ => return,
                        };
                        this.step(by, cx);
                        cx.stop_propagation();
                    }))
                    .child(self.search.clone()),
            )
            .child(pages)
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .my(-RING_ROOM)
                    .child(
                        div()
                            .id("sources")
                            .min_h_0()
                            .py(RING_ROOM)
                            .overflow_y_scroll()
                            .track_scroll(&self.source_scroll)
                            .child(sources),
                    )
                    .child(
                        Scrollbar::new("sources-scrollbar", self.source_scroll.clone())
                            .persistent(),
                    ),
            )
            .child(drag_area().flex_1())
            .child(
                div()
                    .flex_none()
                    .p(layout.space(2.0))
                    .child(self.open_file(theme, cx)),
            )
    }

    fn source_header(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let layout = theme.layout;
        let typography = theme.typography;
        let root = self.source_root(cx)?;
        let (folders, installed) = self.source_rows(cx);
        let package = installed.iter().any(|row| row.root == root);
        let row = folders
            .into_iter()
            .chain(installed)
            .find(|row| row.root == root)?;
        let state = packages::packages(cx).read(cx);
        let target = row.root.clone();
        let updating = matches!(state.activity(), Some(packages::Activity::Updating(updating)) if *updating == target);
        let busy = state.activity().is_some();
        let update = matches!(
            row.state,
            Some(SourceState::UpdateAvailable | SourceState::UpdateFailed(_))
        )
        .then(|| {
            let dialogs = self.package_dialogs.clone();
            let name = row.name.clone();
            Button::new("source-update", "Update")
                .size(Size::Small)
                .busy(updating)
                .disabled(busy)
                .on_click(move |_, _, cx| {
                    dialogs.update(cx, |dialogs, cx| {
                        dialogs.update(target.clone(), name.clone(), false, cx)
                    })
                })
        });
        let readable = package || !matches!(row.state, Some(SourceState::Problem(_)));
        let show = readable.then(|| {
            let root = row.root.clone();
            Button::new("source-show", "Show in Finder")
                .variant(Variant::Ghost)
                .size(Size::Small)
                .on_click(cx.listener(move |this, _, _, cx| this.show_folder(&root, cx)))
        });
        let this = cx.weak_entity();
        let menu_root = row.root.clone();
        let menu = (row.root != self.folder).then(|| {
            DropdownMenu::new("source-actions")
                .trigger(
                    Button::icon(
                        "source-actions-trigger",
                        Icon::Ellipsis,
                        format!("More {} actions", row.name),
                    )
                    .variant(Variant::Ghost)
                    .size(Size::Small)
                    .hide_tooltip(),
                )
                .align(Align::End)
                .items(move |_, cx| {
                    this.upgrade()
                        .map(|scripts| scripts.read(cx).source_actions(&menu_root, &this, cx))
                        .unwrap_or_default()
                })
        });
        let facts = match &row.release {
            Some((label, value)) => h_flex().child(format!("{label} {value}")),
            None => h_flex()
                .when(row.root == self.folder, |facts| {
                    facts.child(div().flex_none().child("Default folder · "))
                })
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_ellipsis_start()
                        .child(row.path.clone()),
                ),
        };
        let failure = match &row.state {
            Some(SourceState::UpdateFailed(message)) => Some(message.clone()),
            _ => None,
        };
        let skua = self.library.as_ref().is_some_and(|library| {
            library.sources.iter().any(|source| {
                source.root == row.root && matches!(source.kind, SourceKind::Skua { .. })
            })
        });
        let pages = skua.then(|| {
            let this = cx.weak_entity();
            let root = row.root.clone();
            Tabs::new("skua-pages", "Skua pages", SKUA_PAGES.len(), |index| {
                Tab::new(SKUA_PAGES[index])
            })
            .line()
            .small()
            .selected(usize::from(self.page == Page::CoreBots))
            .on_select(move |index, _, cx| {
                let page = match index {
                    0 => Page::Library(Some(root.clone())),
                    _ => Page::CoreBots,
                };
                let _ = this.update(cx, |this, cx| this.set_page(page, cx));
            })
        });
        Some(
            v_flex()
                .flex_none()
                .gap(px(2.0))
                .pl(layout.space(5.0))
                .pr(layout.space(3.0))
                .pt(layout.space(3.0))
                .pb(layout.space(1.0))
                .child(
                    h_flex()
                        .min_h(layout.control_sm)
                        .gap(layout.space(2.0))
                        .child(
                            div()
                                .id("source-title")
                                .role(gpui::Role::Heading)
                                .aria_level(1)
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_title(typography)
                                .child(row.name.clone()),
                        )
                        .children(update)
                        .children(show)
                        .children(menu),
                )
                .child(
                    v_flex()
                        .text_caption(typography)
                        .text_color(theme.muted_foreground)
                        .children(
                            row.description
                                .clone()
                                .map(|description| div().child(description)),
                        )
                        .child(facts)
                        .children(
                            failure
                                .map(|message| div().text_color(theme.foreground).child(message)),
                        ),
                )
                .children(pages.map(|pages| div().pt(layout.space(2.0)).child(pages))),
        )
    }

    fn source_list(
        &self,
        id: &'static str,
        label: &'static str,
        rows: Vec<SourceRow>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Tabs {
        let rows: Rc<[SourceRow]> = rows.into();
        let selected = self
            .source_root(cx)
            .and_then(|root| rows.iter().position(|row| row.root == root));
        let marks = theme.clone();
        let choices = rows.clone();
        let this = cx.weak_entity();
        Tabs::new(id, label, rows.len(), move |index| {
            let row = &rows[index];
            let label = match &row.qualifier {
                Some(qualifier) => SharedString::from(format!("{}, {qualifier}", row.name)),
                None => row.name.clone(),
            };
            let mut tab = Tab::new(label).child(
                h_flex()
                    .min_w_0()
                    .gap(marks.space(1.5))
                    .child(div().min_w_0().truncate().child(row.name.clone()))
                    .children(row.qualifier.clone().map(|qualifier| {
                        div()
                            .min_w_0()
                            .truncate()
                            .text_ellipsis_start()
                            .text_caption(marks.typography)
                            .text_color(marks.muted_foreground)
                            .child(qualifier)
                    }))
                    .children(
                        row.state
                            .as_ref()
                            .and_then(|state| source_mark(&marks, state)),
                    ),
            );
            if let Some(scripts) = row.scripts {
                tab = tab.end_slot(SharedString::from(scripts.to_string()));
            }
            if let Some(state) = &row.state {
                tab = tab.description(state.description());
            }
            tab
        })
        .page_sidebar()
        .small()
        .selected(selected.unwrap_or(usize::MAX))
        .on_select(move |index, _, cx| {
            let root = choices[index].root.clone();
            let _ = this.update(cx, |this, cx| this.set_page(Page::Library(Some(root)), cx));
        })
    }

    fn target(&self, cx: &App) -> Option<Entry> {
        if let Some(path) = &self.chosen {
            return Some(Entry::new(
                path.clone(),
                file_name(path).into(),
                None,
                None,
                Arc::default(),
            ));
        }
        let rows = self.rows(cx);
        match rows.get(self.selected.min(rows.len().saturating_sub(1))) {
            Some(Row::Script(entry)) => Some(entry.clone()),
            _ => None,
        }
    }

    fn saw(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        if range == self.visible {
            return;
        }
        self.visible = range;
        self.settled = false;
        self.settle = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let _ = this.update(cx, |this, cx| {
                this.settled = true;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn preload(&mut self, rows: &[Row], cx: &mut Context<Self>) {
        let end = self.visible.end.min(rows.len());
        let start = self.visible.start.min(end);
        let mut skua = Vec::new();
        for (index, row) in rows[start..end].iter().enumerate() {
            let Row::Script(entry) = row else {
                continue;
            };
            if scripting::is_skua(&entry.path) {
                skua.push((start + index, entry.path.clone()));
            } else {
                let key = FormKey::Script(entry.path.clone());
                self.load_form(key, &entry.path, Load::Ahead, cx);
            }
        }
        if !self.settled || self.preloading.is_some() || self.urgent.load(Ordering::SeqCst) > 0 {
            return;
        }
        let selected = self.selected;
        let next = skua
            .into_iter()
            .filter(|(_, path)| self.needs_load(&FormKey::Script(path.clone())))
            .min_by_key(|(index, _)| index.abs_diff(selected));
        if let Some((_, path)) = next {
            self.load_form(FormKey::Script(path.clone()), &path, Load::Ahead, cx);
        }
    }

    fn evict(&mut self, rows: &[Row], target: Option<&Path>) {
        let span = self.visible.len();
        let end = (self.visible.end + span).min(rows.len());
        let start = self.visible.start.saturating_sub(span).min(end);
        let kept: HashSet<&Path> = rows[start..end]
            .iter()
            .filter_map(|row| match row {
                Row::Script(entry) => Some(entry.path.as_path()),
                Row::Source(_) => None,
            })
            .chain(target)
            .chain(self.displayed.as_ref().map(|entry| entry.path.as_path()))
            .collect();
        self.forms.retain(|key, form| match key {
            FormKey::Script(path) => form.edited || kept.contains(path.as_path()),
            FormKey::Entry(_) => true,
        });
        if let Some((path, cancelled)) = &self.preloading
            && !kept.contains(path.as_path())
        {
            cancelled.store(true, Ordering::SeqCst);
        }
    }

    fn needs_load(&self, key: &FormKey) -> bool {
        let current = self
            .forms
            .get(key)
            .is_some_and(|form| form.generation == self.generation);
        !current && !self.loading.contains_key(key)
    }

    fn load_form(&mut self, key: FormKey, path: &Path, load: Load, cx: &mut Context<Self>) {
        if !self.needs_load(&key) {
            return;
        }
        let script = path.to_owned();
        if scripting::is_skua(&script) {
            return self.describe_skua(key, script, load, cx);
        }
        let read = cx.background_spawn(async move {
            let display = file_name(&script);
            let text = fs::read_to_string(&script).map_err(|error| {
                (
                    "This script can't be read",
                    format!("Couldn't read {display}: {error}"),
                )
            })?;
            scripting::extract(&display, &script, &text).map_err(|error| match error {
                ExtractError::Syntax(message) => ("This script has a syntax error", message),
                ExtractError::Meta(message) => ("This script's meta is invalid", message),
            })
        });
        let generation = self.generation;
        let task = cx.spawn({
            let key = key.clone();
            async move |this, cx| {
                let read = read.await;
                let _ = this.update(cx, |this, cx| {
                    this.loading.remove(&key);
                    this.loaded(key, generation, read, None, cx);
                    cx.notify();
                });
            }
        });
        if let FormKey::Script(path) = &key
            && load == Load::Shown
            && !self.forms.contains_key(&key)
        {
            let wake = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(PATIENCE).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            });
            self.waiting = Some((path.clone(), Instant::now(), wake));
        }
        self.loading.insert(key, task);
    }

    fn describe_skua(&mut self, key: FormKey, script: PathBuf, load: Load, cx: &mut Context<Self>) {
        let stop = match (&key, load) {
            (FormKey::Script(_), Load::Shown) => Stop::Superseded {
                turn: self.describing.fetch_add(1, Ordering::SeqCst) + 1,
                latest: self.describing.clone(),
            },
            (FormKey::Script(_), Load::Ahead) => {
                let cancelled = Arc::new(AtomicBool::new(false));
                self.preloading = Some((script.clone(), cancelled.clone()));
                Stop::Ahead {
                    cancelled,
                    urgent: self.urgent.clone(),
                }
            }
            (FormKey::Entry(id), _) => {
                let removed = Arc::new(AtomicBool::new(false));
                self.probes.insert(*id, removed.clone());
                Stop::Removed(removed)
            }
        };
        let urgent = !matches!(stop, Stop::Ahead { .. });
        if urgent {
            self.urgent.fetch_add(1, Ordering::SeqCst);
        }
        let read = cx
            .background_spawn(async move { scripting::describe_skua(&script, &|| stop.stopped()) });
        let generation = self.generation;
        let task = cx.spawn({
            let key = key.clone();
            async move |this, cx| {
                let read = read.await;
                let _ = this.update(cx, |this, cx| {
                    this.loading.remove(&key);
                    if urgent {
                        this.urgent.fetch_sub(1, Ordering::SeqCst);
                    } else {
                        this.preloading = None;
                    }
                    if let FormKey::Entry(id) = &key {
                        this.probes.remove(id);
                    }
                    match read {
                        Err(scripting::SkuaHalt::Stopped) => {}
                        Err(scripting::SkuaHalt::Failed(message)) => this.loaded(
                            key,
                            generation,
                            Err(("This Skua script can't start", message)),
                            None,
                            cx,
                        ),
                        Ok(described) => {
                            let (definition, unread) = match described {
                                scripting::SkuaDescribed::Inputs(definition) => (definition, None),
                                scripting::SkuaDescribed::Unread(reason) => {
                                    (None, Some(reason.into()))
                                }
                            };
                            this.loaded(
                                key,
                                generation,
                                Ok(Declared {
                                    meta: Meta::default(),
                                    inputs: Ok(definition),
                                }),
                                unread,
                                cx,
                            );
                        }
                    }
                    cx.notify();
                });
            }
        });
        self.loading.insert(key, task);
    }

    fn detail_entry(&mut self, target: Option<Entry>) -> Option<Entry> {
        let target = target?;
        if self
            .forms
            .contains_key(&FormKey::Script(target.path.clone()))
        {
            if self.displayed.as_ref().map(|shown| &shown.path) != Some(&target.path) {
                self.detail_scroll.set_offset(gpui::point(px(0.0), px(0.0)));
                self.error_cursor = None;
                self.displayed = Some(target.clone());
            }
            return Some(target);
        }
        let patient = self
            .waiting
            .as_ref()
            .is_some_and(|(path, since, _)| *path == target.path && since.elapsed() < PATIENCE);
        match &self.displayed {
            Some(previous)
                if patient
                    && self
                        .forms
                        .contains_key(&FormKey::Script(previous.path.clone())) =>
            {
                Some(previous.clone())
            }
            _ => Some(target),
        }
    }

    fn loaded(
        &mut self,
        key: FormKey,
        generation: u64,
        read: Result<Declared, (&'static str, String)>,
        unread: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let (meta, read) = match read {
            Ok(Declared { meta, inputs }) => (
                meta,
                inputs.map_err(|message| ("This script's inputs are invalid", message)),
            ),
            Err(broken) => (Meta::default(), Err(broken)),
        };
        if let Some(form) = self.forms.get_mut(&key)
            && let (FormState::Fields { definition, .. }, Ok(Some(read))) = (&form.state, &read)
            && definition == read
        {
            form.generation = generation;
            form.meta = meta;
            return;
        }
        let state = match read {
            Err((title, message)) => FormState::Broken {
                title,
                message: message.into(),
            },
            Ok(None) => FormState::Plain { unread },
            Ok(Some(definition)) => {
                let saved = match &key {
                    FormKey::Script(path) => match script_inputs::saved(path, cx) {
                        Supplied::Values(values) => values,
                        Supplied::StoreUnreadable => Values::new(),
                    },
                    FormKey::Entry(id) => self.entry_values(*id, cx).unwrap_or_default(),
                };
                let drafts = drafts(&definition, &saved);
                let mut texts = Vec::new();
                let mut changes = Vec::new();
                for (index, (field, draft)) in definition.fields().iter().zip(&drafts).enumerate() {
                    let Draft::Text(text) = &draft.draft else {
                        texts.push(None);
                        continue;
                    };
                    let invalid = error(field, draft).is_some();
                    let label = field.label.clone();
                    let text = text.clone();
                    let input = cx.new(|cx| {
                        let mut input = Input::new(label, cx);
                        input.set_text(text, cx);
                        input.set_invalid(invalid, cx);
                        input
                    });
                    let key = key.clone();
                    changes.push(cx.subscribe(
                        &input,
                        move |this, input, event: &InputEvent, cx| {
                            if matches!(event, InputEvent::Changed) {
                                let text = input.read(cx).text().to_owned().into();
                                this.set_draft(&key, index, Draft::Text(text), cx);
                            }
                        },
                    ));
                    texts.push(Some(input));
                }
                FormState::Fields {
                    definition,
                    drafts,
                    texts,
                    _changes: changes,
                }
            }
        };
        let checked = match (&key, &state) {
            (
                FormKey::Entry(_),
                FormState::Fields {
                    definition, drafts, ..
                },
            ) => Some(values(definition, drafts)),
            _ => None,
        };
        self.forms.insert(
            key.clone(),
            Form {
                generation,
                meta,
                edited: false,
                state,
            },
        );
        if let Some(edited) = checked {
            self.store_entry(&key, edited, cx);
        }
    }

    fn set_draft(&mut self, key: &FormKey, index: usize, draft: Draft, cx: &mut Context<Self>) {
        let Some(Form {
            edited,
            state:
                FormState::Fields {
                    definition,
                    drafts,
                    texts,
                    ..
                },
            ..
        }) = self.forms.get_mut(key)
        else {
            return;
        };
        let (Some(field), Some(slot)) = (definition.fields().get(index), drafts.get_mut(index))
        else {
            return;
        };
        *slot = FieldDraft { draft, stale: None };
        *edited = true;
        if let Some(Some(input)) = texts.get(index) {
            let invalid = error(field, slot).is_some();
            input.update(cx, |input, cx| input.set_invalid(invalid, cx));
        }
        let edited = values(definition, drafts);
        self.store_entry(key, edited, cx);
        cx.notify();
    }

    fn store_entry(&mut self, key: &FormKey, edited: Option<Values>, cx: &mut Context<Self>) {
        if let FormKey::Entry(id) = key {
            let id = *id;
            self.edit_queue(cx, |queue| queue.set_values(id, edited));
        }
    }

    fn entry_values(&self, id: EntryId, cx: &App) -> Option<Values> {
        let view = self.view()?;
        let queue = &view.read(cx).queue;
        let entry = queue.entries().iter().find(|entry| entry.id == id)?;
        entry.values.clone()
    }

    fn reset_form(&mut self, key: &FormKey, cx: &mut Context<Self>) {
        let Some(Form {
            state:
                FormState::Fields {
                    definition,
                    drafts,
                    texts,
                    ..
                },
            ..
        }) = self.forms.get_mut(key)
        else {
            return;
        };
        for ((field, draft), input) in definition.fields().iter().zip(drafts.iter_mut()).zip(texts)
        {
            *draft = FieldDraft {
                draft: starting(field),
                stale: None,
            };
            if let (Some(input), Draft::Text(text)) = (input, &draft.draft) {
                let invalid = error(field, draft).is_some();
                let text = text.clone();
                input.update(cx, |input, cx| {
                    input.set_text(text, cx);
                    input.set_invalid(invalid, cx);
                });
            }
        }
        let edited = values(definition, drafts);
        self.store_entry(key, edited, cx);
        cx.notify();
    }

    fn launch_values(&self, path: &Path, cx: &App) -> Option<(Option<&Definition>, Values)> {
        match &self.forms.get(&FormKey::Script(path.to_owned()))?.state {
            FormState::Broken { .. } => None,
            FormState::Plain { unread: None } => Some((None, Values::new())),
            FormState::Plain { unread: Some(_) } => match script_inputs::saved(path, cx) {
                Supplied::Values(values) => Some((None, values)),
                Supplied::StoreUnreadable => Some((None, Values::new())),
            },
            FormState::Fields {
                definition, drafts, ..
            } => {
                if script_inputs::unreadable(cx) {
                    return None;
                }
                Some((Some(definition), values(definition, drafts)?))
            }
        }
    }

    fn launch(&mut self, cx: &mut Context<Self>) -> Option<(PathBuf, Values)> {
        let entry = self.target(cx)?;
        let (definition, values) = self.launch_values(&entry.path, cx)?;
        if definition.is_some() {
            script_inputs::save(&entry.path, values.clone(), cx);
            if let Some(form) = self.forms.get_mut(&FormKey::Script(entry.path.clone())) {
                form.edited = false;
            }
        }
        Some((entry.path, values))
    }

    fn entry_ready(&self, id: EntryId) -> bool {
        match self.forms.get(&FormKey::Entry(id)).map(|form| &form.state) {
            Some(FormState::Plain { .. }) => true,
            Some(FormState::Fields {
                definition, drafts, ..
            }) => values(definition, drafts).is_some(),
            Some(FormState::Broken { .. }) | None => false,
        }
    }

    fn queue(&self, cx: &App) -> (Vec<queue::Entry>, bool) {
        self.view().map_or_else(Default::default, |view| {
            let queue = &view.read(cx).queue;
            (queue.entries().to_vec(), queue.playing())
        })
    }

    fn queued_id(&self, index: usize, cx: &App) -> Option<EntryId> {
        let view = self.view()?;
        let entry = view.read(cx).queue.entries().get(index)?;
        Some(entry.id)
    }

    fn edit_queue(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Queue)) {
        if let Some(view) = self.view() {
            view.update(cx, |view, cx| view.edit_queue(cx, edit));
        }
        cx.notify();
    }

    fn add_to_queue(&mut self, cx: &mut Context<Self>) {
        let Some((script, values)) = self.launch(cx) else {
            return;
        };
        self.edit_queue(cx, |queue| {
            queue.add(script, values);
        });
    }

    fn move_queued(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let Some(id) = self.queued_id(from, cx) else {
            return;
        };
        self.edit_queue(cx, |queue| queue.move_entry(id, to));
        if let Some(index) = self.queue(cx).0.iter().position(|entry| entry.id == id) {
            self.queued = index;
        }
    }

    fn remove_queued(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.queued_id(self.queued, cx) {
            self.edit_queue(cx, |queue| queue.remove(id));
        }
    }

    fn act_on_tab(
        &mut self,
        cx: &mut Context<Self>,
        act: fn(&mut RuffleView, &mut Context<RuffleView>),
    ) {
        if let Some(view) = self.view() {
            view.update(cx, act);
        }
        cx.notify();
    }

    fn options_popover(&self, cx: &mut Context<Self>) -> Popover {
        let this = cx.entity().downgrade();
        Popover::new("options")
            .trigger(Button::new("options", "Options").end_slot(Icon::ChevronDown))
            .content(move |_, _, cx| {
                let Some(scripts) = this.upgrade() else {
                    return div().into_any_element();
                };
                let scripts = scripts.read(cx);
                options_panel(
                    this.clone(),
                    scripts.options(cx),
                    scripts.room.clone(),
                    scripts.skua_selected(cx),
                    cx,
                )
            })
            .align(Align::Start)
            .width(px(376.0))
    }

    fn inputs(&self, theme: &Theme, key: &FormKey, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let layout = theme.layout;
        let Some(form) = self.forms.get(key) else {
            return vec![muted(theme, "Reading inputs…").into_any_element()];
        };
        let (definition, drafts, texts) = match &form.state {
            FormState::Broken { title, message } => {
                return vec![
                    Alert::new("script-broken", Tone::Danger, *title)
                        .body(message.clone())
                        .into_any_element(),
                ];
            }
            FormState::Plain { unread } => return vec![no_inputs(theme, unread.as_ref())],
            FormState::Fields {
                definition,
                drafts,
                texts,
                ..
            } => (definition, drafts, texts),
        };
        let this = cx.entity().downgrade();
        let saved = matches!(key, FormKey::Script(_));
        let (unreadable, stale) = leading(saved, drafts, cx);
        let unreadable = unreadable.then(|| {
            let this = this.clone();
            Alert::new(
                "inputs-unreadable",
                Tone::Warning,
                "Saved inputs couldn't be read",
            )
            .body("Start fresh to back up the file and use each script's defaults.")
            .action(
                Button::new("inputs-start-fresh", "Start fresh")
                    .size(Size::Small)
                    .on_click(move |_, _, cx| {
                        script_inputs::start_fresh(cx);
                        let _ = this.update(cx, |_, cx| cx.notify());
                    }),
            )
        });
        let stale = stale.then(|| {
            let this = this.clone();
            let key = key.clone();
            h_flex()
                .justify_between()
                .gap(layout.space(3.0))
                .child(muted(
                    theme,
                    if saved {
                        "Some saved values don't fit this script anymore."
                    } else {
                        "Some queued values don't fit this script anymore."
                    },
                ))
                .child(
                    Button::new("inputs-reset", "Reset")
                        .size(Size::Small)
                        .on_click(move |_, _, cx| {
                            let _ = this.update(cx, |this, cx| this.reset_form(&key, cx));
                        }),
                )
        });
        let fields =
            definition
                .fields()
                .iter()
                .zip(drafts)
                .enumerate()
                .map(|(index, (field, draft))| {
                    let set = {
                        let this = this.clone();
                        let key = key.clone();
                        move |draft: Draft, cx: &mut App| {
                            let _ =
                                this.update(cx, |this, cx| this.set_draft(&key, index, draft, cx));
                        }
                    };
                    let label = SharedString::from(field.label.clone());
                    let control = match (&field.kind, &draft.draft, &texts[index]) {
                        (Kind::Boolean { .. }, Draft::Toggle(on), _) => {
                            Switch::new(("input", index), label.clone(), *on)
                                .trailing()
                                .on_toggle(move |on, _, cx| set(Draft::Toggle(on), cx))
                                .into_any_element()
                        }
                        (Kind::Select { options, .. }, Draft::Choice(chosen), _) => Select::new(
                            ("input", index),
                            label.clone(),
                            options.iter().cloned().map(SharedString::from),
                            *chosen,
                        )
                        .placeholder("Choose…")
                        .full_width()
                        .invalid(error(field, draft).is_some())
                        .on_select(move |chosen, _, cx| set(Draft::Choice(Some(chosen)), cx))
                        .into_any_element(),
                        (Kind::MultiSelect { options, .. }, Draft::Choices(chosen), _) => {
                            let set = Rc::new(set);
                            v_flex()
                                .gap(layout.space(3.0))
                                .children(options.iter().enumerate().map(|(option, name)| {
                                    let set = set.clone();
                                    let chosen = chosen.clone();
                                    let state = if chosen[option] {
                                        CheckedState::Checked
                                    } else {
                                        CheckedState::Unchecked
                                    };
                                    Checkbox::new(
                                        SharedString::from(format!("input-{index}-{option}")),
                                        name.clone(),
                                        state,
                                    )
                                    .on_toggle(
                                        move |on, _, cx| {
                                            let mut chosen = chosen.clone();
                                            chosen[option] = on;
                                            set(Draft::Choices(chosen), cx);
                                        },
                                    )
                                }))
                                .into_any_element()
                        }
                        (kind, _, Some(input)) => div()
                            .when(matches!(kind, Kind::Number { .. }), |slot| {
                                slot.flex_none().w(px(80.0))
                            })
                            .child(input.clone())
                            .into_any_element(),
                        _ => div().into_any_element(),
                    };
                    let control = match field.kind {
                        Kind::Boolean { .. } => control,
                        Kind::Number { .. } => h_flex()
                            .justify_between()
                            .gap(layout.space(2.0))
                            .child(div().min_w_0().font_weight(FontWeight::MEDIUM).child(label))
                            .child(control)
                            .into_any_element(),
                        _ => mira_ui::primitives::Field::new(label, control).into_any_element(),
                    };
                    let starts_group = field.group.is_some()
                        && definition
                            .fields()
                            .get(index.wrapping_sub(1))
                            .map(|previous| &previous.group)
                            != Some(&field.group);
                    v_flex()
                        .gap(layout.space(1.0))
                        .when_some(
                            field.group.clone().filter(|_| starts_group),
                            |column, group| {
                                column.child(
                                    div()
                                        .pt(layout.space(2.0))
                                        .text_caption(theme.typography)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.muted_foreground)
                                        .child(group),
                                )
                            },
                        )
                        .child(control)
                        .map(|column| match (error(field, draft), &field.description) {
                            (Some(problem), _) => column
                                .child(note(problem, true, theme).text_color(theme.destructive)),
                            (None, Some(description)) => {
                                column.child(note(description.clone(), true, theme))
                            }
                            (None, None) => column,
                        })
                });
        unreadable
            .map(IntoElement::into_any_element)
            .into_iter()
            .chain(stale.map(IntoElement::into_any_element))
            .chain(fields.map(IntoElement::into_any_element))
            .collect()
    }

    fn input_errors(&self, path: &Path, cx: &App) -> usize {
        match self
            .forms
            .get(&FormKey::Script(path.to_owned()))
            .map(|form| &form.state)
        {
            Some(FormState::Fields {
                definition, drafts, ..
            }) if !script_inputs::unreadable(cx) => errored_fields(definition, drafts).len(),
            _ => 0,
        }
    }

    fn jump_to_error(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.displayed.as_ref().map(|entry| entry.path.clone()) else {
            return;
        };
        let Some(Form {
            state:
                FormState::Fields {
                    definition,
                    drafts,
                    texts,
                    ..
                },
            ..
        }) = self.forms.get(&FormKey::Script(path))
        else {
            return;
        };
        let errored = errored_fields(definition, drafts);
        let count = errored.len() as isize;
        if count == 0 {
            return;
        }
        let next = match self.error_cursor {
            Some(at) => (at as isize + by).rem_euclid(count),
            None if by > 0 => 0,
            None => count - 1,
        } as usize;
        let field = errored[next];
        let (unreadable, stale) = leading(true, drafts, cx);
        let item = usize::from(unreadable) + usize::from(stale) + field;
        let input = texts.get(field).cloned().flatten();
        self.error_cursor = Some(next);
        let theme = cx.theme();
        reveal_item(
            &self.detail_scroll,
            item,
            theme.space(6.0),
            error_bar_clearance(theme),
        );
        if let Some(input) = input {
            window.focus(&input.focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn error_bar(&self, theme: &Theme, errors: usize, cx: &mut Context<Self>) -> AnyElement {
        let label: SharedString = match errors {
            1 => "1 error".into(),
            count => format!("{count} errors").into(),
        };
        let step = |id: &'static str, glyph: Icon, name: &'static str, by: isize| {
            Button::icon(id, glyph, name)
                .variant(Variant::Ghost)
                .size(Size::Small)
                .hide_tooltip()
                .on_click(
                    cx.listener(move |this, _, window, cx| this.jump_to_error(by, window, cx)),
                )
        };
        theme
            .floating_bar()
            .id("error-bar")
            .role(gpui::Role::Toolbar)
            .aria_label("Input errors")
            .child(
                div()
                    .pr(theme.space(1.0))
                    .child(status_text("input-errors", label)),
            )
            .child(step(
                "previous-error",
                Icon::ChevronUp,
                "Previous error",
                -1,
            ))
            .child(step("next-error", Icon::ChevronDown, "Next error", 1))
            .into_any_element()
    }

    fn about(&self, path: &Path, key: &FormKey, cx: &App) -> About {
        let meta = self.forms.get(key).map(|form| &form.meta);
        let listed = self
            .library
            .iter()
            .flat_map(|library| &library.entries)
            .find(|entry| entry.path == path);
        About {
            name: meta
                .and_then(|meta| meta.name.clone())
                .map(SharedString::from)
                .or_else(|| listed.map(|entry| entry.name.clone()))
                .unwrap_or_else(|| script_names::name(path, cx)),
            description: meta
                .and_then(|meta| meta.description.clone())
                .map(SharedString::from)
                .or_else(|| listed.and_then(|entry| entry.description.clone())),
            tags: listed.map(|entry| entry.tags.clone()).unwrap_or_default(),
        }
    }

    fn script_detail(
        &self,
        theme: &Theme,
        entry: Option<&Entry>,
        cx: &mut Context<Self>,
    ) -> Option<Detail> {
        let entry = entry?;
        let errors = self.input_errors(&entry.path, cx);
        let bar = (errors > 0).then(|| self.error_bar(theme, errors, cx));
        let key = FormKey::Script(entry.path.clone());
        let about = self.about(&entry.path, &key, cx);
        Some(Detail {
            heading: heading(theme, &about, home_relative(&entry.path), None).into_any_element(),
            body: self.inputs(theme, &key, cx),
            bar,
        })
    }

    fn queued_detail(
        &self,
        theme: &Theme,
        queue: &[queue::Entry],
        cx: &mut Context<Self>,
    ) -> Option<Detail> {
        let layout = theme.layout;
        let queued = queue.get(self.queued)?;
        let failed = match &queued.state {
            queue::State::Ended { outcome, .. } => failure(outcome),
            _ => None,
        };
        let key = FormKey::Entry(queued.id);
        let about = self.about(&queued.script, &key, cx);
        let body = match self.forms.get(&key).map(|form| &form.state) {
            _ if queued.waiting() => self.inputs(theme, &key, cx),
            None => vec![muted(theme, "Reading inputs…").into_any_element()],
            Some(FormState::Broken { title, message }) => vec![
                Alert::new("queued-broken", Tone::Danger, *title)
                    .body(message.clone())
                    .into_any_element(),
            ],
            Some(FormState::Plain { unread }) => vec![no_inputs(theme, unread.as_ref())],
            Some(FormState::Fields { definition, .. }) => queued
                .values
                .as_ref()
                .map(|values| rows(definition, values))
                .unwrap_or_default()
                .into_iter()
                .map(|(label, value)| {
                    row(layout, label, true, div().truncate().child(value)).into_any_element()
                })
                .collect(),
        };
        Some(Detail {
            heading: heading(
                theme,
                &about,
                home_relative(&queued.script),
                Some(
                    Button::new("queued-remove", "Remove")
                        .size(Size::Small)
                        .variant(Variant::Ghost)
                        .disabled(queued.running())
                        .on_click(cx.listener(|this, _, _, cx| this.remove_queued(cx)))
                        .into_any_element(),
                ),
            )
            .into_any_element(),
            body: failed
                .map(|message| {
                    Alert::new("queued-failed", Tone::Danger, "Failed")
                        .body(message)
                        .into_any_element()
                })
                .into_iter()
                .chain(body)
                .collect(),
            bar: None,
        })
    }

    fn queue_list(
        &self,
        theme: &Theme,
        queue: &[queue::Entry],
        width: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = theme.layout;
        let number_width = layout.space(4.0);
        let text_width = width - layout.space(3.0) * 2.0 - layout.space(2.0) * 3.0 - number_width;
        let rows: Rc<[QueueRow]> = queue
            .iter()
            .map(|queued| {
                let key = FormKey::Entry(queued.id);
                let name = self.about(&queued.script, &key, cx).name;
                let form = self.forms.get(&key).map(|form| &form.state);
                let meta: Option<(SharedString, Hsla)> = match &queued.state {
                    queue::State::Waiting => match form {
                        Some(FormState::Broken { .. }) => Some("Can't start".into()),
                        Some(FormState::Fields {
                            definition, drafts, ..
                        }) if values(definition, drafts).is_none() => Some("Needs inputs".into()),
                        _ => None,
                    }
                    .map(|meta| (meta, theme.muted_foreground)),
                    queue::State::Running { .. } => Some(("Running".into(), theme.foreground)),
                    queue::State::Ended { outcome, took } => Some((
                        format!("{} · {}", ended(outcome), duration(*took)).into(),
                        theme.muted_foreground,
                    )),
                };
                let folder = queued
                    .script
                    .parent()
                    .map(home_relative)
                    .unwrap_or_default();
                let room = match &meta {
                    Some((meta, _)) => {
                        text_width - layout.space(2.0) - caption_width(meta, window, theme)
                    }
                    None => text_width,
                };
                QueueRow {
                    name,
                    folder: fit_path_to(&folder, room, window, theme),
                    meta,
                }
            })
            .collect();
        let pinned = queue.iter().filter(|entry| !entry.waiting()).count();
        let quiet = theme.muted_foreground;
        let this = cx.entity().downgrade();
        let that = this.clone();
        List::new("queue", "Queue", rows.len(), move |index| {
            let QueueRow { name, folder, meta } = &rows[index];
            let item = ListItem::new(name.clone())
                .start_slot(
                    div()
                        .min_w(number_width)
                        .text_right()
                        .font_features(mira_ui::theme::tabular())
                        .text_color(quiet)
                        .child((index + 1).to_string()),
                )
                .description(folder.clone());
            match meta {
                Some((meta, color)) => item.end_slot(div().text_color(*color).child(meta.clone())),
                None => item,
            }
        })
        .selected(Some(self.queued))
        .fill()
        .flush()
        .on_select(move |index, _, cx| {
            let _ = this.update(cx, |this, cx| {
                this.queued = index;
                cx.notify();
            });
        })
        .on_reorder(move |from, to, _, cx| {
            let _ = that.update(cx, |this, cx| this.move_queued(from, to, cx));
        })
        .pin_first(pinned)
        .into_any_element()
    }

    fn no_scripts(&self, cx: &mut Context<Self>) -> Empty {
        let folder = self.folder.clone();
        Empty::new(
            "No scripts yet",
            format!(
                "Put .ts or .js files in {}, or add a folder you already have.",
                home_relative(&self.folder)
            ),
        )
        .action(
            Button::new("empty-show", "Show in Finder")
                .size(Size::Small)
                .on_click(cx.listener(move |this, _, _, cx| this.show_folder(&folder, cx))),
        )
        .action(
            Button::new("empty-add", "Add folder…")
                .size(Size::Small)
                .on_click(cx.listener(|this, _, window, cx| this.add_folder(window, cx))),
        )
    }

    fn library_empty(&self, no_scripts: bool, cx: &mut Context<Self>) -> Option<Empty> {
        if self.searching(cx) {
            return Some(Empty::new("No scripts match", "Try a different search."));
        }
        if no_scripts {
            return Some(self.no_scripts(cx));
        }
        let Page::Library(Some(root)) = &self.page else {
            return None;
        };
        let folder_empty = |root: PathBuf, cx: &mut Context<Self>| {
            Empty::new(
                "No scripts here",
                format!("Put .ts or .js files in {}.", home_relative(&root)),
            )
            .action(
                Button::new("empty-source-show", "Show in Finder")
                    .size(Size::Small)
                    .on_click(cx.listener(move |this, _, _, cx| this.show_folder(&root, cx))),
            )
        };
        if *root == self.folder {
            return Some(folder_empty(root.clone(), cx));
        }
        let source = self
            .library
            .as_ref()?
            .sources
            .iter()
            .find(|source| source.root == *root)?;
        Some(match (&source.problem, &source.kind) {
            (Some(problem), SourceKind::Folder) => {
                let root = root.clone();
                Empty::new(format!("Can't load {}", source.name), problem.clone()).action(
                    Button::new("empty-source-locate", "Locate…")
                        .size(Size::Small)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.locate_folder(root.clone(), window, cx)
                        })),
                )
            }
            (Some(problem), SourceKind::Package { .. } | SourceKind::Skua { .. }) => {
                Empty::new(format!("Can't load {}", source.name), problem.clone())
            }
            (None, SourceKind::Package { .. } | SourceKind::Skua { .. }) => {
                Empty::new("No scripts here", "This package has no scripts.")
            }
            (None, SourceKind::Folder) => folder_empty(root.clone(), cx),
        })
    }

    fn library(
        &self,
        library: &Library,
        rows: Rc<[Row]>,
        folder_width: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (folders, installed) = self.source_rows(cx);
        let qualified: HashMap<PathBuf, SharedString> = folders
            .into_iter()
            .chain(installed)
            .filter_map(|row| Some((row.root, row.qualifier?)))
            .collect();
        let headings: Rc<[(SharedString, Option<SharedString>)]> = library
            .sources
            .iter()
            .map(|source| (source.name.clone(), qualified.get(&source.root).cloned()))
            .collect();
        let count = rows.len();
        let this = cx.entity().downgrade();
        let that = this.clone();
        let watcher = this.clone();
        VirtualList::new("scripts", "Scripts", count, move |index| {
            match &rows[index] {
                Row::Source(source) => {
                    let (name, qualifier) = &headings[*source];
                    match qualifier {
                        Some(qualifier) => {
                            ListItem::header(name.clone()).end_slot(qualifier.clone())
                        }
                        None => ListItem::header(name.clone()),
                    }
                }
                Row::Script(entry) => match &entry.folder {
                    Some(folder) => ListItem::new(entry.name.clone()).end_slot(
                        div()
                            .max_w(folder_width)
                            .truncate()
                            .text_ellipsis_start()
                            .child(folder.clone()),
                    ),
                    None => ListItem::new(entry.name.clone()),
                },
            }
        })
        .selected(Some(self.selected.min(count - 1)))
        .on_visible(move |range, _, cx| {
            let _ = watcher.update(cx, |this, cx| this.saw(range, cx));
        })
        .fill()
        .flush()
        .on_select(move |index, _, cx| {
            let _ = this.update(cx, |this, cx| {
                this.selected = index;
                this.chosen = None;
                cx.notify();
            });
        })
        .on_activate(move |index, _, cx| {
            let _ = that.update(cx, |this, cx| {
                this.selected = index;
                this.chosen = None;
                this.run_selected(cx);
            });
        })
        .into_any_element()
    }
}

fn first_script(rows: &[Row]) -> usize {
    rows.iter()
        .position(|row| matches!(row, Row::Script(_)))
        .unwrap_or(0)
}

fn fit_path(path: &str, fits: impl Fn(&str) -> bool) -> SharedString {
    if fits(path) {
        return path.to_owned().into();
    }
    let segments: Vec<&str> = path.split('/').collect();
    let head = match segments.as_slice() {
        ["~" | "", _, ..] => 2,
        _ => 1,
    };
    if segments.len() <= head + 1 {
        return path.to_owned().into();
    }
    let start = segments[..head].join("/");
    let last = segments.len() - 1;
    let cut = |kept: usize| format!("{start}/…/{}", segments[segments.len() - kept..].join("/"));
    if !fits(&cut(1)) {
        return format!("…/{}", segments[last]).into();
    }
    let mut kept = 1;
    while kept + 1 < segments.len() - head && fits(&cut(kept + 1)) {
        kept += 1;
    }
    cut(kept).into()
}

pub(super) fn fit_path_to(
    path: &str,
    width: Pixels,
    window: &Window,
    theme: &Theme,
) -> SharedString {
    fit_path(path, |text| caption_width(text, window, theme) <= width)
}

fn caption_width(text: &str, window: &Window, theme: &Theme) -> Pixels {
    let run = TextRun {
        len: text.len(),
        font: gpui::font(theme.font_family.clone()),
        color: Hsla::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(
            text.to_owned().into(),
            theme.typography.secondary,
            &[run],
            None,
        )
        .width
}

impl Drop for ScriptsWindow {
    fn drop(&mut self) {
        self.describing.fetch_add(1, Ordering::SeqCst);
        if let Some((_, cancelled)) = &self.preloading {
            cancelled.store(true, Ordering::SeqCst);
        }
        for removed in self.probes.values() {
            removed.store(true, Ordering::SeqCst);
        }
    }
}

impl Render for ScriptsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let layout = theme.layout;
        let typography = theme.typography;
        let has_tab = self.shown.is_some();
        let rows = self.rows(cx);
        let chosen = self.target(cx);
        if let Some(entry) = &chosen {
            self.load_form(
                FormKey::Script(entry.path.clone()),
                &entry.path,
                Load::Shown,
                cx,
            );
        }
        let target = chosen.as_ref().map(|entry| entry.path.clone());
        let chosen = self.detail_entry(chosen);
        let (queue, playing) = self.queue(cx);
        let queued: HashSet<EntryId> = self
            .client
            .read(cx)
            .views()
            .iter()
            .flat_map(|view| view.read(cx).queue.entries().iter().map(|entry| entry.id))
            .collect();
        self.forms.retain(|key, _| match key {
            FormKey::Script(_) => true,
            FormKey::Entry(id) => queued.contains(id),
        });
        self.probes.retain(|id, removed| {
            let kept = queued.contains(id);
            if !kept {
                removed.store(true, Ordering::SeqCst);
            }
            kept
        });
        let page = self.shown_page(cx);
        if page == Page::Queue {
            for entry in &queue {
                self.load_form(FormKey::Entry(entry.id), &entry.script, Load::Shown, cx);
            }
        }
        if matches!(page, Page::Library(_)) {
            self.preload(&rows, cx);
        }
        self.evict(&rows, target.as_deref());
        self.queued = self.queued.min(queue.len().saturating_sub(1));
        let valid = chosen
            .as_ref()
            .is_some_and(|entry| self.launch_values(&entry.path, cx).is_some());
        let running = self.running(cx);
        let unfinished = queue
            .iter()
            .filter(|entry| !matches!(entry.state, queue::State::Ended { .. }))
            .count();
        let waiting = queue.iter().any(queue::Entry::waiting);
        let startable = queue
            .iter()
            .filter(|entry| entry.waiting())
            .all(|entry| self.entry_ready(entry.id));
        let finished = queue.len() > unfinished;
        let skippable = queue.iter().any(queue::Entry::running);
        let status = self.run_status(cx);
        let private = matches!(
            self.options(cx).map(|options| options.rooms),
            Some(Rooms::Private { .. })
        );
        let skua = self.skua_selected(cx);
        self.room
            .update(cx, |room, cx| room.set_disabled(!private || skua, cx));
        let header = shown_tab::header(
            "Scripts",
            &self.client,
            self.shown.as_ref(),
            self.tab,
            Self::retarget,
            window,
            cx,
        )
        .map(|header| match status {
            Some(status) => header.child(run_status(
                status,
                cx.listener(|this, _, _, cx| this.stop(cx)),
                &theme,
            )),
            None if !has_tab => header.child(
                div()
                    .id("script-status")
                    .role(gpui::Role::Status)
                    .aria_label("No tabs open")
                    .pr(PADDING)
                    .text_size(typography.secondary)
                    .text_color(theme.muted_foreground)
                    .child("No tabs open"),
            ),
            None => header,
        });
        let no_scripts = matches!(
            &self.library,
            Some(library) if library.entries.is_empty() && library.sources.is_empty()
        );
        let inspector_width = layout.inspector;
        let list_width = window.viewport_size().width - layout.sidebar - inspector_width - px(1.0);
        let inspected = |detail: Option<Detail>| {
            let inline = |pane: Div| {
                pane.pl(theme.space(4.0) + RING_ROOM)
                    .pr(theme.space(4.0) + Scrollbar::GUTTER)
            };
            let scrolled = self.detail_scroll.offset().y < px(0.0);
            v_flex().flex_none().w(inspector_width).h_full().when_some(
                detail,
                |pane, Detail { heading, body, bar }| {
                    let (reserve, covered) = if bar.is_some() {
                        (theme.space(15.0), error_bar_clearance(&theme))
                    } else {
                        (theme.space(4.0), Pixels::ZERO)
                    };
                    pane.child(inline(
                        div().flex_none().pt(theme.space(3.0)).child(heading),
                    ))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .child(mira_ui::nav::pane_start_above(
                                self.detail_scroll.clone(),
                                covered,
                            ))
                            .child(
                                inline(div())
                                    .id("detail")
                                    .size_full()
                                    .overflow_y_scroll()
                                    .track_scroll(&self.detail_scroll)
                                    .flex()
                                    .flex_col()
                                    .gap(theme.space(2.0))
                                    .pt(theme.space(4.0))
                                    .pb(reserve)
                                    .children(body),
                            )
                            .child(mira_ui::nav::pane_end())
                            .when_some(bar, |inputs, bar| {
                                inputs.child(
                                    h_flex()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .bottom_0()
                                        .justify_center()
                                        .px(theme.space(3.0))
                                        .pt(theme.space(6.0))
                                        .pb(theme.space(3.0))
                                        .bg(linear_gradient(
                                            180.0,
                                            linear_color_stop(theme.background.opacity(0.0), 0.0),
                                            linear_color_stop(theme.background, 0.45),
                                        ))
                                        .child(bar),
                                )
                            })
                            .when(scrolled, |inputs| {
                                inputs.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .right_0()
                                        .h(theme.space(6.0))
                                        .bg(linear_gradient(
                                            180.0,
                                            linear_color_stop(theme.background, 0.0),
                                            linear_color_stop(theme.background.opacity(0.0), 1.0),
                                        )),
                                )
                            })
                            .child(
                                Scrollbar::new("detail-scrollbar", self.detail_scroll.clone())
                                    .persistent(),
                            ),
                    )
                },
            )
        };
        let body = h_flex().flex_1().min_h_0().items_stretch();
        let source_header = self.source_header(&theme, cx);
        let body = match &page {
            Page::Library(_) if self.chosen.is_none() && (no_scripts || rows.is_empty()) => body
                .justify_center()
                .items_center()
                .children(self.library_empty(no_scripts, cx)),
            Page::Library(_) => {
                let list = if rows.is_empty() {
                    self.library_empty(no_scripts, cx)
                        .map(|empty| empty.compact().into_any_element())
                } else {
                    self.library
                        .as_ref()
                        .map(|library| self.library(library, rows, list_width * 0.45, cx))
                };
                let detail = self.script_detail(&theme, chosen.as_ref(), cx);
                body.child(v_flex().flex_1().min_w_0().children(list))
                    .child(theme.column_rule())
                    .child(inspected(detail))
            }
            Page::Queue if !has_tab => body.justify_center().items_center().child(Empty::new(
                "No tabs open",
                "Open a tab to queue scripts on it.",
            )),
            Page::Queue if queue.is_empty() => body.justify_center().items_center().child(
                Empty::new("Nothing queued", "Pick a script and choose Add to queue."),
            ),
            Page::Queue => {
                let list = self.queue_list(&theme, &queue, list_width, window, cx);
                let detail = self.queued_detail(&theme, &queue, cx);
                body.child(v_flex().flex_1().min_w_0().child(list))
                    .child(theme.column_rule())
                    .child(inspected(detail))
            }
            Page::CoreBots if !has_tab => body.justify_center().items_center().child(Empty::new(
                "No tabs open",
                "Open a tab to change its CoreBots options.",
            )),
            Page::CoreBots => body.child(div().flex_1().min_w_0().child(self.core_bots.clone())),
            Page::Log => body.justify_center().items_center().child(Empty::new(
                "Script logs",
                "Coming soon. Until then, script output goes to mira.log.",
            )),
        }
        .relative()
        .child(Toaster::layer(&self.toasts, window, cx));
        let footer = theme.footer().flex_wrap().child(self.options_popover(cx));
        let footer =
            match page {
                Page::Library(_) => Some(
                    footer
                        .child(div().flex_1())
                        .child(
                            Button::new("queue", "Add to queue")
                                .end_slot(mira_ui::primitives::Kbd::hint("⌘⏎"))
                                .disabled(!has_tab || !valid)
                                .on_click(cx.listener(|this, _, _, cx| this.add_to_queue(cx))),
                        )
                        .child(
                            Button::new("run", "Run")
                                .end_slot(mira_ui::primitives::Kbd::hint("⏎"))
                                .variant(Variant::Primary)
                                .disabled(!has_tab || !valid || running)
                                .on_click(cx.listener(|this, _, _, cx| this.run_selected(cx))),
                        ),
                ),
                Page::Queue => Some(
                    footer
                        .child(
                            Button::new("queue-clear-finished", "Clear finished")
                                .variant(Variant::Ghost)
                                .disabled(!finished)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.edit_queue(cx, Queue::clear_finished);
                                })),
                        )
                        .child(
                            Button::new("queue-clear", "Clear queue")
                                .variant(Variant::Ghost)
                                .disabled(queue.len() == usize::from(skippable))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.edit_queue(cx, Queue::clear);
                                })),
                        )
                        .child(div().flex_1())
                        .when(playing && skippable, |footer| {
                            footer.child(Button::new("queue-skip", "Skip").on_click(cx.listener(
                                |this, _, _, cx| this.act_on_tab(cx, RuffleView::skip_script),
                            )))
                        })
                        .child(div().flex_none().w(QUEUE_RUN_WIDTH).child(
                            if playing || skippable {
                                Button::new("queue-stop", "Stop queue").on_click(cx.listener(
                                    |this, _, _, cx| this.act_on_tab(cx, RuffleView::stop_script),
                                ))
                            } else {
                                Button::new("queue-start", "Start queue")
                                    .variant(Variant::Primary)
                                    .disabled(!has_tab || !waiting || !startable)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.edit_queue(cx, Queue::play)
                                    }))
                            },
                        )),
                ),
                Page::CoreBots | Page::Log => None,
            };
        let sidebar = self.sidebar(&theme, unfinished, cx);
        v_flex()
            .size_full()
            .window_surface(&theme)
            .key_context(format!("{FOCUS_SCOPE} {CONTEXT}").as_str())
            .id("scripts")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseSession, window, _| window.remove_window()))
            .on_action(cx.listener(|this, _: &AddToQueue, _, cx| {
                if matches!(this.shown_page(cx), Page::Library(_)) {
                    this.add_to_queue(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &crate::app_menu::Find, window, cx| {
                window.focus(&this.search.focus_handle(cx), cx);
            }))
            .child(mira_ui::nav::pointer_probe())
            .child(crate::quit::Overlay)
            .child(header)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(sidebar)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .children(source_header)
                            .child(body)
                            .children(footer),
                    ),
            )
            .child(self.package_dialogs.clone())
    }
}

struct RunStatus {
    text: SharedString,
    activity: Activity,
    stop: Option<SharedString>,
    failure: Option<Failure>,
}

struct Failure {
    title: SharedString,
    script: SharedString,
    message: SharedString,
}

fn run_status(
    status: RunStatus,
    stop: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    theme: &Theme,
) -> Div {
    h_flex()
        .min_w_0()
        .max_w(px(360.0))
        .gap(theme.space(2.0))
        .pr(theme.space(3.0))
        .child(
            StatusLabel::new("script-status", status.activity, status.text)
                .single_line()
                .when_some(status.failure, |label, failure| {
                    label.details(
                        HoverCard::new("script-failure", failure.title)
                            .placement(Placement::new(Side::Bottom, Align::End))
                            .path_row("Script", failure.script)
                            .body(failure.message),
                    )
                }),
        )
        .children(status.stop.map(|name| {
            Button::new("script-stop", format!("Stop {name}"))
                .variant(Variant::Outline)
                .size(Size::Small)
                .flex_none()
                .child("Stop")
                .on_click(stop)
        }))
}

struct Detail {
    heading: AnyElement,
    body: Vec<AnyElement>,
    bar: Option<AnyElement>,
}

fn errored_fields(definition: &Definition, drafts: &[FieldDraft]) -> Vec<usize> {
    definition
        .fields()
        .iter()
        .zip(drafts)
        .enumerate()
        .filter(|(_, (field, draft))| error(field, draft).is_some())
        .map(|(index, _)| index)
        .collect()
}

fn error_bar_clearance(theme: &Theme) -> Pixels {
    theme.space(17.0)
}

fn reveal_item(scroll: &ScrollHandle, index: usize, above: Pixels, below: Pixels) {
    let Some(item) = scroll.bounds_for_item(index) else {
        return;
    };
    let viewport = scroll.bounds();
    let mut offset = scroll.offset();
    let top = item.top() + offset.y;
    let bottom = item.bottom() + offset.y;
    if top < viewport.top() + above {
        offset.y += viewport.top() + above - top;
    } else if bottom > viewport.bottom() - below {
        offset.y -= bottom - (viewport.bottom() - below);
    } else {
        return;
    }
    offset.y = offset.y.clamp(-scroll.max_offset().y, px(0.0));
    scroll.set_offset(offset);
}

fn status_text(id: &'static str, text: impl Into<SharedString>) -> impl IntoElement {
    let text = text.into();
    div()
        .id(id)
        .role(gpui::Role::Status)
        .aria_label(text.clone())
        .child(text)
}

fn leading(saved: bool, drafts: &[FieldDraft], cx: &App) -> (bool, bool) {
    (
        saved && script_inputs::unreadable(cx),
        drafts.iter().any(|draft| draft.stale.is_some()),
    )
}

fn heading(
    theme: &Theme,
    about: &About,
    detail: SharedString,
    action: Option<AnyElement>,
) -> impl IntoElement {
    let layout = theme.layout;
    let typography = theme.typography;
    let description = about.description.clone().map(|description| {
        div()
            .mt(layout.space(1.0))
            .line_clamp(3)
            .text_ellipsis()
            .child(description)
    });
    let tags = (!about.tags.is_empty()).then(|| {
        let count = match about.tags.len() {
            1 => "1 tag".to_owned(),
            count => format!("{count} tags"),
        };
        let listed: Vec<&str> = about.tags.iter().map(AsRef::as_ref).collect();
        h_flex().mt(layout.space(1.0)).child(
            div().flex_none().child(
                HoverCard::new("script-tags", "Tags")
                    .fit()
                    .body(listed.join(", "))
                    .child(div().child(count)),
            ),
        )
    });
    h_flex()
        .items_start()
        .gap(layout.space(2.0))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(about.name.clone()),
                )
                .child(
                    v_flex()
                        .text_size(typography.secondary)
                        .line_height(typography.secondary_line)
                        .text_color(theme.muted_foreground)
                        .child(detail)
                        .children(description)
                        .children(tags),
                ),
        )
        .children(action)
}

fn muted(theme: &Theme, text: impl Into<SharedString>) -> impl IntoElement {
    let typography = theme.typography;
    div()
        .text_size(typography.body)
        .line_height(typography.body_line)
        .text_color(theme.muted_foreground)
        .child(text.into())
}

fn no_inputs(theme: &Theme, unread: Option<&SharedString>) -> AnyElement {
    match unread {
        None => muted(theme, "This script has no inputs.").into_any_element(),
        Some(reason) => Alert::new(
            "options-unread",
            Tone::Warning,
            "Couldn't read this script's options",
        )
        .body(format!("It runs with its defaults. {reason}"))
        .into_any_element(),
    }
}

struct QueueRow {
    name: SharedString,
    folder: SharedString,
    meta: Option<(SharedString, Hsla)>,
}

fn ended(outcome: &Outcome) -> &'static str {
    match outcome {
        Outcome::Completed => "Finished",
        Outcome::Failed { .. } | Outcome::Killed(_) => "Failed",
        Outcome::Stopped => "Stopped",
        Outcome::Exited { .. } => "Exited",
    }
}

fn duration(took: std::time::Duration) -> String {
    let seconds = took.as_secs();
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

fn options_panel(
    this: WeakEntity<ScriptsWindow>,
    options: Option<ScriptOptions>,
    room_input: Entity<Input>,
    skua: bool,
    cx: &App,
) -> AnyElement {
    let layout = cx.theme().layout;
    let editable = options.is_some();
    let rooms_editable = editable && !skua;
    let options = options.unwrap_or_default();
    let private = matches!(options.rooms, Rooms::Private { .. });
    let set_private = {
        let this = this.clone();
        move |on, _: &mut Window, cx: &mut App| {
            let _ = this.update(cx, |this, cx| this.set_private_rooms(on, cx));
        }
    };
    let toggle = |set: fn(&mut ScriptOptions, bool)| {
        let this = this.clone();
        move |on, _: &mut Window, cx: &mut App| {
            let _ = this.update(cx, |this, cx| {
                this.change_options(cx, |options| set(options, on));
            });
        }
    };
    let theme = cx.theme().clone();
    v_flex()
        .gap(layout.space(2.0))
        .when(!editable, |options| {
            options.child(note("Log in to change script settings.", true, &theme))
        })
        .child(
            Switch::new("safe-start", "Safe start and stop", options.safe_start_stop)
                .trailing()
                .description("Best effort attempt to start or stop the script at your house.")
                .disabled(!editable)
                .on_toggle(toggle(|options, on| options.safe_start_stop = on)),
        )
        .child(
            Switch::new(
                "restart",
                "Restart after reconnecting",
                options.restart_after_reconnect,
            )
            .trailing()
            .description("Restart the script when the same account reconnects.")
            .disabled(!editable)
            .on_toggle(toggle(|options, on| options.restart_after_reconnect = on)),
        )
        .child(div().h(px(1.0)).my(layout.space(1.0)).bg(theme.border))
        .child(
            Switch::new("private-rooms", "Private rooms", private)
                .trailing()
                .description("Scripts join private rooms instead of public ones.")
                .disabled(!rooms_editable)
                .on_toggle(set_private),
        )
        .child(
            v_flex()
                .gap(layout.space(1.0))
                .child(
                    h_flex()
                        .justify_between()
                        .gap(layout.space(3.0))
                        .min_h(layout.control)
                        .child(row_label("Room", rooms_editable && private))
                        .child(div().flex_none().w(px(96.0)).child(room_input)),
                )
                .child(note(ROOM_NOTE, rooms_editable && private, &theme)),
        )
        .when(skua, |panel| panel.child(note(SKUA_ROOMS, true, &theme)))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECEIPT: &str = r#"{"repository":1,"owner":"o","repo":"r","path":"","branch":"main","commit":"1111111111111111111111111111111111111111","tree":"2222222222222222222222222222222222222222","files":{}}"#;

    #[test]
    fn the_library_lists_runnable_scripts_grouped_by_source() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-scripts-{:x}", u64::from_ne_bytes(random)));
        let built_in = temp.join("scripts");
        let community = temp.join("community");
        let missing = temp.join("unplugged");
        let authoring = temp.join("authoring");
        let packages = temp.join("packages");
        let skua = temp.join("skua/Scripts");
        assert_eq!(
            scan_all(&built_in, &[], &packages, &skua),
            Library::default()
        );
        for (file, contents) in [
            (
                "scripts/zeta.ts",
                "export const meta = { name: \"Alchemy\", description: \"Brews potions.\" } satisfies Meta;",
            ),
            ("scripts/Alpha.js", ""),
            ("scripts/notes.txt", ""),
            ("scripts/types.d.ts", ""),
            ("scripts/farming/daily.mts", ""),
            ("scripts/farming/hunt.ts", ""),
            ("scripts/node_modules/lib/index.js", ""),
            ("scripts/.git/hook.js", ""),
            ("community/daily.ts", "export const meta = { name: 5 };"),
            ("community/bosses/dage.ts", ""),
            (
                "community/bosses/Gold.cs",
                "public class Gold { public void ScriptMain(IScriptInterface bot) { } }",
            ),
            (
                "community/bosses/Helper.cs",
                "public static class Helper { }",
            ),
            (
                "authoring/package.json",
                r#"{"name":"Work in progress","mira":{}}"#,
            ),
            ("authoring/scripts/draft.ts", ""),
            ("authoring/lib/helper.ts", ""),
            (
                "packages/dailies/package.json",
                r#"{"name":"@lucent/dailies","version":"0.0.3","description":"Daily runs.","mira":{}}"#,
            ),
            ("packages/dailies/scripts/spin.ts", ""),
            ("packages/dailies/lib/wheel.ts", ""),
            ("packages/broken/package.json", "{"),
            ("packages/broken/.mira-install.json", RECEIPT),
            ("packages/deleted/.mira-install.json", RECEIPT),
            ("packages/deleted/scripts/spin.ts", ""),
            ("packages/stripped/package.json", r#"{"name":"left-pad"}"#),
            ("packages/stripped/.mira-install.json", RECEIPT),
            (
                "packages/future/package.json",
                r#"{"name":"Future","mira":{"requires":"999.0.0"}}"#,
            ),
            ("packages/future/scripts/later.ts", ""),
            ("packages/npm/package.json", r#"{"name":"left-pad"}"#),
            ("packages/npm/scripts/pad.ts", ""),
            ("packages/not-a-package/loose.ts", ""),
            ("skua/Scripts/.mira-install.json", RECEIPT),
            (
                "skua/Scripts/scripts.json",
                r#"[{"name":"null","path":"CoreBots.cs"},{"name":"Free Daily Boost","description":"Claims the free daily boost.","tags":["daily","boost","null","","Daily"],"path":"Dailies/FreeDailyBoost.cs"},{"name":"No details","description":"null","tags":["null"],"path":"Other/NoDetails.cs"},{"name":"Gone","path":"Gone.cs"},{"name":"Escape","path":"../outside.cs"}]"#,
            ),
            ("skua/Scripts/CoreBots.cs", ""),
            ("skua/Scripts/Other/NoDetails.cs", ""),
            ("skua/Scripts/Dailies/FreeDailyBoost.cs", ""),
            ("skua/outside.cs", ""),
        ] {
            let path = temp.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let library = scan_all(
            &built_in,
            &[community.clone(), missing.clone(), authoring.clone()],
            &packages,
            &skua,
        );
        let listed: Vec<(Option<usize>, Option<&str>, &str)> = library
            .entries
            .iter()
            .map(|entry| (entry.source, entry.folder.as_deref(), entry.name.as_ref()))
            .collect();
        assert_eq!(
            listed,
            [
                (None, None, "Alchemy"),
                (None, None, "Alpha.js"),
                (None, Some("farming"), "daily.mts"),
                (None, Some("farming"), "hunt.ts"),
                (Some(0), None, "daily.ts"),
                (Some(0), Some("bosses"), "dage.ts"),
                (Some(0), Some("bosses"), "Gold.cs"),
                (Some(2), None, "draft.ts"),
                (Some(4), None, "spin.ts"),
                (Some(8), Some("Dailies"), "Free Daily Boost"),
                (Some(8), Some("Other"), "No details"),
            ]
        );
        assert_eq!(
            library.entries[0].description.as_deref(),
            Some("Brews potions.")
        );
        let skua_details: Vec<(Option<&str>, Vec<&str>)> = library
            .entries
            .iter()
            .filter(|entry| entry.source == Some(8))
            .map(|entry| {
                (
                    entry.description.as_deref(),
                    entry.tags.iter().map(|tag| tag.as_ref()).collect(),
                )
            })
            .collect();
        assert_eq!(
            skua_details,
            [
                (Some("Claims the free daily boost."), vec!["daily", "boost"]),
                (None, vec![]),
            ]
        );
        let sources: Vec<(&Path, &str, Option<&str>, usize)> = library
            .sources
            .iter()
            .map(|source| {
                (
                    source.root.as_path(),
                    source.name.as_ref(),
                    source.problem.as_deref(),
                    source.scripts,
                )
            })
            .collect();
        let invalid = sources[3].2.unwrap();
        assert!(
            invalid.starts_with("Its package.json isn't valid: "),
            "{invalid}"
        );
        assert_eq!(
            sources,
            [
                (community.as_path(), "community", None, 3),
                (missing.as_path(), "unplugged", Some("Can't read"), 0),
                (authoring.as_path(), "Work in progress", None, 1),
                (
                    packages.join("broken").as_path(),
                    "broken",
                    Some(invalid),
                    0
                ),
                (
                    packages.join("dailies").as_path(),
                    "@lucent/dailies",
                    None,
                    1
                ),
                (
                    packages.join("deleted").as_path(),
                    "deleted",
                    Some("Its package.json is missing."),
                    0
                ),
                (
                    packages.join("future").as_path(),
                    "future",
                    Some("Needs Mira 999.0.0 or newer."),
                    0
                ),
                (
                    packages.join("stripped").as_path(),
                    "stripped",
                    Some("This isn't a Mira script package."),
                    0
                ),
                (skua.as_path(), "Skua", None, 2),
            ]
        );
        for index in [3, 5, 7] {
            assert!(matches!(
                &library.sources[index].kind,
                SourceKind::Package { origin: Some(origin), .. }
                    if origin.url == "https://github.com/o/r"
            ));
        }
        assert_eq!(library.sources[2].kind, SourceKind::Folder);
        assert_eq!(
            library.sources[4].kind,
            SourceKind::Package {
                version: Some("0.0.3".into()),
                description: Some("Daily runs.".into()),
                origin: None,
            }
        );
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn search_and_the_chosen_source_narrow_the_list() {
        let entry = |name: &str, folder: Option<&str>, source: Option<usize>| Entry {
            source,
            ..Entry::new(
                PathBuf::from(name),
                name.to_owned().into(),
                folder.map(|folder| folder.to_owned().into()),
                None,
                Arc::default(),
            )
        };
        let source = |name: &str| Source {
            root: PathBuf::from(name),
            name: name.to_owned().into(),
            kind: SourceKind::Folder,
            problem: None,
            scripts: 0,
        };
        let library = Library {
            entries: vec![
                entry("daily.ts", None, None),
                Entry::new(
                    PathBuf::from("hunt.ts"),
                    "Frogzard hunt".into(),
                    None,
                    None,
                    Arc::default(),
                ),
                entry("daily.ts", Some("weekly"), Some(0)),
                Entry {
                    source: Some(0),
                    ..Entry::new(
                        PathBuf::from("dage.ts"),
                        "dage.ts".into(),
                        None,
                        Some("Fights Dage the Evil.".into()),
                        Arc::from(["Legion".into(), "boss".into()]),
                    )
                },
                entry("spin.ts", None, Some(1)),
            ],
            sources: vec![source("community"), source("dailies"), source("unplugged")],
        };
        let names_in = |query: &str, scope: Scope| -> Vec<String> {
            library_rows(&library, query, scope)
                .iter()
                .map(|row| match row {
                    Row::Source(index) => format!("[{}]", library.sources[*index].name),
                    Row::Script(entry) => entry.name.to_string(),
                })
                .collect()
        };
        let names = |query: &str| names_in(query, Scope::All);
        assert_eq!(
            names(""),
            [
                "daily.ts",
                "Frogzard hunt",
                "[community]",
                "daily.ts",
                "dage.ts",
                "[dailies]",
                "spin.ts"
            ]
        );
        assert_eq!(names("daily"), ["daily.ts", "[community]", "daily.ts"]);
        assert_eq!(names("dailies"), ["[dailies]", "spin.ts"]);
        assert_eq!(names("weekly"), ["[community]", "daily.ts"]);
        assert_eq!(names("frogzard"), ["Frogzard hunt"]);
        assert_eq!(names("hunt.ts"), ["Frogzard hunt"]);
        assert_eq!(names("the evil"), ["[community]", "dage.ts"]);
        assert_eq!(names("legion"), ["[community]", "dage.ts"]);
        assert!(names("zzz").is_empty());
        assert_eq!(names_in("", Scope::BuiltIn), ["daily.ts", "Frogzard hunt"]);
        assert_eq!(names_in("", Scope::Source(0)), ["daily.ts", "dage.ts"]);
        assert_eq!(names_in("", Scope::Source(1)), ["spin.ts"]);
    }

    type Interaction = Box<dyn Fn(&mut gpui::HeadlessAppContext, gpui::AnyWindowHandle)>;

    fn update_scripts(
        cx: &mut gpui::HeadlessAppContext,
        window: gpui::AnyWindowHandle,
        f: impl Fn(&mut ScriptsWindow, &mut Context<ScriptsWindow>),
    ) {
        cx.update_window(window, |root, _, cx| {
            let scripts = root
                .downcast::<ScriptsWindow>()
                .expect("the window shows Scripts");
            scripts.update(cx, f);
        })
        .expect("update Scripts");
        cx.run_until_parked();
    }

    fn pointer(
        position: gpui::Point<Pixels>,
        button: Option<gpui::MouseButton>,
    ) -> impl Fn(&mut gpui::HeadlessAppContext, gpui::AnyWindowHandle) {
        move |cx, window| {
            let mut events: Vec<gpui::PlatformInput> = [px(0.0), px(1.0)]
                .map(|nudge| {
                    gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                        position: position + gpui::point(nudge, px(0.0)),
                        pressed_button: None,
                        modifiers: Default::default(),
                    })
                })
                .into();
            if let Some(button) = button {
                events.push(gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    position,
                    button,
                    click_count: 1,
                    first_mouse: false,
                    modifiers: Default::default(),
                }));
                events.push(gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                    position,
                    button,
                    click_count: 1,
                    modifiers: Default::default(),
                }));
            }
            for event in events {
                cx.update_window(window, |_, window, cx| {
                    window.dispatch_event(event, cx);
                    window.refresh();
                })
                .expect("send the pointer event");
                cx.run_until_parked();
            }
            cx.advance_clock(Duration::from_secs(1));
        }
    }

    #[test]
    fn snapshot_sources() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-sources-{:x}", u64::from_ne_bytes(random)));
        let commit = "4e1c2f0".to_owned() + &"0".repeat(33);
        let receipt = format!(
            r#"{{"repository":1,"owner":"toommyliu","repo":"mira","path":"script-packages/dailies","branch":"main","commit":"{commit}","tree":"{commit}","files":{{}}}}"#
        );
        let skua_receipt = format!(
            r#"{{"repository":2,"owner":"auqw","repo":"Scripts","path":"","branch":"Skua","commit":"{commit}","tree":"{commit}","files":{{}}}}"#
        );
        for (file, contents) in [
            ("scripts/hunt.ts", ""),
            ("scripts/bosses/dage.ts", ""),
            ("community/daily.ts", ""),
            ("backup/scripts/old-hunt.ts", ""),
            (
                "packages/toommyliu-mira-dailies/package.json",
                r#"{"name":"Daily quests","mira":{}}"#,
            ),
            ("packages/toommyliu-mira-dailies/scripts/spin.ts", ""),
            (
                "packages/toommyliu-mira-dailies/.mira-install.json",
                receipt.as_str(),
            ),
            (
                "packages/farming/package.json",
                r#"{"name":"Farming","version":"1.2.0","description":"Gold, XP and class point farms.","mira":{}}"#,
            ),
            ("packages/farming/scripts/hunt.ts", ""),
            ("packages/farming/scripts/merge/yulgar.ts", ""),
            (
                "packages/future/package.json",
                r#"{"name":"Future","mira":{"requires":"9.0.0"}}"#,
            ),
            ("skua/Scripts/.mira-install.json", skua_receipt.as_str()),
            (
                "skua/Scripts/scripts.json",
                r#"[{"name":"Free Daily Boost","description":"Claims the daily boost reward you pick.","tags":["daily","boost","free"],"path":"Dailies/FreeDailyBoost.cs"}]"#,
            ),
            ("skua/Scripts/Dailies/FreeDailyBoost.cs", ""),
        ] {
            let path = temp.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let size = gpui::size(px(WIDTH), px(HEIGHT));
        let build = |window: &mut Window, cx: &mut App| {
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::settings::add_script_folder(temp.join("community"), cx);
            crate::settings::add_script_folder(temp.join("unplugged"), cx);
            crate::settings::add_script_folder(temp.join("backup/scripts"), cx);
            crate::packages::init(
                temp.join("packages"),
                temp.join("skua"),
                temp.join("github-token"),
                cx,
            );
            crate::packages::packages(cx).update(cx, |packages, _| {
                packages.mark(
                    temp.join("packages/toommyliu-mira-dailies"),
                    Status::Available(
                        crate::packages::Sha::parse(&("9a8b7c6".to_owned() + &"0".repeat(33)))
                            .unwrap(),
                    ),
                )
            });
            crate::script_inputs::init(temp.join("inputs.json"), cx);
            crate::shortcuts::apply(cx);
            cx.new(|cx| {
                let client = Client::empty_for_tests(&temp, cx);
                let mut scripts = ScriptsWindow::new(
                    client,
                    temp.join("scripts"),
                    temp.join("types"),
                    Target::Selected,
                    window,
                    cx,
                );
                scripts.shown = Some(ShownTab::fake(
                    "Alt 1",
                    crate::automation::settings::Automation::default(),
                ));
                scripts
            })
        };
        let select = |root: PathBuf| -> Interaction {
            Box::new(move |cx, window| {
                let page = Page::Library(Some(root.clone()));
                update_scripts(cx, window, move |scripts, cx| {
                    scripts.set_page(page.clone(), cx)
                });
            })
        };
        let shots: [(&str, Interaction); 11] = [
            (
                "scripts-sources-collapsed",
                Box::new(|cx, window| {
                    update_scripts(cx, window, |scripts, cx| {
                        scripts.folders_open = false;
                        cx.notify();
                    })
                }),
            ),
            (
                "scripts-sources-packages-focused",
                Box::new({
                    let middle = 218;
                    let collapse = pointer(
                        gpui::point(px(40.0), px(middle as f32)),
                        Some(gpui::MouseButton::Left),
                    );
                    move |cx, window| {
                        update_scripts(cx, window, |scripts, cx| {
                            scripts.folders_open = false;
                            cx.notify();
                        });
                        collapse(cx, window);
                        update_scripts(cx, window, |scripts, _| {
                            assert!(!scripts.packages_open, "the click collapses Packages")
                        });
                        for key in ["shift-tab", "tab"] {
                            cx.update_window(window, |_, window, cx| {
                                window.dispatch_keystroke(gpui::Keystroke::parse(key).unwrap(), cx);
                            })
                            .unwrap();
                            cx.run_until_parked();
                        }
                        let image = cx.capture_screenshot(window).unwrap();
                        let scale = image.width() / WIDTH as u32;
                        let middle = middle * scale;
                        let row: Vec<_> = (0..image.width())
                            .map(|x| *image.get_pixel(x, middle))
                            .collect();
                        let background = row[0];
                        let edge = row
                            .iter()
                            .position(|pixel| *pixel != background)
                            .expect("the focus ring around Packages");
                        let ring = row[edge + scale as usize];
                        let inside = edge
                            + row[edge..]
                                .iter()
                                .position(|pixel| *pixel == background)
                                .unwrap();
                        let gap = row[inside..]
                            .iter()
                            .position(|pixel| *pixel != background)
                            .unwrap();
                        assert!(
                            gap >= 3 * scale as usize,
                            "the focus ring touches the Packages label"
                        );
                        let column = edge as u32 + 30 * scale;
                        let stroke = |rows: std::ops::Range<u32>| {
                            rows.filter(|&y| *image.get_pixel(column, y) == ring)
                                .count()
                        };
                        let above = stroke(middle - 16 * scale..middle);
                        let below = stroke(middle..middle + 16 * scale);
                        assert!(above > 0, "the focus ring has no top edge");
                        assert_eq!(above, below, "the sidebar clips the focus ring");
                    }
                }),
            ),
            ("scripts-sources", Box::new(|_, _| {})),
            (
                "scripts-source-actions",
                Box::new({
                    let pick = select(temp.join("packages/toommyliu-mira-dailies"));
                    let open = pointer(
                        gpui::point(px(WIDTH - 24.0), px(62.0)),
                        Some(gpui::MouseButton::Left),
                    );
                    move |cx, window| {
                        pick(cx, window);
                        open(cx, window);
                    }
                }),
            ),
            (
                "scripts-source-problem",
                select(temp.join("packages/future")),
            ),
            (
                "scripts-source-package",
                select(temp.join("packages/farming")),
            ),
            (
                "scripts-source-update",
                select(temp.join("packages/toommyliu-mira-dailies")),
            ),
            ("scripts-source-folder", select(temp.join("community"))),
            ("scripts-source-default", select(temp.join("scripts"))),
            ("scripts-source-skua", select(temp.join("skua/Scripts"))),
            (
                "scripts-updates-card",
                Box::new(pointer(gpui::point(px(159.0), px(341.0)), None)),
            ),
        ];
        for (name, interact) in shots {
            for written in mira_ui::snapshot_with(name, size, build, interact) {
                assert!(written.exists(), "{} was not written", written.display());
            }
        }
        let smallest = gpui::size(px(MIN_WIDTH), px(MIN_HEIGHT));
        let shots: [(&str, Interaction); 2] = [
            ("scripts-sources-min", Box::new(|_, _| {})),
            (
                "scripts-source-update-min",
                select(temp.join("packages/toommyliu-mira-dailies")),
            ),
        ];
        for (name, interact) in shots {
            for written in mira_ui::snapshot_with(name, smallest, build, interact) {
                assert!(written.exists(), "{} was not written", written.display());
            }
        }
        fs::remove_dir_all(temp).unwrap();
    }

    struct SkuaInputs(Entity<ScriptsWindow>, FormKey);

    impl Render for SkuaInputs {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            let key = self.1.clone();
            let inputs = self
                .0
                .update(cx, |scripts, cx| scripts.inputs(&theme, &key, cx));
            div()
                .size_full()
                .window_surface(&theme)
                .p(theme.space(4.0))
                .child(
                    v_flex()
                        .w(theme.layout.inspector)
                        .gap(theme.space(2.0))
                        .children(inputs),
                )
        }
    }

    #[test]
    fn snapshot_skua_inputs() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-skua-inputs-{:x}", u64::from_ne_bytes(random)));
        let field = |key: &str, group: Option<&str>, label: &str, kind: Kind| scripting::Field {
            key: key.into(),
            group: group.map(Into::into),
            label: label.into(),
            description: None,
            kind,
        };
        let definition = Definition::new(vec![
            field(
                "BoostReward",
                None,
                "Choose Your Daily Boost Reward",
                Kind::Select {
                    options: vec!["LowestQuantOwned".into(), "EXP".into(), "Gold".into()],
                    default: Some("LowestQuantOwned".into()),
                },
            ),
            field(
                "Generic:mode",
                Some("Generic"),
                "Mode",
                Kind::Select {
                    options: vec!["Farm".into(), "Buy".into()],
                    default: Some("Farm".into()),
                },
            ),
            field(
                "Select:Sword",
                Some("Select"),
                "Abyssal Sword",
                Kind::Boolean { default: true },
            ),
            field(
                "Select:Helm",
                Some("Select"),
                "Abyssal Helm",
                Kind::Boolean { default: false },
            ),
        ]);
        let size = gpui::size(px(360.0), px(320.0));
        let unread: SharedString = "Object reference not set to an instance of an object. (at CoreBots.CheckInventory(String item, Int32 quant, Boolean toInv) in CoreBots.cs:line 938)".into();
        let scenes = [
            ("scripts-skua-inputs", Some(definition), None),
            ("scripts-skua-unread", None, Some(unread)),
        ];
        for (name, definition, unread) in scenes {
            let temp = temp.clone();
            for written in mira_ui::snapshot(name, size, move |window, cx| {
                let temp = temp.clone();
                crate::settings::init(temp.join("settings.json"), |_| {}, cx);
                crate::script_inputs::init(temp.join("script-inputs.json"), cx);
                crate::packages::init(
                    temp.join("packages"),
                    temp.join("skua"),
                    temp.join("github-token"),
                    cx,
                );
                let key = FormKey::Script(temp.join("skua/Scripts/Dailies/FreeDailyBoost.cs"));
                let scripts = cx.new(|cx| {
                    let client = Client::empty_for_tests(&temp, cx);
                    ScriptsWindow::new(
                        client,
                        temp.join("scripts"),
                        temp.join("types"),
                        Target::Selected,
                        window,
                        cx,
                    )
                });
                let shown = key.clone();
                let definition = definition.clone();
                let unread = unread.clone();
                scripts.update(cx, |scripts, cx| {
                    scripts.loaded(
                        shown,
                        0,
                        Ok(Declared {
                            meta: Meta::default(),
                            inputs: Ok(definition),
                        }),
                        unread,
                        cx,
                    )
                });
                cx.new(|_| SkuaInputs(scripts, key))
            }) {
                assert!(written.exists(), "{} was not written", written.display());
            }
        }
    }

    struct OptionsPanels {
        scripts: Entity<ScriptsWindow>,
        rooms: [Entity<Input>; 4],
    }

    impl Render for OptionsPanels {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let layout = cx.theme().layout;
            let this = self.scripts.downgrade();
            let panel =
                |options: Option<ScriptOptions>, room: &Entity<Input>, skua: bool, cx: &App| {
                    div()
                        .w(px(376.0))
                        .p(layout.space(3.0))
                        .rounded(px(8.0))
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .child(options_panel(this.clone(), options, room.clone(), skua, cx))
                };
            let pinned = ScriptOptions {
                rooms: Rooms::Private {
                    room: PrivateRoom::parse("4242"),
                },
                ..ScriptOptions::default()
            };
            let public = ScriptOptions {
                rooms: Rooms::Public,
                ..ScriptOptions::default()
            };
            h_flex()
                .size_full()
                .items_start()
                .gap(layout.space(4.0))
                .p(layout.space(4.0))
                .window_surface(cx.theme())
                .child(panel(Some(pinned), &self.rooms[0], false, cx))
                .child(panel(Some(public), &self.rooms[1], false, cx))
                .child(panel(None, &self.rooms[2], false, cx))
                .child(panel(Some(pinned), &self.rooms[3], true, cx))
        }
    }

    #[test]
    fn snapshot_options() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-options-{:x}", u64::from_ne_bytes(random)));
        let size = gpui::size(px(1616.0), px(420.0));
        for written in mira_ui::snapshot("scripts-options", size, move |window, cx| {
            let temp = temp.clone();
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::packages::init(
                temp.join("packages"),
                temp.join("skua"),
                temp.join("github-token"),
                cx,
            );
            let scripts = cx.new(|cx| {
                let client = Client::empty_for_tests(&temp, cx);
                ScriptsWindow::new(
                    client,
                    temp.join("scripts"),
                    temp.join("types"),
                    Target::Selected,
                    window,
                    cx,
                )
            });
            let rooms = [("4242", false), ("", true), ("", true), ("4242", true)].map(
                |(text, disabled)| {
                    cx.new(|cx| {
                        let mut room = Input::new("Room", cx).placeholder("Random");
                        room.set_text(text, cx);
                        room.set_disabled(disabled, cx);
                        room
                    })
                },
            );
            cx.new(|_| OptionsPanels { scripts, rooms })
        }) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }

    fn snapshot_window(
        name: &str,
        size: (f32, f32),
        tab: &'static str,
        files: &[(&str, &str)],
        setup: fn(&mut ScriptsWindow),
    ) {
        snapshot_window_with(name, size, tab, files, setup, |_, _| {});
    }

    fn snapshot_window_with(
        name: &str,
        size: (f32, f32),
        tab: &'static str,
        files: &[(&str, &str)],
        setup: fn(&mut ScriptsWindow),
        interact: impl Fn(&mut gpui::HeadlessAppContext, gpui::AnyWindowHandle),
    ) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-scripts-{:x}", u64::from_ne_bytes(random)));
        for (file, contents) in files {
            let path = temp.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let size = gpui::size(px(size.0), px(size.1));
        let build = |window: &mut Window, cx: &mut App| {
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::packages::init(
                temp.join("packages"),
                temp.join("skua"),
                temp.join("github-token"),
                cx,
            );
            crate::script_inputs::init(temp.join("inputs.json"), cx);
            cx.new(|cx| {
                let client = Client::empty_for_tests(&temp, cx);
                let mut scripts = ScriptsWindow::new(
                    client,
                    temp.join("scripts"),
                    temp.join("types"),
                    Target::Selected,
                    window,
                    cx,
                );
                scripts.shown = Some(ShownTab::fake(
                    tab,
                    crate::automation::settings::Automation::default(),
                ));
                setup(&mut scripts);
                scripts
            })
        };
        for written in mira_ui::snapshot_with(name, size, build, interact) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }

    const STRESS: &[(&str, &str)] = &[
        (
            "scripts/legion/daily-quests-and-weekly-bosses/ultra-bosses-with-a-very-long-folder-name/legion-revenant-fealty-and-exalted-crown-farm-with-fallbacks.ts",
            r#"export const meta = { name: "Legion Revenant fealty and Exalted Crown farm with every fallback room", description: "Farms all three Legion Fealty quests in order, banks the rewards between runs, swaps to the fallback room when the first one is full, and stops when the Exalted Crown drops or the bank has no free slot left. Needs Legion Revenant unlocked and at least 50 free inventory slots before it starts." };
export const inputs = {
  target: { type: "string", label: "The monster or the comma separated list of monsters this script should hunt first", description: "Use the exact in-game names, separated by commas, in the order the script should try them. Names with apostrophes are fine.", required: true },
  kills: { type: "number", label: "How many kills before banking the drops", description: "A whole number from 1 to 100000.", integer: true, min: 1, max: 100000, default: 100000 },
  loot: { type: "boolean", label: "Pick up every drop including the ones nobody wants", description: "Turn this off if the inventory fills up before the run ends, which it will." },
  mode: { type: "select", label: "Mode", options: ["Farm the quest items only and skip everything else", "Quest", "Merge"], default: "Farm the quest items only and skip everything else" },
  buy: { type: "select", label: "Buy", options: ["Exalted Crown of the Undead Legion Champion", "Helm of the Eternal Void Highlord", "Cape", "Sword", "Shield", "Pet"], multiple: true, default: ["Exalted Crown of the Undead Legion Champion", "Helm of the Eternal Void Highlord", "Cape"] },
} as const;
export default function* () {}"#,
        ),
        (
            "scripts/Wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww.ts",
            "",
        ),
        ("scripts/a.ts", ""),
        (
            "packages/long/package.json",
            r#"{"name":"The Complete Legion Revenant And Void Highlord Farming Collection","version":"12.345.6789-beta.12","description":"Every farm the community has written for Legion Revenant, Void Highlord, Archmage and the seasonal events, kept up to date weekly by volunteers.","mira":{}}"#,
        ),
        ("packages/long/scripts/daily.ts", ""),
    ];

    #[test]
    fn snapshot_stress() {
        let select: fn(&mut ScriptsWindow) = |_| {};
        for (name, size) in [
            ("stress-scripts", (WIDTH, HEIGHT)),
            ("stress-scripts-min", (MIN_WIDTH, MIN_HEIGHT)),
        ] {
            snapshot_window(
                name,
                size,
                "Sepulchure farm alt with a long name",
                STRESS,
                select,
            );
        }
        for (name, size) in [
            ("stress-scripts-long", (WIDTH, 900.0)),
            ("stress-scripts-long-min", (MIN_WIDTH, MIN_HEIGHT)),
        ] {
            snapshot_window(name, size, "Alt 1", STRESS, |scripts| scripts.selected = 2);
        }
        snapshot_window_with(
            "stress-scripts-package-min",
            (MIN_WIDTH, MIN_HEIGHT),
            "Alt 1",
            STRESS,
            |_| {},
            |cx, window| {
                update_scripts(cx, window, |scripts, cx| {
                    let root = scripts.library.as_ref().and_then(|library| {
                        library
                            .sources
                            .iter()
                            .find(|source| source.root.ends_with("long"))
                            .map(|source| source.root.clone())
                    });
                    if let Some(root) = root {
                        scripts.set_page(Page::Library(Some(root)), cx);
                    }
                });
            },
        );
    }

    const NESTED: &[(&str, &str)] = &[
        ("scripts/hunt.ts", ""),
        ("scripts/bosses/dage.ts", ""),
        ("scripts/legion/daily-quests/ultras/temple.ts", ""),
        (
            "scripts/legion/daily-quests/ultras/void-highlord-farm.ts",
            r#"export const meta = { name: "Void Highlord farm" };"#,
        ),
        (
            "packages/community/package.json",
            r#"{"name":"Community","version":"1.4.0","mira":{}}"#,
        ),
        ("packages/community/scripts/daily.ts", ""),
        ("packages/community/scripts/bosses/nulgath.ts", ""),
    ];

    const INPUTS: &[(&str, &str)] = &[(
        "scripts/inputs.ts",
        include_str!("../../examples/inputs.ts"),
    )];

    #[test]
    fn snapshot_library() {
        for (name, size, tab) in [
            ("scripts-nested", (WIDTH, HEIGHT), "Alt 1"),
            ("scripts-nested-min", (MIN_WIDTH, MIN_HEIGHT), "Alt 1"),
            (
                "scripts-long-tab-min",
                (MIN_WIDTH, MIN_HEIGHT),
                "Sepulchure farm alt with a long name",
            ),
        ] {
            snapshot_window(name, size, tab, NESTED, |_| {});
        }
        snapshot_window_with(
            "scripts-search-min",
            (MIN_WIDTH, MIN_HEIGHT),
            "Alt 1",
            NESTED,
            |scripts| scripts.page = Page::Queue,
            |cx, window| {
                update_scripts(cx, window, |scripts, cx| {
                    scripts
                        .search
                        .update(cx, |search, cx| search.set_text("daily", cx))
                })
            },
        );
        snapshot_window(
            "scripts-filtered-min",
            (MIN_WIDTH, MIN_HEIGHT),
            "Alt 1",
            NESTED,
            |scripts| scripts.page = Page::Library(Some(scripts.packages.join("community"))),
        );
    }

    #[test]
    fn snapshot_toasts_in_the_pane() {
        use mira_ui::primitives::Toast;

        for (name, toast) in [
            (
                "scripts-toast-installing-min",
                Toast::new("Installing package…").loading(),
            ),
            (
                "scripts-toast-installed-min",
                Toast::new("Installed Dailies").tone(Tone::Success),
            ),
            (
                "scripts-toast-failed-min",
                Toast::new("Couldn't install package")
                    .body(
                        "Couldn't find o/missing. If it's private, add a GitHub token in Settings.",
                    )
                    .tone(Tone::Danger),
            ),
        ] {
            snapshot_window_with(
                name,
                (MIN_WIDTH, MIN_HEIGHT),
                "Alt 1",
                NESTED,
                |_| {},
                move |cx, window| {
                    update_scripts(cx, window, |scripts, cx| {
                        Toaster::push(&scripts.toasts, toast.clone(), cx);
                    })
                },
            );
        }
    }

    #[test]
    fn snapshot_inputs_in_the_inspector() {
        for (name, size) in [
            ("scripts-inputs", (WIDTH, 760.0)),
            ("scripts-inputs-min", (MIN_WIDTH, MIN_HEIGHT)),
        ] {
            snapshot_window(name, size, "Alt 1", INPUTS, |_| {});
        }
        snapshot_window(
            "scripts-inputs-ready-min",
            (MIN_WIDTH, MIN_HEIGHT),
            "Alt 1",
            &[("scripts/hunt.ts", include_str!("../../examples/hunt.ts"))],
            |_| {},
        );
    }

    #[test]
    fn snapshot_inputs_scroll_under_a_pinned_heading() {
        snapshot_window_with(
            "scripts-inputs-scrolled-min",
            (MIN_WIDTH, MIN_HEIGHT),
            "Alt 1",
            INPUTS,
            |_| {},
            |cx, window| {
                update_scripts(cx, window, |scripts, cx| {
                    scripts
                        .detail_scroll
                        .set_offset(gpui::point(px(0.0), px(-120.0)));
                    cx.notify();
                });
            },
        );
    }

    const SKUA: &[(&str, &str)] = &[
        ("skua/Scripts/.mira-install.json", RECEIPT),
        (
            "skua/Scripts/scripts.json",
            r#"[{"name":"Free Daily Boost","description":"Claims the daily boost reward you pick.","tags":["daily"],"path":"Dailies/FreeDailyBoost.cs"}]"#,
        ),
        ("skua/Scripts/CoreBots.cs", ""),
        ("skua/Scripts/Dailies/FreeDailyBoost.cs", ""),
    ];

    #[test]
    fn snapshot_core_bots() {
        use super::super::core_bots::Wardrobe;
        use crate::core_bots::Gear;
        for (name, size) in [
            ("scripts-core-bots", (WIDTH, HEIGHT)),
            ("scripts-core-bots-min", (MIN_WIDTH, MIN_HEIGHT)),
            ("scripts-core-bots-tall", (WIDTH, 2400.0)),
        ] {
            snapshot_window_with(
                name,
                size,
                "Alt 1",
                SKUA,
                |scripts| scripts.page = Page::CoreBots,
                |cx, window| {
                    cx.update_window(window, |root, _, cx| {
                        let scripts = root.downcast::<ScriptsWindow>().unwrap();
                        let page = scripts.read(cx).core_bots.clone();
                        page.update(cx, |page, cx| {
                            page.fake(
                                "hero",
                                "PrivateRooms: True\nPrivateRoomNr: 4242\nSoloClassSelect: Legion Revenant\nSoloModeSelect: Solo\nFarmClassSelect: Lich\nFarmEquipCheck: False\nHelm1Select: Hollowborn Reaper's Hood\nWeapon1Select: Necrotic Sword of Doom\nAntiLag: False\n",
                                Some(Wardrobe::fake(
                                    &["ArchPaladin", "Legion Revenant", "Lich"],
                                    &[
                                        (Gear::Helm, &["Hollowborn Reaper's Hood"]),
                                        (Gear::Weapon, &["Necrotic Sword of Doom"]),
                                    ],
                                )),
                                [
                                    ("Legion Revenant".to_owned(), vec!["Base", "Solo", "Farm"]),
                                    ("Lich".to_owned(), vec!["Base"]),
                                ]
                                .into_iter()
                                .collect(),
                                cx,
                            )
                        });
                    })
                    .unwrap();
                    cx.run_until_parked();
                },
            );
        }
    }

    #[test]
    fn snapshot_options_open_from_the_footer() {
        snapshot_window_with(
            "scripts-options-open-min",
            (MIN_WIDTH, MIN_HEIGHT),
            "Alt 1",
            NESTED,
            |_| {},
            |cx, window| {
                let position = gpui::point(px(264.0), px(MIN_HEIGHT - 20.0));
                for event in [
                    gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                        position,
                        button: gpui::MouseButton::Left,
                        click_count: 1,
                        first_mouse: false,
                        modifiers: Default::default(),
                    }),
                    gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                        position,
                        button: gpui::MouseButton::Left,
                        click_count: 1,
                        modifiers: Default::default(),
                    }),
                ] {
                    cx.update_window(window, |_, window, cx| {
                        window.dispatch_event(event, cx);
                    })
                    .expect("click Options");
                    cx.run_until_parked();
                }
            },
        );
    }

    #[test]
    fn snapshot_empty_pages() {
        let pages: [(&str, &[(&str, &str)], fn(&mut ScriptsWindow)); 3] = [
            ("scripts-empty-min", &[], |_| {}),
            ("scripts-queue-min", NESTED, |scripts| {
                scripts.page = Page::Queue
            }),
            ("scripts-log-min", NESTED, |scripts| {
                scripts.page = Page::Log
            }),
        ];
        for (name, files, setup) in pages {
            snapshot_window(name, (MIN_WIDTH, MIN_HEIGHT), "Alt 1", files, setup);
        }
    }

    const LONG_ERROR: &str = "Couldn't find Ultra Avatar Tyndarius in r4 of ultratyndarius after 30 seconds. Check that the cell matches the map and that the monster spawns while you're in the room.
Cells visited: Enter, r1, r2, r3, r4.";

    struct Headers {
        client: Entity<Client>,
        shown: ShownTab,
        statuses: Vec<(String, Activity, bool)>,
    }

    impl Render for Headers {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            v_flex()
                .size_full()
                .window_surface(&theme)
                .children(self.statuses.iter().map(|(text, activity, stoppable)| {
                    let failure = text.strip_prefix("hunt failed: ").map(|message| Failure {
                        title: "hunt failed".into(),
                        script: "~/Documents/Mira/scripts/hunt.ts".into(),
                        message: message.to_owned().into(),
                    });
                    shown_tab::header(
                        "Scripts",
                        &self.client,
                        Some(&self.shown),
                        Target::Selected,
                        |_, _, _| {},
                        window,
                        cx,
                    )
                    .child(run_status(
                        RunStatus {
                            text: text.clone().into(),
                            activity: *activity,
                            stop: stoppable.then_some("hunt".into()),
                            failure,
                        },
                        |_, _, _| {},
                        &theme,
                    ))
                }))
        }
    }

    fn snapshot_headers(
        name: &str,
        height: f32,
        statuses: Vec<(String, Activity, bool)>,
        interact: impl Fn(&mut gpui::HeadlessAppContext, gpui::AnyWindowHandle),
    ) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-headers-{:x}", u64::from_ne_bytes(random)));
        let size = gpui::size(px(MIN_WIDTH), px(height));
        let build = |_: &mut Window, cx: &mut App| {
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            cx.new(|cx| Headers {
                client: Client::empty_for_tests(&temp, cx),
                shown: ShownTab::fake("Alt 1", crate::automation::settings::Automation::default()),
                statuses: statuses.clone(),
            })
        };
        for written in mira_ui::snapshot_with(name, size, build, interact) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }

    #[test]
    fn snapshot_run_status_in_the_header() {
        let statuses = [
            ("Running hunt".to_owned(), Activity::Running, true),
            (
                "hunt is waiting for you".to_owned(),
                Activity::NeedsYou,
                true,
            ),
            ("Stopping hunt".to_owned(), Activity::Waiting, false),
            ("hunt stopped".to_owned(), Activity::Stopped, false),
            (
                "hunt failed: Bank is full".to_owned(),
                Activity::Failed,
                false,
            ),
            (
                format!("hunt failed: {LONG_ERROR}"),
                Activity::Failed,
                false,
            ),
            (
                "Running void-highlord-farm-with-a-very-long-script-name".to_owned(),
                Activity::Running,
                true,
            ),
        ];
        snapshot_headers(
            "scripts-run-status",
            statuses.len() as f32 * 38.0,
            statuses.into(),
            |_, _| {},
        );
        snapshot_headers(
            "scripts-run-status-failure",
            320.0,
            vec![(
                format!("hunt failed: {LONG_ERROR}"),
                Activity::Failed,
                false,
            )],
            pointer(gpui::point(px(MIN_WIDTH - 200.0), px(19.0)), None),
        );
    }

    struct QueuePage {
        scripts: Entity<ScriptsWindow>,
        entries: Vec<queue::Entry>,
    }

    impl Render for QueuePage {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            let width = window.viewport_size().width - theme.layout.inspector;
            let entries = self.entries.clone();
            let (list, detail) = self.scripts.update(cx, |scripts, cx| {
                for entry in &entries {
                    scripts.load_form(FormKey::Entry(entry.id), &entry.script, Load::Shown, cx);
                }
                (
                    scripts.queue_list(&theme, &entries, width, window, cx),
                    scripts.queued_detail(&theme, &entries, cx),
                )
            });
            h_flex()
                .size_full()
                .items_stretch()
                .window_surface(&theme)
                .child(v_flex().flex_1().min_w_0().child(list))
                .child(
                    theme
                        .inspector_pane()
                        .p(theme.space(4.0))
                        .children(detail.map(|Detail { heading, body, .. }| {
                            v_flex()
                                .gap(theme.space(4.0))
                                .child(heading)
                                .child(v_flex().gap(theme.space(2.0)).children(body))
                        })),
                )
        }
    }

    #[test]
    fn snapshot_queue() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-queue-{:x}", u64::from_ne_bytes(random)));
        let scripts = [
            ("scripts/hunt.ts", include_str!("../../examples/hunt.ts")),
            ("scripts/legion/daily-quests/ultras/temple.ts", ""),
            ("packages/community/scripts/daily.ts", ""),
            (
                "scripts/hunt-copy/hunt.ts",
                include_str!("../../examples/hunt.ts"),
            ),
        ];
        let mut queue = Queue::default();
        for (file, contents) in scripts {
            let path = temp.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            queue.add(path, Values::new());
        }
        let now = Instant::now();
        queue.play();
        queue.start_next(now);
        queue.ended(&Outcome::Completed, now + Duration::from_secs(150));
        queue.start_next(now);
        let entries = queue.entries().to_vec();
        for (name, width, height) in [
            ("scripts-queue", WIDTH - 208.0, HEIGHT - 78.0),
            ("scripts-queue-narrow", MIN_WIDTH - 208.0, MIN_HEIGHT - 78.0),
        ] {
            let size = gpui::size(px(width), px(height));
            for written in mira_ui::snapshot(name, size, |window, cx| {
                crate::settings::init(temp.join("settings.json"), |_| {}, cx);
                crate::packages::init(
                    temp.join("packages"),
                    temp.join("skua"),
                    temp.join("github-token"),
                    cx,
                );
                crate::script_inputs::init(temp.join("inputs.json"), cx);
                let scripts = cx.new(|cx| {
                    let client = Client::empty_for_tests(&temp, cx);
                    let mut scripts = ScriptsWindow::new(
                        client,
                        temp.join("scripts"),
                        temp.join("types"),
                        Target::Selected,
                        window,
                        cx,
                    );
                    scripts.queued = 2;
                    scripts
                });
                let entries = entries.clone();
                cx.new(|_| QueuePage { scripts, entries })
            }) {
                assert!(written.exists(), "{} was not written", written.display());
            }
        }
    }

    #[gpui::test]
    fn a_package_appearing_above_the_selection_keeps_it(cx: &mut gpui::TestAppContext) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-rescan-{:x}", u64::from_ne_bytes(random)));
        let write = |file: &str, contents: &str| {
            let path = temp.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        };
        write("scripts/hunt.ts", "");
        write("packages/zeta/package.json", r#"{"name":"Zeta","mira":{}}"#);
        write("packages/zeta/scripts/spin.ts", "");
        cx.update(mira_ui::init);
        let (scripts, cx) = cx.add_window_view(|window, cx| {
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::packages::init(
                temp.join("packages"),
                temp.join("skua"),
                temp.join("github-token"),
                cx,
            );
            crate::script_inputs::init(temp.join("inputs.json"), cx);
            let client = Client::empty_for_tests(&temp, cx);
            ScriptsWindow::new(
                client,
                temp.join("scripts"),
                temp.join("types"),
                Target::Selected,
                window,
                cx,
            )
        });
        cx.run_until_parked();
        let spin = temp.join("packages/zeta/scripts/spin.ts");
        scripts.update(cx, |scripts, cx| {
            let rows = scripts.rows(cx);
            scripts.selected = rows
                .iter()
                .position(|row| matches!(row, Row::Script(entry) if entry.path == spin))
                .unwrap();
        });

        write(
            "packages/alpha/package.json",
            r#"{"name":"Alpha","mira":{}}"#,
        );
        write("packages/alpha/scripts/daily.ts", "");
        let dialogs = scripts.read_with(cx, |scripts, _| scripts.package_dialogs.clone());
        dialogs.update(cx, |_, cx| cx.emit(PackageEvent::Changed));
        cx.run_until_parked();
        let daily = temp.join("packages/alpha/scripts/daily.ts");
        scripts.read_with(cx, |scripts, cx| {
            assert!(
                scripts
                    .rows(cx)
                    .iter()
                    .any(|row| matches!(row, Row::Script(entry) if entry.path == daily))
            );
            assert_eq!(scripts.target(cx).map(|entry| entry.path), Some(spin));
        });
        fs::remove_dir_all(temp).unwrap();
    }

    #[gpui::test]
    fn returning_to_the_window_picks_up_script_changes(cx: &mut gpui::TestAppContext) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-refocus-{:x}", u64::from_ne_bytes(random)));
        let write = |file: &str, contents: &str| {
            let path = temp.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        };
        write("scripts/hunt.ts", "export const meta = { name: \"Hunt\" };");
        cx.update(mira_ui::init);
        let (scripts, cx) = cx.add_window_view(|window, cx| {
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::packages::init(
                temp.join("packages"),
                temp.join("skua"),
                temp.join("github-token"),
                cx,
            );
            crate::script_inputs::init(temp.join("inputs.json"), cx);
            window.activate_window();
            let client = Client::empty_for_tests(&temp, cx);
            ScriptsWindow::new(
                client,
                temp.join("scripts"),
                temp.join("types"),
                Target::Selected,
                window,
                cx,
            )
        });
        cx.run_until_parked();
        let names = |cx: &mut gpui::VisualTestContext| {
            scripts.read_with(cx, |scripts, cx| {
                scripts
                    .rows(cx)
                    .iter()
                    .filter_map(|row| match row {
                        Row::Script(entry) => Some(entry.name.to_string()),
                        Row::Source(_) => None,
                    })
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(names(cx), ["Hunt"]);

        cx.deactivate_window();
        write(
            "scripts/hunt.ts",
            "export const meta = { name: \"Hunt Ultra\" };",
        );
        write("scripts/daily.ts", "");
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        assert_eq!(names(cx), ["daily.ts", "Hunt Ultra"]);
        fs::remove_dir_all(temp).unwrap();
    }

    #[gpui::test]
    fn installing_a_package_selects_its_first_script(cx: &mut gpui::TestAppContext) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-install-{:x}", u64::from_ne_bytes(random)));
        let write = |file: &str, contents: &str| {
            let path = temp.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        };
        write("scripts/hunt.ts", "");
        cx.update(mira_ui::init);
        let (scripts, cx) = cx.add_window_view(|window, cx| {
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::packages::init(
                temp.join("packages"),
                temp.join("skua"),
                temp.join("github-token"),
                cx,
            );
            crate::script_inputs::init(temp.join("inputs.json"), cx);
            let client = Client::empty_for_tests(&temp, cx);
            ScriptsWindow::new(
                client,
                temp.join("scripts"),
                temp.join("types"),
                Target::Selected,
                window,
                cx,
            )
        });
        cx.run_until_parked();
        let selected = |cx: &mut gpui::VisualTestContext| {
            scripts.read_with(cx, |scripts, cx| scripts.target(cx).map(|entry| entry.path))
        };
        assert_eq!(selected(cx), Some(temp.join("scripts/hunt.ts")));

        write(
            "packages/dailies/package.json",
            r#"{"name":"Dailies","mira":{}}"#,
        );
        write("packages/dailies/scripts/spin.ts", "");
        write("packages/dailies/scripts/wheel.ts", "");
        let dialogs = scripts.read_with(cx, |scripts, _| scripts.package_dialogs.clone());
        dialogs.update(cx, |_, cx| {
            cx.emit(PackageEvent::Installed(temp.join("packages/dailies")))
        });
        cx.run_until_parked();
        assert_eq!(
            selected(cx),
            Some(temp.join("packages/dailies/scripts/spin.ts"))
        );
        fs::remove_dir_all(temp).unwrap();
    }

    #[gpui::test]
    fn the_library_loads_the_scripts_in_view_and_lets_go_of_far_ones(
        cx: &mut gpui::TestAppContext,
    ) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-preload-{:x}", u64::from_ne_bytes(random)));
        let folder = temp.join("scripts");
        fs::create_dir_all(&folder).unwrap();
        for index in 0..150 {
            fs::write(
                folder.join(format!("s{index:03}.ts")),
                "export const inputs = { kills: { type: \"number\", label: \"Kills\" } };\nexport default function* () {}",
            )
            .unwrap();
        }
        cx.update(mira_ui::init);
        let (scripts, cx) = cx.add_window_view(|window, cx| {
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::packages::init(
                temp.join("packages"),
                temp.join("skua"),
                temp.join("github-token"),
                cx,
            );
            crate::script_inputs::init(temp.join("inputs.json"), cx);
            let client = Client::empty_for_tests(&temp, cx);
            ScriptsWindow::new(
                client,
                folder.clone(),
                temp.join("types"),
                Target::Selected,
                window,
                cx,
            )
        });
        cx.simulate_resize(gpui::size(px(MIN_WIDTH), px(MIN_HEIGHT)));
        let settle = |cx: &mut gpui::VisualTestContext| {
            for _ in 0..3 {
                cx.run_until_parked();
                cx.update(|window, cx| window.simulate_next_frame(cx));
            }
            cx.run_until_parked();
        };
        settle(cx);
        let loaded = |name: &str, cx: &mut gpui::VisualTestContext| {
            let key = FormKey::Script(folder.join(name));
            scripts.read_with(cx, |scripts, _| scripts.forms.contains_key(&key))
        };
        assert!(
            loaded("s005.ts", cx),
            "a row in view loads before it's chosen"
        );
        assert!(!loaded("s100.ts", cx), "a row far out of view waits");

        let edited = FormKey::Script(folder.join("s003.ts"));
        scripts.update(cx, |scripts, cx| {
            scripts.set_draft(&edited, 0, Draft::Text("7".into()), cx)
        });
        scripts.update(cx, |scripts, cx| {
            scripts.selected = 120;
            cx.notify();
        });
        settle(cx);
        assert!(loaded("s118.ts", cx), "rows around the new position load");
        assert!(!loaded("s000.ts", cx), "rows left far behind let go");
        assert!(loaded("s003.ts", cx), "a form with unsaved edits stays");
        fs::remove_dir_all(temp).unwrap();
    }

    #[gpui::test]
    fn skua_scripts_in_view_load_one_at_a_time_once_the_list_settles(
        cx: &mut gpui::TestAppContext,
    ) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp = std::env::temp_dir().join(format!(
            "mira-skua-preload-{:x}",
            u64::from_ne_bytes(random)
        ));
        let skua = temp.join("skua/Scripts");
        let listed: Vec<String> = (0..40)
            .map(|index| format!(r#"{{"name":"Script {index:02}","path":"s{index:02}.cs"}}"#))
            .collect();
        fs::create_dir_all(&skua).unwrap();
        fs::write(skua.join(".mira-install.json"), RECEIPT).unwrap();
        fs::write(skua.join("scripts.json"), format!("[{}]", listed.join(","))).unwrap();
        for index in 0..40 {
            fs::write(skua.join(format!("s{index:02}.cs")), "").unwrap();
        }
        cx.update(mira_ui::init);
        let (scripts, cx) = cx.add_window_view(|window, cx| {
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::packages::init(
                temp.join("packages"),
                temp.join("skua"),
                temp.join("github-token"),
                cx,
            );
            crate::script_inputs::init(temp.join("inputs.json"), cx);
            let client = Client::empty_for_tests(&temp, cx);
            ScriptsWindow::new(
                client,
                temp.join("scripts"),
                temp.join("types"),
                Target::Selected,
                window,
                cx,
            )
        });
        cx.simulate_resize(gpui::size(px(MIN_WIDTH), px(MIN_HEIGHT)));
        let frames = |count: usize, cx: &mut gpui::VisualTestContext| {
            for _ in 0..count {
                cx.run_until_parked();
                cx.update(|window, cx| window.simulate_next_frame(cx));
            }
            cx.run_until_parked();
        };
        frames(2, cx);
        scripts.update(cx, |scripts, cx| {
            scripts.set_page(Page::Library(Some(skua.clone())), cx)
        });
        frames(3, cx);
        let loaded = |name: &str, cx: &mut gpui::VisualTestContext| {
            let key = FormKey::Script(skua.join(name));
            scripts.read_with(cx, |scripts, _| scripts.forms.contains_key(&key))
        };
        assert!(loaded("s00.cs", cx), "the chosen script loads at once");
        assert!(
            !loaded("s03.cs", cx),
            "the rest wait for the list to settle"
        );

        cx.executor().advance_clock(SETTLE);
        frames(40, cx);
        for name in ["s01.cs", "s02.cs", "s03.cs", "s05.cs"] {
            assert!(loaded(name, cx), "{name} loads once the list settles");
        }
        assert!(!loaded("s39.cs", cx), "rows out of view don't load");
        fs::remove_dir_all(temp).unwrap();
    }

    #[gpui::test]
    fn search_covers_every_source_until_a_source_is_chosen(cx: &mut gpui::TestAppContext) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-search-{:x}", u64::from_ne_bytes(random)));
        let write = |file: &str, contents: &str| {
            let path = temp.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        };
        write("scripts/hunt.ts", "");
        write(
            "packages/dailies/package.json",
            r#"{"name":"Dailies","mira":{}}"#,
        );
        write("packages/dailies/scripts/spin.ts", "");
        cx.update(mira_ui::init);
        let (scripts, cx) = cx.add_window_view(|window, cx| {
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::packages::init(
                temp.join("packages"),
                temp.join("skua"),
                temp.join("github-token"),
                cx,
            );
            crate::script_inputs::init(temp.join("inputs.json"), cx);
            let client = Client::empty_for_tests(&temp, cx);
            ScriptsWindow::new(
                client,
                temp.join("scripts"),
                temp.join("types"),
                Target::Selected,
                window,
                cx,
            )
        });
        cx.run_until_parked();
        let selected = |cx: &mut gpui::VisualTestContext| {
            scripts.read_with(cx, |scripts, cx| scripts.target(cx).map(|entry| entry.path))
        };
        let dailies = temp.join("packages/dailies");
        let choose_dailies = |cx: &mut gpui::VisualTestContext| {
            scripts.update(cx, |scripts, cx| {
                scripts.set_page(Page::Library(Some(dailies.clone())), cx)
            });
            cx.run_until_parked();
        };
        choose_dailies(cx);
        assert_eq!(selected(cx), Some(dailies.join("scripts/spin.ts")));

        let search = scripts.read_with(cx, |scripts, _| scripts.search.clone());
        search.update(cx, |search, cx| search.set_text("hunt", cx));
        cx.run_until_parked();
        assert_eq!(selected(cx), Some(temp.join("scripts/hunt.ts")));

        choose_dailies(cx);
        assert_eq!(
            search.read_with(cx, |search, _| search.text().to_owned()),
            ""
        );
        assert_eq!(selected(cx), Some(dailies.join("scripts/spin.ts")));

        fs::remove_dir_all(&dailies).unwrap();
        let dialogs = scripts.read_with(cx, |scripts, _| scripts.package_dialogs.clone());
        dialogs.update(cx, |_, cx| cx.emit(PackageEvent::Changed));
        cx.run_until_parked();
        assert_eq!(
            scripts.read_with(cx, |scripts, _| scripts.page.clone()),
            Page::Library(None)
        );
        assert_eq!(selected(cx), Some(temp.join("scripts/hunt.ts")));
        fs::remove_dir_all(temp).unwrap();
    }

    #[gpui::test]
    fn the_inputs_bar_counts_inputs_to_fix_and_steps_through_them(cx: &mut gpui::TestAppContext) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp = std::env::temp_dir().join(format!("mira-fix-{:x}", u64::from_ne_bytes(random)));
        let script = temp.join("scripts/inputs.ts");
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(&script, include_str!("../../examples/inputs.ts")).unwrap();
        cx.update(mira_ui::init);
        let (scripts, cx) = cx.add_window_view(|window, cx| {
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::packages::init(
                temp.join("packages"),
                temp.join("skua"),
                temp.join("github-token"),
                cx,
            );
            crate::script_inputs::init(temp.join("inputs.json"), cx);
            let client = Client::empty_for_tests(&temp, cx);
            ScriptsWindow::new(
                client,
                temp.join("scripts"),
                temp.join("types"),
                Target::Selected,
                window,
                cx,
            )
        });
        cx.simulate_resize(gpui::size(px(MIN_WIDTH), px(MIN_HEIGHT)));
        cx.run_until_parked();
        let errors = |cx: &mut gpui::VisualTestContext| {
            scripts.read_with(cx, |scripts, cx| scripts.input_errors(&script, cx))
        };
        assert_eq!(errors(cx), 2);

        let text = |field: usize, cx: &mut gpui::VisualTestContext| {
            scripts.read_with(cx, |scripts, _| {
                let Some(Form {
                    state: FormState::Fields { texts, .. },
                    ..
                }) = scripts.forms.get(&FormKey::Script(script.clone()))
                else {
                    panic!("the inputs are loaded");
                };
                texts[field].clone().expect("a text field")
            })
        };
        let offset = text(4, cx);
        let focused = |input: &Entity<Input>, cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
        };
        let jump = |by: isize, cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| {
                scripts.update(cx, |scripts, cx| scripts.jump_to_error(by, window, cx))
            });
            cx.run_until_parked();
        };
        jump(1, cx);
        assert!(focused(&offset, cx), "the first input to fix is Offset");
        jump(1, cx);
        let scrolled = scripts.read_with(cx, |scripts, _| scripts.detail_scroll.offset().y);
        assert!(scrolled < px(0.0), "stepping to Room scrolls it into view");
        jump(1, cx);
        assert!(
            focused(&offset, cx),
            "stepping past the last input wraps to the first"
        );

        offset.update(cx, |input, cx| input.set_text("3", cx));
        cx.run_until_parked();
        assert_eq!(errors(cx), 1);
        fs::remove_dir_all(temp).unwrap();
    }

    #[gpui::test]
    fn tabbing_through_inputs_keeps_the_focused_field_in_view(cx: &mut gpui::TestAppContext) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp = std::env::temp_dir().join(format!("mira-tab-{:x}", u64::from_ne_bytes(random)));
        let script = temp.join("scripts/inputs.ts");
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(&script, include_str!("../../examples/inputs.ts")).unwrap();
        cx.update(mira_ui::init);
        let (scripts, cx) = cx.add_window_view(|window, cx| {
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::packages::init(
                temp.join("packages"),
                temp.join("skua"),
                temp.join("github-token"),
                cx,
            );
            crate::script_inputs::init(temp.join("inputs.json"), cx);
            window.activate_window();
            let client = Client::empty_for_tests(&temp, cx);
            ScriptsWindow::new(
                client,
                temp.join("scripts"),
                temp.join("types"),
                Target::Selected,
                window,
                cx,
            )
        });
        cx.simulate_resize(gpui::size(px(MIN_WIDTH), px(MIN_HEIGHT)));
        cx.run_until_parked();
        let input = |field: usize, cx: &mut gpui::VisualTestContext| {
            scripts.read_with(cx, |scripts, _| {
                let Some(Form {
                    state: FormState::Fields { texts, .. },
                    ..
                }) = scripts.forms.get(&FormKey::Script(script.clone()))
                else {
                    panic!("the inputs are loaded");
                };
                texts[field].clone().expect("a text field")
            })
        };
        let (name, offset) = (input(0, cx), input(4, cx));
        cx.update(|window, cx| window.focus(&name.focus_handle(cx), cx));
        cx.run_until_parked();
        for _ in 0..4 {
            cx.simulate_keystrokes("tab");
            cx.run_until_parked();
        }
        assert!(cx.update(|window, cx| offset.focus_handle(cx).is_focused(window)));
        let at_offset = scripts.read_with(cx, |scripts, _| scripts.detail_scroll.offset().y);
        assert!(
            at_offset < px(0.0),
            "Offset is below the fold, so tabbing to it scrolls"
        );
        for _ in 0..3 {
            cx.simulate_keystrokes("tab");
            cx.run_until_parked();
        }
        let at_mode = scripts.read_with(cx, |scripts, _| scripts.detail_scroll.offset().y);
        assert!(
            at_mode < at_offset,
            "tabbing on to Mode scrolls further, even with Offset still marked invalid"
        );
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn sources_with_the_same_name_show_the_nearest_folder_that_tells_them_apart() {
        let rows = |rows: &[(&str, &[&str])]| -> Vec<(SharedString, Vec<String>)> {
            rows.iter()
                .map(|(name, segments)| {
                    (
                        SharedString::from(*name),
                        segments.iter().map(|segment| segment.to_string()).collect(),
                    )
                })
                .collect()
        };
        assert_eq!(
            qualifiers(&rows(&[
                ("scripts", &["Mira", "Documents", "~"]),
                ("scripts", &["aqw", "~"]),
                ("community", &["aqw", "~"]),
            ])),
            [Some("Mira".into()), Some("aqw".into()), None]
        );
        assert_eq!(
            qualifiers(&rows(&[
                ("scripts", &["Mira", "a", "~"]),
                ("scripts", &["Mira", "b", "~"]),
            ])),
            [Some("a/Mira".into()), Some("b/Mira".into())]
        );
    }

    #[test]
    fn long_paths_drop_whole_folders_from_the_middle() {
        let fits = |limit: usize| move |text: &str| text.chars().count() <= limit;
        let path = "~/Documents/Mira/packages/story-scripts/scripts/merge/yulgar-merge.ts";
        assert_eq!(fit_path(path, fits(100)), path);
        assert_eq!(
            fit_path(path, fits(50)),
            "~/Documents/…/scripts/merge/yulgar-merge.ts"
        );
        assert_eq!(fit_path(path, fits(30)), "~/Documents/…/yulgar-merge.ts");
        assert_eq!(fit_path(path, fits(20)), "…/yulgar-merge.ts");
        assert_eq!(
            fit_path("/Volumes/aqw/community/bosses/dage.ts", fits(30)),
            "/Volumes/…/bosses/dage.ts"
        );
        assert_eq!(
            fit_path("~/Documents/dage.ts", fits(5)),
            "~/Documents/dage.ts"
        );
    }
}
