use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

pub(crate) const HEAVY: &str = "heavy/";
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
            "icons/arrow-left.svg" => include_str!("../assets/icons/lucide/arrow-left.svg"),
            "icons/arrow-right.svg" => include_str!("../assets/icons/lucide/arrow-right.svg"),
            "icons/arrow-up.svg" => include_str!("../assets/icons/lucide/arrow-up.svg"),
            "icons/check.svg" => include_str!("../assets/icons/lucide/check.svg"),
            "icons/chevron-down.svg" => include_str!("../assets/icons/lucide/chevron-down.svg"),
            "icons/chevron-right.svg" => include_str!("../assets/icons/lucide/chevron-right.svg"),
            "icons/chevron-up.svg" => include_str!("../assets/icons/lucide/chevron-up.svg"),
            "icons/minus.svg" => include_str!("../assets/icons/lucide/minus.svg"),
            "icons/plus.svg" => include_str!("../assets/icons/lucide/plus.svg"),
            "icons/x.svg" => include_str!("../assets/icons/lucide/x.svg"),
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
