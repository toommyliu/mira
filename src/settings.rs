use mira_ui::StyledExt as _;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Global, Hsla,
    InteractiveElement as _, IntoElement, KeyBinding, Keystroke, Menu, MenuItem,
    ParentElement as _, Render, Role, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, TitlebarOptions, Window, WindowAppearance, WindowBounds,
    WindowHandle, WindowOptions, actions, div, point, prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, ColorToken, FOCUS_SCOPE, Theme,
    components::{Disclosure, Empty, Group, ShortcutField},
    h_flex,
    primitives::{
        Button, ColorPicker, Dialog, Icon, Input, InputEvent, Numeric, Radio, RadioGroup,
        Scrollbar, Select, Switch, Tab, Tabs,
        button::{Size, Variant as ButtonVariant},
    },
    theme::Typography,
    theme::{UI_FONT, parse_hex, to_hex},
    v_flex,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

mod previews;

use crate::{
    packages,
    ruffle_view::{CloseSession, bar_title, drag_area, window_bar},
    shortcuts::{self, Category, Overrides, Shortcut, Taken},
};

actions!(
    settings,
    [
        OpenSettings,
        UseSystemAppearance,
        UseLightAppearance,
        UseDarkAppearance
    ]
);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];

    fn label(self) -> &'static str {
        match self {
            Appearance::System => "System",
            Appearance::Light => "Light",
            Appearance::Dark => "Dark",
        }
    }

    fn variants(self) -> &'static [Variant] {
        match self {
            Appearance::System => &Variant::ALL,
            Appearance::Light => &[Variant::Light],
            Appearance::Dark => &[Variant::Dark],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    Light,
    Dark,
}

impl Variant {
    const ALL: [Variant; 2] = [Variant::Light, Variant::Dark];

    fn label(self) -> &'static str {
        match self {
            Variant::Light => "Light",
            Variant::Dark => "Dark",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Hex(Hsla);

impl Serialize for Hex {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&to_hex(self.0))
    }
}

impl<'de> Deserialize<'de> for Hex {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse_hex(&text)
            .map(Hex)
            .ok_or_else(|| serde::de::Error::custom(format!("{text} isn't a #RRGGBB color")))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
struct FontSize<const MIN: u8, const MAX: u8, const DEFAULT: u8>(u8);

type TextSize = FontSize<11, 16, 13>;
type MonoSize = FontSize<9, 14, 11>;

impl<const MIN: u8, const MAX: u8, const DEFAULT: u8> FontSize<MIN, MAX, DEFAULT> {
    fn parse(text: &str) -> Option<Self> {
        text.trim()
            .parse::<u8>()
            .ok()
            .filter(|size| (MIN..=MAX).contains(size))
            .map(Self)
    }

    fn pixels(self) -> gpui::Pixels {
        px(f32::from(self.0))
    }
}

impl<const MIN: u8, const MAX: u8, const DEFAULT: u8> Default for FontSize<MIN, MAX, DEFAULT> {
    fn default() -> Self {
        Self(DEFAULT)
    }
}

impl<'de, const MIN: u8, const MAX: u8, const DEFAULT: u8> Deserialize<'de>
    for FontSize<MIN, MAX, DEFAULT>
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(u8::deserialize(deserializer)?.clamp(MIN, MAX)))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Colors {
    light: BTreeMap<ColorToken, Hex>,
    dark: BTreeMap<ColorToken, Hex>,
}

impl Colors {
    fn of(&self, variant: Variant) -> &BTreeMap<ColorToken, Hex> {
        match variant {
            Variant::Light => &self.light,
            Variant::Dark => &self.dark,
        }
    }

