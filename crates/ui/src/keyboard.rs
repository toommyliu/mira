use std::ffi::c_void;

use gpui::Keystroke;

const KEY_CODES: u16 = 128;
const KEYPAD: [u16; 19] = [
    65, 67, 69, 71, 75, 76, 78, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92,
];
const COMMAND: u32 = 1;
const SHIFT: u32 = 2;

pub fn pressed(keystroke: &Keystroke) -> Keystroke {
    with_layout(|layout| {
        Some(pressed_with(
            keystroke,
            live(layout, keystroke.modifiers.platform),
        ))
    })
    .unwrap_or_else(|| keystroke.clone())
}

pub fn typed(keystroke: &Keystroke) -> Keystroke {
    with_layout(|layout| {
        Some(typed_with(
            keystroke,
            live(layout, keystroke.modifiers.platform),
        ))
    })
    .unwrap_or_else(|| keystroke.clone())
}

fn live(layout: *const c_void, command: bool) -> impl Fn(u16, bool) -> Option<String> {
    move |code, shift| {
        if !command {
            return translate(layout, code, if shift { SHIFT } else { 0 });
        }
        let plain = translate(layout, code, COMMAND);
        if !shift {
            return plain;
        }
        match translate(layout, code, COMMAND | SHIFT) {
            both if both != plain => both,
            _ => translate(layout, code, SHIFT),
        }
    }
}

fn pressed_with(keystroke: &Keystroke, layout: impl Fn(u16, bool) -> Option<String>) -> Keystroke {
    let mut pressed = keystroke.clone();
    if !keystroke.modifiers.shift
        && let Some(base) = find_unshifted(&keystroke.key, layout)
    {
        pressed.modifiers.shift = true;
        pressed.key = base;
    }
    pressed
}

fn typed_with(keystroke: &Keystroke, layout: impl Fn(u16, bool) -> Option<String>) -> Keystroke {
    let mut typed = keystroke.clone();
    if keystroke.modifiers.shift
        && let Some(symbol) = find_shifted(&keystroke.key, layout)
    {
        typed.modifiers.shift = false;
        typed.key = symbol;
    }
    typed
}

fn single(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let only = chars.next()?;
    chars.next().is_none().then_some(only)
}

fn main_keys() -> impl Iterator<Item = u16> {
    (0..KEY_CODES).filter(|code| !KEYPAD.contains(code))
}

fn find_shifted(base: &str, translate: impl Fn(u16, bool) -> Option<String>) -> Option<String> {
    if single(base)?.is_ascii_lowercase() {
        return None;
    }
    main_keys()
        .find(|&code| translate(code, false).as_deref() == Some(base))
        .and_then(|code| translate(code, true))
        .filter(|symbol| symbol != base && single(symbol).is_some())
}

fn find_unshifted(symbol: &str, translate: impl Fn(u16, bool) -> Option<String>) -> Option<String> {
    if single(symbol)?.is_ascii_alphabetic() {
        return None;
    }
    let mut shifted = None;
    for code in main_keys() {
        let plain = translate(code, false);
        if plain.as_deref() == Some(symbol) {
            return None;
        }
        if shifted.is_none() && translate(code, true).as_deref() == Some(symbol) {
            shifted = plain.filter(|plain| single(plain).is_some_and(|c| !c.is_ascii_alphabetic()));
        }
    }
    shifted
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
    fn TISGetInputSourceProperty(source: *mut c_void, key: *const c_void) -> *const c_void;
    static kTISPropertyUnicodeKeyLayoutData: *const c_void;
    fn LMGetKbdType() -> u8;
    fn UCKeyTranslate(
        layout: *const c_void,
        code: u16,
        action: u16,
        modifiers: u32,
        keyboard: u32,
        options: u32,
        dead_keys: *mut u32,
        capacity: usize,
        length: *mut usize,
        text: *mut u16,
    ) -> i32;
}

unsafe extern "C" {
    fn pthread_main_np() -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
    fn CFRelease(object: *const c_void);
}

fn with_layout<T>(f: impl FnOnce(*const c_void) -> Option<T>) -> Option<T> {
    if unsafe { pthread_main_np() } == 0 {
        return None;
    }
    unsafe {
        let source = TISCopyCurrentKeyboardLayoutInputSource();
        if source.is_null() {
            return None;
        }
        let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
        let found = if data.is_null() {
            None
        } else {
            f(CFDataGetBytePtr(data).cast())
        };
        CFRelease(source);
        found
    }
}

