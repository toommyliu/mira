#[cfg(not(target_os = "macos"))]
compile_error!("Mira supports macOS only.");

mod account_settings;
mod accounts;
mod army;
mod automation;
mod bridge;
mod combat_profiles;
mod domain;
mod jitter;
mod logging;
mod net;
mod ruffle;
mod ruffle_view;
mod script_inputs;
mod scripting;
mod settings;
mod setups;

use std::{iter, path::PathBuf};

use gpui::{
    App, AppContext as _, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, actions, point, px, size,
};

actions!(mira, [Quit]);

fn main() -> anyhow::Result<()> {
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
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            ruffle_view::bind_keys(cx);
            ruffle_view::bind_profile_keys(cx);
            cx.on_action(|_: &Quit, cx| cx.quit());
            settings::init(data.join("settings.json"), set_menus, cx);
            account_settings::init(data.join("account-settings"), cx);
            combat_profiles::init(data.join("combat-profiles.json"), cx);
            script_inputs::init(data.join("script-inputs.json"), cx);
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
            set_menus(cx);
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

fn set_menus(cx: &mut App) {
    let appearance = settings::appearance_menu(cx);
    cx.set_menus(vec![
        Menu {
            name: "Mira".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Settings…", settings::OpenSettings),
                MenuItem::action("Accounts", ruffle_view::OpenAccounts),
                MenuItem::action("Run Script…", ruffle_view::OpenScripts),
                MenuItem::action("Combat Profiles…", ruffle_view::OpenCombatProfiles),
                MenuItem::action("Environment…", ruffle_view::OpenEnvironment),
                MenuItem::action("Follower…", ruffle_view::OpenFollower),
                MenuItem::action("Loader/Grabber…", ruffle_view::OpenLoaderGrabber),
                MenuItem::action("Packets…", ruffle_view::OpenPackets),
                MenuItem::separator(),
                MenuItem::action("Quit Mira", Quit),
            ],
        },
        Menu {
            name: "View".into(),
            disabled: false,
            items: vec![appearance],
        },
    ]);
}
