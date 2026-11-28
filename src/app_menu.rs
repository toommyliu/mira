use gpui::{App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType, Window, actions};
use mira_ui::primitives::text_field;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAboutPanelOptionApplicationVersion, NSAboutPanelOptionVersion, NSApplication,
};
use objc2_foundation::{NSDictionary, NSString, ns_string};

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
        ToggleFullScreen
    ]
);

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
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
    let Some(main) = objc2::MainThreadMarker::new() else {
        return;
    };
    let version = NSString::from_str(env!("CARGO_PKG_VERSION"));
    let keys = unsafe {
        [
            NSAboutPanelOptionApplicationVersion,
            NSAboutPanelOptionVersion,
        ]
    };
    // An empty build version stops AppKit from appending CFBundleVersion, the workflow run number.
    let values: [&AnyObject; 2] = [&version, ns_string!("")];
    let options = NSDictionary::from_slices(&keys, &values);
    unsafe {
        NSApplication::sharedApplication(main).orderFrontStandardAboutPanelWithOptions(&options)
    };
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
            MenuItem::action("Find", text_field::Find),
        ]),
        Menu::new("View").items([
            settings::appearance_menu(cx),
            MenuItem::separator(),
            MenuItem::action("Sidebar", game::ToggleSidebar),
            MenuItem::action("Grid View", game::ToggleGrid),
            MenuItem::separator(),
            MenuItem::action("Client", game::OpenClient),
            MenuItem::action("Manager", game::OpenManager),
            MenuItem::action("Combat profiles", game::OpenCombatProfiles),
            MenuItem::separator(),
            MenuItem::action("Scripts", game::OpenScripts),
            MenuItem::action("Environment", game::OpenEnvironment),
            MenuItem::action("Loader/Grabber", game::OpenLoaderGrabber),
            MenuItem::action("Packets", game::OpenPackets),
            MenuItem::separator(),
            MenuItem::action("Toggle Full Screen", ToggleFullScreen),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
        ]),
    ]
}
