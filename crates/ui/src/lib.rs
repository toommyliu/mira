mod assets;
pub mod components;
pub mod motion;
pub mod nav;
pub mod primitives;
pub mod theme;

pub use assets::Assets;
pub use theme::{ActiveTheme, PointerCursor, Theme, Tone};

use std::rc::Rc;

use gpui::{App, KeyBinding, Window, actions};

use primitives::Toaster;

pub(crate) type IndexHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

pub const FOCUS_SCOPE: &str = "FocusScope";

actions!(ui, [FocusNext, FocusPrevious]);

pub fn init(cx: &mut App) {
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
