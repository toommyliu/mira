use std::{
    collections::HashMap,
    fs, io, iter,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, EntityId, FocusHandle,
    Focusable as _, FontWeight, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, PathPromptOptions, Pixels, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, TextRun, TitlebarOptions,
    WeakEntity, Window, WindowBounds, WindowHandle, WindowOptions, actions, div,
    prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, Theme, Tone, h_flex,
    primitives::{
        Alert, Align, Badge, Button, Checkbox, CheckedState, Dot, DropdownMenu, Icon, Input,
        InputEvent, List, ListItem, MenuItem, Popover, Radio, RadioGroup, Scrollbar, Select,
        Switch, ToggleGroup, ToggleItem, VirtualList,
        button::{Size, Variant},
    },
    theme::{LINE_BODY, LINE_SMALL, ROW, TEXT_BODY, TEXT_SMALL},
    v_flex,
};

use super::{
    Changed, Client, RuffleView,
    account::file_name,
    client::CloseSession,
    form::{LABEL_WIDTH, note, row, row_label, switch_row},
    script_form::{
        Draft, FieldDraft, Queued, ROOMS, SPECIFIC_ROOM, TabState, drafts, error, room_choice,
        room_note, starting, values,
    },
    shown_tab::{self, ShownTab, Target},
};
use crate::{
    account_settings::{self, RoomNumber, Rooms, Scripts as ScriptOptions},
    accounts::AccountId,
    script_inputs,
    scripting::{
        self, Definition, ExtractError, Kind, Launch, SCRIPT_EXTENSIONS, Supplied, Values,
    },
    settings,
};

actions!(mira, [OpenScripts]);

const WIDTH: f32 = 720.0;
const HEIGHT: f32 = 500.0;
const MIN_WIDTH: f32 = 600.0;
const MIN_HEIGHT: f32 = 360.0;
const PADDING: Pixels = px(16.0);
const GAP: Pixels = px(20.0);
const LIST_WIDTH: Pixels = px(290.0);
const SEARCH_WIDTH: Pixels = px(260.0);
const PANES_WIDTH: Pixels = px(180.0);
const RUN_WIDTH: Pixels = px(80.0);
const QUEUE_RUN_WIDTH: Pixels = px(104.0);
const FILTER_WIDTH: Pixels = px(300.0);
const FILTER_HEIGHT: Pixels = px(280.0);
const FILTER_PATH_WIDTH: Pixels = px(220.0);
const RING_ROOM: Pixels = px(6.0);
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
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    root: PathBuf,
    name: SharedString,
    kind: SourceKind,
    readable: bool,
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
                source.readable = false;
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
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let source = Source {
            root: root.clone(),
            name: name.into(),
            kind: SourceKind::Folder,
            readable: true,
            scripts: 0,
        };
        library.add(source, scan(root));
    }
    for root in package_roots(packages) {
        let (source, scripts) = package(root);
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
        .filter(|path| path.join("package.json").is_file())
        .collect();
    roots.sort();
    roots
}

