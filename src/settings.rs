use std::{
    fs, io,
    path::{Path, PathBuf},
};

use gpui::{
    App, AppContext as _, Bounds, Context, FocusHandle, Global, InteractiveElement as _,
    IntoElement, KeyBinding, Menu, MenuItem, ParentElement as _, Render, SharedString, Styled as _,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, actions, div, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, Theme,
    components::Group,
    primitives::{Select, Switch},
    theme::TEXT_BODY,
};
use serde::{Deserialize, Serialize};

use crate::ruffle_view::{CloseSession, bar_title, drag_area, window_bar};

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
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    appearance: Appearance,
    pointer_cursors: bool,
    script_folders: Vec<PathBuf>,
    skip_shop_warning: bool,
    hidden_recent_scripts: Vec<PathBuf>,
}

impl Settings {
    fn load(path: &Path) -> io::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
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

struct Open(WindowHandle<SettingsWindow>);

impl Global for Open {}

pub(crate) fn init(path: PathBuf, on_change: fn(&mut App), cx: &mut App) {
    let settings = Settings::load(&path).unwrap_or_else(|error| {
        tracing::warn!(%error, path = %path.display(), "couldn't read settings");
        Settings::default()
    });
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
    let mut theme = match settings.appearance {
        Appearance::System => Theme::for_window(cx.window_appearance()),
        Appearance::Light => Theme::light(),
        Appearance::Dark => Theme::dark(),
    };
    theme.pointer_cursor = settings.pointer_cursors;
    cx.set_global(theme);
    cx.refresh_windows();
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
            size(px(480.0), px(180.0)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Settings".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(12.0))),
        }),
        app_owns_titlebar_drag: true,
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| SettingsWindow {
            focus: cx.focus_handle(),
        });
        window.focus(&view.read(cx).focus.clone(), cx);
        view
    }) {
        Ok(window) => cx.set_global(Open(window)),
        Err(error) => tracing::error!(%error, "couldn't open settings"),
    }
}

struct SettingsWindow {
    focus: FocusHandle,
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let settings = &cx.global::<Store>().settings;
        let appearances = Appearance::ALL
            .map(|appearance| SharedString::from(appearance.label()))
            .to_vec();
        let chosen = Appearance::ALL
            .iter()
            .position(|&appearance| appearance == settings.appearance);
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.base)
            .text_size(TEXT_BODY)
            .text_color(theme.text)
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .on_action(|_: &CloseSession, window, _| window.remove_window())
            .child(mira_ui::nav::pointer_probe())
            .child(
                window_bar(window, cx)
                    .child(bar_title("Settings", cx))
                    .child(drag_area().flex_1()),
            )
            .child(
                div().px(px(16.0)).pt(px(12.0)).child(
                    Group::new()
                        .title("Appearance")
                        .row(
                            "Theme",
                            None,
                            Select::new("appearance", "Theme", appearances, chosen)
                                .width(px(128.0))
                                .align_item_with_trigger()
                                .on_select(|index, _, cx| choose(Appearance::ALL[index], cx)),
                        )
                        .row(
                            "Pointer cursors",
                            Some("Show a hand cursor over clickable controls.".into()),
                            Switch::new(
                                "pointer-cursors",
                                "Pointer cursors",
                                settings.pointer_cursors,
                            )
                            .hide_label()
                            .on_toggle(|on, _, cx| {
                                change(cx, |settings| settings.pointer_cursors = on)
                            }),
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
        let chosen = Settings {
            appearance: Appearance::Dark,
            pointer_cursors: true,
            script_folders: vec![PathBuf::from("/Volumes/USB/bots")],
            skip_shop_warning: true,
            hidden_recent_scripts: vec![PathBuf::from("/Volumes/USB/bots/old.ts")],
        };
        chosen.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), chosen);
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
}