fn translate(layout: *const c_void, code: u16, modifiers: u32) -> Option<String> {
    let mut dead_keys = 0;
    let mut text = [0u16; 4];
    let mut length = 0;
    let status = unsafe {
        UCKeyTranslate(
            layout,
            code,
            0,
            modifiers,
            u32::from(LMGetKbdType()),
            1,
            &mut dead_keys,
            text.len(),
            &mut length,
            text.as_mut_ptr(),
        )
    };
    (status == 0 && length > 0).then(|| String::from_utf16_lossy(&text[..length]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn us(code: u16, shift: bool) -> Option<String> {
        let (plain, shifted) = match code {
            0 => ("a", "A"),
            18 => ("1", "!"),
            28 => ("8", "*"),
            67 => ("*", "*"),
            30 => ("]", "}"),
            43 => (",", "<"),
            _ => return None,
        };
        Some(if shift { shifted } else { plain }.to_owned())
    }

    fn german(code: u16, shift: bool) -> Option<String> {
        let (plain, shifted) = match code {
            10 => ("<", ">"),
            41 => ("ö", "Ö"),
            43 => (",", ";"),
            _ => return None,
        };
        Some(if shift { shifted } else { plain }.to_owned())
    }

    fn keys(source: &'static str) -> Keystroke {
        Keystroke::parse(source).unwrap()
    }

    fn czech(code: u16, shift: bool) -> Option<String> {
        let (plain, shifted) = match code {
            26 => ("ý", "7"),
            _ => return None,
        };
        Some(if shift { shifted } else { plain }.to_owned())
    }

    fn dvorak_qwerty_command(code: u16, shift: bool) -> Option<String> {
        let (plain, shifted) = match code {
            13 => ("w", "<"),
            43 => (",", "<"),
            _ => return None,
        };
        Some(if shift { shifted } else { plain }.to_owned())
    }

    #[test]
    fn a_shifted_symbol_reads_as_the_keys_pressed_and_back() {
        assert_eq!(pressed_with(&keys("cmd-<"), us), keys("cmd-shift-,"));
        assert_eq!(pressed_with(&keys("cmd-}"), us), keys("cmd-shift-]"));
        assert_eq!(pressed_with(&keys("cmd-!"), us), keys("cmd-shift-1"));
        assert_eq!(typed_with(&keys("cmd-shift-,"), us), keys("cmd-<"));
        assert_eq!(typed_with(&keys("cmd-shift-]"), us), keys("cmd-}"));
        assert_eq!(typed_with(&keys("cmd-shift-1"), us), keys("cmd-!"));
        for unchanged in ["cmd-,", "cmd-shift-a", "cmd-a", "shift-tab", "alt-i"] {
            assert_eq!(pressed_with(&keys(unchanged), us), keys(unchanged));
            assert_eq!(typed_with(&keys(unchanged), us), keys(unchanged));
        }
        assert_eq!(pressed_with(&keys("cmd-*"), us), keys("cmd-shift-8"));
        assert_eq!(typed_with(&keys("cmd-shift-8"), us), keys("cmd-*"));
        assert_eq!(pressed_with(&keys("cmd-<"), german), keys("cmd-<"));
        assert_eq!(pressed_with(&keys("cmd-Ö"), german), keys("cmd-shift-ö"));
        assert_eq!(typed_with(&keys("cmd-shift-ö"), german), keys("cmd-Ö"));
        assert_eq!(pressed_with(&keys("cmd-7"), czech), keys("cmd-shift-ý"));
        assert_eq!(typed_with(&keys("cmd-shift-ý"), czech), keys("cmd-7"));
        assert_eq!(
            pressed_with(&keys("cmd-<"), dvorak_qwerty_command),
            keys("cmd-shift-,")
        );
        assert_eq!(
            typed_with(&keys("cmd-shift-,"), dvorak_qwerty_command),
            keys("cmd-<")
        );
        assert_eq!(typed_with(&keys("cmd-shift-,"), german), keys("cmd-;"));
    }
}
