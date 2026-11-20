use mira_ui::StyledExt as _;
use std::{
    collections::{HashMap, HashSet},
    fs, io, iter,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable as _,
    FontWeight, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _,
    PathPromptOptions, Pixels, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, TextRun, TitlebarOptions, WeakEntity, Window, WindowBounds,
    WindowHandle, WindowOptions, actions, div, prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, Theme, Tone, h_flex,
    primitives::{
        Alert, Align, Badge, Button, Checkbox, CheckedState, Dot, DropdownMenu, Icon, Input,
        InputEvent, List, ListItem, MenuItem, Popover, Scrollbar, Select, Switch, Tab, Tabs,
        Toaster, VirtualList,
        button::{Size, Variant},
    },
    theme::RING_ROOM,
    v_flex,
};

use super::{
    Changed, Client, RuffleView,
    account::file_name,
    client::CloseSession,
    form::{note, row, row_label, switch_row},
    package_dialogs::{PackageDialogs, PackageEvent},
    queue::{self, EntryId, Queue},
    script::failure,
    script_form::{Draft, FieldDraft, ROOM_NOTE, drafts, error, room_text, rows, starting, values},
    shown_tab::{self, ShownTab, Target},
};
use crate::{
    account_settings::{self, Scripts as ScriptOptions},
    accounts::AccountId,
    domain::rooms::{PrivateRoom, Rooms},
    packages::{self, Manifest, ManifestError, Origin, Status},
    script_inputs,
    scripting::{self, Definition, ExtractError, Kind, Launch, Outcome, Supplied, Values},
    settings,
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

const WIDTH: f32 = 720.0;
const HEIGHT: f32 = 500.0;
const MIN_WIDTH: f32 = 600.0;
const MIN_HEIGHT: f32 = 360.0;
const PADDING: Pixels = px(16.0);
const SEARCH_WIDTH: Pixels = px(260.0);
const PANES_WIDTH: Pixels = px(180.0);
const RUN_WIDTH: Pixels = px(80.0);
const QUEUE_RUN_WIDTH: Pixels = px(104.0);
const FILTER_WIDTH: Pixels = px(300.0);
const FILTER_HEIGHT: Pixels = px(280.0);
const FILTER_PATH_WIDTH: Pixels = px(220.0);
const PATIENCE: Duration = Duration::from_millis(150);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    path: PathBuf,
    name: SharedString,
    folder: Option<SharedString>,
    source: Option<usize>,
}

