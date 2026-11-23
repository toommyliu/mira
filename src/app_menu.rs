use gpui::{App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType, Window, actions};
use mira_ui::primitives::text_field;
use objc2_app_kit::NSApplication;

use crate::{quit::Quit, ruffle_view as game, settings};

actions!(
    mira,
    [
        About,
        Hide,
        HideOthers,
        ShowAll,
        Minimize,
        Zoom,
        ToggleFullScreen,
        Find
    ]
);

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
        KeyBinding::new("cmd-f", Find, None),
    ]);
    cx.on_action(|_: &About, _| show_about());
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Minimize, cx| in_active_window(cx, Window::minimize_window));
    cx.on_action(|_: &Zoom, cx| in_active_window(cx, Window::zoom_window));
    cx.on_action(|_: &ToggleFullScreen, cx| in_active_window(cx, Window::toggle_fullscreen));
}

fn show_about() {
    if let Some(main) = objc2::MainThreadMarker::new() {
        NSApplication::sharedApplication(main).orderFrontStandardAboutPanel(None);
    }
}

fn in_active_window(cx: &mut App, act: fn(&Window)) {
    if let Some(window) = cx.active_window() {
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, _| act(window));
        });
    }
}

pub(crate) fn set_menus(cx: &mut App) {
    let menus = menus(cx);
    cx.set_menus(menus);
}

fn menus(cx: &App) -> Vec<Menu> {
    vec![
        Menu::new("Mira").items([
            MenuItem::action("About Mira", About),
            MenuItem::action("Check for Updates…", crate::updates::CheckForUpdates),
            MenuItem::separator(),
            MenuItem::action("Settings…", settings::OpenSettings),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Mira", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit Mira", Quit),
        ]),
        Menu::new("File").items([MenuItem::action("Close Window", game::CloseSession)]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", text_field::Undo, OsAction::Undo),
            MenuItem::os_action("Redo", text_field::Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", text_field::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", text_field::Copy, OsAction::Copy),
            MenuItem::os_action("Paste", text_field::Paste, OsAction::Paste),
            MenuItem::os_action("Select All", text_field::SelectAll, OsAction::SelectAll),
            MenuItem::separator(),
            MenuItem::action("Find", Find),
        ]),
        Menu::new("View").items([
            settings::appearance_menu(cx),
            MenuItem::separator(),
            MenuItem::action("Sidebar", game::ToggleSidebar),
            MenuItem::action("Grid View", game::ToggleGrid),
            MenuItem::separator(),
            MenuItem::action("Client", game::OpenClient),
            MenuItem::action("Manager", game::OpenManager),
            MenuItem::action("Combat Profiles", game::OpenCombatProfiles),
            MenuItem::separator(),
            MenuItem::action("Scripts", game::OpenScripts),
            MenuItem::action("Environment", game::OpenEnvironment),
            MenuItem::action("Loader/Grabber", game::OpenLoaderGrabber),
            MenuItem::action("Packets", game::OpenPackets),
            MenuItem::separator(),
            MenuItem::action("Toggle Full Screen", ToggleFullScreen),
        ]),
        Menu::new("Script").items([
            MenuItem::action("Load Script…", game::LoadScript),
            MenuItem::action("Edit Inputs…", game::EditScriptInputs),
            MenuItem::separator(),
            MenuItem::action("Start/Stop Script", game::ToggleScript),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
        ]),
    ]
}
