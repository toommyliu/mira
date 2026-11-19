use std::rc::Rc;

use gpui::{
    App, ElementId, FocusHandle, InteractiveElement as _, IntoElement, Keystroke, Modifiers,
    MouseButton, ParentElement as _, RenderOnce, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme as _, PointerCursor as _, h_flex, nav};

fn pressed(keystroke: &Keystroke) -> (bool, String) {
    let pressed = crate::keyboard::pressed(keystroke);
    (pressed.modifiers.shift, pressed.key)
}

pub fn format_keystroke(keystroke: &Keystroke) -> SharedString {
    keys(keystroke).concat().into()
}

fn keys(keystroke: &Keystroke) -> Vec<String> {
    let modifiers = keystroke.modifiers;
    let (shift, key) = pressed(keystroke);
    let mut keys: Vec<String> = [
        (modifiers.control, "⌃"),
        (modifiers.alt, "⌥"),
        (shift, "⇧"),
        (modifiers.platform, "⌘"),
    ]
    .into_iter()
    .filter(|&(held, _)| held)
    .map(|(_, glyph)| glyph.to_string())
    .collect();
    keys.push(match key.as_str() {
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
    });
    keys
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

const PILL_WIDTH: gpui::Pixels = px(64.0);

type ShortcutHandler =
    Rc<dyn Fn(Option<Keystroke>, &mut Window, &mut App) -> Result<(), SharedString>>;

struct Recorder {
    on_change: Option<ShortcutHandler>,
    done: bool,
    pointer: bool,
    pending: Option<String>,
    error: Option<SharedString>,
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
                Outcome::Ignored => {}
                Outcome::Hint(key) => {
                    recorder.update(cx, |recorder, _| recorder.pending = Some(key))
                }
                Outcome::Restart => recorder.update(cx, |recorder, _| {
                    recorder.done = false;
                    recorder.pointer = false;
                    recorder.pending = None;
                    recorder.error = None;
                }),
                Outcome::Rejected(error) => recorder.update(cx, |recorder, _| {
                    recorder.pending = None;
                    recorder.error = Some(error);
                }),
                Outcome::Done => recorder.update(cx, |recorder, _| {
                    recorder.done = true;
                    recorder.pending = None;
                    recorder.error = None;
                }),
            }
            window.refresh();
            cx.stop_propagation();
        });
        Recorder {
            on_change: None,
            done: false,
            pointer: false,
            pending: None,
            error: None,
            _intercept: intercept,
        }
    })
}

enum Outcome {
    Pass,
    Ignored,
    Hint(String),
    Restart,
    Rejected(SharedString),
    Done,
}

fn restart(keystroke: &Keystroke) -> Outcome {
    match keystroke.key.as_str() {
        "enter" | "space" if !keystroke.modifiers.modified() => Outcome::Restart,
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
    let modified = crate::keyboard::pressed(keystroke).modifiers.modified();
    let chosen = match key {
        "tab" if !modified || keystroke.modifiers == Modifiers::shift() => return Outcome::Pass,
        "escape" if !modified => return Outcome::Done,
        "backspace" | "delete" if !modified => None,
        _ if modified || key.starts_with('f') && key.len() > 1 => Some(keystroke.clone()),
        _ if key.chars().count() == 1 => {
            return keys(keystroke)
                .pop()
                .map_or(Outcome::Ignored, Outcome::Hint);
        }
        _ => return Outcome::Ignored,
    };
    match on_change.map(|on_change| on_change(chosen, window, cx)) {
        Some(Err(error)) => Outcome::Rejected(error),
        _ => Outcome::Done,
    }
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
        f: impl Fn(Option<Keystroke>, &mut Window, &mut App) -> Result<(), SharedString> + 'static,
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
        let (recording, ring, pending, error) = recorder.update(cx, |recorder, _| {
            recorder.on_change = on_change;
            if !focused {
                recorder.done = false;
                recorder.pointer = false;
                recorder.pending = None;
                recorder.error = None;
            }
            let recording = focused && !recorder.done;
            (
                recording,
                recording || focused && !recorder.pointer,
                recorder.pending.clone(),
                recorder.error.clone(),
            )
        });
        let value_text = self
            .value
            .as_ref()
            .map_or_else(|| "None".to_string(), spoken);
        let radius = theme.layout.control_sm / 2.0;
        let invalid = error.is_some();
        let typography = theme.typography;
        let quiet = |text: &str| {
            div()
                .text_color(theme.muted_foreground)
                .child(text.to_owned())
        };
        let shown: Vec<gpui::AnyElement> = match (&self.value, recording, pending) {
            (_, true, Some(key)) => vec![
                div().child(key).into_any_element(),
                quiet("…").into_any_element(),
            ],
            (_, true, None) => vec![quiet("Press…").into_any_element()],
            (Some(keystroke), false, _) => keys(keystroke)
                .into_iter()
                .map(|key| div().child(key).into_any_element())
                .collect(),
            (None, false, _) => vec![quiet("None").into_any_element()],
        };
        let mark_pointer = recorder.clone();
        let pill = div()
            .id(self.id.clone())
            .role(Role::Button)
            .aria_label(self.label)
            .aria_value(SharedString::from(value_text))
            .aria_description(match &error {
                Some(error) => error.clone(),
                None => "Press a key combination while recording. Backspace clears, Escape cancels, and Enter records again.".into(),
            })
            .track_focus(&focus.clone().tab_stop(true))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                mark_pointer.update(cx, |recorder, _| {
                    recorder.done = false;
                    recorder.pointer = true;
                });
            })
            .on_click({
                let focus = focus.clone();
                move |_, window, cx| {
                    recorder.update(cx, |recorder, _| {
                        recorder.done = false;
                        recorder.pointer = true;
                    });
                    window.focus(&focus, cx);
                    window.refresh();
                }
            })
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(3.0))
            .when(recording, |pill| pill.min_w(PILL_WIDTH))
            .h(theme.layout.control_sm)
            .px(theme.space(2.5))
            .rounded(radius)
            .bg(theme.secondary)
            .border_1()
            .border_color(if invalid {
                theme.invalid_border()
            } else if ring {
                theme.ring
            } else {
                gpui::transparent_black()
            })
            .when(!focused, |field| {
                field.hover(|style| {
                    style.bg(crate::theme::perceptual_mix(
                        theme.secondary,
                        theme.foreground,
                        0.05,
                    ))
                })
            })
            .when(invalid, |field| {
                field.child(theme.halo(theme.invalid_halo(), radius, px(1.0), px(2.0)))
            })
            .when(ring && !invalid, |field| {
                field.child(
                    theme
                        .ring_around(theme.ring, radius, px(1.0))
                        .debug_selector(|| "shortcut-ring".into()),
                )
            })
            .pointer_cursor(&theme)
            .text_size(typography.secondary)
            .line_height(typography.secondary_line)
            .text_color(theme.foreground)
            .children(shown);
        h_flex()
            .flex_none()
            .gap(theme.space(1.0))
            .when_some(error, |row, error| {
                row.child(
                    div()
                        .debug_selector(|| "shortcut-error".into())
                        .text_size(typography.secondary)
                        .line_height(typography.secondary_line)
                        .text_color(theme.destructive)
                        .child(error),
                )
            })
            .child(pill)
    }
}
