use std::{path::PathBuf, sync::Arc};

use gpui::{
    App, Entity, HeadlessAppContext, Keystroke, Pixels, PlatformHeadlessRenderer, Render, Size,
    Window,
};
use gpui_apple::metal_renderer::MetalHeadlessRenderer;
use mira_platform::AppleTextSystem;

use crate::{Assets, Theme};

pub fn snapshot<V: Render + 'static>(
    name: &str,
    size: Size<Pixels>,
    build: impl Fn(&mut Window, &mut App) -> Entity<V>,
) -> Vec<PathBuf> {
    snapshot_after(name, size, &[], build)
}

pub fn snapshot_after<V: Render + 'static>(
    name: &str,
    size: Size<Pixels>,
    keys: &[&'static str],
    build: impl Fn(&mut Window, &mut App) -> Entity<V>,
) -> Vec<PathBuf> {
    snapshot_with(name, size, build, |cx, window| {
        for key in keys {
            let keystroke =
                Keystroke::parse(key).unwrap_or_else(|_| panic!("{key} is not a keystroke"));
            cx.update_window(window, |_, window, cx| {
                window.dispatch_keystroke(keystroke, cx);
            })
            .expect("press the key");
            cx.run_until_parked();
        }
    })
}

pub fn snapshot_with<V: Render + 'static>(
    name: &str,
    size: Size<Pixels>,
    build: impl Fn(&mut Window, &mut App) -> Entity<V>,
    interact: impl Fn(&mut HeadlessAppContext, gpui::AnyWindowHandle),
) -> Vec<PathBuf> {
    let dir = snapshot_dir();
    std::fs::create_dir_all(&dir).expect("create the snapshot folder");
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(AppleTextSystem::new()),
        Arc::new(Assets),
        || {
            Ok(Some(
                Box::new(MetalHeadlessRenderer::new()?) as Box<dyn PlatformHeadlessRenderer>
            ))
        },
    );
    cx.update(|cx| {
        crate::init(cx);
        cx.set_reduce_motion(true);
    });
    [("dark", Theme::dark()), ("light", Theme::light())]
        .into_iter()
        .map(|(appearance, theme)| {
            cx.update(|cx| cx.set_global(theme.clone()));
            let window = cx
                .open_window(size, |window, cx| {
                    let root = build(window, cx);
                    cx.set_global(theme.clone());
                    root
                })
                .expect("open a headless window");
            cx.run_until_parked();
            cx.update_window(window.into(), |_, window, _| window.activate_window())
                .expect("activate the window");
            interact(&mut cx, window.into());
            cx.update_window(window.into(), |_, window, _| window.refresh())
                .expect("redraw the window");
            cx.run_until_parked();
            let image = cx
                .capture_screenshot(window.into())
                .expect("render the window");
            let path = dir.join(format!("{name}-{appearance}.png"));
            image.save(&path).expect("write the snapshot");
            path
        })
        .collect()
}

fn snapshot_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("find the test binary");
    exe.ancestors()
        .nth(3)
        .expect("the test binary sits in <target>/<profile>/deps")
        .join("snapshots")
}
