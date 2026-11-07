#[cfg(not(target_os = "macos"))]
compile_error!("Lyra supports macOS only.");

mod automation;
mod logging;
mod net;
mod ruffle;
mod ruffle_view;

use gpui::{
    Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, actions, px, size,
};

actions!(lyra, [Quit]);

fn main() -> anyhow::Result<()> {
    let logging = logging::init();
    let data = dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!("No application data directory"))?
        .join("Lyra");
    std::fs::create_dir_all(&data)?;
    let device = net::device_id(&data.join("device-id"))?;
    gpui_platform::application().run(move |cx| {
        // AppKit termination does not unwind main's stack.
        let mut logging = Some(logging);
        cx.on_app_quit(move |_| {
            let logging = logging.take();
            async move { drop(logging) }
        })
        .detach();
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(vec![Menu {
            name: "Lyra".into(),
            disabled: false,
            items: vec![MenuItem::action("Quit Lyra", Quit)],
        }]);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(960.0), px(550.0) + ruffle_view::BAR_HEIGHT),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Lyra".into()),
                ..Default::default()
            }),
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            let view = ruffle_view::RuffleView::new(data, device, window, cx)
                .expect("create the AQW player view");
            window.focus(&gpui::Focusable::focus_handle(view.read(cx), cx), cx);
            view
        });
        if let Err(error) = opened {
            tracing::error!(%error, "couldn't open Lyra");
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
