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
mod ruffle;
mod ruffle_view;
mod script_inputs;
mod scripting;
mod settings;
mod setups;
mod shortcuts;
#[cfg(any(test, feature = "storyboard"))]
mod storyboard;

use std::{iter, path::PathBuf};

use gpui::{
    AppContext as _, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, actions, point, px, size,
};

actions!(mira, [Quit]);

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
    gpui_platform::application()
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
            ruffle_view::bind_keys(cx);
            ruffle_view::bind_profile_keys(cx);
            settings::init(data.join("settings.json"), settings_changed, cx);
            shortcuts::apply(cx);
            cx.on_keyboard_layout_change(settings_changed).detach();
            account_settings::init(data.join("account-settings"), cx);
            combat_profiles::init(data.join("combat-profiles.json"), cx);
            script_inputs::init(data.join("script-inputs.json"), cx);
            packages::init(
                documents.join("packages"),
                documents.join("skua"),
                data.join("github-token"),
                cx,
            );
            scripting::init_skua(scripting::SkuaSetup {
                runner: packages::resources()
                    .unwrap_or_default()
                    .join("skua/Mira.Skua.dll"),
                home: documents.join("skua"),
                cache: dirs::cache_dir()
                    .unwrap_or_else(|| data.clone())
                    .join("Mira/skua"),
            });
            packages::start(cx);
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
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(960.0), px(550.0) + ruffle_view::BAR_HEIGHT),
                    cx,
                ))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Mira".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.0), px(12.0))),
                }),
                window_background: WindowBackgroundAppearance::Transparent,
                window_min_size: Some(size(px(920.0), px(528.0) + ruffle_view::BAR_HEIGHT)),
                app_owns_titlebar_drag: true,
                ..Default::default()
            };
            let accounts = data.join("accounts.json");
            let manager_device = device.clone();
            let start = settings::start(cx);
            let opened = cx.open_window(options, |window, cx| {
                settings::follow_system(window);
                let view = ruffle_view::Client::new(
                    data,
                    accounts.clone(),
                    device,
                    armies,
                    start == settings::Start::Client,
                    window,
                    cx,
                )
                .expect("create the AQW player view");
                window.focus(&gpui::Focusable::focus_handle(view.read(cx), cx), cx);
                view
            });
            let main = match opened {
                Ok(main) => main,
                Err(error) => {
                    tracing::error!(%error, "couldn't open Mira");
                    cx.quit();
                    return;
                }
            };
            let Ok(client) = main.entity(cx) else {
                cx.quit();
                return;
            };
            let load_client = client.clone();
            let (load_folder, load_types) = (scripts.clone(), types.clone());
            cx.on_action(move |_: &ruffle_view::LoadScript, cx| {
                let client = load_client.clone();
                let (folder, types) = (load_folder.clone(), load_types.clone());
                cx.defer(move |cx| ruffle_view::load_script(client, folder, types, cx));
            });
            let scripts_client = client.clone();
            cx.on_action(move |_: &ruffle_view::OpenScripts, cx| {
                let client = scripts_client.clone();
                let (folder, types) = (scripts.clone(), types.clone());
                cx.defer(move |cx| {
                    ruffle_view::open_scripts(client, folder, types, ruffle_view::Target::Selected, cx)
                });
            });
            let environment_client = client.clone();
            cx.on_action(move |_: &ruffle_view::OpenEnvironment, cx| {
                let client = environment_client.clone();
                cx.defer(move |cx| {
                    ruffle_view::open_environment(client, ruffle_view::Target::Selected, cx)
                });
            });
            let follower_client = client.clone();
            cx.on_action(move |_: &ruffle_view::OpenFollower, cx| {
                let client = follower_client.clone();
                cx.defer(move |cx| {
                    ruffle_view::open_follower(client, ruffle_view::Target::Selected, cx)
                });
            });
            let grabber_client = client.clone();
            cx.on_action(move |_: &ruffle_view::OpenLoaderGrabber, cx| {
                let client = grabber_client.clone();
                cx.defer(move |cx| {
                    ruffle_view::open_loader_grabber(client, ruffle_view::Target::Selected, cx)
                });
            });
            let packets_client = client.clone();
            cx.on_action(move |_: &ruffle_view::OpenPackets, cx| {
                let client = packets_client.clone();
                cx.defer(move |cx| {
                    ruffle_view::open_packets(client, ruffle_view::Target::Selected, cx)
                });
            });
            cx.on_action(|_: &ruffle_view::OpenCombatProfiles, cx| {
                cx.defer(|cx| ruffle_view::open_combat_profiles(None, cx));
            });
            if start == settings::Start::Manager {
                ruffle_view::open_accounts(
                    main,
                    client.clone(),
                    accounts.clone(),
                    manager_device.clone(),
                    cx,
                );
            }
            cx.on_action(move |_: &ruffle_view::OpenAccounts, cx| {
                let (client, accounts, device) =
                    (client.clone(), accounts.clone(), manager_device.clone());
                cx.defer(move |cx| ruffle_view::open_accounts(main, client, accounts, device, cx));
            });
            cx.on_window_closed(move |cx, closed| {
                if closed == main.window_id() || cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
    Ok(())
}

fn settings_changed(cx: &mut gpui::App) {
    shortcuts::apply(cx);
    app_menu::set_menus(cx);
}
