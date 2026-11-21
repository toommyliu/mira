use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let icon = match path {
            "icons/arrow-down.svg" => include_str!("../assets/icons/lucide/arrow-down.svg"),
            "icons/arrow-down-to-line.svg" => {
                include_str!("../assets/icons/lucide/arrow-down-to-line.svg")
            }
            "icons/arrow-left.svg" => include_str!("../assets/icons/lucide/arrow-left.svg"),
            "icons/arrow-right.svg" => include_str!("../assets/icons/lucide/arrow-right.svg"),
            "icons/arrow-up.svg" => include_str!("../assets/icons/lucide/arrow-up.svg"),
            "icons/bell.svg" => include_str!("../assets/icons/lucide/bell.svg"),
            "icons/bell-ring.svg" => include_str!("../assets/icons/lucide/bell-ring.svg"),
            "icons/check.svg" => include_str!("../assets/icons/lucide/check.svg"),
            "icons/spinner-track.svg" => include_str!("../assets/icons/spinner-track.svg"),
            "icons/spinner-arc.svg" => include_str!("../assets/icons/spinner-arc.svg"),
            "icons/grip-vertical.svg" => include_str!("../assets/icons/lucide/grip-vertical.svg"),
            "icons/file-code.svg" => include_str!("../assets/icons/lucide/file-code.svg"),
            "icons/map-pin.svg" => include_str!("../assets/icons/lucide/map-pin.svg"),
            "icons/server.svg" => include_str!("../assets/icons/lucide/server.svg"),
            "icons/user.svg" => include_str!("../assets/icons/lucide/user.svg"),
            "icons/chevron-down.svg" => include_str!("../assets/icons/lucide/chevron-down.svg"),
            "icons/chevron-right.svg" => include_str!("../assets/icons/lucide/chevron-right.svg"),
            "icons/chevron-up.svg" => include_str!("../assets/icons/lucide/chevron-up.svg"),
            "icons/circle-alert.svg" => include_str!("../assets/icons/lucide/circle-alert.svg"),
            "icons/circle-check.svg" => include_str!("../assets/icons/lucide/circle-check.svg"),
            "icons/clock.svg" => include_str!("../assets/icons/lucide/clock.svg"),
            "icons/ellipsis.svg" => include_str!("../assets/icons/lucide/ellipsis.svg"),
            "icons/download.svg" => include_str!("../assets/icons/lucide/download.svg"),
            "icons/eye.svg" => include_str!("../assets/icons/lucide/eye.svg"),
            "icons/eye-off.svg" => include_str!("../assets/icons/lucide/eye-off.svg"),
            "icons/info.svg" => include_str!("../assets/icons/lucide/info.svg"),
            "icons/layout-grid.svg" => include_str!("../assets/icons/lucide/layout-grid.svg"),
            "icons/minus.svg" => include_str!("../assets/icons/lucide/minus.svg"),
            "icons/pin.svg" => include_str!("../assets/icons/lucide/pin.svg"),
            "icons/plus.svg" => include_str!("../assets/icons/lucide/plus.svg"),
            "icons/refresh-cw.svg" => include_str!("../assets/icons/lucide/refresh-cw.svg"),
            "icons/rotate-ccw.svg" => include_str!("../assets/icons/lucide/rotate-ccw.svg"),
            "icons/search.svg" => include_str!("../assets/icons/lucide/search.svg"),
            "icons/square.svg" => include_str!("../assets/icons/lucide/square.svg"),
            "icons/triangle-alert.svg" => {
                include_str!("../assets/icons/lucide/triangle-alert.svg")
            }
            "icons/x.svg" => include_str!("../assets/icons/lucide/x.svg"),
            "icons/circle-dot.svg" => include_str!("../assets/icons/lucide/circle-dot.svg"),
            "icons/file-plus-2.svg" => include_str!("../assets/icons/lucide/file-plus-2.svg"),
            "icons/pencil.svg" => include_str!("../assets/icons/lucide/pencil.svg"),
            "icons/loader-circle.svg" => include_str!("../assets/icons/lucide/loader-circle.svg"),
            "icons/circle.svg" => include_str!("../assets/icons/lucide/circle.svg"),
            "icons/circle-stop.svg" => include_str!("../assets/icons/lucide/circle-stop.svg"),
            "icons/sidebar.svg" => include_str!("../assets/icons/sidebar.svg"),
            "icons/sidebar-open.svg" => include_str!("../assets/icons/sidebar-open.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(icon.as_bytes())))
    }

    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
