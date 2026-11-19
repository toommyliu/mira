mod open_type;
mod platform;
mod text_system;

use std::{rc::Rc, sync::Arc};

use gpui_macos::MacPlatform;

pub use text_system::{AppleTextSystem, set_font_smoothing};

pub fn application() -> gpui::Application {
    gpui::Application::with_platform(Rc::new(MiraPlatform {
        mac: MacPlatform::new(false),
        text_system: Arc::new(AppleTextSystem::new()),
    }))
}

struct MiraPlatform {
    mac: MacPlatform,
    text_system: Arc<AppleTextSystem>,
}