fn package(root: PathBuf) -> (Source, io::Result<Vec<Entry>>) {
    let folder_name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let manifest = fs::read(root.join("package.json")).and_then(|bytes| {
        serde_json::from_slice::<serde_json::Value>(&bytes).map_err(io::Error::from)
    });
    let text = |manifest: &serde_json::Value, key: &str| {
        manifest
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(|text| SharedString::from(text.to_owned()))
    };
    let (name, kind, scripts) = match manifest {
        Ok(manifest) => {
            let scripts = match scan(&root.join("scripts")) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
                scanned => scanned,
            };
            (
                text(&manifest, "name").unwrap_or_else(|| folder_name.into()),
                SourceKind::Package {
                    version: text(&manifest, "version"),
                    description: text(&manifest, "description"),
                },
                scripts,
            )
        }
        Err(error) => (
            folder_name.into(),
            SourceKind::Package {
                version: None,
                description: None,
            },
            Err(error),
        ),
    };
    let source = Source {
        root,
        name,
        kind,
        readable: true,
        scripts: 0,
    };
    (source, scripts)
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
        } else if runnable(name) {
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

fn runnable(name: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    SCRIPT_EXTENSIONS.contains(&extension) && !stem.ends_with(".d")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    Library,
    Queue,
}

struct Open(WindowHandle<ScriptsWindow>);

impl gpui::Global for Open {}

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
    room_pending: bool,
    room_error: bool,
    selected: usize,
    filter: Option<PathBuf>,
    pane: Pane,
    queued: usize,
    chosen: Option<PathBuf>,
    forms: HashMap<PathBuf, Form>,
    generation: u64,
    loading: Option<(PathBuf, Task<()>)>,
    waiting: Option<(PathBuf, Instant, Task<()>)>,
    displayed: Option<Entry>,
    detail_scroll: ScrollHandle,
    tabs: HashMap<EntityId, TabState>,
    _view_changes: Option<Subscription>,
    _subscriptions: [Subscription; 3],
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
        let room = cx.new(|cx| Input::new("Room number", cx).max_length(5));
        let subscriptions = [
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
                let number = room.read(cx).text().to_owned();
                match event {
                    InputEvent::Submitted if RoomNumber::parse(&number).is_some() => {
                        window.blur(cx)
                    }
                    InputEvent::Submitted => this.room_error = true,
                    InputEvent::Changed => {
                        this.room_error = room_note(&number).is_err();
                        if this.room_choice(cx) == SPECIFIC_ROOM {
                            this.choose_room_number(&number, cx);
                        }
                    }
                }
                this.show_room_error(cx);
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
            room_pending: false,
            room_error: false,
            selected: 0,
            filter: None,
            pane: Pane::Library,
            queued: 0,
            chosen: None,
            forms: HashMap::new(),
            generation: 0,
            loading: None,
            waiting: None,
            displayed: None,
            detail_scroll: ScrollHandle::new(),
            tabs: HashMap::new(),
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
        self.room_pending = false;
        self.room_error = false;
        let number = match self.options(cx).map(|options| options.rooms) {
            Some(Rooms::Specific { number }) => number.get().to_string(),
            _ => String::new(),
        };
        self.room.update(cx, |input, cx| input.set_text(number, cx));
        self.show_room_error(cx);
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

    fn tab_id(&self) -> Option<EntityId> {
        self.shown.as_ref().map(|shown| shown.id)
    }

    fn running(&self, cx: &App) -> bool {
        self.view()
            .is_some_and(|view| view.read(cx).script.running())
    }

    fn account(&self, cx: &App) -> Option<AccountId> {
        self.view()?.read(cx).settings_account.clone()
    }

    fn status(&self, theme: &Theme, cx: &App) -> (SharedString, Hsla) {
        if self.shown.is_none() {
            return ("No tabs open".into(), theme.text_tertiary);
        }
        let Some(view) = self.view() else {
            return ("Nothing running".into(), theme.text_tertiary);
        };
        let script = &view.read(cx).script;
        match script.status() {
            Some(text) if script.running() => (text.into(), theme.accent_text),
            Some(text) if script.failed() => (text.into(), theme.danger_text),
            Some(text) => (text.into(), theme.text_tertiary),
            None => ("Nothing running".into(), theme.text_tertiary),
        }
    }

    fn show_room_error(&mut self, cx: &mut Context<Self>) {
        let invalid = self.room_choice(cx) == SPECIFIC_ROOM && self.room_error;
        self.room
            .update(cx, |input, cx| input.set_invalid(invalid, cx));
        cx.notify();
    }

    fn options(&self, cx: &App) -> Option<ScriptOptions> {
        let account = self.account(cx)?;
        Some(account_settings::get(&account, cx).scripts)
    }

    fn change_options(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut ScriptOptions)) {
        if let Some(account) = self.account(cx) {
            account_settings::change(&account, cx, |settings| edit(&mut settings.scripts));
        }
        cx.notify();
    }

    fn room_choice(&self, cx: &App) -> usize {
        if self.room_pending {
            return SPECIFIC_ROOM;
        }
        room_choice(self.options(cx).unwrap_or_default().rooms)
    }

    fn choose_room(&mut self, choice: usize, window: &mut Window, cx: &mut Context<Self>) {
        let rooms = match choice {
            0 => Rooms::Public,
            1 => Rooms::RandomPrivate,
            _ => {
                let number = self.room.read(cx).text().to_owned();
                self.room_error = room_note(&number).is_err();
                self.choose_room_number(&number, cx);
                window.focus(&self.room.focus_handle(cx), cx);
                return self.show_room_error(cx);
            }
        };
        self.room_pending = false;
        self.change_options(cx, |options| options.rooms = rooms);
        self.show_room_error(cx);
    }

    fn choose_room_number(&mut self, number: &str, cx: &mut Context<Self>) {
        let Some(number) = RoomNumber::parse(number) else {
            self.room_pending = true;
            return;
        };
        self.room_pending = false;
        self.change_options(cx, |options| options.rooms = Rooms::Specific { number });
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        let built_in = self.folder.clone();
        let added = settings::script_folders(cx).to_vec();
        let packages = self.packages.clone();
        let types = self.types.clone();
        let scanned = cx.background_spawn(async move {
            let folders = iter::once(&built_in).chain(&added);
            scripting::link_types(folders.map(PathBuf::as_path), &types);
            scan_all(&built_in, &added, &packages)
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
        if self.running(cx) || self.room_pending {
            return;
        }
        let (Some(view), Some((launch, _))) = (self.view(), self.launch(cx)) else {
            return;
        };
        view.update(cx, |view, cx| view.start_script(launch, cx));
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.view() {
            view.update(cx, |view, cx| view.cancel_script(cx));
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
                        .font_weight(FontWeight::NORMAL)
                        .pressed(self.filter.is_some())
                        .end_slot(
                            div()
                                .text_color(theme.text_tertiary)
                                .child(Icon::ChevronDown),
                        ),
                )
                .align(Align::End)
                .width(FILTER_WIDTH)
                .max_height(FILTER_HEIGHT)
                .items(move |window, cx| {
                    this.upgrade()
                        .map(|scripts| scripts.read(cx).filter_items(&this, window))
                        .unwrap_or_default()
                }),
        )
    }

    fn filter_items(&self, this: &WeakEntity<Self>, window: &Window) -> Vec<MenuItem> {
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
                .description(fit_path_to(&home_relative(root), FILTER_PATH_WIDTH, window))
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
            let count: SharedString = if added.readable {
                added.scripts.to_string().into()
            } else {
                "Can't read".into()
            };
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
        let unreadable = self
            .library
            .as_ref()
            .is_some_and(|library| library.sources.iter().any(|source| !source.readable));
        DropdownMenu::new("sources")
            .trigger(
                Button::new("sources", "Sources")
                    .when(unreadable, |button| {
                        button.start_slot(Dot::new(Tone::Danger))
                    })
                    .end_slot(Icon::ChevronDown),
            )
            .items(move |_, cx| {
                this.upgrade()
                    .map(|scripts| scripts.read(cx).source_items(&this))
                    .unwrap_or_default()
            })
    }

    fn source_items(&self, this: &WeakEntity<Self>) -> Vec<MenuItem> {
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
                    let item = if source.readable {
                        MenuItem::submenu(path, vec![show(&source.root), remove(&source.root)])
                    } else {
                        MenuItem::submenu(path, vec![locate(&source.root), remove(&source.root)])
                            .start_slot(Dot::new(Tone::Danger))
                            .description("Can't read")
                    };
                    items.push(item);
                }
                SourceKind::Package { version, .. } => {
                    let label: SharedString = match version {
                        Some(version) => format!("{} {version}", source.name).into(),
                        None => source.name.clone(),
                    };
                    packages.push(MenuItem::submenu(
                        label,
                        vec![
                            show(&source.root),
                            MenuItem::action("Update", |_, _| {}).disabled(true),
                            MenuItem::action("Move to Trash", |_, _| {}).disabled(true),
                        ],
                    ));
                }
            }
        }
        let add = this.clone();
        items.push(MenuItem::action("Add folder…", move |_, cx| {
            let _ = add.update(cx, |this, cx| this.add_folder(cx));
        }));
        items.push(MenuItem::separator());
        items.push(MenuItem::header("Packages"));
        items.extend(packages);
        items.push(MenuItem::action("Install package…", |_, _| {}).disabled(true));
        items.push(MenuItem::action("Check for updates", |_, _| {}).disabled(true));
        items
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

    fn load_form(&mut self, path: &Path, cx: &mut Context<Self>) {
        let current = self
            .forms
            .get(path)
            .is_some_and(|form| form.generation == self.generation);
        let pending = self
            .loading
            .as_ref()
            .is_some_and(|(loading, _)| loading == path);
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
        let path = path.to_owned();
        let generation = self.generation;
        let task = cx.spawn({
            let path = path.clone();
            async move |this, cx| {
                let read = read.await;
                let _ = this.update(cx, |this, cx| {
                    this.loading = None;
                    this.loaded(path, generation, read, cx);
                    cx.notify();
                });
            }
        });
        if !self.forms.contains_key(&path) {
            let wake = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(PATIENCE).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            });
            self.waiting = Some((path.clone(), Instant::now(), wake));
        }
        self.loading = Some((path, task));
    }

    fn detail_entry(&mut self, target: Option<Entry>) -> Option<Entry> {
        let target = target?;
        if self.forms.contains_key(&target.path) {
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
            Some(previous) if patient && self.forms.contains_key(&previous.path) => {
                Some(previous.clone())
            }
            _ => Some(target),
        }
    }

    fn loaded(
        &mut self,
        path: PathBuf,
        generation: u64,
        read: Result<Option<Definition>, (&'static str, String)>,
        cx: &mut Context<Self>,
    ) {
        if let Some(form) = self.forms.get_mut(&path)
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
                let saved = match script_inputs::saved(&path, cx) {
                    Supplied::Values(values) => values,
                    Supplied::StoreUnreadable => Values::new(),
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
                    let path = path.clone();
                    changes.push(cx.subscribe(
                        &input,
                        move |this, input, event: &InputEvent, cx| {
                            if matches!(event, InputEvent::Changed) {
                                let text = input.read(cx).text().to_owned().into();
                                this.set_draft(&path, index, Draft::Text(text), cx);
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
        self.forms.insert(path, Form { generation, state });
    }

    fn set_draft(&mut self, path: &Path, index: usize, draft: Draft, cx: &mut Context<Self>) {
        let Some(Form {
            state:
                FormState::Fields {
                    definition,
                    drafts,
                    texts,
                    ..
                },
            ..
        }) = self.forms.get_mut(path)
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
        cx.notify();
    }

    fn reset_form(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(Form {
            state:
                FormState::Fields {
                    definition,
                    drafts,
                    texts,
                    ..
                },
            ..
        }) = self.forms.get_mut(path)
        else {
            return;
        };
        for ((field, draft), input) in definition.fields().iter().zip(drafts).zip(texts) {
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
        cx.notify();
    }

    fn launch_values(&self, path: &Path, cx: &App) -> Option<(Option<&Definition>, Values)> {
        match &self.forms.get(path)?.state {
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

    fn launch(&mut self, cx: &mut Context<Self>) -> Option<(Launch, Queued)> {
        let entry = self.target(cx)?;
        let (definition, values) = self.launch_values(&entry.path, cx)?;
        let launch = Launch {
            entry: entry.path.clone(),
            inputs: Supplied::Values(values.clone()),
        };
        let queued = Queued::new(entry.name, launch.clone(), definition, &values);
        if definition.is_some() {
            script_inputs::save(&entry.path, values, cx);
        }
        Some((launch, queued))
    }

    fn tab_state(&self) -> Option<&TabState> {
        self.tabs.get(&self.tab_id()?)
    }

    fn tab_state_mut(&mut self) -> Option<&mut TabState> {
        let id = self.tab_id()?;
        Some(self.tabs.entry(id).or_default())
    }

    fn queue(&self) -> &[Queued] {
        self.tab_state()
            .map_or(&[][..], |state| state.queue.as_slice())
    }

    fn queue_running(&self) -> bool {
        self.tab_state().is_some_and(|state| state.queue_running)
    }

    fn add_to_queue(&mut self, cx: &mut Context<Self>) {
        let Some((_, queued)) = self.launch(cx) else {
            return;
        };
        if let Some(state) = self.tab_state_mut() {
            state.queue.push(queued);
        }
        cx.notify();
    }

    fn move_queued(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let running = self.queue_running();
        let Some(state) = self.tab_state_mut() else {
            return;
        };
        let queue = &mut state.queue;
        if from >= queue.len() || to >= queue.len() || (running && (from == 0 || to == 0)) {
            return;
        }
        let moved = queue.remove(from);
        queue.insert(to, moved);
        self.queued = to;
        cx.notify();
    }

    fn remove_queued(&mut self, cx: &mut Context<Self>) {
        let index = self.queued;
        let Some(state) = self.tab_state_mut() else {
            return;
        };
        if index >= state.queue.len() {
            return;
        }
        state.queue.remove(index);
        state.queue_running &= !state.queue.is_empty();
        let remaining = state.queue.len();
        self.queued = index.min(remaining.saturating_sub(1));
        cx.notify();
    }

    fn clear_queue(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.tab_state_mut() else {
            return;
        };
        state.queue.clear();
        state.queue_running = false;
        self.queued = 0;
        cx.notify();
    }

    fn toggle_queue(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.tab_state_mut() {
            state.queue_running = !state.queue_running && !state.queue.is_empty();
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
                let options = scripts.options(cx);
                let editable = options.is_some();
                let options = options.unwrap_or_default();
                let room = scripts.room_choice(cx);
                let room_error = scripts.room_error;
                let room_hint = room_note(scripts.room.read(cx).text());
                let room_input = scripts.room.clone();
                let choose_room = this.clone();
                let toggle = |set: fn(&mut ScriptOptions, bool)| {
                    let this = this.clone();
                    move |on, _: &mut Window, cx: &mut App| {
                        let _ = this.update(cx, |this, cx| {
                            this.change_options(cx, |options| set(options, on));
                        });
                    }
                };
                let theme = cx.theme().clone();
                let (Ok(hint) | Err(hint)) = room_hint;
                let radios = ROOMS
                    .into_iter()
                    .enumerate()
                    .map(|(index, label)| {
                        let radio = Radio::new(label);
                        let radio = if editable { radio } else { radio.disabled() };
                        match index {
                            SPECIFIC_ROOM if room == SPECIFIC_ROOM => radio
                                .description(hint)
                                .invalid(room_error)
                                .accessory(div().w(px(80.0)).child(room_input.clone())),
                            SPECIFIC_ROOM => radio.description(hint),
                            _ => radio,
                        }
                    })
                    .collect();
                v_flex()
                    .gap(px(8.0))
                    .when(!editable, |options| {
                        options.child(note("Log in to change script settings.", true, &theme))
                    })
                    .child(
                        v_flex()
                            .gap(px(4.0))
                            .child(switch_row(
                                "Safe start and stop",
                                editable,
                                Switch::new(
                                    "safe-start",
                                    "Safe start and stop",
                                    options.safe_start_stop,
                                )
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
                            .gap(px(4.0))
                            .child(switch_row(
                                "Restart after reconnecting",
                                editable,
                                Switch::new(
                                    "restart",
                                    "Restart after reconnecting",
                                    options.restart_after_reconnect,
                                )
                                .on_toggle(toggle(
                                    |options, on| options.restart_after_reconnect = on,
                                )),
                            ))
                            .child(note(
                                "Restart the active script when the same account reconnects.",
                                editable,
                                &theme,
                            )),
                    )
                    .child(div().h(px(1.0)).my(px(4.0)).bg(theme.hairline))
                    .child(
                        h_flex()
                            .items_start()
                            .gap(px(12.0))
                            .child(
                                h_flex()
                                    .flex_none()
                                    .w(px(64.0))
                                    .h(ROW)
                                    .child(row_label("Rooms", editable)),
                            )
                            .child(div().flex_1().min_w_0().pt(px(4.0)).child(
                                RadioGroup::new("rooms", "Rooms", radios, Some(room)).on_select(
                                    move |room, window, cx| {
                                        let _ = choose_room.update(cx, |this, cx| {
                                            this.choose_room(room, window, cx)
                                        });
                                    },
                                ),
                            )),
                    )
                    .into_any_element()
            })
            .width(px(376.0))
    }

    fn inputs(&self, theme: &Theme, path: &Path, cx: &mut Context<Self>) -> AnyElement {
        let Some(form) = self.forms.get(path) else {
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
        let unreadable = script_inputs::unreadable(cx).then(|| {
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
            let path = path.to_owned();
            h_flex()
                .justify_between()
                .gap(px(12.0))
                .child(muted(
                    theme,
                    "Some saved values don't fit this script anymore.",
                ))
                .child(
                    Button::new("inputs-reset", "Reset")
                        .size(Size::Small)
                        .on_click(move |_, _, cx| {
                            let _ = this.update(cx, |this, cx| this.reset_form(&path, cx));
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
                        let path = path.to_owned();
                        move |draft: Draft, cx: &mut App| {
                            let _ =
                                this.update(cx, |this, cx| this.set_draft(&path, index, draft, cx));
                        }
                    };
                    let label = SharedString::from(field.label.clone());
                    let control = match (&field.kind, &draft.draft, &texts[index]) {
                        (Kind::Boolean { .. }, Draft::Toggle(on), _) => switch_row(
                            label.clone(),
                            true,
                            Switch::new(("input", index), label, *on)
                                .on_toggle(move |on, _, cx| set(Draft::Toggle(on), cx)),
                        ),
                        (Kind::Select { options, .. }, Draft::Choice(chosen), _) => row(
                            label.clone(),
                            true,
                            Select::new(
                                ("input", index),
                                label,
                                options.iter().cloned().map(SharedString::from),
                                *chosen,
                            )
                            .placeholder("Choose…")
                            .full_width()
                            .on_select(move |chosen, _, cx| set(Draft::Choice(Some(chosen)), cx)),
                        ),
                        (Kind::MultiSelect { options, .. }, Draft::Choices(chosen), _) => {
                            let set = Rc::new(set);
                            h_flex()
                                .items_start()
                                .gap(px(12.0))
                                .child(
                                    h_flex()
                                        .flex_none()
                                        .w(LABEL_WIDTH)
                                        .h(ROW)
                                        .child(row_label(label, true)),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap(px(6.0))
                                        .py(px(4.0))
                                        .children(options.iter().enumerate().map(
                                            |(option, name)| {
                                                let set = set.clone();
                                                let chosen = chosen.clone();
                                                let state = if chosen[option] {
                                                    CheckedState::Checked
                                                } else {
                                                    CheckedState::Unchecked
                                                };
                                                Checkbox::new(
                                                    SharedString::from(format!(
                                                        "input-{index}-{option}"
                                                    )),
                                                    name.clone(),
                                                    state,
                                                )
                                                .on_toggle(move |on, _, cx| {
                                                    let mut chosen = chosen.clone();
                                                    chosen[option] = on;
                                                    set(Draft::Choices(chosen), cx);
                                                })
                                            },
                                        )),
                                )
                        }
                        (kind, _, Some(input)) => row(
                            label,
                            true,
                            div()
                                .when(matches!(kind, Kind::Number { .. }), |slot| slot.w(px(80.0)))
                                .child(input.clone()),
                        ),
                        _ => div(),
                    };
                    v_flex().gap(px(4.0)).child(control).map(|column| {
                        match (error(field, draft), &field.description) {
                            (Some(problem), _) => column
                                .child(note(problem, true, theme).text_color(theme.danger_text)),
                            (None, Some(description)) => {
                                column.child(note(description.clone(), true, theme))
                            }
                            (None, None) => column,
                        }
                    })
                });
        v_flex()
            .gap(px(8.0))
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
        let path = fit_path_to(&home_relative(&entry.path), width, window);
        v_flex()
            .gap(px(14.0))
            .child(heading(theme, entry.name.clone(), path, None))
            .child(self.inputs(theme, &entry.path, cx))
            .into_any_element()
    }

    fn queued_detail(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let queue = self.queue();
        let Some(queued) = queue.get(self.queued) else {
            return div().into_any_element();
        };
        let index = self.queued;
        let current = self.queue_running() && index == 0;
        let position: SharedString = if current {
            "Running now".into()
        } else {
            format!("{} of {} in the queue", index + 1, queue.len()).into()
        };
        v_flex()
            .gap(px(14.0))
            .child(heading(
                theme,
                queued.name.clone(),
                position,
                Some(
                    Button::new("queued-remove", "Remove")
                        .size(Size::Small)
                        .variant(Variant::Ghost)
                        .disabled(current)
                        .on_click(cx.listener(|this, _, _, cx| this.remove_queued(cx)))
                        .into_any_element(),
                ),
            ))
            .child(if queued.rows.is_empty() {
                muted(theme, "This script has no inputs.").into_any_element()
            } else {
                v_flex()
                    .gap(px(8.0))
                    .children(queued.rows.iter().map(|(label, value)| {
                        row(label.clone(), true, div().truncate().child(value.clone()))
                    }))
                    .into_any_element()
            })
            .into_any_element()
    }

    fn queue_list(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let queue = self.queue();
        if queue.is_empty() {
            return empty_frame(theme)
                .child("Nothing queued. Pick a script in Library and choose Add to queue.")
                .into_any_element();
        }
        let running = self.queue_running();
        let accent = theme.accent_text;
        let names: Rc<[SharedString]> = queue.iter().map(|queued| queued.name.clone()).collect();
        let this = cx.entity().downgrade();
        let that = this.clone();
        let list = List::new("queue", "Queue", names.len(), move |index| {
            let item = ListItem::new(names[index].clone());
            if running && index == 0 {
                item.end_slot(div().text_color(accent).child("Running"))
            } else {
                item
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
        .pin_first(usize::from(running));
        v_flex().size_full().child(list).into_any_element()
    }

    fn library(&self, theme: &Theme, rows: Rc<[Row]>, cx: &mut Context<Self>) -> AnyElement {
        let note = || empty_frame(theme);
        let Some(library) = &self.library else {
            return note().into_any_element();
        };
        if library.entries.is_empty() && library.sources.is_empty() {
            let folder = self.folder.clone();
            return note()
                .child(
                    div()
                        .text_size(TEXT_BODY)
                        .line_height(LINE_BODY)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child("No scripts yet"),
                )
                .child(format!(
                    "Put .ts or .js files in {}, or add a folder you already have.",
                    home_relative(&self.folder)
                ))
                .child(
                    div()
                        .flex()
                        .gap(px(8.0))
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
                        Some(folder) => ListItem::new(entry.name.clone()).end_slot(folder.clone()),
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
    if !source.readable {
        return "Can't read".into();
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

pub(super) fn fit_path_to(path: &str, width: Pixels, window: &Window) -> SharedString {
    let font = window.text_style().font();
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
            .shape_line(text.to_owned().into(), TEXT_SMALL, &[run], None)
            .width
            <= width
    })
}

impl Render for ScriptsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let has_tab = self.shown.is_some();
        let rows = self.rows(cx);
        let chosen = self.target(cx);
        if let Some(entry) = &chosen {
            self.load_form(&entry.path, cx);
        }
        let chosen = self.detail_entry(chosen);
        self.queued = self.queued.min(self.queue().len().saturating_sub(1));
        let valid = chosen
            .as_ref()
            .is_some_and(|entry| self.launch_values(&entry.path, cx).is_some());
        let running = self.running(cx);
        let queued = self.queue().len();
        let queue_running = self.queue_running();
        let room_ready = !self.room_pending;
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
                .flex_none()
                .max_w(px(360.0))
                .pr(PADDING)
                .truncate()
                .text_size(TEXT_SMALL)
                .text_color(status_color)
                .child(status),
        );
        let this = cx.entity().downgrade();
        let panes = ToggleGroup::new(
            "pane",
            "Show",
            [
                ToggleItem::new("Library"),
                if queued == 0 {
                    ToggleItem::new("Queue")
                } else {
                    ToggleItem::new("Queue").end_slot(Badge::new(queued.to_string()))
                },
            ],
            match self.pane {
                Pane::Library => 0,
                Pane::Queue => 1,
            },
        )
        .full_width()
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
        let toolbar = h_flex()
            .flex_none()
            .gap(px(12.0))
            .px(PADDING)
            .pt(px(14.0))
            .child(div().w(PANES_WIDTH).child(panes))
            .child(div().flex_1())
            .when(library, |toolbar| {
                toolbar.child(
                    h_flex()
                        .gap(px(6.0))
                        .children(self.filter(&theme, cx))
                        .child(search),
                )
            });
        let detail_width = window.viewport_size().width - PADDING * 2.0 - LIST_WIDTH - GAP;
        let (list, detail) = match self.pane {
            Pane::Library => (
                self.library(&theme, rows, cx),
                self.script_detail(&theme, chosen.as_ref(), detail_width, window, cx),
            ),
            Pane::Queue => (self.queue_list(&theme, cx), self.queued_detail(&theme, cx)),
        };
        let body = h_flex()
            .flex_1()
            .min_h_0()
            .items_start()
            .gap(GAP)
            .p(PADDING)
            .child(v_flex().flex_none().w(LIST_WIDTH).h_full().child(list))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .ml(-RING_ROOM)
                    .mr(-Scrollbar::GUTTER)
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
        let footer = h_flex()
            .flex_none()
            .gap(px(6.0))
            .px(PADDING)
            .py(px(10.0))
            .border_t_1()
            .border_color(theme.hairline)
            .child(self.options_popover(cx));
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
                        .end_slot(key_hint("⌘⏎"))
                        .disabled(!has_tab || !valid)
                        .on_click(cx.listener(|this, _, _, cx| this.add_to_queue(cx))),
                )
                .child(div().flex_none().w(RUN_WIDTH).child(if running {
                    Button::new("stop", "Stop")
                        .on_click(cx.listener(|this, _, _, cx| this.stop(cx)))
                } else {
                    Button::new("run", "Run")
                        .end_slot(key_hint("⏎"))
                        .variant(Variant::Primary)
                        .disabled(!has_tab || !valid || !room_ready)
                        .on_click(cx.listener(|this, _, _, cx| this.run_selected(cx)))
                })),
            Pane::Queue => footer
                .child(
                    Button::new("queue-clear", "Clear queue")
                        .variant(Variant::Ghost)
                        .disabled(queued == 0)
                        .on_click(cx.listener(|this, _, _, cx| this.clear_queue(cx))),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .w(QUEUE_RUN_WIDTH)
                        .child(if queue_running {
                            Button::new("queue-stop", "Stop queue")
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_queue(cx)))
                        } else {
                            Button::new("queue-start", "Start queue")
                                .variant(Variant::Primary)
                                .disabled(!has_tab || queued == 0 || !room_ready)
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_queue(cx)))
                        }),
                ),
        };
        v_flex()
            .size_full()
            .bg(theme.base)
            .text_size(TEXT_BODY)
            .text_color(theme.text)
            .key_context(FOCUS_SCOPE)
            .id("scripts")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseSession, window, _| window.remove_window()))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let keystroke = &event.keystroke;
                if this.pane == Pane::Library
                    && keystroke.key == "enter"
                    && keystroke.modifiers.platform
                {
                    this.add_to_queue(cx);
                    cx.stop_propagation();
                }
            }))
            .child(mira_ui::nav::pointer_probe())
            .child(header)
            .child(toolbar)
            .child(body)
            .child(footer)
    }
}

fn heading(
    theme: &Theme,
    name: SharedString,
    detail: SharedString,
    action: Option<AnyElement>,
) -> impl IntoElement {
    h_flex()
        .items_start()
        .gap(px(8.0))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(TEXT_SMALL)
                        .line_height(LINE_SMALL)
                        .text_color(theme.text_tertiary)
                        .child(detail),
                ),
        )
        .children(action)
}

fn empty_frame(theme: &Theme) -> gpui::Div {
    v_flex()
        .size_full()
        .gap(px(10.0))
        .p(px(12.0))
        .rounded(theme.radius.panel)
        .bg(theme.surface)
        .border_1()
        .border_color(theme.hairline)
        .text_size(TEXT_SMALL)
        .line_height(LINE_SMALL)
        .text_color(theme.text_secondary)
}

fn key_hint(keys: &'static str) -> impl IntoElement {
    div().text_size(TEXT_SMALL).opacity(0.55).child(keys)
}

fn muted(theme: &Theme, text: &'static str) -> impl IntoElement {
    div()
        .text_size(TEXT_BODY)
        .line_height(LINE_BODY)
        .text_color(theme.text_tertiary)
        .child(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_lists_runnable_scripts_grouped_by_source() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-scripts-{:x}", u64::from_ne_bytes(random)));
        let built_in = temp.join("scripts");
        let community = temp.join("community");
        let missing = temp.join("unplugged");
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
                "packages/dailies/package.json",
                r#"{"name":"@lucent/dailies","version":"0.0.3","description":"Daily runs."}"#,
            ),
            ("packages/dailies/scripts/spin.ts", ""),
            ("packages/dailies/lib/wheel.ts", ""),
            ("packages/broken/package.json", "{"),
            ("packages/not-a-package/loose.ts", ""),
        ] {
            let path = temp.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        let library = scan_all(&built_in, &[community.clone(), missing.clone()], &packages);
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
                (Some(3), None, "spin.ts"),
            ]
        );
        let sources: Vec<(&Path, &str, bool, usize)> = library
            .sources
            .iter()
            .map(|source| {
                (
                    source.root.as_path(),
                    source.name.as_ref(),
                    source.readable,
                    source.scripts,
                )
            })
            .collect();
        assert_eq!(
            sources,
            [
                (community.as_path(), "community", true, 2),
                (missing.as_path(), "unplugged", false, 0),
                (packages.join("broken").as_path(), "broken", false, 0),
                (
                    packages.join("dailies").as_path(),
                    "@lucent/dailies",
                    true,
                    1
                ),
            ]
        );
        assert_eq!(
            library.sources[3].kind,
            SourceKind::Package {
                version: Some("0.0.3".into()),
                description: Some("Daily runs.".into()),
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
            readable: true,
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
