use std::num::NonZeroU32;
use std::ops::Range;
use std::time::Duration;

use ruffle_core::events::{ImeEvent, MouseWheelDelta, TextControlCode};
use ruffle_core::{PlayerEvent, ViewportDimensions};

#[derive(Clone, Debug, PartialEq)]
pub enum ViewEvent {
    Resized(Viewport),
    MouseMove(Point),
    MouseDown {
        at: Point,
        button: MouseButton,
        clicks: usize,
    },
    MouseUp {
        at: Point,
        button: MouseButton,
    },
    MouseLeave,
    Wheel(WheelDelta),
    KeyDown(Key),
    KeyUp(Key),
    Text(String),
    Ime(Ime),
    Focus(bool),
    Visible(bool),
    Throttle(Option<Duration>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    width: NonZeroU32,
    height: NonZeroU32,
    scale: f64,
}

impl Viewport {
    pub fn new(logical_width: f32, logical_height: f32, scale: f32) -> Option<Viewport> {
        if !scale.is_finite()
            || scale <= 0.0
            || !logical_width.is_finite()
            || !logical_height.is_finite()
        {
            return None;
        }
        let width = NonZeroU32::new((logical_width * scale).round() as u32)?;
        let height = NonZeroU32::new((logical_height * scale).round() as u32)?;
        Some(Viewport {
            width,
            height,
            scale: scale.into(),
        })
    }

    pub(crate) fn width(self) -> u32 {
        self.width.get()
    }

    pub(crate) fn height(self) -> u32 {
        self.height.get()
    }

    pub(crate) fn scale(self) -> f64 {
        self.scale
    }