    fn of_mut(&mut self, variant: Variant) -> &mut BTreeMap<ColorToken, Hex> {
        match variant {
            Variant::Light => &mut self.light,
            Variant::Dark => &mut self.dark,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Start {
    #[default]
    Client,
    Manager,
}

impl Start {
    const ALL: [Start; 2] = [Start::Client, Start::Manager];

    fn label(self) -> &'static str {
        match self {
            Start::Client => "Client",
            Start::Manager => "Manager",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    start: Start,
    sidebar: bool,
    appearance: Appearance,
    pointer_cursors: bool,
    font_smoothing: bool,
    font: Option<String>,
    mono_font: Option<String>,
    text_size: TextSize,
    mono_size: MonoSize,
    colors: Colors,
    script_folders: Vec<PathBuf>,
    skip_shop_warning: bool,
    hidden_recent_scripts: Vec<PathBuf>,
    #[serde(with = "shortcuts")]
    shortcuts: Overrides,
}

impl Settings {
    fn theme(&self, variant: Variant) -> Theme {
        let colors: Vec<_> = self
            .colors
            .of(variant)
            .iter()
            .map(|(&token, &Hex(color))| (token, color))
            .collect();
        let mut theme = Theme::customized(variant == Variant::Dark, &colors);
        theme.pointer_cursor = self.pointer_cursors;
        if let Some(font) = &self.font {
            theme.font_family = font.clone().into();
        }
        if let Some(font) = &self.mono_font {
            theme.mono_family = font.clone().into();
        }
        theme.typography = Typography {
            mono: self.mono_size.pixels(),
            ..Typography::sized(f32::from(self.text_size.0))
        };
        theme
    }

    fn variant(&self, window: WindowAppearance) -> Variant {
        match self.appearance {
            Appearance::Light => Variant::Light,
            Appearance::Dark => Variant::Dark,
            Appearance::System if Theme::for_window(window).dark_mode => Variant::Dark,
            Appearance::System => Variant::Light,
        }
    }

    fn reset(&mut self, pane: Pane) {
        let Settings {
            appearance,
            pointer_cursors,
            font_smoothing,
            font,
            mono_font,
            text_size,
            mono_size,
            colors,
            shortcuts,
            ..
        } = Settings::default();
        match pane {
            Pane::General => {}
            Pane::Appearance => {
                self.appearance = appearance;
                self.pointer_cursors = pointer_cursors;
                self.font_smoothing = font_smoothing;
                self.font = font;
                self.mono_font = mono_font;
                self.text_size = text_size;
                self.mono_size = mono_size;
                self.colors = colors;
            }
            Pane::Shortcuts => self.shortcuts = shortcuts,
        }
    }

    fn set_color(&mut self, variant: Variant, token: ColorToken, color: Hsla) {
        self.colors.of_mut(variant).remove(&token);
        if to_hex(self.theme(variant).color(token)) != to_hex(color) {
            self.colors.of_mut(variant).insert(token, Hex(color));
        }
    }

    fn load(path: &Path) -> io::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    fn open(path: &Path) -> Option<Self> {
        let settings = match Self::load(path) {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "couldn't read settings");
                return None;
            }
        };
        let current = serde_json::to_vec_pretty(&settings).ok();
        if fs::read(path).ok() != current
            && let Err(error) = settings.save(path)
        {
            tracing::warn!(%error, path = %path.display(), "couldn't save settings");
        }
        Some(settings)
    }

    fn save(&self, path: &Path) -> io::Result<()> {
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(&self)?)?;
        fs::rename(temporary, path)
    }
}

struct Store {
    path: PathBuf,
    settings: Settings,
    on_change: fn(&mut App),
}

impl Global for Store {}

const SETTINGS_WIDTH: f32 = 720.0;
const SETTINGS_HEIGHT: f32 = 600.0;
const SETTINGS_MIN_WIDTH: f32 = 620.0;
const SETTINGS_MIN_HEIGHT: f32 = 400.0;

struct Open(WindowHandle<SettingsWindow>);

impl Global for Open {}

pub(crate) fn init(path: PathBuf, on_change: fn(&mut App), cx: &mut App) {
    let settings = Settings::open(&path).unwrap_or_default();
    cx.set_global(Store {
        path,
        settings,
        on_change,
    });
    apply(cx);
    cx.bind_keys([KeyBinding::new("cmd-,", OpenSettings, None)]);
    cx.on_action(|_: &OpenSettings, cx| cx.defer(open));
    cx.on_action(|_: &UseSystemAppearance, cx| choose(Appearance::System, cx));
    cx.on_action(|_: &UseLightAppearance, cx| choose(Appearance::Light, cx));
    cx.on_action(|_: &UseDarkAppearance, cx| choose(Appearance::Dark, cx));
}

pub(crate) fn follow_system(window: &mut Window) {
    window
        .observe_window_appearance(|_, cx| {
            if cx.global::<Store>().settings.appearance == Appearance::System {
                apply(cx);
            }
        })
        .detach();
}

pub(crate) fn appearance_menu(cx: &App) -> MenuItem {
    let chosen = cx.global::<Store>().settings.appearance;
    MenuItem::submenu(Menu {
        name: "Appearance".into(),
        disabled: false,
        items: vec![
            MenuItem::action("System", UseSystemAppearance).checked(chosen == Appearance::System),
            MenuItem::action("Light", UseLightAppearance).checked(chosen == Appearance::Light),
            MenuItem::action("Dark", UseDarkAppearance).checked(chosen == Appearance::Dark),
        ],
    })
}

pub(crate) fn start(cx: &App) -> Start {
    cx.global::<Store>().settings.start
}

pub(crate) fn sidebar(cx: &App) -> bool {
    cx.global::<Store>().settings.sidebar
}

pub(crate) fn set_sidebar(open: bool, cx: &mut App) {
    change(cx, |settings| settings.sidebar = open);
}

pub(crate) fn script_folders(cx: &App) -> &[PathBuf] {
    &cx.global::<Store>().settings.script_folders
}

pub(crate) fn add_script_folder(folder: PathBuf, cx: &mut App) {
    change(cx, |settings| {
        if !settings.script_folders.contains(&folder) {
            settings.script_folders.push(folder);
        }
    });
}

pub(crate) fn replace_script_folder(old: &Path, new: PathBuf, cx: &mut App) {
    change(cx, |settings| {
        let folders = &mut settings.script_folders;
        if folders.contains(&new) {
            folders.retain(|kept| kept != old);
        } else if let Some(slot) = folders.iter_mut().find(|kept| *kept == old) {
            *slot = new;
        }
    });
}

pub(crate) fn remove_script_folder(folder: &Path, cx: &mut App) {
    change(cx, |settings| {
        settings.script_folders.retain(|kept| kept != folder);
    });
}

pub(crate) fn hidden_recent_scripts(cx: &App) -> &[PathBuf] {
    &cx.global::<Store>().settings.hidden_recent_scripts
}

pub(crate) fn hide_recent_script(script: PathBuf, cx: &mut App) {
    change(cx, |settings| {
        if !settings.hidden_recent_scripts.contains(&script) {
            settings.hidden_recent_scripts.push(script);
        }
    });
}

pub(crate) fn show_recent_script(script: &Path, cx: &mut App) {
    change(cx, |settings| {
        settings
            .hidden_recent_scripts
            .retain(|hidden| hidden != script);
    });
}

pub(crate) fn shop_warning(cx: &App) -> bool {
    !cx.global::<Store>().settings.skip_shop_warning
}

pub(crate) fn set_shop_warning(on: bool, cx: &mut App) {
    change(cx, |settings| settings.skip_shop_warning = !on);
}

pub(crate) fn shortcuts(cx: &App) -> &Overrides {
    &cx.global::<Store>().settings.shortcuts
}

fn set_shortcut(shortcut: Shortcut, keys: Option<Keystroke>, cx: &mut App) -> Result<(), Taken> {
    let os = cx.platform_info().os;
    let mut overrides = shortcuts(cx).clone();
    shortcuts::assign(
        &mut overrides,
        shortcut,
        keys.map(|keys| mira_ui::keyboard::pressed(&keys).unparse(os)),
        |keys| shortcuts::taken_by_app(keys, cx),
    )?;
    change(cx, |settings| settings.shortcuts = overrides);
    Ok(())
}

fn choose(appearance: Appearance, cx: &mut App) {
    change(cx, |settings| settings.appearance = appearance);
}

fn change(cx: &mut App, edit: impl FnOnce(&mut Settings)) {
    let store = cx.global_mut::<Store>();
    let before = store.settings.clone();
    edit(&mut store.settings);
    if store.settings == before {
        return;
    }
    if let Err(error) = store.settings.save(&store.path) {
        tracing::warn!(%error, path = %store.path.display(), "couldn't save settings");
    }
    let on_change = store.on_change;
    apply(cx);
    on_change(cx);
}

fn apply(cx: &mut App) {
    let settings = &cx.global::<Store>().settings;
    mira_platform::set_font_smoothing(settings.font_smoothing);
    let theme = settings.theme(settings.variant(cx.window_appearance()));
    cx.set_global(theme);
    cx.refresh_windows();
}

fn reset_button(
    id: impl std::fmt::Debug,
    label: &str,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    Button::icon(
        SharedString::from(format!("{id:?}-reset")),
        Icon::RotateCcw,
        format!("Reset {}", label.to_lowercase()),
    )
    .size(Size::Small)
    .tooltip("Reset")
    .on_click(move |_, window, cx| on_click(window, cx))
}

fn with_reset(reset: Option<Button>, control: impl IntoElement, theme: &Theme) -> gpui::Div {
    h_flex()
        .gap(theme.space(2.0))
        .children(reset)
        .child(control)
}

fn reset_size(input: &Entity<Input>, default: u8) -> impl Fn(&mut Window, &mut App) + 'static {
    let input = input.clone();
    move |_, cx| input.update(cx, |input, cx| input.set_text(default.to_string(), cx))
}