impl Entry {
    fn matches(&self, query: &str) -> bool {
        self.name.to_lowercase().contains(query)
            || self
                .folder
                .as_ref()
                .is_some_and(|folder| folder.to_lowercase().contains(query))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SourceKind {
    Folder,
    Package {
        version: Option<SharedString>,
        description: Option<SharedString>,
        origin: Option<Origin>,
    },
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
    DefaultFolder,
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
    let headed = |heading: Row, source: Option<usize>| -> Vec<Row> {
        let rows: Vec<Row> = scripts(source).collect();
        if rows.is_empty() {
            return rows;
        }
        iter::once(heading).chain(rows).collect()
    };
    match scope {
        Scope::BuiltIn => return headed(Row::DefaultFolder, None),
        Scope::Source(index) => return headed(Row::Source(index), Some(index)),
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
        if named || !scripts.is_empty() {
            rows.push(Row::Source(index));
            rows.extend(scripts);
        }
    }
    rows
}

fn scan_all(built_in: &Path, added: &[PathBuf], packages: &Path) -> Library {
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
    library
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
        } else if scripting::runnable(name) {
            let folder = dir
                .strip_prefix(root)
                .ok()
                .filter(|folder| !folder.as_os_str().is_empty())
                .map(|folder| folder.display().to_string().into());
            entries.push(Entry {
                name: name.to_owned().into(),
                folder,
                path,
                source: None,
            });
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Library,
    Queue,
}

struct Open(WindowHandle<ScriptsWindow>);

impl gpui::Global for Open {}

pub(super) fn close(cx: &mut App) {
    if let Some(open) = cx.try_global::<Open>().map(|open| open.0) {
        open.update(cx, |_, window, _| window.remove_window()).ok();
    }
}

pub(crate) fn open(
    client: Entity<Client>,
    folder: PathBuf,
    types: PathBuf,
    target: Target,
    cx: &mut App,
) {
    if let Some(handle) = cx.try_global::<Open>().map(|open| open.0)
        && handle
            .update(cx, |scripts, window, cx| {
                window.activate_window();
                scripts.retarget(target, cx);
                scripts.reopen(window, cx);
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
        let _ = handle.update(cx, |scripts, _, cx| {
            if scripts.shown.is_some() && !scripts.running(cx) {
                scripts.choose_file(cx);
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
    types: PathBuf,
    library: Option<Library>,
    scanning: Option<Task<()>>,
    search: Entity<Input>,
    room: Entity<Input>,
    selected: usize,
    filter: Option<PathBuf>,
    pane: Pane,
    queued: usize,
    chosen: Option<PathBuf>,
    forms: HashMap<FormKey, Form>,
    generation: u64,
    loading: HashMap<FormKey, Task<()>>,
    waiting: Option<(PathBuf, Instant, Task<()>)>,
    displayed: Option<Entry>,
    detail_scroll: ScrollHandle,
    package_dialogs: Entity<PackageDialogs>,
    _view_changes: Option<Subscription>,
    _subscriptions: [Subscription; 6],
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum FormKey {
    Script(PathBuf),
    Entry(EntryId),
}

struct Form {
    generation: u64,
    state: FormState,
}

enum FormState {
    Broken {
        title: &'static str,
        message: SharedString,
    },
    Plain,
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
        let package_dialogs = cx.new(|cx| PackageDialogs::new(window, cx));
        let subscriptions = [
            cx.observe(&packages::packages(cx), |_, _, cx| cx.notify()),
            cx.subscribe(&package_dialogs, |this, _, event: &PackageEvent, cx| {
                if let PackageEvent::Installed(dir) = event {
                    this.pane = Pane::Library;
                    this.filter = Some(this.packages.join(dir));
                }
                this.generation += 1;
                this.rescan(cx);
            }),
            cx.observe(&client, |this, _, cx| this.show(cx)),
            cx.subscribe(&search, |this, _, event: &InputEvent, cx| match event {
                InputEvent::Changed => {
                    this.pane = Pane::Library;
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
        ];
        let mut scripts = Self {
            client,
            focus: cx.focus_handle(),
            tab,
            shown: None,
            packages: folder.with_file_name("packages"),
            folder,
            types,
            library: None,
            scanning: None,
            search,
            room,
            selected: 0,
            filter: None,
            pane: Pane::Library,
            queued: 0,
            chosen: None,
            forms: HashMap::new(),
            generation: 0,
            loading: HashMap::new(),
            waiting: None,
            displayed: None,
            detail_scroll: ScrollHandle::new(),
            package_dialogs,
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
        self._view_changes = self
            .view()
            .map(|view| cx.subscribe(&view, |_, _, _: &Changed, cx| cx.notify()));
        self.queued = 0;
        self.chosen = None;
        self.show_saved_room(cx);
        self.select_last_run(cx);
    }

    fn reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.generation += 1;
        self.rescan(cx);
        window.focus(&self.search.focus_handle(cx), cx);
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

    fn status(&self, theme: &Theme, cx: &App) -> (SharedString, Hsla) {
        if self.shown.is_none() {
            return ("No tabs open".into(), theme.muted_foreground);
        }
        let Some(view) = self.view() else {
            return ("Nothing running".into(), theme.muted_foreground);
        };
        let script = &view.read(cx).script;
        match script.status() {
            Some(text) if script.running() => (text.into(), theme.foreground),
            Some(text) if script.failed() => (text.into(), theme.destructive),
            Some(text) => (text.into(), theme.muted_foreground),
            None => ("Nothing running".into(), theme.muted_foreground),
        }
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
        let built_in = self.folder.clone();
        let added = settings::script_folders(cx).to_vec();
        let packages = self.packages.clone();
        let types = self.types.clone();
        let scanned = cx.background_spawn(async move {
            let library = scan_all(&built_in, &added, &packages);
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
                this.library = Some(scanned);
                if first {
                    this.select_last_run(cx);
                }
                let rows = this.rows(cx);
                if !matches!(rows.get(this.selected), Some(Row::Script(_))) {
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
        let Some(library) = &self.library else {
            return Rc::new([]);
        };
        let query = self.search.read(cx).text().trim().to_lowercase();
        library_rows(library, &query, self.scope()).into()
    }

    fn scope(&self) -> Scope {
        let Some(filter) = &self.filter else {
            return Scope::All;
        };
        if *filter == self.folder {
            return Scope::BuiltIn;
        }
        self.library
            .as_ref()
            .and_then(|library| {
                library
                    .sources
                    .iter()
                    .position(|source| source.root == *filter)
            })
            .map_or(Scope::All, Scope::Source)
    }

    fn set_filter(&mut self, filter: Option<PathBuf>, cx: &mut Context<Self>) {
        self.filter = filter;
        self.chosen = None;
        self.selected = first_script(&self.rows(cx));
        cx.notify();
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

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                let _ = this.update(cx, |this, cx| {
                    this.pane = Pane::Library;
                    this.chosen = Some(path);
                    cx.notify();
                });
            }
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

    fn add_folder(&mut self, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(folder) = paths.into_iter().next() else {
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

    fn locate_folder(&mut self, old: PathBuf, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(folder) = paths.into_iter().next() else {
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

    fn filter(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<DropdownMenu> {
        if self.library.as_ref()?.sources.is_empty() {
            return None;
        }
        let this = cx.entity().downgrade();
        Some(
            DropdownMenu::new("filter")
                .trigger(
                    Button::new("filter", "Filter")
                        .pressed(self.filter.is_some())
                        .end_slot(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(Icon::ChevronDown),
                        ),
                )
                .align(Align::End)
                .width(FILTER_WIDTH)
                .max_height(FILTER_HEIGHT)
                .items(move |window, cx| {
                    this.upgrade()
                        .map(|scripts| scripts.read(cx).filter_items(&this, window, cx.theme()))
                        .unwrap_or_default()
                }),
        )
    }

    fn filter_items(
        &self,
        this: &WeakEntity<Self>,
        window: &Window,
        theme: &Theme,
    ) -> Vec<MenuItem> {
        let Some(library) = &self.library else {
            return Vec::new();
        };
        let choose = |label: SharedString, filter: Option<PathBuf>| {
            let this = this.clone();
            let checked = self.filter == filter;
            MenuItem::radio(label, checked, move |_, cx| {
                let _ = this.update(cx, |this, cx| this.set_filter(filter.clone(), cx));
            })
        };
        let source = |label: SharedString, root: &Path, count: SharedString| {
            choose(label, Some(root.to_owned()))
                .description(fit_path_to(
                    &home_relative(root),
                    FILTER_PATH_WIDTH,
                    window,
                    theme,
                ))
                .end_slot(count)
        };
        let built_in = library
            .entries
            .iter()
            .filter(|entry| entry.source.is_none())
            .count();
        let mut items = vec![
            choose("All sources".into(), None)
                .end_slot(SharedString::from(library.entries.len().to_string())),
            MenuItem::separator(),
            MenuItem::header("Folders"),
            source(
                "Default folder".into(),
                &self.folder,
                built_in.to_string().into(),
            ),
        ];
        let mut packages = Vec::new();
        for added in &library.sources {
            let count = added
                .problem
                .clone()
                .unwrap_or_else(|| added.scripts.to_string().into());
            let item = source(added.name.clone(), &added.root, count);
            match added.kind {
                SourceKind::Folder => items.push(item),
                SourceKind::Package { .. } => packages.push(item),
            }
        }
        if !packages.is_empty() {
            items.push(MenuItem::separator());
            items.push(MenuItem::header("Packages"));
            items.extend(packages);
        }
        items
    }

    fn sources(&self, cx: &mut Context<Self>) -> DropdownMenu {
        let this = cx.entity().downgrade();
        let troubled = self.library.as_ref().is_some_and(|library| {
            library
                .sources
                .iter()
                .any(|source| source.problem.is_some())
        });
        let updates = packages::packages(cx).read(cx).any_available();
        let dot = match (troubled, updates) {
            (true, _) => Some(Tone::Danger),
            (false, true) => Some(Tone::Accent),
            (false, false) => None,
        };
        DropdownMenu::new("sources")
            .trigger(
                Button::new("sources", "Sources")
                    .when_some(dot, |button, tone| button.start_slot(Dot::new(tone)))
                    .end_slot(Icon::ChevronDown),
            )
            .items(move |_, cx| {
                this.upgrade()
                    .map(|scripts| scripts.read(cx).source_items(&this, cx))
                    .unwrap_or_default()
            })
    }

    fn source_items(&self, this: &WeakEntity<Self>, cx: &App) -> Vec<MenuItem> {
        let act =
            |label: &'static str, root: &Path, f: fn(&mut Self, PathBuf, &mut Context<Self>)| {
                let this = this.clone();
                let root = root.to_owned();
                MenuItem::action(label, move |_, cx| {
                    let _ = this.update(cx, |this, cx| f(this, root.clone(), cx));
                })
            };
        let show = |root: &Path| {
            act("Show in Finder", root, |this, root, cx| {
                this.show_folder(&root, cx)
            })
        };
        let remove = |root: &Path| {
            act("Remove from library", root, |this, root, cx| {
                this.remove_folder(&root, cx)
            })
        };
        let locate = |root: &Path| act("Locate…", root, Self::locate_folder);
        let sources = self
            .library
            .as_ref()
            .map_or(&[][..], |library| library.sources.as_slice());
        let mut items = vec![
            MenuItem::header("Folders"),
            MenuItem::submenu(home_relative(&self.folder), vec![show(&self.folder)]),
        ];
        let mut packages = Vec::new();
        for source in sources {
            match &source.kind {
                SourceKind::Folder => {
                    let path = home_relative(&source.root);
                    let item = match &source.problem {
                        None => {
                            MenuItem::submenu(path, vec![show(&source.root), remove(&source.root)])
                        }
                        Some(problem) => MenuItem::submenu(
                            path,
                            vec![locate(&source.root), remove(&source.root)],
                        )
                        .start_slot(Dot::new(Tone::Danger))
                        .description(problem.clone()),
                    };
                    items.push(item);
                }
                SourceKind::Package {
                    version, origin, ..
                } => packages.push(self.package_item(source, version, origin, &show, cx)),
            }
        }
        let add = this.clone();
        items.push(MenuItem::action("Add folder…", move |_, cx| {
            let _ = add.update(cx, |this, cx| this.add_folder(cx));
        }));
        items.push(MenuItem::separator());
        items.push(MenuItem::header("Packages"));
        items.extend(packages);
        let state = packages::packages(cx).read(cx);
        let busy = state.activity().is_some();
        let dialogs = self.package_dialogs.clone();
        items.push(
            MenuItem::action("Install package…", move |window, cx| {
                dialogs.update(cx, |dialogs, cx| dialogs.open_install(window, cx));
            })
            .disabled(busy),
        );
        let installed = sources.iter().any(|source| {
            matches!(
                source.kind,
                SourceKind::Package {
                    origin: Some(_),
                    ..
                }
            )
        });
        let checking = matches!(state.activity(), Some(packages::Activity::Checking));
        let dialogs = self.package_dialogs.clone();
        items.push(
            MenuItem::action(
                if checking {
                    "Checking for updates…"
                } else {
                    "Check for updates"
                },
                move |_, cx| dialogs.update(cx, |dialogs, cx| dialogs.check(cx)),
            )
            .disabled(busy || !installed),
        );
        items
    }

    fn package_item(
        &self,
        source: &Source,
        version: &Option<SharedString>,
        origin: &Option<Origin>,
        show: &dyn Fn(&Path) -> MenuItem,
        cx: &App,
    ) -> MenuItem {
        let state = packages::packages(cx).read(cx);
        let busy = state.activity().is_some();
        let dir = source
            .root
            .file_name()
            .map(|dir| dir.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = source.name.clone();
        let mut actions = vec![show(&source.root)];
        if let Some(origin) = origin {
            let url = origin.url.clone();
            actions.push(MenuItem::action("Open on GitHub", move |_, cx| {
                cx.open_url(&url);
            }));
            let dialogs = self.package_dialogs.clone();
            let (dir, name) = (dir.clone(), name.clone());
            actions.push(
                MenuItem::action("Update", move |_, cx| {
                    dialogs.update(cx, |dialogs, cx| {
                        dialogs.update(dir.clone(), name.clone(), false, cx)
                    });
                })
                .disabled(busy),
            );
        }
        let dialogs = self.package_dialogs.clone();
        let target = dir.clone();
        actions.push(
            MenuItem::action("Move to Trash", move |_, cx| {
                dialogs.update(cx, |dialogs, cx| {
                    dialogs.remove(target.clone(), name.clone(), cx)
                });
            })
            .disabled(busy),
        );
        let item = MenuItem::submenu(source.name.clone(), actions);
        let label = version
            .clone()
            .or_else(|| origin.as_ref().map(|origin| origin.commit.clone().into()));
        let item = match label {
            Some(label) => item.end_slot(Badge::new(label)),
            None => item,
        };
        match (&source.problem, state.status(&dir)) {
            (Some(problem), _) => item
                .start_slot(Dot::new(Tone::Danger))
                .description(problem.clone()),
            (None, Some(Status::Available)) => item
                .start_slot(Dot::new(Tone::Accent))
                .description("Update available"),
            (None, Some(Status::Failed(message))) => item
                .start_slot(Dot::new(Tone::Warning))
                .description(message.clone()),
            (None, None) => item,
        }
    }

    fn set_pane(&mut self, pane: Pane, cx: &mut Context<Self>) {
        self.pane = pane;
        cx.notify();
    }

    fn target(&self, cx: &App) -> Option<Entry> {
        if let Some(path) = &self.chosen {
            return Some(Entry {
                name: file_name(path).into(),
                path: path.clone(),
                folder: None,
                source: None,
            });
        }
        let rows = self.rows(cx);
        match rows.get(self.selected.min(rows.len().saturating_sub(1))) {
            Some(Row::Script(entry)) => Some(entry.clone()),
            _ => None,
        }
    }

    fn load_form(&mut self, key: FormKey, path: &Path, cx: &mut Context<Self>) {
        let current = self
            .forms
            .get(&key)
            .is_some_and(|form| form.generation == self.generation);
        let pending = self.loading.contains_key(&key);
        if current || pending {
            return;
        }
        let script = path.to_owned();
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
                ExtractError::Inputs(message) => ("This script's inputs are invalid", message),
            })
        });
        let generation = self.generation;
        let task = cx.spawn({
            let key = key.clone();
            async move |this, cx| {
                let read = read.await;
                let _ = this.update(cx, |this, cx| {
                    this.loading.remove(&key);
                    this.loaded(key, generation, read, cx);
                    cx.notify();
                });
            }
        });
        if let FormKey::Script(path) = &key
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

    fn detail_entry(&mut self, target: Option<Entry>) -> Option<Entry> {
        let target = target?;
        if self
            .forms
            .contains_key(&FormKey::Script(target.path.clone()))
        {
            if self.displayed.as_ref().map(|shown| &shown.path) != Some(&target.path) {
                self.detail_scroll.set_offset(gpui::point(px(0.0), px(0.0)));
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
        read: Result<Option<Definition>, (&'static str, String)>,
        cx: &mut Context<Self>,
    ) {
        if let Some(form) = self.forms.get_mut(&key)
            && let (FormState::Fields { definition, .. }, Ok(Some(read))) = (&form.state, &read)
            && definition == read
        {
            form.generation = generation;
            return;
        }
        let state = match read {
            Err((title, message)) => FormState::Broken {
                title,
                message: message.into(),
            },
            Ok(None) => FormState::Plain,
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
        self.forms.insert(key.clone(), Form { generation, state });
        if let Some(edited) = checked {
            self.store_entry(&key, edited, cx);
        }
    }

    fn set_draft(&mut self, key: &FormKey, index: usize, draft: Draft, cx: &mut Context<Self>) {
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
        let (Some(field), Some(slot)) = (definition.fields().get(index), drafts.get_mut(index))
        else {
            return;
        };
        *slot = FieldDraft { draft, stale: None };
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
            FormState::Plain => Some((None, Values::new())),
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
        }
        Some((entry.path, values))
    }

    fn entry_ready(&self, id: EntryId) -> bool {
        match self.forms.get(&FormKey::Entry(id)).map(|form| &form.state) {
            Some(FormState::Plain) => true,
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
                options_panel(this.clone(), scripts.options(cx), scripts.room.clone(), cx)
            })
            .width(px(376.0))
    }

    fn inputs(
        &self,
        theme: &Theme,
        key: &FormKey,
        width: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = theme.layout;
        let Some(form) = self.forms.get(key) else {
            return muted(theme, "Reading inputs…").into_any_element();
        };
        let (definition, drafts, texts) = match &form.state {
            FormState::Broken { title, message } => {
                return Alert::new("script-broken", Tone::Danger, *title)
                    .body(message.clone())
                    .into_any_element();
            }
            FormState::Plain => {
                return muted(theme, "This script has no inputs.").into_any_element();
            }
            FormState::Fields {
                definition,
                drafts,
                texts,
                ..
            } => (definition, drafts, texts),
        };
        let this = cx.entity().downgrade();
        let saved = matches!(key, FormKey::Script(_));
        let unreadable = (saved && script_inputs::unreadable(cx)).then(|| {
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
        let stale = drafts.iter().any(|draft| draft.stale.is_some()).then(|| {
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
                                .hide_label()
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
                            .when(matches!(kind, Kind::Number { .. }), |slot| slot.w(px(80.0)))
                            .child(input.clone())
                            .into_any_element(),
                        _ => div().into_any_element(),
                    };
                    let control = if width < theme.layout.inspector {
                        mira_ui::primitives::Field::new(label, control).into_any_element()
                    } else {
                        row(layout, label, true, control).into_any_element()
                    };
                    v_flex()
                        .gap(layout.space(1.0))
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
        v_flex()
            .gap(layout.space(2.0))
            .children(unreadable)
            .children(stale)
            .children(fields)
            .into_any_element()
    }

    fn script_detail(
        &self,
        theme: &Theme,
        entry: Option<&Entry>,
        width: Pixels,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(entry) = entry else {
            return div().into_any_element();
        };
        let path = fit_path_to(&home_relative(&entry.path), width, window, theme);
        v_flex()
            .gap(theme.space(4.0))
            .child(heading(theme, entry.name.clone(), path, None))
            .child(self.inputs(theme, &FormKey::Script(entry.path.clone()), width, cx))
            .into_any_element()
    }

    fn queued_detail(
        &self,
        theme: &Theme,
        queue: &[queue::Entry],
        width: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = theme.layout;
        let Some(queued) = queue.get(self.queued) else {
            return div().into_any_element();
        };
        let position: SharedString = match &queued.state {
            queue::State::Waiting => format!(
                "{} of {} waiting",
                queue[..self.queued]
                    .iter()
                    .filter(|entry| entry.waiting())
                    .count()
                    + 1,
                queue.iter().filter(|entry| entry.waiting()).count()
            )
            .into(),
            queue::State::Running { .. } => "Running now".into(),
            queue::State::Ended { outcome, took } => {
                format!("{} after {}", ended(outcome), duration(*took)).into()
            }
        };
        let failed = match &queued.state {
            queue::State::Ended { outcome, .. } => failure(outcome),
            _ => None,
        };
        let key = FormKey::Entry(queued.id);
        let body = match self.forms.get(&key).map(|form| &form.state) {
            _ if queued.waiting() => self.inputs(theme, &key, width, cx),
            None => muted(theme, "Reading inputs…").into_any_element(),
            Some(FormState::Broken { title, message }) => {
                Alert::new("queued-broken", Tone::Danger, *title)
                    .body(message.clone())
                    .into_any_element()
            }
            Some(FormState::Plain) => muted(theme, "This script has no inputs.").into_any_element(),
            Some(FormState::Fields { definition, .. }) => {
                let rows = queued
                    .values
                    .as_ref()
                    .map(|values| rows(definition, values))
                    .unwrap_or_default();
                v_flex()
                    .gap(layout.space(2.0))
                    .children(rows.into_iter().map(|(label, value)| {
                        row(layout, label, true, div().truncate().child(value))
                    }))
                    .into_any_element()
            }
        };
        v_flex()
            .gap(theme.space(4.0))
            .child(heading(
                theme,
                file_name(&queued.script).into(),
                position,
                Some(
                    Button::new("queued-remove", "Remove")
                        .size(Size::Small)
                        .variant(Variant::Ghost)
                        .disabled(queued.running())
                        .on_click(cx.listener(|this, _, _, cx| this.remove_queued(cx)))
                        .into_any_element(),
                ),
            ))
            .when_some(failed, |detail, message| {
                detail.child(Alert::new("queued-failed", Tone::Danger, "Failed").body(message))
            })
            .child(body)
            .into_any_element()
    }

    fn queue_list(
        &self,
        theme: &Theme,
        queue: &[queue::Entry],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if queue.is_empty() {
            return empty_frame(theme)
                .child("Nothing queued. Pick a script in Library and choose Add to queue.")
                .into_any_element();
        }
        let rows: Rc<[QueueRow]> = queue
            .iter()
            .map(|queued| {
                let form = self
                    .forms
                    .get(&FormKey::Entry(queued.id))
                    .map(|form| &form.state);
                let meta = match &queued.state {
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
                QueueRow {
                    name: file_name(&queued.script).into(),
                    meta,
                }
            })
            .collect();
        let pinned = queue.iter().filter(|entry| !entry.waiting()).count();
        let this = cx.entity().downgrade();
        let that = this.clone();
        let list = List::new("queue", "Queue", rows.len(), move |index| {
            let QueueRow { name, meta } = &rows[index];
            let item = ListItem::new(name.clone());
            match meta {
                Some((meta, color)) => item.end_slot(div().text_color(*color).child(meta.clone())),
                None => item,
            }
        })
        .selected(Some(self.queued))
        .fill()
        .on_select(move |index, _, cx| {
            let _ = this.update(cx, |this, cx| {
                this.queued = index;
                cx.notify();
            });
        })
        .on_reorder(move |from, to, _, cx| {
            let _ = that.update(cx, |this, cx| this.move_queued(from, to, cx));
        })
        .pin_first(pinned);
        v_flex().size_full().child(list).into_any_element()
    }

    fn library(&self, theme: &Theme, rows: Rc<[Row]>, cx: &mut Context<Self>) -> AnyElement {
        let layout = theme.layout;
        let typography = theme.typography;
        let note = || empty_frame(theme);
        let Some(library) = &self.library else {
            return note().into_any_element();
        };
        if library.entries.is_empty() && library.sources.is_empty() {
            let folder = self.folder.clone();
            return note()
                .child(
                    div()
                        .text_size(typography.body)
                        .line_height(typography.body_line)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .child("No scripts yet"),
                )
                .child(format!(
                    "Put .ts or .js files in {}, or add a folder you already have.",
                    home_relative(&self.folder)
                ))
                .child(
                    div()
                        .flex()
                        .gap(layout.space(2.0))
                        .child(
                            Button::new("empty-show", "Show in Finder")
                                .size(Size::Small)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.show_folder(&folder, cx)
                                    }),
                                ),
                        )
                        .child(
                            Button::new("empty-add", "Add folder…")
                                .size(Size::Small)
                                .on_click(cx.listener(|this, _, _, cx| this.add_folder(cx))),
                        ),
                )
                .into_any_element();
        }
        let list = if rows.is_empty() {
            let query = self.search.read(cx).text().trim().to_owned();
            let text = if query.is_empty() {
                "No scripts here yet.".to_owned()
            } else {
                format!("No scripts match \u{201c}{query}\u{201d}.")
            };
            note().child(text).into_any_element()
        } else {
            let sources: Rc<[Source]> = library.sources.clone().into();
            let built_in: SharedString = library
                .entries
                .iter()
                .filter(|entry| entry.source.is_none())
                .count()
                .to_string()
                .into();
            let count = rows.len();
            let this = cx.entity().downgrade();
            let that = this.clone();
            VirtualList::new("scripts", "Scripts", count, move |index| {
                match &rows[index] {
                    Row::DefaultFolder => {
                        ListItem::header("Default folder").end_slot(built_in.clone())
                    }
                    Row::Source(source) => {
                        let source = &sources[*source];
                        ListItem::header(source.name.clone()).end_slot(source_meta(source))
                    }
                    Row::Script(entry) => match &entry.folder {
                        Some(folder) => ListItem::new(entry.name.clone()).end_slot(
                            div()
                                .max_w(px(100.0))
                                .truncate()
                                .text_ellipsis_start()
                                .child(folder.clone()),
                        ),
                        None => ListItem::new(entry.name.clone()),
                    },
                }
            })
            .selected(Some(self.selected.min(count - 1)))
            .fill()
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
        };
        v_flex().size_full().child(list).into_any_element()
    }
}

fn first_script(rows: &[Row]) -> usize {
    rows.iter()
        .position(|row| matches!(row, Row::Script(_)))
        .unwrap_or(0)
}

fn source_meta(source: &Source) -> SharedString {
    if let Some(problem) = &source.problem {
        return problem.clone();
    }
    match &source.kind {
        SourceKind::Package {
            version: Some(version),
            ..
        } => version.clone(),
        _ => source.scripts.to_string().into(),
    }
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
    let font = gpui::font(theme.font_family.clone());
    fit_path(path, |text| {
        let run = TextRun {
            len: text.len(),
            font: font.clone(),
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
            <= width
    })
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
            self.load_form(FormKey::Script(entry.path.clone()), &entry.path, cx);
        }
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
        if self.pane == Pane::Queue {
            for entry in &queue {
                self.load_form(FormKey::Entry(entry.id), &entry.script, cx);
            }
        }
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
        let (status, status_color) = self.status(&theme, cx);
        let header = shown_tab::header(
            "Scripts",
            &self.client,
            self.shown.as_ref(),
            self.tab,
            Self::retarget,
            window,
            cx,
        )
        .child(
            div()
                .id("script-status")
                .role(gpui::Role::Status)
                .aria_label(status.clone())
                .min_w_0()
                .max_w(px(360.0))
                .pr(PADDING)
                .truncate()
                .text_size(typography.secondary)
                .text_color(status_color)
                .child(status),
        );
        let this = cx.entity().downgrade();
        let panes = Tabs::new("pane", "Show", 2, move |index| {
            if index == 0 {
                Tab::new("Library")
            } else {
                let tab = Tab::new("Queue");
                if unfinished > 0 {
                    tab.end_slot(Badge::new(unfinished.to_string()))
                } else {
                    tab
                }
            }
        })
        .selected(usize::from(self.pane == Pane::Queue))
        .on_select(move |index, _, cx| {
            let pane = if index == 0 {
                Pane::Library
            } else {
                Pane::Queue
            };
            let _ = this.update(cx, |this, cx| this.set_pane(pane, cx));
        });
        let library = self.pane == Pane::Library;
        let search = div()
            .w(SEARCH_WIDTH)
            .capture_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let by = match event.keystroke.key.as_str() {
                    "up" => -1,
                    "down" => 1,
                    _ => return,
                };
                this.step(by, cx);
                cx.stop_propagation();
            }))
            .child(self.search.clone());
        let toolbar = theme
            .toolbar()
            .child(div().w(PANES_WIDTH).child(panes))
            .child(div().flex_1())
            .when(library, |toolbar| {
                toolbar.child(
                    h_flex()
                        .gap(layout.space(1.5))
                        .children(self.filter(&theme, cx))
                        .child(search),
                )
            });
        let list_width = (window.viewport_size().width * 0.4).min(theme.layout.script_list_max);
        let detail_width =
            window.viewport_size().width - list_width - px(1.0) - theme.space(4.0) * 2.0;
        let (list, detail) = match self.pane {
            Pane::Library => (
                self.library(&theme, rows, cx),
                self.script_detail(&theme, chosen.as_ref(), detail_width, window, cx),
            ),
            Pane::Queue => (
                self.queue_list(&theme, &queue, cx),
                self.queued_detail(&theme, &queue, detail_width, cx),
            ),
        };
        let body = h_flex()
            .flex_1()
            .min_h_0()
            .items_start()
            .child(
                v_flex()
                    .flex_none()
                    .w(list_width)
                    .h_full()
                    .p(theme.space(3.0))
                    .border_r_1()
                    .border_color(theme.border)
                    .child(list),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .p(theme.space(4.0))
                    .child(
                        div()
                            .id("detail")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.detail_scroll)
                            .pl(RING_ROOM)
                            .pr(Scrollbar::GUTTER)
                            .pb(RING_ROOM)
                            .child(detail),
                    )
                    .child(Scrollbar::new(
                        "detail-scrollbar",
                        self.detail_scroll.clone(),
                    )),
            );
        let footer = theme.footer().flex_wrap().child(self.options_popover(cx));
        let footer = match self.pane {
            Pane::Library => footer
                .child(self.sources(cx))
                .child(
                    Button::new("choose-file", "Choose file…")
                        .variant(Variant::Ghost)
                        .disabled(!has_tab || running)
                        .on_click(cx.listener(|this, _, _, cx| this.choose_file(cx))),
                )
                .child(div().flex_1())
                .child(
                    Button::new("queue", "Add to queue")
                        .end_slot(mira_ui::primitives::Kbd::hint("⌘⏎"))
                        .disabled(!has_tab || !valid)
                        .on_click(cx.listener(|this, _, _, cx| this.add_to_queue(cx))),
                )
                .child(div().flex_none().w(RUN_WIDTH).child(if running {
                    Button::new("stop", "Stop")
                        .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
                } else {
                    Button::new("run", "Run")
                        .end_slot(mira_ui::primitives::Kbd::hint("⏎"))
                        .variant(Variant::Primary)
                        .disabled(!has_tab || !valid)
                        .on_click(cx.listener(|this, _, _, cx| this.run_selected(cx)))
                })),
            Pane::Queue => footer
                .child(
                    Button::new("queue-clear", "Clear queue")
                        .variant(Variant::Ghost)
                        .disabled(queue.len() == usize::from(skippable))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.edit_queue(cx, Queue::clear);
                        })),
                )
                .child(
                    Button::new("queue-clear-finished", "Clear finished")
                        .variant(Variant::Ghost)
                        .disabled(!finished)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.edit_queue(cx, Queue::clear_finished);
                        })),
                )
                .child(div().flex_1())
                .when(playing && skippable, |footer| {
                    footer.child(Button::new("queue-skip", "Skip").on_click(
                        cx.listener(|this, _, _, cx| this.act_on_tab(cx, RuffleView::skip_script)),
                    ))
                })
                .child(
                    div()
                        .flex_none()
                        .w(QUEUE_RUN_WIDTH)
                        .child(if playing || skippable {
                            Button::new("queue-stop", "Stop queue").on_click(cx.listener(
                                |this, _, _, cx| this.act_on_tab(cx, RuffleView::stop_script),
                            ))
                        } else {
                            Button::new("queue-start", "Start queue")
                                .variant(Variant::Primary)
                                .disabled(!has_tab || !waiting || !startable)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.edit_queue(cx, Queue::play)),
                                )
                        }),
                ),
        };
        v_flex()
            .size_full()
            .window_surface(&theme)
            .key_context(format!("{FOCUS_SCOPE} {CONTEXT}").as_str())
            .id("scripts")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseSession, window, _| window.remove_window()))
            .on_action(cx.listener(|this, _: &AddToQueue, _, cx| {
                if this.pane == Pane::Library {
                    this.add_to_queue(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &crate::app_menu::Find, window, cx| {
                this.set_pane(Pane::Library, cx);
                window.focus(&this.search.focus_handle(cx), cx);
            }))
            .child(mira_ui::nav::pointer_probe())
            .child(header)
            .child(toolbar)
            .child(body)
            .child(footer)
            .child(self.package_dialogs.clone())
            .child(Toaster::layer(
                &self.package_dialogs.read(cx).toasts.clone(),
                window,
                cx,
            ))
    }
}

fn heading(
    theme: &Theme,
    name: SharedString,
    detail: SharedString,
    action: Option<AnyElement>,
) -> impl IntoElement {
    let layout = theme.layout;
    let typography = theme.typography;
    h_flex()
        .items_start()
        .gap(layout.space(2.0))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().truncate().font_weight(FontWeight::MEDIUM).child(name))
                .child(
                    div()
                        .truncate()
                        .text_size(typography.secondary)
                        .line_height(typography.secondary_line)
                        .text_color(theme.muted_foreground)
                        .child(detail),
                ),
        )
        .children(action)
}

fn empty_frame(theme: &Theme) -> gpui::Div {
    let layout = theme.layout;
    let typography = theme.typography;
    v_flex()
        .size_full()
        .gap(layout.space(2.5))
        .p(layout.space(3.0))
        .rounded(theme.radius.panel)
        .bg(theme.card)
        .border_1()
        .border_color(theme.border)
        .text_size(typography.secondary)
        .line_height(typography.secondary_line)
        .text_color(theme.muted_foreground)
}

fn muted(theme: &Theme, text: &'static str) -> impl IntoElement {
    let typography = theme.typography;
    div()
        .text_size(typography.body)
        .line_height(typography.body_line)
        .text_color(theme.muted_foreground)
        .child(text)
}

struct QueueRow {
    name: SharedString,
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
    cx: &App,
) -> AnyElement {
    let layout = cx.theme().layout;
    let editable = options.is_some();
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
            v_flex()
                .gap(layout.space(1.0))
                .child(switch_row(
                    layout,
                    "Safe start and stop",
                    editable,
                    Switch::new("safe-start", "Safe start and stop", options.safe_start_stop)
                        .on_toggle(toggle(|options, on| options.safe_start_stop = on)),
                ))
                .child(note(
                    "Best effort attempt to start or stop the script at your house.",
                    editable,
                    &theme,
                )),
        )
        .child(
            v_flex()
                .gap(layout.space(1.0))
                .child(switch_row(
                    layout,
                    "Restart after reconnecting",
                    editable,
                    Switch::new(
                        "restart",
                        "Restart after reconnecting",
                        options.restart_after_reconnect,
                    )
                    .on_toggle(toggle(|options, on| options.restart_after_reconnect = on)),
                ))
                .child(note(
                    "Restart the script when the same account reconnects.",
                    editable,
                    &theme,
                )),
        )
        .child(div().h(px(1.0)).my(layout.space(1.0)).bg(theme.border))
        .child(
            v_flex()
                .gap(layout.space(1.0))
                .child(switch_row(
                    layout,
                    "Private rooms",
                    editable,
                    Switch::new("private-rooms", "Private rooms", private).on_toggle(set_private),
                ))
                .child(note(
                    "Scripts join private rooms instead of public ones.",
                    editable,
                    &theme,
                )),
        )
        .when(editable && private, |options| {
            options.child(
                v_flex()
                    .gap(layout.space(1.0))
                    .child(
                        h_flex()
                            .justify_between()
                            .gap(layout.space(3.0))
                            .min_h(layout.control)
                            .child(row_label("Room", true))
                            .child(div().flex_none().w(px(96.0)).child(room_input)),
                    )
                    .child(note(ROOM_NOTE, true, &theme)),
            )
        })
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
        assert_eq!(scan_all(&built_in, &[], &packages), Library::default());
        for (file, contents) in [
            ("scripts/zeta.ts", ""),
            ("scripts/Alpha.js", ""),
            ("scripts/notes.txt", ""),
            ("scripts/types.d.ts", ""),
            ("scripts/farming/daily.mts", ""),
            ("scripts/farming/hunt.ts", ""),
            ("scripts/node_modules/lib/index.js", ""),
            ("scripts/.git/hook.js", ""),
            ("community/daily.ts", ""),
            ("community/bosses/dage.ts", ""),
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
        ] {
            let path = temp.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let library = scan_all(
            &built_in,
            &[community.clone(), missing.clone(), authoring.clone()],
            &packages,
        );
        let listed: Vec<(Option<usize>, Option<&str>, &str)> = library
            .entries
            .iter()
            .map(|entry| (entry.source, entry.folder.as_deref(), entry.name.as_ref()))
            .collect();
        assert_eq!(
            listed,
            [
                (None, None, "Alpha.js"),
                (None, None, "zeta.ts"),
                (None, Some("farming"), "daily.mts"),
                (None, Some("farming"), "hunt.ts"),
                (Some(0), None, "daily.ts"),
                (Some(0), Some("bosses"), "dage.ts"),
                (Some(2), None, "draft.ts"),
                (Some(4), None, "spin.ts"),
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
                (community.as_path(), "community", None, 2),
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
                    Some(
                        "This isn't a Mira script package. Its package.json needs a \"mira\" field."
                    ),
                    0
                ),
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
    fn search_and_the_folder_filter_narrow_the_list() {
        let entry = |name: &str, folder: Option<&str>, source: Option<usize>| Entry {
            path: PathBuf::from(name),
            name: name.to_owned().into(),
            folder: folder.map(|folder| folder.to_owned().into()),
            source,
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
                entry("hunt.ts", None, None),
                entry("daily.ts", Some("weekly"), Some(0)),
                entry("dage.ts", None, Some(0)),
                entry("spin.ts", None, Some(1)),
            ],
            sources: vec![source("community"), source("dailies")],
        };
        let names_in = |query: &str, scope: Scope| -> Vec<String> {
            library_rows(&library, query, scope)
                .iter()
                .map(|row| match row {
                    Row::DefaultFolder => "[Default folder]".to_owned(),
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
                "hunt.ts",
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
        assert!(names("zzz").is_empty());
        assert_eq!(
            names_in("", Scope::BuiltIn),
            ["[Default folder]", "daily.ts", "hunt.ts"]
        );
        assert_eq!(
            names_in("", Scope::Source(0)),
            ["[community]", "daily.ts", "dage.ts"]
        );
        assert_eq!(
            names_in("daily", Scope::Source(0)),
            ["[community]", "daily.ts"]
        );
        assert!(names_in("hunt", Scope::Source(1)).is_empty());
    }

    struct OpenSources(Entity<ScriptsWindow>);

    impl Render for OpenSources {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let layout = cx.theme().layout;
            let menu = self
                .0
                .update(cx, |scripts, cx| scripts.sources(cx))
                .width(px(320.0))
                .default_open(true);
            div()
                .size_full()
                .bg(cx.theme().background)
                .p(layout.space(4.0))
                .child(div().w(px(140.0)).child(menu))
        }
    }

    #[test]
    fn snapshot_package_sources() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-sources-{:x}", u64::from_ne_bytes(random)));
        let commit = "4e1c2f0".to_owned() + &"0".repeat(33);
        let receipt = format!(
            r#"{{"repository":1,"owner":"toommyliu","repo":"mira","path":"script-packages/dailies","branch":"main","commit":"{commit}","tree":"{commit}","files":{{}}}}"#
        );
        for (file, contents) in [
            (
                "packages/toommyliu-mira-dailies/package.json",
                r#"{"name":"Dailies","mira":{}}"#,
            ),
            ("packages/toommyliu-mira-dailies/scripts/spin.ts", ""),
            (
                "packages/toommyliu-mira-dailies/.mira-install.json",
                receipt.as_str(),
            ),
            (
                "packages/farming/package.json",
                r#"{"name":"Farming","version":"1.2.0","mira":{}}"#,
            ),
            ("packages/farming/scripts/hunt.ts", ""),
            (
                "packages/future/package.json",
                r#"{"name":"Future","mira":{"requires":"9.0.0"}}"#,
            ),
        ] {
            let path = temp.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let size = gpui::size(px(560.0), px(440.0));
        for written in mira_ui::snapshot("scripts-sources-packages", size, move |window, cx| {
            let temp = temp.clone();
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::packages::init(temp.join("packages"), temp.join("github-token"), cx);
            crate::packages::packages(cx).update(cx, |packages, _| {
                packages.mark("toommyliu-mira-dailies", Status::Available)
            });
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
            let library = scan_all(&temp.join("scripts"), &[], &temp.join("packages"));
            scripts.update(cx, |scripts, _| scripts.library = Some(library));
            cx.new(|_| OpenSources(scripts))
        }) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }

    struct OptionsPanels {
        scripts: Entity<ScriptsWindow>,
    }

    impl Render for OptionsPanels {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let layout = cx.theme().layout;
            let this = self.scripts.downgrade();
            let room = self.scripts.read(cx).room.clone();
            let panel = |options: Option<ScriptOptions>, cx: &App| {
                div()
                    .w(px(376.0))
                    .p(layout.space(3.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .child(options_panel(this.clone(), options, room.clone(), cx))
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
                .bg(cx.theme().background)
                .child(panel(Some(pinned), cx))
                .child(panel(Some(public), cx))
                .child(panel(None, cx))
        }
    }

    #[test]
    fn snapshot_options() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-options-{:x}", u64::from_ne_bytes(random)));
        let size = gpui::size(px(1224.0), px(360.0));
        for written in mira_ui::snapshot("scripts-options", size, move |window, cx| {
            let temp = temp.clone();
            crate::settings::init(temp.join("settings.json"), |_| {}, cx);
            crate::packages::init(temp.join("packages"), temp.join("github-token"), cx);
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
            scripts.update(cx, |scripts, cx| {
                scripts
                    .room
                    .update(cx, |room, cx| room.set_text("4242", cx));
            });
            cx.new(|_| OptionsPanels { scripts })
        }) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }

    #[test]
    fn snapshot_nested_library() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-scripts-{:x}", u64::from_ne_bytes(random)));
        for file in [
            "scripts/hunt.ts",
            "scripts/bosses/dage.ts",
            "scripts/legion/daily-quests/ultras/temple.ts",
            "scripts/legion/daily-quests/ultras/void-highlord-farm.ts",
        ] {
            let path = temp.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        for (name, w, h) in [
            ("scripts-nested", WIDTH, HEIGHT),
            ("scripts-nested-min", MIN_WIDTH, MIN_HEIGHT),
        ] {
            for written in mira_ui::snapshot(name, gpui::size(px(w), px(h)), |window, cx| {
                let temp = temp.clone();
                crate::settings::init(temp.join("settings.json"), |_| {}, cx);
                crate::packages::init(temp.join("packages"), temp.join("github-token"), cx);
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
            }) {
                assert!(written.exists(), "{} was not written", written.display());
            }
        }
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