    pub(crate) fn dimensions(self) -> ViewportDimensions {
        ViewportDimensions {
            width: self.width(),
            height: self.height(),
            scale_factor: self.scale,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WheelDelta {
    Lines(f32),
    Pixels(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub code: KeyCode,
    pub modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Named(NamedKey),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedKey {
    Enter,
    Tab,
    Backspace,
    Delete,
    Escape,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Shift,
    Control,
    Alt,
    Command,
    CapsLock,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ime {
    Preedit {
        text: String,
        cursor: Option<Range<usize>>,
    },
    Commit(String),
}

#[derive(Debug)]
pub(crate) enum Effect {
    Event(PlayerEvent),
    Resize(Viewport),
    MouseInStage(bool),
    Visible(bool),
    Throttle(Option<Duration>),
}

pub(crate) fn translate(event: ViewEvent, viewport: Viewport, out: &mut Vec<Effect>) {
    use PlayerEvent as Event;
    let scale = viewport.scale();
    let button = |button| match button {
        MouseButton::Left => ruffle_core::events::MouseButton::Left,
        MouseButton::Right => ruffle_core::events::MouseButton::Right,
        MouseButton::Middle => ruffle_core::events::MouseButton::Middle,
        MouseButton::Other => ruffle_core::events::MouseButton::Unknown,
    };
    let event = match event {
        ViewEvent::Resized(viewport) => {
            out.push(Effect::Resize(viewport));
            return;
        }
        ViewEvent::Visible(visible) => {
            out.push(Effect::Visible(visible));
            return;
        }
        ViewEvent::Throttle(throttle) => {
            out.push(Effect::Throttle(throttle));
            return;
        }
        ViewEvent::MouseMove(at) => {
            out.push(Effect::MouseInStage(true));
            Event::MouseMove {
                x: f64::from(at.x) * scale,
                y: f64::from(at.y) * scale,
            }
        }
        ViewEvent::MouseDown {
            at,
            button: pressed,
            clicks,
        } => Event::MouseDown {
            x: f64::from(at.x) * scale,
            y: f64::from(at.y) * scale,
            button: button(pressed),
            index: Some(clicks.saturating_sub(1)),
        },
        ViewEvent::MouseUp {
            at,
            button: released,
        } => Event::MouseUp {
            x: f64::from(at.x) * scale,
            y: f64::from(at.y) * scale,
            button: button(released),
        },
        ViewEvent::MouseLeave => {
            out.push(Effect::MouseInStage(false));
            Event::MouseLeave
        }
        ViewEvent::Wheel(delta) => Event::MouseWheel {
            delta: match delta {
                WheelDelta::Lines(n) => MouseWheelDelta::Lines(n.into()),
                WheelDelta::Pixels(n) => MouseWheelDelta::Pixels(f64::from(n) * scale),
            },
        },
        ViewEvent::KeyDown(key) => {
            out.push(Effect::Event(Event::KeyDown {
                key: descriptor(key),
            }));
            if let Some(code) = text_control(key) {
                out.push(Effect::Event(Event::TextControl { code }));
            }
            return;
        }
        ViewEvent::KeyUp(key) => Event::KeyUp {
            key: descriptor(key),
        },
        ViewEvent::Text(text) => {
            out.extend(
                text.chars()
                    .filter(|c| !c.is_control())
                    .map(|codepoint| Effect::Event(Event::TextInput { codepoint })),
            );
            return;
        }
        ViewEvent::Ime(Ime::Preedit { text, cursor }) => Event::Ime(ImeEvent::Preedit(
            text,
            cursor.map(|range| (range.start, range.end)),
        )),
        ViewEvent::Ime(Ime::Commit(text)) => Event::Ime(ImeEvent::Commit(text)),
        ViewEvent::Focus(true) => Event::FocusGained,
        ViewEvent::Focus(false) => Event::FocusLost,
    };
    out.push(Effect::Event(event));
}

fn descriptor(key: Key) -> ruffle_core::events::KeyDescriptor {
    use ruffle_core::events::{
        KeyDescriptor, KeyLocation, LogicalKey, NamedKey as RuffleKey, PhysicalKey,
    };
    let (logical_key, physical_key) = match key.code {
        KeyCode::Char(c) => (LogicalKey::Character(c), physical_character(c)),
        KeyCode::Named(named) => {
            let (logical, physical) = match named {
                NamedKey::Enter => (RuffleKey::Enter, PhysicalKey::Enter),
                NamedKey::Tab => (RuffleKey::Tab, PhysicalKey::Tab),
                NamedKey::Backspace => (RuffleKey::Backspace, PhysicalKey::Backspace),
                NamedKey::Delete => (RuffleKey::Delete, PhysicalKey::Delete),
                NamedKey::Escape => (RuffleKey::Escape, PhysicalKey::Escape),
                NamedKey::Insert => (RuffleKey::Insert, PhysicalKey::Insert),
                NamedKey::Home => (RuffleKey::Home, PhysicalKey::Home),
                NamedKey::End => (RuffleKey::End, PhysicalKey::End),
                NamedKey::PageUp => (RuffleKey::PageUp, PhysicalKey::PageUp),
                NamedKey::PageDown => (RuffleKey::PageDown, PhysicalKey::PageDown),
                NamedKey::ArrowLeft => (RuffleKey::ArrowLeft, PhysicalKey::ArrowLeft),
                NamedKey::ArrowRight => (RuffleKey::ArrowRight, PhysicalKey::ArrowRight),
                NamedKey::ArrowUp => (RuffleKey::ArrowUp, PhysicalKey::ArrowUp),
                NamedKey::ArrowDown => (RuffleKey::ArrowDown, PhysicalKey::ArrowDown),
                NamedKey::Shift => (RuffleKey::Shift, PhysicalKey::ShiftLeft),
                NamedKey::Control => (RuffleKey::Control, PhysicalKey::ControlLeft),
                NamedKey::Alt => (RuffleKey::Alt, PhysicalKey::AltLeft),
                NamedKey::Command => (RuffleKey::Super, PhysicalKey::SuperLeft),
                NamedKey::CapsLock => (RuffleKey::CapsLock, PhysicalKey::CapsLock),
                NamedKey::F1 => (RuffleKey::F1, PhysicalKey::F1),
                NamedKey::F2 => (RuffleKey::F2, PhysicalKey::F2),
                NamedKey::F3 => (RuffleKey::F3, PhysicalKey::F3),
                NamedKey::F4 => (RuffleKey::F4, PhysicalKey::F4),
                NamedKey::F5 => (RuffleKey::F5, PhysicalKey::F5),
                NamedKey::F6 => (RuffleKey::F6, PhysicalKey::F6),
                NamedKey::F7 => (RuffleKey::F7, PhysicalKey::F7),
                NamedKey::F8 => (RuffleKey::F8, PhysicalKey::F8),
                NamedKey::F9 => (RuffleKey::F9, PhysicalKey::F9),
                NamedKey::F10 => (RuffleKey::F10, PhysicalKey::F10),
                NamedKey::F11 => (RuffleKey::F11, PhysicalKey::F11),
                NamedKey::F12 => (RuffleKey::F12, PhysicalKey::F12),
                NamedKey::F13 => (RuffleKey::F13, PhysicalKey::F13),
                NamedKey::F14 => (RuffleKey::F14, PhysicalKey::F14),
                NamedKey::F15 => (RuffleKey::F15, PhysicalKey::F15),
                NamedKey::F16 => (RuffleKey::F16, PhysicalKey::F16),
                NamedKey::F17 => (RuffleKey::F17, PhysicalKey::F17),
                NamedKey::F18 => (RuffleKey::F18, PhysicalKey::F18),
                NamedKey::F19 => (RuffleKey::F19, PhysicalKey::F19),
                NamedKey::F20 => (RuffleKey::F20, PhysicalKey::F20),
                NamedKey::F21 => (RuffleKey::F21, PhysicalKey::F21),
                NamedKey::F22 => (RuffleKey::F22, PhysicalKey::F22),
                NamedKey::F23 => (RuffleKey::F23, PhysicalKey::F23),
                NamedKey::F24 => (RuffleKey::F24, PhysicalKey::F24),
            };
            (LogicalKey::Named(logical), physical)
        }
    };
    KeyDescriptor {
        physical_key,
        logical_key,
        key_location: KeyLocation::Standard,
    }
}

fn physical_character(c: char) -> ruffle_core::events::PhysicalKey {
    use ruffle_core::events::PhysicalKey as P;
    match c.to_ascii_lowercase() {
        'a' => P::KeyA,
        'b' => P::KeyB,
        'c' => P::KeyC,
        'd' => P::KeyD,
        'e' => P::KeyE,
        'f' => P::KeyF,
        'g' => P::KeyG,
        'h' => P::KeyH,
        'i' => P::KeyI,
        'j' => P::KeyJ,
        'k' => P::KeyK,
        'l' => P::KeyL,
        'm' => P::KeyM,
        'n' => P::KeyN,
        'o' => P::KeyO,
        'p' => P::KeyP,
        'q' => P::KeyQ,
        'r' => P::KeyR,
        's' => P::KeyS,
        't' => P::KeyT,
        'u' => P::KeyU,
        'v' => P::KeyV,
        'w' => P::KeyW,
        'x' => P::KeyX,
        'y' => P::KeyY,
        'z' => P::KeyZ,
        '0' | ')' => P::Digit0,
        '1' | '!' => P::Digit1,
        '2' | '@' => P::Digit2,
        '3' | '#' => P::Digit3,
        '4' | '$' => P::Digit4,
        '5' | '%' => P::Digit5,
        '6' | '^' => P::Digit6,
        '7' | '&' => P::Digit7,
        '8' | '*' => P::Digit8,
        '9' | '(' => P::Digit9,
        '`' | '~' => P::Backquote,
        '-' | '_' => P::Minus,
        '=' | '+' => P::Equal,
        '[' | '{' => P::BracketLeft,
        ']' | '}' => P::BracketRight,
        '\\' | '|' => P::Backslash,
        ';' | ':' => P::Semicolon,
        '\'' | '"' => P::Quote,
        ',' | '<' => P::Comma,
        '.' | '>' => P::Period,
        '/' | '?' => P::Slash,
        ' ' => P::Space,
        _ => P::Unknown,
    }
}

fn text_control(key: Key) -> Option<TextControlCode> {
    use NamedKey as N;
    use TextControlCode as T;
    let m = key.modifiers;
    let shortcut = if cfg!(target_os = "macos") {
        m.command
    } else {
        m.control
    };
    let word = if cfg!(target_os = "macos") {
        m.alt
    } else {
        m.control
    };
    Some(match key.code {
        KeyCode::Char(c) if shortcut => match c.to_ascii_lowercase() {
            'a' => T::SelectAll,
            'c' => T::Copy,
            'v' => T::Paste,
            'x' => T::Cut,
            _ => return None,
        },
        KeyCode::Named(N::Enter) => T::Enter,
        KeyCode::Named(N::Backspace) if word => T::BackspaceWord,
        KeyCode::Named(N::Backspace) => T::Backspace,
        KeyCode::Named(N::Delete) if word => T::DeleteWord,
        KeyCode::Named(N::Delete) => T::Delete,
        KeyCode::Named(N::ArrowLeft) => {
            match (m.shift, word, m.command && cfg!(target_os = "macos")) {
                (true, _, true) => T::SelectLeftLine,
                (false, _, true) => T::MoveLeftLine,
                (true, true, _) => T::SelectLeftWord,
                (false, true, _) => T::MoveLeftWord,
                (true, false, _) => T::SelectLeft,
                (false, false, _) => T::MoveLeft,
            }
        }
        KeyCode::Named(N::ArrowRight) => {
            match (m.shift, word, m.command && cfg!(target_os = "macos")) {
                (true, _, true) => T::SelectRightLine,
                (false, _, true) => T::MoveRightLine,
                (true, true, _) => T::SelectRightWord,
                (false, true, _) => T::MoveRightWord,
                (true, false, _) => T::SelectRight,
                (false, false, _) => T::MoveRight,
            }
        }
        KeyCode::Named(N::Home) => match (m.shift, shortcut) {
            (true, true) => T::SelectLeftDocument,
            (false, true) => T::MoveLeftDocument,
            (true, false) => T::SelectLeftLine,
            (false, false) => T::MoveLeftLine,
        },
        KeyCode::Named(N::End) => match (m.shift, shortcut) {
            (true, true) => T::SelectRightDocument,
            (false, true) => T::MoveRightDocument,
            (true, false) => T::SelectRightLine,
            (false, false) => T::MoveRightLine,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releasing_a_movement_shortcut_does_not_leave_modifiers_held() {
        use ruffle_core::events::KeyCode as FlashKey;

        for modifier in [NamedKey::Control, NamedKey::Alt] {
            let player = ruffle_core::PlayerBuilder::new().build();
            let mut player = player.lock().unwrap();
            let mut effects = Vec::new();
            let key = |code| Key {
                code,
                modifiers: Modifiers::default(),
            };
            for event in [
                ViewEvent::KeyDown(key(KeyCode::Named(modifier))),
                ViewEvent::KeyDown(key(KeyCode::Char('w'))),
                ViewEvent::KeyUp(key(KeyCode::Char('w'))),
                ViewEvent::KeyUp(key(KeyCode::Named(modifier))),
                ViewEvent::KeyDown(key(KeyCode::Char('w'))),
            ] {
                translate(
                    event,
                    Viewport::new(100.0, 100.0, 1.0).unwrap(),
                    &mut effects,
                );
                for effect in effects.drain(..) {
                    if let Effect::Event(event) = effect {
                        player.handle_event(event);
                    }
                }
            }
            assert_eq!(
                player.mutate_with_update_context(|context| (
                    context.input.is_key_down(FlashKey::CONTROL),
                    context.input.is_key_down(FlashKey::ALT),
                    context.input.is_key_down(FlashKey::W),
                )),
                (false, false, true),
                "{modifier:?}",
            );
        }
    }

    #[test]
    fn overlapping_movement_keys_release_independently() {
        use ruffle_core::events::KeyCode as FlashKey;

        let player = ruffle_core::PlayerBuilder::new().build();
        let mut player = player.lock().unwrap();
        let mut effects = Vec::new();
        let key = |c| Key {
            code: KeyCode::Char(c),
            modifiers: Modifiers::default(),
        };
        for (event, expected) in [
            (ViewEvent::KeyDown(key('w')), (true, false)),
            (ViewEvent::KeyDown(key('d')), (true, true)),
            (ViewEvent::KeyUp(key('w')), (false, true)),
            (ViewEvent::KeyUp(key('d')), (false, false)),
            (ViewEvent::KeyDown(key('W')), (true, false)),
            (ViewEvent::KeyUp(key('w')), (false, false)),
        ] {
            translate(
                event,
                Viewport::new(100.0, 100.0, 1.0).unwrap(),
                &mut effects,
            );
            for effect in effects.drain(..) {
                if let Effect::Event(event) = effect {
                    player.handle_event(event);
                }
            }
            assert_eq!(
                player.mutate_with_update_context(|context| (
                    context.input.is_key_down(FlashKey::W),
                    context.input.is_key_down(FlashKey::D),
                )),
                expected,
            );
        }
    }

    #[test]
    fn view_events_translate_to_literal_player_effects() {
        let cases = [
            (
                ViewEvent::MouseDown {
                    at: Point { x: 10.0, y: 20.5 },
                    button: MouseButton::Left,
                    clicks: 2,
                },
                "[Event(MouseDown { x: 20.0, y: 41.0, button: Left, index: Some(1) })]",
            ),
            (
                ViewEvent::KeyDown(Key {
                    code: KeyCode::Named(NamedKey::Backspace),
                    modifiers: Modifiers::default(),
                }),
                "[Event(KeyDown { key: KeyDescriptor { physical_key: Backspace, logical_key: Named(Backspace), key_location: Standard } }), Event(TextControl { code: Backspace })]",
            ),
            (
                ViewEvent::Text("é\n".into()),
                "[Event(TextInput { codepoint: 'é' })]",
            ),
            (ViewEvent::Focus(false), "[Event(FocusLost)]"),
            (ViewEvent::Visible(false), "[Visible(false)]"),
        ];
        for (event, expected) in cases {
            let mut effects = Vec::new();
            translate(
                event,
                Viewport::new(1024.0, 740.0, 2.0).unwrap(),
                &mut effects,
            );
            assert_eq!(format!("{effects:?}"), expected);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn command_c_is_a_key_and_copy() {
        let mut effects = Vec::new();
        translate(
            ViewEvent::KeyDown(Key {
                code: KeyCode::Char('c'),
                modifiers: Modifiers {
                    command: true,
                    ..Default::default()
                },
            }),
            Viewport::new(1024.0, 740.0, 2.0).unwrap(),
            &mut effects,
        );
        assert_eq!(
            format!("{effects:?}"),
            "[Event(KeyDown { key: KeyDescriptor { physical_key: KeyC, logical_key: Character('c'), key_location: Standard } }), Event(TextControl { code: Copy })]"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn control_is_the_text_shortcut_modifier() {
        let control = Modifiers {
            control: true,
            ..Default::default()
        };
        let control_shift = Modifiers {
            shift: true,
            ..control
        };
        let command = Modifiers {
            command: true,
            ..Default::default()
        };
        let cases = [
            (KeyCode::Char('c'), control, "Some(Copy)"),
            (KeyCode::Char('V'), control_shift, "Some(Paste)"),
            (
                KeyCode::Named(NamedKey::Backspace),
                control,
                "Some(BackspaceWord)",
            ),
            (
                KeyCode::Named(NamedKey::ArrowLeft),
                control_shift,
                "Some(SelectLeftWord)",
            ),
            (
                KeyCode::Named(NamedKey::Home),
                control,
                "Some(MoveLeftDocument)",
            ),
            (KeyCode::Char('c'), command, "None"),
            (
                KeyCode::Named(NamedKey::ArrowLeft),
                command,
                "Some(MoveLeft)",
            ),
        ];
        for (code, modifiers, expected) in cases {
            let key = Key { code, modifiers };
            assert_eq!(format!("{:?}", text_control(key)), expected, "{key:?}");
        }
    }

    #[test]
    fn viewport_validates_and_scales_logical_dimensions() {
        assert_eq!(
            Viewport::new(1024.0, 740.0, 2.0).map(|v| (v.width(), v.height(), v.scale())),
            Some((2048, 1480, 2.0))
        );
        for (width, height, scale) in [
            (0.0, 740.0, 2.0),
            (-1.0, 10.0, 1.0),
            (10.0, 0.0, 1.0),
            (10.0, 10.0, f32::NAN),
            (10.0, 10.0, 0.0),
        ] {
            assert_eq!(Viewport::new(width, height, scale), None);
        }
    }
}