fn sized(input: Entity<Input>, theme: &Theme) -> gpui::Div {
    h_flex()
        .gap(theme.space(1.5))
        .child(div().w(px(48.0)).child(input))
        .child(div().text_color(theme.muted_foreground).child("px"))
}

fn size_input<const MIN: u8, const MAX: u8, const DEFAULT: u8>(
    label: &'static str,
    size: FontSize<MIN, MAX, DEFAULT>,
    apply: fn(&mut Settings, FontSize<MIN, MAX, DEFAULT>),
    cx: &mut Context<SettingsWindow>,
) -> (Entity<Input>, Subscription) {
    let input = cx.new(|cx| {
        let mut input = Input::new(label, cx)
            .numeric(Numeric {
                min: i64::from(MIN),
                max: i64::from(MAX),
                step: 1,
            })
            .small();
        input.set_text(size.0.to_string(), cx);
        input
    });
    let changes = cx.subscribe(&input, move |_, input, event, cx| {
        if !matches!(event, InputEvent::Changed) {
            return;
        }
        let size = FontSize::<MIN, MAX, DEFAULT>::parse(input.read(cx).text());
        input.update(cx, |input, cx| input.set_invalid(size.is_none(), cx));
        if let Some(size) = size {
            change(cx, |settings| apply(settings, size));
        }
    });
    (input, changes)
}

fn font_select(
    id: &'static str,
    label: &'static str,
    defaults: &[(&'static str, Option<&'static str>)],
    installed: &[SharedString],
    current: Option<&str>,
    apply: fn(&mut Settings, Option<String>),
) -> Select {
    let mut labels: Vec<SharedString> = Vec::new();
    let mut families: Vec<Option<SharedString>> = Vec::new();
    for &(name, family) in defaults {
        labels.push(name.into());
        families.push(family.map(SharedString::from));
    }
    for name in installed {
        labels.push(name.clone());
        families.push(Some(name.clone()));
    }
    let chosen = families
        .iter()
        .position(|family| family.as_deref() == current)
        .unwrap_or_else(|| {
            let font: SharedString = current.unwrap_or_default().to_owned().into();
            labels.push(font.clone());
            families.push(Some(font));
            families.len() - 1
        });
    Select::new(id, label, labels, Some(chosen))
        .width(px(176.0))
        .on_select(move |index, _, cx| {
            let font = families[index].as_ref().map(ToString::to_string);
            change(cx, |settings| apply(settings, font));
        })
}

fn colors(
    title: &'static str,
    shown: bool,
    tokens: &[ColorToken],
    settings: &Settings,
    themes: [&Theme; 2],
    cx: &App,
) -> Section {
    let theme = cx.theme();
    let layout = theme.layout;
    let variants = settings.appearance.variants();
    let reset_slot = layout.control_sm;
    let cell = reset_slot + layout.space(1.0) + ColorPicker::width(theme);
    let column = |text: &'static str| {
        div()
            .w(cell)
            .pl(reset_slot + layout.space(1.0))
            .flex()
            .justify_center()
            .child(text)
    };
    let header = h_flex()
        .gap(layout.space(2.0))
        .children(variants.iter().map(|variant| column(variant.label())));
    let section = Section {
        title: shown.then_some(title.into()),
        search_title: Some(title.into()),
        header_end: Some(header.into_any_element()),
        rows: Vec::new(),
    };
    tokens.iter().fold(section, |section, &token| {
        let cells = variants.iter().map(|&variant| {
            let shown = match variant {
                Variant::Light => themes[0],
                Variant::Dark => themes[1],
            };
            let name = format!("{} {}", variant.label(), token.label().to_lowercase());
            let reset = settings.colors.of(variant).contains_key(&token).then(|| {
                reset_button((variant, token), &name, move |_, cx| {
                    change(cx, |settings| {
                        settings.colors.of_mut(variant).remove(&token);
                    })
                })
            });
            h_flex()
                .w(cell)
                .gap(layout.space(1.0))
                .child(div().flex_none().w(reset_slot).children(reset))
                .child(
                    ColorPicker::new(
                        SharedString::from(format!("{variant:?}-{token:?}")),
                        name,
                        shown.color(token),
                    )
                    .on_change(move |color, _, cx| {
                        change(cx, |settings| settings.set_color(variant, token, color))
                    }),
                )
        });
        section.row(Row::new(
            token.label(),
            None,
            h_flex().gap(layout.space(2.0)).children(cells),
        ))
    })
}

fn open(cx: &mut App) {
    if let Some(window) = cx.try_global::<Open>().map(|open| open.0)
        && window
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(SETTINGS_WIDTH), px(SETTINGS_HEIGHT)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Settings".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(12.0))),
        }),
        app_owns_titlebar_drag: true,
        is_minimizable: false,
        window_min_size: Some(size(px(SETTINGS_MIN_WIDTH), px(SETTINGS_MIN_HEIGHT))),
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| {
            let settings = SettingsWindow::new(cx);
            settings.load_fonts(cx);
            settings
        });
        window.focus(&view.read(cx).focus.clone(), cx);
        view
    }) {
        Ok(window) => cx.set_global(Open(window)),
        Err(error) => tracing::error!(%error, "couldn't open settings"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    General,
    Appearance,
    Shortcuts,
}

impl Pane {
    const ALL: [Pane; 3] = [Pane::General, Pane::Appearance, Pane::Shortcuts];

    fn label(self) -> &'static str {
        match self {
            Pane::General => "General",
            Pane::Appearance => "Appearance",
            Pane::Shortcuts => "Shortcuts",
        }
    }

    fn changed(self, settings: &Settings) -> bool {
        let mut reset = settings.clone();
        reset.reset(self);
        reset != *settings
    }

    fn reset_description(self) -> &'static str {
        match self {
            Pane::General => "",
            Pane::Appearance => {
                "Mode, colors, fonts, font sizes, font smoothing and cursor pointers go back to their defaults."
            }
            Pane::Shortcuts => "Every shortcut goes back to its default.",
        }
    }
}

struct Row {
    label: SharedString,
    description: Option<SharedString>,
    control: AnyElement,
}

