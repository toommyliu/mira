#[cfg(not(target_os = "macos"))]
compile_error!("Mira supports macOS only.");

mod accounts;
mod app_menu;
mod army;
mod automation;
mod beep;
mod bridge;
mod characters;
mod clock;
mod combat_profiles;
mod core_bots;
mod data_file;
mod domain;
mod file_panel;
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
mod shared_objects;
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
    let files = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("No home directory"))?
        .join("Mira");
    let armies = army::Armies::new(files.join("armies"));
    let scripts = files.join("scripts");
    let types = data.join("types");
    let installed_packages = files.join("packages");
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
            cx.spawn(async move |cx| {
                if updates::offer_move(cx) {
                    cx.update(|cx| cx.quit());
                    return;
                }
                cx.update(move |cx| {
                    ruffle_view::bind_keys(cx);
                    ruffle_view::bind_profile_keys(cx);
                    settings::init(data.join("settings.json"), settings_changed, cx);
                    shortcuts::apply(cx);
                    cx.on_keyboard_layout_change(settings_changed).detach();
                    characters::init(data.join("characters"), cx);
                    combat_profiles::init(data.join("combat-profiles.json"), cx);
                    script_inputs::init(data.join("script-inputs.json"), cx);
                    packages::init(
                        installed_packages.clone(),
                        files.join("skua"),
                        data.join("github-token"),
                        cx,
                    );
                    scripting::init_skua(scripting::SkuaSetup {
                        runner: packages::resources()
                            .unwrap_or_default()
                            .join("skua/Mira.Skua.dll"),
                        home: files.join("skua"),
                        cache: dirs::cache_dir()
                            .unwrap_or_else(|| data.clone())
                            .join("Mira/skua"),
                    });
                    let (mapped, mapping) = (types.clone(), installed_packages.clone());
                    cx.observe(&packages::packages(cx), move |state, cx| {
                        if state.read(cx).activity().is_none() {
                            let (types, packages) = (mapped.clone(), mapping.clone());
                            cx.background_spawn(async move {
                                scripting::link_packages(&types, &packages);
                            })
                            .detach();
                        }
                    })
                    .detach();
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
                        scripting::link_packages(&installed, &installed_packages);
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
                    ruffle_view::init_tools(scripts, types, cx);
                    cx.on_action(|_: &ruffle_view::OpenClient, cx| {
                        cx.defer(|cx| match ruffle_view::current_client(cx) {
                            Ok(window) => {
                                let _ = window.update(cx, |_, window, _| window.activate_window());
                            }
                            Err(error) => tracing::error!(%error, "couldn't open the client"),
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
            })
            .detach();
        });
    Ok(())
}

fn settings_changed(cx: &mut gpui::App) {
    shortcuts::apply(cx);
    app_menu::set_menus(cx);
}
