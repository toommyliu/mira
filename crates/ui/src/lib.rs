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
pub use theme::{ActiveTheme, PointerCursor, Theme, Tone};

use std::rc::Rc;

use gpui::{App, KeyBinding, Window, actions};

use primitives::Toaster;

pub(crate) type IndexHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

pub const FOCUS_SCOPE: &str = "FocusScope";

actions!(ui, [FocusNext, FocusPrevious]);

pub fn init(cx: &mut App) {
    register_fonts(cx.text_system());
    cx.set_global(Theme::for_window(cx.window_appearance()));
    Toaster::init(cx);
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

fn register_fonts(system: &gpui::TextSystem) {
    system
        .add_fonts(vec![
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Medium.ttf")),
        ])
        .expect("register bundled Inter faces");
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_inter_shapes_regular_and_medium_at_their_native_widths() {
        use gpui::PlatformTextSystem as _;
        let platform = std::sync::Arc::new(gpui_apple::AppleTextSystem::new());
        let system = gpui::TextSystem::new(platform.clone());
        super::register_fonts(&system);
        let sample = "Accounts and combat profiles";
        for (weight, expected) in [
            (gpui::FontWeight::NORMAL, 184.114),
            (gpui::FontWeight::MEDIUM, 186.243),
        ] {
            let mut font = gpui::font(crate::theme::UI_FONT);
            font.weight = weight;
            let run = gpui::FontRun {
                len: sample.len(),
                font_id: system.resolve_font(&font),
            };
            let width = f32::from(
                platform
                    .layout_line(sample, crate::theme::TEXT_BODY, &[run])
                    .width,
            );
            assert!(
                (width - expected).abs() < 0.05,
                "Inter {} at 13 px shaped to {width} px, expected {expected}",
                weight.0
            );
        }
    }
}
