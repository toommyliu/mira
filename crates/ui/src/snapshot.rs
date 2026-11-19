use std::{path::PathBuf, sync::Arc};

use gpui::{
    App, Entity, HeadlessAppContext, Keystroke, Pixels, PlatformHeadlessRenderer, Render,
    RenderImage, Size, Window,
};
use gpui_apple::{AppleTextSystem, metal_renderer::MetalHeadlessRenderer};

use crate::{Assets, Theme};

fn headless() -> HeadlessAppContext {
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
    cx
}

pub struct PreviewRenderer(HeadlessAppContext);

impl PreviewRenderer {
    pub fn new(theme: Theme) -> anyhow::Result<Self> {
        let mut cx = headless();
        cx.update(|cx| {
            if theme.palette().is_some() {
                Theme::load_mira_font(cx)?;
            }
            cx.set_global(theme);
            anyhow::Ok(())
        })?;
        Ok(Self(cx))
    }

    pub fn render<V: Render + 'static>(
        &mut self,
        size: Size<Pixels>,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> anyhow::Result<Arc<RenderImage>> {
        let window = self.0.open_window(size, build)?;
        self.0.run_until_parked();
        let image = self.0.capture_screenshot(window.into());
        self.0
            .update_window(window.into(), |_, window, _| window.remove_window())?;
        self.0.run_until_parked();
        let mut image = image?;
        for pixel in image.pixels_mut() {
            pixel.0.swap(0, 2);
        }
        Ok(Arc::new(RenderImage::new(vec![image::Frame::new(image)])))
    }
}

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
    let dir = snapshot_dir();
    std::fs::create_dir_all(&dir).expect("create the snapshot folder");
    let mut cx = headless();
    [("dark", Theme::dark()), ("light", Theme::light())]
        .into_iter()
        .map(|(appearance, theme)| {
            cx.update(|cx| cx.set_global(theme));
            let window = cx
                .open_window(size, |window, cx| build(window, cx))
                .expect("open a headless window");
            cx.run_until_parked();
            cx.update_window(window.into(), |_, window, _| window.activate_window())
                .expect("activate the window");
            for key in keys {
                let Ok(keystroke) = Keystroke::parse(key) else {
                    panic!("{key} is not a keystroke");
                };
                cx.update_window(window.into(), |_, window, cx| {
                    window.dispatch_keystroke(keystroke, cx);
                })
                .expect("press the key");
                cx.run_until_parked();
            }
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
