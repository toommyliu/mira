#[cfg(not(target_os = "macos"))]
compile_error!("Mira supports macOS only.");

mod accounts;
mod appearance;
mod army;
mod automation;
mod bridge;
mod domain;
mod logging;
mod net;
mod ruffle;
mod ruffle_view;
mod scripting;

use gpui::{
    App, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBackgroundAppearance,
    WindowBounds, WindowOptions, actions, point, px, size,
};

actions!(mira, [Quit]);

fn main() -> anyhow::Result<()> {
    let logging = logging::init();
    let data = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("No application data directory"))?
        .join("Mira");
    std::fs::create_dir_all(&data)?;
    let device = net::device_id(&data.join("device-id"))?;
    let armies = army::Armies::new(
        dirs::document_dir()
            .ok_or_else(|| anyhow::anyhow!("No Documents directory"))?
            .join("Mira")
            .join("armies"),
    );
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
            cx.on_action(|_: &Quit, cx| cx.quit());
            appearance::init(cx, set_menus);
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
            let opened = cx.open_window(options, |window, cx| {
                appearance::follow_system(window);
                let view = ruffle_view::Client::new(data, device, armies, window, cx)
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
            cx.on_action(move |_: &ruffle_view::OpenAccounts, cx| {
                let (client, accounts) = (client.clone(), accounts.clone());
                cx.defer(move |cx| ruffle_view::open_accounts(main, client, accounts, cx));
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
    let appearance = appearance::menu(cx);
    cx.set_menus(vec![Menu {
        name: "Mira".into(),
        disabled: false,
        items: vec![
            MenuItem::action("Accounts", ruffle_view::OpenAccounts),
            appearance,
            MenuItem::separator(),
            MenuItem::action("Quit Mira", Quit),
        ],
    }]);
}
