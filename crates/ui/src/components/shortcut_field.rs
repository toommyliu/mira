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

pub fn format_keystroke(keystroke: &Keystroke) -> SharedString {
    let mut text = String::new();
    let modifiers = keystroke.modifiers;
    if modifiers.control {
        text.push('⌃');
    }
    if modifiers.alt {
        text.push('⌥');
    }
    if modifiers.shift {
        text.push('⇧');
    }
    if modifiers.platform {
        text.push('⌘');
    }
    let key = match keystroke.key.as_str() {
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
    let mut words = Vec::new();
    if modifiers.control {
        words.push("Control".to_string());
    }
    if modifiers.alt {
        words.push("Option".to_string());
    }
    if modifiers.shift {
        words.push("Shift".to_string());
    }
    if modifiers.platform {
        words.push("Command".to_string());
    }
    words.push(keystroke.key.to_uppercase());
    words.join(" ")
}

type ShortcutHandler = Rc<dyn Fn(Option<Keystroke>, &mut Window, &mut App)>;

struct Recorder {
    on_change: Option<ShortcutHandler>,
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
            if !focus.is_focused(window) {
                return;
            }
            let on_change = this
                .upgrade()
                .and_then(|recorder| recorder.read(cx).on_change.clone());
            if record(&event.keystroke, on_change.as_ref(), window, cx) {
                cx.stop_propagation();
            }
        });
        Recorder {
            on_change: None,
            _intercept: intercept,
        }
    })
}

fn record(
    keystroke: &Keystroke,
    on_change: Option<&ShortcutHandler>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let key = keystroke.key.as_str();
    let modified = keystroke.modifiers.modified();
    match key {
        "tab" if !modified || keystroke.modifiers == Modifiers::shift() => return false,
        "escape" if !modified => window.blur(cx),
        "backspace" | "delete" if !modified => {
            if let Some(on_change) = on_change {
                on_change(None, window, cx);
            }
        }
        _ if modified || key.starts_with('f') && key.len() > 1 => {
            if let Some(on_change) = on_change {
                on_change(Some(keystroke.clone()), window, cx);
            }
            window.blur(cx);
        }
        _ => {}
    }
    true
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
        let recording = focus.is_focused(window);
        let theme = cx.theme().clone();
        let on_change = self.on_change.clone();
        recorder(&self.id, focus.clone(), window, cx)
            .update(cx, |recorder, _| recorder.on_change = on_change);
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
                "Focus and press a key combination. Backspace clears, Escape cancels.",
            )
            .track_focus(&focus.clone().tab_stop(true))
            .on_click({
                let focus = focus.clone();
                move |_, window, cx| window.focus(&focus, cx)
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
            .when(recording, |field| {
                field.child(theme.focus_ring(theme.radius.control))
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