impl Row {
    fn new(
        label: impl Into<SharedString>,
        description: Option<&'static str>,
        control: impl IntoElement,
    ) -> Self {
        Self {
            label: label.into(),
            description: description.map(SharedString::from),
            control: control.into_any_element(),
        }
    }

    fn matches(&self, query: &str) -> bool {
        contains(&self.label, query)
            || self
                .description
                .as_ref()
                .is_some_and(|description| contains(description, query))
    }
}

#[derive(Default)]
struct Section {
    title: Option<SharedString>,
    search_title: Option<SharedString>,
    header_end: Option<AnyElement>,
    rows: Vec<Row>,
}

impl Section {
    fn titled(title: impl Into<SharedString>) -> Self {
        Self {
            title: Some(title.into()),
            ..Self::default()
        }
    }

    fn row(mut self, row: Row) -> Self {
        self.rows.push(row);
        self
    }

    fn into_group(self) -> Group {
        let group = self
            .title
            .into_iter()
            .fold(Group::new(), |group, title| group.title(title));
        let group = self
            .header_end
            .into_iter()
            .fold(group, |group, end| group.header_end(end));
        self.rows.into_iter().fold(group, |group, row| {
            group.row(row.label, row.description, row.control)
        })
    }
}

enum Block {
    Section(Section),
    Element(AnyElement),
}

fn contains(text: &str, query: &str) -> bool {
    text.to_lowercase().contains(query)
}

struct SettingsWindow {
    focus: FocusHandle,
    scroll: ScrollHandle,
    pane: Pane,
    advanced: bool,
    search: Entity<Input>,
    text_size: Entity<Input>,
    mono_size: Entity<Input>,
    fonts: Rc<[SharedString]>,
    github_token: Entity<Input>,
    resetting: Option<Pane>,
    _subscriptions: [Subscription; 4],
}

impl SettingsWindow {
    fn new(cx: &mut Context<Self>) -> Self {
        let saved = packages::packages(cx)
            .read(cx)
            .token()
            .unwrap_or_default()
            .to_owned();
        let github_token = cx.new(|cx| {
            let mut input = Input::new("GitHub token", cx)
                .password()
                .placeholder("Optional");
            input.set_text(saved, cx);
            input
        });
        let search = cx.new(|cx| {
            Input::new("Search settings", cx)
                .placeholder("Search")
                .search()
        });
        let token_changes = cx.subscribe(&github_token, |_, input, event, cx| {
            if !matches!(event, InputEvent::Changed) {
                return;
            }
            let text = input.read(cx).text().to_owned();
            let saved =
                packages::packages(cx).update(cx, |packages, cx| packages.set_token(&text, cx));
            if let Err(error) = saved {
                tracing::error!(%error, "couldn't save the GitHub token");
            }
        });
        let settings = &cx.global::<Store>().settings;
        let (text_size, size_changes) = size_input(
            "Interface font size",
            settings.text_size,
            |settings, size| settings.text_size = size,
            cx,
        );
        let settings = &cx.global::<Store>().settings;
        let (mono_size, mono_changes) = size_input(
            "Code font size",
            settings.mono_size,
            |settings, size| settings.mono_size = size,
            cx,
        );
        let search_changes = cx.subscribe(&search, |this, _, event, cx| {
            if matches!(event, InputEvent::Changed) {
                this.scroll.set_offset(point(px(0.0), px(0.0)));
                cx.notify();
            }
        });
        Self {
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            pane: Pane::General,
            advanced: false,
            resetting: None,
            search,
            text_size,
            mono_size,
            fonts: Rc::from([]),
            github_token,
            _subscriptions: [token_changes, search_changes, size_changes, mono_changes],
        }
    }

    fn load_fonts(&self, cx: &mut Context<Self>) {
        let system = cx.text_system().clone();
        let names = cx.background_spawn(async move {
            system
                .all_font_names()
                .into_iter()
                .filter(|name| !name.starts_with('.') && name != UI_FONT)
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let names = names.await;
            let _ = this.update(cx, |this, cx| {
                this.fonts = names.into_iter().map(SharedString::from).collect();
                cx.notify();
            });
        })
        .detach();
    }

    fn blocks(&self, pane: Pane, advanced: bool, cx: &mut Context<Self>) -> Vec<Block> {
        match pane {
            Pane::General => self.general(cx),
            Pane::Appearance => self.appearance(advanced, cx),
            Pane::Shortcuts => self.shortcuts(cx),
        }
    }

    fn general(&self, cx: &App) -> Vec<Block> {
        let settings = &cx.global::<Store>().settings;
        let theme = cx.theme();
        let starts = Start::ALL
            .map(|start| {
                let preview = match start {
                    Start::Client => previews::client(theme),
                    Start::Manager => previews::manager(theme),
                };
                Radio::new(start.label()).preview(preview.w(previews::WIDE).h(previews::HEIGHT))
            })
            .into_iter()
            .collect();
        let start = Start::ALL.iter().position(|&start| start == settings.start);
        vec![
            Block::Section(Section::default().row(Row::new(
                "Open at launch",
                None,
                RadioGroup::new("start", "Open at launch", starts, start).on_select(
                    |index, _, cx| change(cx, |settings| settings.start = Start::ALL[index]),
                ),
            ))),
            Block::Section(Section::titled("GitHub").row(Row::new(
                "Token",
                Some(
                    "Used for all GitHub requests. Unlocks private repos and a higher rate limit.",
                ),
                div().w(px(200.0)).child(self.github_token.clone()),
            ))),
        ]
    }

