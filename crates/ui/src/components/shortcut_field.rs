use std::rc::Rc;

use gpui::{
    App, ElementId, FocusHandle, InteractiveElement as _, IntoElement, Keystroke, Modifiers,
    ParentElement as _, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, prelude::FluentBuilder as _, px,
};

use crate::primitives::Kbd;
use crate::{
    ActiveTheme as _, PointerCursor as _, nav,
    theme::{CONTROL_MD, TEXT_SMALL},
};

fn pressed(keystroke: &Keystroke) -> (bool, String) {
    let pressed = crate::keyboard::pressed(keystroke);
    (pressed.modifiers.shift, pressed.key)
}

pub fn format_keystroke(keystroke: &Keystroke) -> SharedString {
    let mut text = String::new();
    let modifiers = keystroke.modifiers;
    let (shift, key) = pressed(keystroke);
    if modifiers.control {
        text.push('⌃');
    }
    if modifiers.alt {
        text.push('⌥');
    }
    if shift {
        text.push('⇧');
    }
    if modifiers.platform {
        text.push('⌘');
    }
    let key = match key.as_str() {
        "space" => "Space".to_string(),
        "enter" => "↩".to_string(),
        "backspace" => "⌫".to_string(),
        "tab" => "⇥".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        key if key.len() == 1 => key.to_uppercase(),
        key => {
            let mut chars = key.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        }
    };
    text.push_str(&key);
    text.into()
}

fn spoken(keystroke: &Keystroke) -> String {
    let modifiers = keystroke.modifiers;
    let (shift, key) = pressed(keystroke);
    let mut words = Vec::new();
    if modifiers.control {
        words.push("Control".to_string());
    }
    if modifiers.alt {
        words.push("Option".to_string());
    }
    if shift {
        words.push("Shift".to_string());
    }
    if modifiers.platform {
        words.push("Command".to_string());
    }
    words.push(key.to_uppercase());
    words.join(" ")
}

type ShortcutHandler = Rc<dyn Fn(Option<Keystroke>, &mut Window, &mut App)>;

struct Recorder {
    on_change: Option<ShortcutHandler>,
    done: bool,
    _intercept: Subscription,
}

fn recorder(
    id: &ElementId,
    focus: FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> gpui::Entity<Recorder> {
    window.use_keyed_state((id.clone(), "recorder"), cx, move |_, cx| {
        let this: gpui::WeakEntity<Recorder> = cx.weak_entity();
        let intercept = cx.intercept_keystrokes(move |event, window, cx| {
            let Some(recorder) = this.upgrade() else {
                return;
            };
            if !focus.is_focused(window) {
                return;
            }
            let (on_change, done) = {
                let recorder = recorder.read(cx);
                (recorder.on_change.clone(), recorder.done)
            };
            let outcome = if done {
                restart(&event.keystroke)
            } else {
                record(&event.keystroke, on_change.as_ref(), window, cx)
            };
            match outcome {
                Outcome::Pass => return,
                Outcome::Recording => recorder.update(cx, |recorder, _| recorder.done = false),
                Outcome::Done => recorder.update(cx, |recorder, _| recorder.done = true),
            }
            window.refresh();
            cx.stop_propagation();
        });
        Recorder {
            on_change: None,
            done: false,
            _intercept: intercept,
        }
    })
}

enum Outcome {
    Pass,
    Recording,
    Done,
}

fn restart(keystroke: &Keystroke) -> Outcome {
    match keystroke.key.as_str() {
        "enter" | "space" if !keystroke.modifiers.modified() => Outcome::Recording,
        _ => Outcome::Pass,
    }
}

fn record(
    keystroke: &Keystroke,
    on_change: Option<&ShortcutHandler>,
    window: &mut Window,
    cx: &mut App,
) -> Outcome {
    let key = keystroke.key.as_str();
    let modified = keystroke.modifiers.modified();
    let chosen = match key {
        "tab" if !modified || keystroke.modifiers == Modifiers::shift() => return Outcome::Pass,
        "escape" if !modified => return Outcome::Done,
        "backspace" | "delete" if !modified => None,
        _ if modified || key.starts_with('f') && key.len() > 1 => Some(keystroke.clone()),
        _ => return Outcome::Recording,
    };
    if let Some(on_change) = on_change {
        on_change(chosen, window, cx);
    }
    Outcome::Done
}

#[derive(IntoElement)]
pub struct ShortcutField {
    id: ElementId,
    label: SharedString,
    value: Option<Keystroke>,
    on_change: Option<ShortcutHandler>,
}

impl ShortcutField {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        value: Option<Keystroke>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value,
            on_change: None,
        }
    }

    pub fn on_change(
        mut self,
        f: impl Fn(Option<Keystroke>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for ShortcutField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let focused = focus.is_focused(window);
        let theme = cx.theme().clone();
        let on_change = self.on_change.clone();
        let recorder = recorder(&self.id, focus.clone(), window, cx);
        let recording = recorder.update(cx, |recorder, _| {
            recorder.on_change = on_change;
            if !focused {
                recorder.done = false;
            }
            focused && !recorder.done
        });
        let value_text = self
            .value
            .as_ref()
            .map_or_else(|| "None".to_string(), spoken);
        div()
            .id(self.id.clone())
            .role(Role::Button)
            .aria_label(self.label)
            .aria_value(SharedString::from(value_text))
            .aria_description(
                "Press a key combination while recording. Backspace clears, Escape cancels, and Enter records again.",
            )
            .track_focus(&focus.clone().tab_stop(true))
            .on_click({
                let focus = focus.clone();
                move |_, window, cx| {
                    recorder.update(cx, |recorder, _| recorder.done = false);
                    window.focus(&focus, cx);
                    window.refresh();
                }
            })
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .w(px(180.0))
            .h(CONTROL_MD)
            .px(px(8.0))
            .rounded(theme.radius.control)
            .bg(theme.fill)
            .border_1()
            .border_color(if recording { theme.focus } else { theme.border })
            .when(focused, |field| {
                field.child(theme.ring_around(theme.focus, theme.radius.control, px(1.0)))
            })
            .pointer_cursor(&theme)
            .text_size(TEXT_SMALL)
            .child(match (&self.value, recording) {
                (_, true) => div()
                    .text_color(theme.text_secondary)
                    .child("Press a shortcut")
                    .into_any_element(),
                (Some(keystroke), false) => {
                    Kbd::new(format_keystroke(keystroke)).into_any_element()
                }
                (None, false) => div()
                    .text_color(theme.text_tertiary)
                    .child("Not set")
                    .into_any_element(),
            })
            .when(recording, |field| {
                field.child(
                    div()
                        .text_size(TEXT_SMALL)
                        .text_color(theme.text_tertiary)
                        .child("esc"),
                )
            })
    }
}
