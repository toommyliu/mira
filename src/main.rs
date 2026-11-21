#[cfg(not(target_os = "macos"))]
compile_error!("Mira supports macOS only.");

mod account_settings;
mod accounts;
mod app_menu;
mod army;
mod automation;
mod beep;
mod bridge;
mod combat_profiles;
mod domain;
mod jitter;
mod logging;
mod net;
mod packages;
mod quit;
mod ruffle;
mod ruffle_view;
mod script_inputs;
mod scripting;
mod settings;
mod setups;
mod shortcuts;
#[cfg(any(test, feature = "storyboard"))]
mod storyboard;
mod updates;

use std::{iter, path::PathBuf};

use gpui::AppContext as _;

#[cfg_attr(feature = "storyboard", allow(unreachable_code))]
fn main() -> anyhow::Result<()> {
    #[cfg(feature = "storyboard")]
    return storyboard::run();
    let logging = logging::init();
    let data = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("No application data directory"))?
        .join("Mira");
    std::fs::create_dir_all(&data)?;
    let device = net::device_id(&data.join("device-id"))?;
    let documents = dirs::document_dir()
        .ok_or_else(|| anyhow::anyhow!("No Documents directory"))?
        .join("Mira");
    let armies = army::Armies::new(documents.join("armies"));
    let scripts = documents.join("scripts");
    let types = data.join("types");
    let app = mira_platform::application();
    app.on_reopen(ruffle_view::open_launch_window);
    app.with_quit_mode(gpui::QuitMode::Explicit)
        .with_assets(mira_ui::Assets)
        .run(move |cx| {
            // AppKit termination does not unwind main's stack.
            let mut logging = Some(logging);
            cx.on_app_quit(move |_| {
                let logging = logging.take();
                async move { drop(logging) }
            })
            .detach();
            mira_ui::init(cx);
            app_menu::init(cx);
            quit::init(cx);
            updates::init(cx);
            ruffle_view::bind_keys(cx);
            ruffle_view::bind_profile_keys(cx);
            settings::init(data.join("settings.json"), settings_changed, cx);
            shortcuts::apply(cx);
            cx.on_keyboard_layout_change(settings_changed).detach();
            account_settings::init(data.join("account-settings"), cx);
            combat_profiles::init(data.join("combat-profiles.json"), cx);
            script_inputs::init(data.join("script-inputs.json"), cx);
            packages::init(documents.join("packages"), data.join("github-token"), cx);
            packages::start(cx);
            updates::start(cx);
            let installed = types.clone();
            let folders: Vec<_> = iter::once(scripts.clone())
                .chain(settings::script_folders(cx).iter().cloned())
                .collect();
            cx.background_spawn(async move {
                if let Err(error) = scripting::install_types(&installed) {
                    tracing::warn!(%error, path = %installed.display(), "couldn't install the script types");
                }
                scripting::link_types(folders.iter().map(PathBuf::as_path), &installed);
            })
            .detach();
            app_menu::set_menus(cx);
            let accounts = data.join("accounts.json");
            ruffle_view::init_client(data, accounts.clone(), device.clone(), armies, cx);
            ruffle_view::open_launch_window(cx);
            if cx.windows().is_empty() {
                cx.quit();
                return;
            }
            let (load_folder, load_types) = (scripts.clone(), types.clone());
            cx.on_action(move |_: &ruffle_view::LoadScript, cx| {
                let (folder, types) = (load_folder.clone(), load_types.clone());
                cx.defer(move |cx| {
                    if let Some(client) = client(cx) {
                        ruffle_view::load_script(client, folder, types, cx);
                    }
                });
            });
            cx.on_action(move |_: &ruffle_view::OpenScripts, cx| {
                let (folder, types) = (scripts.clone(), types.clone());
                cx.defer(move |cx| {
                    if let Some(client) = client(cx) {
                        ruffle_view::open_scripts(
                            client,
                            folder,
                            types,
                            ruffle_view::Target::Selected,
                            cx,
                        );
                    }
                });
            });
            cx.on_action(|_: &ruffle_view::OpenEnvironment, cx| {
                cx.defer(|cx| {
                    if let Some(client) = client(cx) {
                        ruffle_view::open_environment(client, ruffle_view::Target::Selected, cx);
                    }
                });
            });
            cx.on_action(|_: &ruffle_view::OpenLoaderGrabber, cx| {
                cx.defer(|cx| {
                    if let Some(client) = client(cx) {
                        ruffle_view::open_loader_grabber(client, ruffle_view::Target::Selected, cx);
                    }
                });
            });
            cx.on_action(|_: &ruffle_view::OpenPackets, cx| {
                cx.defer(|cx| {
                    if let Some(client) = client(cx) {
                        ruffle_view::open_packets(client, ruffle_view::Target::Selected, cx);
                    }
                });
            });
            cx.on_action(|_: &ruffle_view::OpenCombatProfiles, cx| {
                cx.defer(|cx| ruffle_view::open_combat_profiles(None, cx));
            });
            cx.on_action(move |_: &ruffle_view::OpenManager, cx| {
                let (accounts, device) = (accounts.clone(), device.clone());
                cx.defer(move |cx| {
                    ruffle_view::open_manager(accounts, device, cx);
                });
            });
            cx.activate(true);
        });
    Ok(())
}

fn client(cx: &mut gpui::App) -> Option<gpui::Entity<ruffle_view::Client>> {
    ruffle_view::current_client(cx)
        .and_then(|window| window.entity(cx))
        .inspect_err(|error| tracing::error!(%error, "couldn't open the client"))
        .ok()
}

fn settings_changed(cx: &mut gpui::App) {
    shortcuts::apply(cx);
    app_menu::set_menus(cx);
}
