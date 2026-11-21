mod assets;
pub mod components;
pub mod keyboard;
pub mod motion;
pub mod nav;
pub mod primitives;
#[cfg(feature = "test-support")]
mod snapshot;
mod style;
pub mod theme;

pub use assets::Assets;
#[cfg(feature = "test-support")]
pub use snapshot::{snapshot, snapshot_after, snapshot_with};
pub use style::{StyledExt, h_flex, v_flex};
pub use theme::{ActiveTheme, ColorToken, PointerCursor, Theme, Tone};

use std::rc::Rc;

use gpui::{App, KeyBinding, Window, actions};

pub(crate) type IndexHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

pub const FOCUS_SCOPE: &str = "FocusScope";

actions!(ui, [FocusNext, FocusPrevious]);

pub fn init(cx: &mut App) {
    cx.set_global(Theme::for_window(cx.window_appearance()));
    nav::track_input(cx);
    primitives::input::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("tab", FocusNext, Some(FOCUS_SCOPE)),
        KeyBinding::new("shift-tab", FocusPrevious, Some(FOCUS_SCOPE)),
    ]);
    cx.on_action(|_: &FocusNext, cx| move_focus(false, cx));
    cx.on_action(|_: &FocusPrevious, cx| move_focus(true, cx));
}

fn move_focus(backwards: bool, cx: &mut App) {
    let Some(window) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        nav::request_reveal(cx);
        window
            .update(cx, |_, window, cx| {
                if backwards {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
            })
            .ok();
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn font_smoothing_thickens_light_text_most_and_off_draws_every_color_alike() {
        use gpui::PlatformTextSystem as _;
        let platform = std::sync::Arc::new(mira_platform::AppleTextSystem::new());
        let system = gpui::TextSystem::new(platform.clone());
        let font_id = system.resolve_font(&gpui::font(crate::theme::SYSTEM_SANS));
        let glyph_id = platform
            .glyph_for_char(font_id, 'a')
            .expect("the system font has an a");
        let ink = |color: gpui::Hsla| -> u64 {
            let params = gpui::RenderGlyphParams {
                font_id,
                glyph_id,
                font_size: crate::theme::TEXT_BODY,
                subpixel_variant: gpui::Point::default(),
                scale_factor: 2.0,
                is_emoji: false,
                subpixel_rendering: false,
                dilation: platform.glyph_dilation_for_color(color),
            };
            let bounds = platform.glyph_raster_bounds(&params).expect("measure a");
            let (_, mask) = platform.rasterize_glyph(&params, bounds).expect("draw a");
            mask.iter().map(|&alpha| u64::from(alpha)).sum()
        };

        mira_platform::set_font_smoothing(true);
        let (smoothed_light, smoothed_dark) = (ink(gpui::white()), ink(gpui::black()));
        mira_platform::set_font_smoothing(false);
        let (plain_light, plain_dark) = (ink(gpui::white()), ink(gpui::black()));

        assert_eq!(plain_light, plain_dark);
        assert!(
            smoothed_dark > plain_dark,
            "{smoothed_dark} should exceed {plain_dark}"
        );
        assert!(
            smoothed_light > smoothed_dark,
            "{smoothed_light} should exceed {smoothed_dark}"
        );
    }
}
