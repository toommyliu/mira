#[cfg(not(target_os = "macos"))]
compile_error!("Mira supports macOS only.");

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
    Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, actions, point, px, size,
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
    gpui_platform::application().run(move |cx| {
        // AppKit termination does not unwind main's stack.
        let mut logging = Some(logging);
        cx.on_app_quit(move |_| {
            let logging = logging.take();
            async move { drop(logging) }
        })
        .detach();
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        ruffle_view::bind_keys(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(vec![Menu {
            name: "Mira".into(),
            disabled: false,
            items: vec![MenuItem::action("Quit Mira", Quit)],
        }]);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(
                    px(960.0),
                    px(550.0) + ruffle_view::BAR_HEIGHT + ruffle_view::STRIP_HEIGHT,
                ),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Mira".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(12.0), px(11.0))),
            }),
            window_background: WindowBackgroundAppearance::Transparent,
            app_owns_titlebar_drag: true,
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            let view = ruffle_view::Client::new(data, device, armies, window, cx)
                .expect("create the AQW player view");
            window.focus(&gpui::Focusable::focus_handle(view.read(cx), cx), cx);
            view
        });
        if let Err(error) = opened {
            tracing::error!(%error, "couldn't open Mira");
            cx.quit();
        }
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
    Ok(())
}