    fn appearance(&self, advanced: bool, cx: &mut Context<Self>) -> Vec<Block> {
        let settings = &cx.global::<Store>().settings;
        let light = settings.theme(Variant::Light);
        let dark = settings.theme(Variant::Dark);
        let modes = Appearance::ALL
            .map(|appearance| {
                let preview = match appearance {
                    Appearance::System => previews::split(&light, &dark),
                    Appearance::Light => previews::theme(&light),
                    Appearance::Dark => previews::theme(&dark),
                };
                Radio::new(appearance.label())
                    .preview(preview.w(previews::NARROW).h(previews::HEIGHT))
            })
            .into_iter()
            .collect();
        let chosen = Appearance::ALL
            .iter()
            .position(|&appearance| appearance == settings.appearance);
        let this = cx.weak_entity();
        let disclosure = h_flex().px(cx.theme().space(3.0)).child(
            Disclosure::new("advanced-colors", "Advanced", advanced).on_toggle(
                move |open, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.advanced = open;
                        cx.notify();
                    });
                },
            ),
        );
        let mut blocks = vec![
            Block::Section(
                Section::default().row(Row::new(
                    "Mode",
                    None,
                    RadioGroup::new("appearance", "Mode", modes, chosen)
                        .on_select(|index, _, cx| choose(Appearance::ALL[index], cx)),
                )),
            ),
            Block::Section(colors(
                "Colors",
                true,
                &ColorToken::SEEDS,
                settings,
                [&light, &dark],
                cx,
            )),
            Block::Element(disclosure.into_any_element()),
        ];
        if advanced {
            let theme = &cx.theme().clone();
            blocks.push(Block::Section(
                Section {
                    search_title: Some("Advanced".into()),
                    ..Section::default()
                }
                .row(Row::new(
                    "Interface font",
                    None,
                    with_reset(
                        settings.font.is_some().then(|| {
                            reset_button("font", "Interface font", |_, cx| {
                                change(cx, |settings| settings.font = None)
                            })
                        }),
                        font_select(
                            "font",
                            "Interface font",
                            &[("Inter", None), ("System", Some(".SystemUIFont"))],
                            &self.fonts,
                            settings.font.as_deref(),
                            |settings, font| settings.font = font,
                        ),
                        theme,
                    ),
                ))
                .row(Row::new(
                    "Interface font size",
                    None,
                    with_reset(
                        (settings.text_size != TextSize::default()).then(|| {
                            reset_button(
                                "text-size",
                                "Interface font size",
                                reset_size(&self.text_size, TextSize::default().0),
                            )
                        }),
                        sized(self.text_size.clone(), theme),
                        theme,
                    ),
                ))
                .row(Row::new(
                    "Code font",
                    None,
                    with_reset(
                        settings.mono_font.is_some().then(|| {
                            reset_button("mono-font", "Code font", |_, cx| {
                                change(cx, |settings| settings.mono_font = None)
                            })
                        }),
                        font_select(
                            "mono-font",
                            "Code font",
                            &[("System", None)],
                            &self.fonts,
                            settings.mono_font.as_deref(),
                            |settings, font| settings.mono_font = font,
                        ),
                        theme,
                    ),
                ))
                .row(Row::new(
                    "Code font size",
                    None,
                    with_reset(
                        (settings.mono_size != MonoSize::default()).then(|| {
                            reset_button(
                                "mono-size",
                                "Code font size",
                                reset_size(&self.mono_size, MonoSize::default().0),
                            )
                        }),
                        sized(self.mono_size.clone(), theme),
                        theme,
                    ),
                ))
                .row(Row::new(
                    "Font smoothing",
                    Some("Thicken text strokes, most of all for light text."),
                    Switch::new("font-smoothing", "Font smoothing", settings.font_smoothing)
                        .hide_label()
                        .on_toggle(|on, _, cx| change(cx, |settings| settings.font_smoothing = on)),
                ))
                .row(Row::new(
                    "Use cursor pointers",
                    Some("Show a hand cursor over clickable controls."),
                    Switch::new(
                        "pointer-cursors",
                        "Use cursor pointers",
                        settings.pointer_cursors,
                    )
                    .hide_label()
                    .on_toggle(|on, _, cx| change(cx, |settings| settings.pointer_cursors = on)),
                )),
            ));
            blocks.push(Block::Section(colors(
                "Advanced colors",
                false,
                &ColorToken::ADVANCED,
                settings,
                [&light, &dark],
                cx,
            )));
        }
        blocks
    }

    fn shortcuts(&self, cx: &mut Context<Self>) -> Vec<Block> {
        Category::ALL
            .into_iter()
            .map(|category| Block::Section(self.category(category, cx)))
            .collect()
    }

    fn category(&self, category: Category, cx: &mut Context<Self>) -> Section {
        let layout = cx.theme().layout;
        let os = cx.platform_info().os;
        let overrides = shortcuts(cx);
        Shortcut::ALL
            .iter()
            .filter(|shortcut| shortcut.category() == category)
            .fold(Section::titled(category.label()), |section, &shortcut| {
                let keys = shortcut
                    .keys(overrides)
                    .and_then(|keys| Keystroke::parse_with_os(keys, os).ok());
                let record = move |keys: Option<Keystroke>, _: &mut Window, cx: &mut App| {
                    set_shortcut(shortcut, keys, cx)
                        .map_err(|Taken(owner)| SharedString::from(format!("Used by {owner}")))
                };
                let changed = !shortcut.is_default(overrides);
                let reset = changed.then(|| {
                    reset_button(shortcut, shortcut.label(), move |window, cx| {
                        record(shortcut.default_keystroke(), window, cx).ok();
                    })
                });
                let field = ShortcutField::new(
                    SharedString::from(format!("{shortcut:?}")),
                    shortcut.label(),
                    keys,
                )
                .on_change(record);
                section.row(Row::new(
                    shortcut.label(),
                    None,
                    h_flex().gap(layout.space(1.0)).children(reset).child(field),
                ))
            })
    }

    fn results(&self, query: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut found = Vec::new();
        for pane in Pane::ALL {
            for block in self.blocks(pane, true, cx) {
                let Block::Section(section) = block else {
                    continue;
                };
                let heading: SharedString =
                    match section.title.as_ref().or(section.search_title.as_ref()) {
                        Some(title) => format!("{} · {title}", pane.label()).into(),
                        None => pane.label().into(),
                    };
                let whole = contains(&heading, query);
                let rows: Vec<Row> = section
                    .rows
                    .into_iter()
                    .filter(|row| whole || row.matches(query))
                    .collect();
                if !rows.is_empty() {
                    let section = Section {
                        title: Some(heading),
                        search_title: None,
                        header_end: section.header_end,
                        rows,
                    };
                    found.push(section.into_group().into_any_element());
                }
            }
        }
        if found.is_empty() {
            found.push(
                Empty::new("No settings match", "Try a different search.").into_any_element(),
            );
        }
        found
    }

    fn reset(&mut self, pane: Pane, cx: &mut Context<Self>) {
        self.resetting = None;
        change(cx, |settings| settings.reset(pane));
        if pane == Pane::Appearance {
            for (input, size) in [
                (&self.text_size, TextSize::default().0),
                (&self.mono_size, MonoSize::default().0),
            ] {
                input.update(cx, |input, cx| input.set_text(size.to_string(), cx));
            }
        }
        cx.notify();
    }

    fn reset_dialog(&self, cx: &mut Context<Self>) -> Dialog {
        let pane = self.resetting.unwrap_or(self.pane);
        let closing = cx.weak_entity();
        Dialog::new(
            "reset-page",
            self.resetting.is_some(),
            format!("Reset {}?", pane.label().to_lowercase()),
        )
        .alert()
        .description(pane.reset_description())
        .action(
            Button::new("reset-cancel", "Cancel").on_click(cx.listener(|this, _, _, cx| {
                this.resetting = None;
                cx.notify();
            })),
        )
        .action(
            Button::new("reset-accept", "Reset")
                .variant(ButtonVariant::Danger)
                .on_click(cx.listener(move |this, _, _, cx| this.reset(pane, cx))),
        )
        .on_close(move |_, cx| {
            let _ = closing.update(cx, |this, cx| {
                this.resetting = None;
                cx.notify();
            });
        })
    }

    fn show(&mut self, pane: Pane, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| {
            if !search.text().is_empty() {
                search.set_text("", cx);
            }
        });
        if self.pane != pane {
            self.pane = pane;
            self.scroll.set_offset(point(px(0.0), px(0.0)));
            cx.notify();
        }
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layout = cx.theme().layout;
        mira_ui::nav::set_scroll_root(&self.scroll, window, cx);
        let query = self.search.read(cx).text().trim().to_lowercase();
        let groups: Vec<AnyElement> = if query.is_empty() {
            self.blocks(self.pane, self.advanced, cx)
                .into_iter()
                .map(|block| match block {
                    Block::Section(section) => section.into_group().into_any_element(),
                    Block::Element(element) => element,
                })
                .collect()
        } else {
            self.results(&query, cx)
        };
        let this = cx.weak_entity();
        let tabs = Tabs::new(
            "settings-pane",
            "Settings sections",
            Pane::ALL.len(),
            |index| Tab::new(Pane::ALL[index].label()),
        )
        .vertical()
        .selected(if query.is_empty() {
            Pane::ALL
                .iter()
                .position(|&pane| pane == self.pane)
                .unwrap_or(0)
        } else {
            usize::MAX
        })
        .on_select(move |index, _, cx| {
            let _ = this.update(cx, |this, cx| this.show(Pane::ALL[index], cx));
        });
        let changed = self.pane.changed(&cx.global::<Store>().settings);
        let reset_dialog = self.reset_dialog(cx);
        let confirm_reset = cx.listener(|this, _, _, cx| {
            this.resetting = Some(this.pane);
            cx.notify();
        });
        let theme = cx.theme();
        let title = if query.is_empty() {
            self.pane.label()
        } else {
            "Results"
        };
        let header = h_flex()
            .flex_none()
            .pt(layout.space(4.0))
            .w_full()
            .max_w(px(600.0))
            .mx_auto()
            .px(layout.space(7.0))
            .justify_between()
            .child(
                div()
                    .id("page-heading")
                    .role(Role::Heading)
                    .aria_level(1)
                    .aria_label(title)
                    .text_heading(theme.typography)
                    .child(title),
            )
            .when(query.is_empty() && self.pane != Pane::General, |header| {
                header.child(
                    Button::new("reset-page", "Reset to defaults…")
                        .variant(ButtonVariant::Ghost)
                        .size(Size::Small)
                        .disabled(!changed)
                        .on_click(confirm_reset),
                )
            });
        div()
            .flex()
            .flex_col()
            .size_full()
            .window_surface(theme)
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .on_action(|_: &CloseSession, window, _| window.remove_window())
            .child(mira_ui::nav::pointer_probe())
            .child(reset_dialog)
            .child(
                window_bar(window, cx)
                    .child(bar_title("Settings", cx))
                    .child(drag_area().flex_1().h_full()),
            )
            .child(
                h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h_0()
                    .child(
                        theme
                            .sidebar_pane()
                            .h_full()
                            .child(
                                div()
                                    .flex_none()
                                    .p(layout.space(2.0))
                                    .child(self.search.clone()),
                            )
                            .child(tabs)
                            .child(div().flex_1())
                            .child(
                                h_flex()
                                    .flex_none()
                                    .flex_wrap()
                                    .justify_between()
                                    .gap(layout.space(2.0))
                                    .p(layout.space(2.0))
                                    .pl(layout.space(4.0))
                                    .child(
                                        div()
                                            .text_caption(theme.typography)
                                            .text_color(theme.muted_foreground)
                                            .child(concat!("Mira ", env!("CARGO_PKG_VERSION"))),
                                    )
                                    .child(
                                        Button::new("check-updates", "Check for updates")
                                            .variant(ButtonVariant::Ghost)
                                            .size(Size::XSmall)
                                            .disabled(true),
                                    ),
                            ),
                    )
                    .child(
                        v_flex().flex_1().min_w_0().child(header).child(
                            div()
                                .relative()
                                .flex_1()
                                .min_h_0()
                                .child(
                                    div()
                                        .id("settings")
                                        .size_full()
                                        .max_w(px(600.0))
                                        .mx_auto()
                                        .overflow_y_scroll()
                                        .track_scroll(&self.scroll)
                                        .flex()
                                        .flex_col()
                                        .gap(layout.space(4.0))
                                        .px(layout.space(4.0))
                                        .pt(layout.space(3.0))
                                        .pb(layout.space(4.0))
                                        .children(groups),
                                )
                                .child(Scrollbar::new("settings-scrollbar", self.scroll.clone())),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_to_the_system_appearance_and_arrow_cursors_and_survive_restarts() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let path = std::env::temp_dir().join(format!(
            "mira-settings-{:x}.json",
            u64::from_ne_bytes(random)
        ));
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        assert_eq!(Settings::default().appearance, Appearance::System);
        assert!(!Settings::default().pointer_cursors);
        assert!(!Settings::default().font_smoothing);
        assert_eq!(Settings::default().start, Start::Client);
        assert!(!Settings::default().sidebar);
        let chosen = Settings {
            start: Start::Manager,
            sidebar: true,
            appearance: Appearance::Dark,
            pointer_cursors: true,
            font_smoothing: true,
            font: Some("Menlo".into()),
            mono_font: Some("Courier".into()),
            text_size: FontSize(15),
            mono_size: FontSize(12),
            colors: Colors {
                light: BTreeMap::new(),
                dark: BTreeMap::from([(ColorToken::Accent, Hex(parse_hex("#3B82F6").unwrap()))]),
            },
            script_folders: vec![PathBuf::from("/Volumes/USB/bots")],
            skip_shop_warning: true,
            hidden_recent_scripts: vec![PathBuf::from("/Volumes/USB/bots/old.ts")],
            shortcuts: Overrides::from([(Shortcut::Bank, None)]),
        };
        chosen.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), chosen);
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["colors"]["dark"]["accent"], "#3B82F6");
        assert_eq!(saved["textSize"], 15);
        assert_eq!(
            serde_json::from_str::<Settings>(r#"{"textSize":40}"#)
                .unwrap()
                .text_size,
            FontSize(16),
            "a size outside the offered range is clamped instead of discarding the file"
        );
        let theme = chosen.theme(Variant::Dark);
        assert_eq!(theme.font_family.as_ref(), "Menlo");
        assert_eq!(theme.mono_family.as_ref(), "Courier");
        assert_eq!(theme.typography.mono, px(12.0));
        assert_eq!(TextSize::parse("16"), Some(FontSize(16)));
        assert_eq!(TextSize::parse("17"), None);
        assert_eq!(TextSize::parse(""), None);
        assert_eq!(theme.typography.body, px(15.0));
        fs::write(&path, r#"{"appearance":"light"}"#).unwrap();
        assert_eq!(
            Settings::load(&path).unwrap(),
            Settings {
                appearance: Appearance::Light,
                ..Settings::default()
            }
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_custom_color_paints_only_its_variant_and_picking_the_default_clears_it() {
        let blue = parse_hex("#1D4ED8").unwrap();
        let mut settings = Settings::default();
        settings.set_color(Variant::Dark, ColorToken::Accent, blue);
        assert_eq!(to_hex(settings.theme(Variant::Dark).primary), "#1D4ED8");
        assert_eq!(
            to_hex(settings.theme(Variant::Dark).primary_foreground),
            "#FAFAFA"
        );
        assert_eq!(
            to_hex(settings.theme(Variant::Light).primary),
            to_hex(Theme::light().primary)
        );
        settings.set_color(Variant::Dark, ColorToken::Accent, Theme::dark().primary);
        assert_eq!(settings.colors, Colors::default());
        let navy = parse_hex("#101830").unwrap();
        settings.set_color(Variant::Dark, ColorToken::Background, navy);
        let card = settings.theme(Variant::Dark).card;
        settings.set_color(Variant::Dark, ColorToken::Card, card);
        assert_eq!(
            settings.colors.dark.keys().collect::<Vec<_>>(),
            [&ColorToken::Background],
            "a card that matches what the background derives isn't an override"
        );
        assert!(
            serde_json::from_str::<Settings>(r#"{"colors":{"dark":{"primary":"blue"}}}"#).is_err()
        );
    }

    #[gpui::test]
    fn typing_a_size_applies_it_and_resetting_restores_the_default(cx: &mut gpui::TestAppContext) {
        let path = std::env::temp_dir().join(format!(
            "mira-settings-size-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        cx.update(|cx| {
            mira_ui::init(cx);
            init(path.clone(), |_| {}, cx);
            packages::init(
                std::env::temp_dir().join("mira-settings-size-packages"),
                std::env::temp_dir().join("mira-settings-size-token"),
                cx,
            );
        });
        let (view, cx) = cx.add_window_view(|_, cx| SettingsWindow::new(cx));
        let size = |cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| cx.global::<Store>().settings.text_size)
        };
        let input = cx.update(|_, cx| view.read(cx).text_size.clone());
        cx.update(|_, cx| input.update(cx, |input, cx| input.set_text("15", cx)));
        cx.run_until_parked();
        assert_eq!(size(cx), FontSize(15));
        cx.update(|_, cx| input.update(cx, |input, cx| input.set_text("40", cx)));
        cx.run_until_parked();
        assert_eq!(size(cx), FontSize(15), "an out-of-range size isn't applied");
        cx.update(|window, cx| reset_size(&input, TextSize::default().0)(window, cx));
        cx.run_until_parked();
        assert_eq!(size(cx), TextSize::default());
        assert_eq!(cx.update(|_, cx| input.read(cx).text().to_owned()), "13");
        fs::remove_file(path).ok();
    }

    #[gpui::test]
    fn resetting_a_page_restores_only_that_pages_settings(cx: &mut gpui::TestAppContext) {
        let path = std::env::temp_dir().join(format!(
            "mira-settings-reset-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        let chosen = Settings {
            start: Start::Manager,
            appearance: Appearance::Dark,
            pointer_cursors: true,
            font_smoothing: true,
            font: Some("Menlo".into()),
            mono_font: Some("Courier".into()),
            text_size: FontSize(15),
            mono_size: FontSize(12),
            colors: Colors {
                light: BTreeMap::new(),
                dark: BTreeMap::from([(ColorToken::Accent, Hex(parse_hex("#3B82F6").unwrap()))]),
            },
            script_folders: vec![PathBuf::from("/Volumes/USB/bots")],
            shortcuts: Overrides::from([(Shortcut::Bank, Some("cmd-k".into()))]),
            ..Settings::default()
        };
        chosen.save(&path).unwrap();
        cx.update(|cx| {
            mira_ui::init(cx);
            init(path.clone(), |_| {}, cx);
            packages::init(
                std::env::temp_dir().join("mira-settings-reset-packages"),
                std::env::temp_dir().join("mira-settings-reset-token"),
                cx,
            );
        });
        let (view, cx) = cx.add_window_view(|_, cx| SettingsWindow::new(cx));
        let settings = |cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| cx.global::<Store>().settings.clone())
        };
        assert!(Pane::Appearance.changed(&settings(cx)));
        assert!(!Pane::General.changed(&settings(cx)));

        view.update(cx, |this, cx| this.reset(Pane::Appearance, cx));
        cx.run_until_parked();
        let expected = Settings {
            start: Start::Manager,
            script_folders: chosen.script_folders.clone(),
            shortcuts: chosen.shortcuts.clone(),
            ..Settings::default()
        };
        assert_eq!(settings(cx), expected);
        assert_eq!(Settings::load(&path).unwrap(), expected);
        let field = |input: fn(&SettingsWindow) -> &Entity<Input>,
                     cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| input(view.read(cx)).read(cx).text().to_owned())
        };
        assert_eq!(field(|this| &this.text_size, cx), "13");
        assert_eq!(field(|this| &this.mono_size, cx), "11");
        assert!(!Pane::Appearance.changed(&settings(cx)));
        assert!(Pane::Shortcuts.changed(&settings(cx)));

        view.update(cx, |this, cx| {
            this.pane = Pane::Shortcuts;
            cx.notify();
        });
        cx.run_until_parked();
        let root = cx.update(|_, cx| view.read(cx).focus.clone());
        cx.update(|window, cx| {
            window.activate_window();
            window.focus(&root, cx);
        });
        cx.simulate_keystrokes("tab tab tab");
        cx.run_until_parked();
        let button = cx.update(|window, cx| window.focused(cx)).unwrap();
        view.update(cx, |this, cx| {
            this.resetting = Some(Pane::Shortcuts);
            cx.notify();
        });
        cx.run_until_parked();
        view.update(cx, |this, cx| this.reset(Pane::Shortcuts, cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("tab");
        cx.run_until_parked();
        assert!(
            cx.update(|window, cx| window.focused(cx).is_some_and(|focus| focus != button)),
            "Tab moves on once the reset button is disabled"
        );
        assert_eq!(
            settings(cx),
            Settings {
                shortcuts: Overrides::new(),
                ..expected
            }
        );
        assert!(!Pane::Shortcuts.changed(&settings(cx)));
        fs::remove_file(path).ok();
    }

    #[gpui::test]
    fn a_recorded_shortcut_is_saved_and_bound_and_a_taken_one_is_not(
        cx: &mut gpui::TestAppContext,
    ) {
        let path = std::env::temp_dir().join(format!(
            "mira-settings-shortcut-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        cx.update(|cx| {
            init(path.clone(), crate::shortcuts::apply, cx);
            crate::shortcuts::apply(cx);
            let keys = |keys: &'static str| Some(Keystroke::parse(keys).unwrap());
            set_shortcut(Shortcut::Bank, keys("cmd-k"), cx).unwrap();
            assert_eq!(
                crate::shortcuts::keys_for(&crate::ruffle_view::OpenBank, cx).as_deref(),
                Some("⌘K")
            );
            assert!(set_shortcut(Shortcut::StartStopScript, keys("cmd-,"), cx).is_err());
            assert_eq!(Shortcut::StartStopScript.keys(shortcuts(cx)), Some("cmd-r"));
        });
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["shortcuts"]["bank"], "cmd-k");
        assert_eq!(saved["shortcuts"]["startStopScript"], "cmd-r");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn opening_writes_the_full_settings_but_never_over_an_unreadable_file() {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let path = std::env::temp_dir().join(format!(
            "mira-settings-open-{:x}.json",
            u64::from_ne_bytes(random)
        ));
        assert_eq!(Settings::open(&path), Some(Settings::default()));
        let written: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            written["shortcuts"].as_object().unwrap().len(),
            Shortcut::ALL.len()
        );
        assert_eq!(written["shortcuts"]["loadScript"], "cmd-o");

        fs::write(&path, r#"{"appearance":"dark"}"#).unwrap();
        assert!(Settings::open(&path).is_some());
        let written: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["appearance"], "dark");
        assert_eq!(written["shortcuts"]["bank"], "cmd-b");

        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::open(&path), None);
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn snapshot_settings_window() {
        let edited = Settings {
            appearance: Appearance::System,
            colors: Colors {
                light: BTreeMap::from([(ColorToken::Accent, Hex(parse_hex("#2563EB").unwrap()))]),
                dark: BTreeMap::from([
                    (ColorToken::Accent, Hex(parse_hex("#3B82F6").unwrap())),
                    (ColorToken::Background, Hex(parse_hex("#101830").unwrap())),
                ]),
            },
            shortcuts: Overrides::from([
                (Shortcut::Bank, Some("cmd-k".into())),
                (Shortcut::Packets, None),
            ]),
            ..Settings::default()
        };
        for (name, height, pane, settings, query) in [
            (
                "settings",
                SETTINGS_HEIGHT,
                Pane::General,
                Settings::default(),
                "",
            ),
            (
                "settings-appearance",
                980.0,
                Pane::Appearance,
                edited.clone(),
                "",
            ),
            (
                "settings-appearance-light",
                SETTINGS_HEIGHT,
                Pane::Appearance,
                Settings {
                    appearance: Appearance::Light,
                    ..Settings::default()
                },
                "",
            ),
            (
                "settings-shortcuts",
                1300.0,
                Pane::Shortcuts,
                edited.clone(),
                "",
            ),
            (
                "settings-reset",
                SETTINGS_HEIGHT,
                Pane::Appearance,
                edited.clone(),
                "",
            ),
            (
                "settings-min",
                SETTINGS_MIN_HEIGHT,
                Pane::Appearance,
                Settings::default(),
                "",
            ),
            (
                "settings-search",
                SETTINGS_HEIGHT,
                Pane::General,
                edited,
                "color",
            ),
        ] {
            for written in mira_ui::snapshot(
                name,
                size(
                    px(if name == "settings-min" {
                        SETTINGS_MIN_WIDTH
                    } else {
                        SETTINGS_WIDTH
                    }),
                    px(height),
                ),
                move |window, cx| {
                    cx.set_global(Store {
                        path: PathBuf::new(),
                        settings: settings.clone(),
                        on_change: |_| {},
                    });
                    packages::init(
                        std::env::temp_dir().join("mira-settings-snapshot-packages"),
                        std::env::temp_dir().join("mira-settings-snapshot-token"),
                        cx,
                    );
                    cx.new(|cx| {
                        let mut settings = SettingsWindow::new(cx);
                        settings.pane = pane;
                        settings.advanced = pane == Pane::Appearance;
                        settings.resetting = (name == "settings-reset").then_some(pane);
                        settings
                            .search
                            .update(cx, |search, cx| search.set_text(query, cx));
                        window.focus(&settings.focus, cx);
                        settings
                    })
                },
            ) {
                assert!(written.exists(), "{} was not written", written.display());
            }
        }
    }

    #[test]
    fn snapshot_settings_with_large_text_in_another_font() {
        let settings = Settings {
            font: Some("Georgia".into()),
            text_size: FontSize(16),
            mono_size: FontSize(13),
            ..Settings::default()
        };
        let themed = settings.clone();
        for written in mira_ui::snapshot_with(
            "settings-text",
            size(px(SETTINGS_WIDTH), px(900.0)),
            move |window, cx| {
                cx.set_global(Store {
                    path: PathBuf::new(),
                    settings: settings.clone(),
                    on_change: |_| {},
                });
                packages::init(
                    std::env::temp_dir().join("mira-settings-snapshot-packages"),
                    std::env::temp_dir().join("mira-settings-snapshot-token"),
                    cx,
                );
                cx.new(|cx| {
                    let mut settings = SettingsWindow::new(cx);
                    settings.pane = Pane::Appearance;
                    settings.advanced = true;
                    window.focus(&settings.focus, cx);
                    settings
                })
            },
            move |cx, _| {
                cx.update(|cx| {
                    let dark = cx.theme().dark_mode;
                    let variant = if dark { Variant::Dark } else { Variant::Light };
                    cx.set_global(themed.theme(variant));
                });
            },
        ) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }
}
