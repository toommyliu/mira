use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

pub(crate) const HEAVY: &str = "heavy/";
pub(crate) const SIDEBAR: &str = "icons/sidebar.svg";
pub(crate) const SIDEBAR_OPEN: &str = "icons/sidebar-open.svg";
pub(crate) const SPINNER: &str = "icons/spinner.svg";
const STROKE: &str = "stroke-width=\"2.25\"";
const HEAVY_STROKE: &str = "stroke-width=\"3\"";

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let (path, stroke) = match path.strip_prefix(HEAVY) {
            Some(path) => (path, HEAVY_STROKE),
            None => (path, STROKE),
        };
        let icon = match path {
            "icons/arrow-down.svg" => include_str!("../assets/icons/lucide/arrow-down.svg"),
            "icons/arrow-down-to-line.svg" => {
                include_str!("../assets/icons/lucide/arrow-down-to-line.svg")
            }
            "icons/arrow-left.svg" => include_str!("../assets/icons/lucide/arrow-left.svg"),
            "icons/arrow-right.svg" => include_str!("../assets/icons/lucide/arrow-right.svg"),
            "icons/arrow-up.svg" => include_str!("../assets/icons/lucide/arrow-up.svg"),
            "icons/bell.svg" => include_str!("../assets/icons/lucide/bell.svg"),
            "icons/check.svg" => include_str!("../assets/icons/lucide/check.svg"),
            "icons/chevron-down.svg" => include_str!("../assets/icons/lucide/chevron-down.svg"),
            "icons/chevron-right.svg" => include_str!("../assets/icons/lucide/chevron-right.svg"),
            "icons/chevron-up.svg" => include_str!("../assets/icons/lucide/chevron-up.svg"),
            "icons/clock.svg" => include_str!("../assets/icons/lucide/clock.svg"),
            "icons/ellipsis.svg" => include_str!("../assets/icons/lucide/ellipsis.svg"),
            "icons/eye.svg" => include_str!("../assets/icons/lucide/eye.svg"),
            "icons/eye-off.svg" => include_str!("../assets/icons/lucide/eye-off.svg"),
            "icons/layout-grid.svg" => include_str!("../assets/icons/lucide/layout-grid.svg"),
            "icons/minus.svg" => include_str!("../assets/icons/lucide/minus.svg"),
            "icons/pin.svg" => include_str!("../assets/icons/lucide/pin.svg"),
            "icons/plus.svg" => include_str!("../assets/icons/lucide/plus.svg"),
            "icons/rotate-ccw.svg" => include_str!("../assets/icons/lucide/rotate-ccw.svg"),
            "icons/search.svg" => include_str!("../assets/icons/lucide/search.svg"),
            "icons/square.svg" => include_str!("../assets/icons/lucide/square.svg"),
            "icons/x.svg" => include_str!("../assets/icons/lucide/x.svg"),
            SIDEBAR => include_str!("../assets/icons/sidebar.svg"),
            SIDEBAR_OPEN => include_str!("../assets/icons/sidebar-open.svg"),
            SPINNER => include_str!("../assets/icons/spinner.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Owned(
            icon.replace("stroke-width=\"2\"", stroke).into_bytes(),
        )))
    }

    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
