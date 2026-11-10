use std::collections::HashMap;

use gpui::{
    App, Bounds, FocusHandle, Global, IntoElement, KeyDownEvent, Pixels, ScrollHandle, Styled as _,
    Window, WindowId, canvas, px,
};

const REVEAL_MARGIN: Pixels = px(24.0);

#[derive(Default)]
struct Reveal {
    pending: bool,
    roots: HashMap<WindowId, ScrollHandle>,
}

impl Global for Reveal {}

pub fn set_scroll_root(handle: &ScrollHandle, window: &Window, cx: &mut App) {
    cx.default_global::<Reveal>()
        .roots
        .insert(window.window_handle().window_id(), handle.clone());
}

pub(crate) fn request_reveal(cx: &mut App) {
    cx.default_global::<Reveal>().pending = true;
}

pub(crate) fn reveal_probe() -> impl IntoElement {
    canvas(reveal, |_, _, _, _| {}).absolute().inset_0()
}

fn reveal(target: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let Some(state) = cx.try_global::<Reveal>() else {
        return;
    };
    if !state.pending {
        return;
    }
    let Some(root) = state
        .roots
        .get(&window.window_handle().window_id())
        .cloned()
    else {
        return;
    };
    cx.global_mut::<Reveal>().pending = false;
    let viewport = root.bounds();
    if target.right() < viewport.left() || target.left() > viewport.right() {
        return;
    }
    let mut offset = root.offset();
    let top = viewport.top() + REVEAL_MARGIN;
    let bottom = viewport.bottom() - REVEAL_MARGIN;
    if target.top() < top {
        offset.y += top - target.top();
    } else if target.bottom() > bottom {
        offset.y -= target.bottom() - bottom;
    } else {
        return;
    }
    offset.y = offset.y.clamp(-root.max_offset().y, px(0.0));
    root.set_offset(offset);
    window.request_animation_frame();
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Next,
    Prev,
    First,
    Last,
    PageDown,
    PageUp,
}

impl Step {
    pub fn from_key(event: &KeyDownEvent, axis: Axis) -> Option<Step> {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.shift {
            return None;
        }
        let cmd = modifiers.platform;
        match (event.keystroke.key.as_str(), axis) {
            ("down", Axis::Vertical) | ("right", Axis::Horizontal) if cmd => Some(Step::Last),
            ("up", Axis::Vertical) | ("left", Axis::Horizontal) if cmd => Some(Step::First),
            ("down", Axis::Vertical) | ("right", Axis::Horizontal) => Some(Step::Next),
            ("up", Axis::Vertical) | ("left", Axis::Horizontal) => Some(Step::Prev),
            ("home", _) => Some(Step::First),
            ("end", _) => Some(Step::Last),
            ("pagedown", Axis::Vertical) => Some(Step::PageDown),
            ("pageup", Axis::Vertical) => Some(Step::PageUp),
            _ => None,
        }
    }
}

pub fn step(
    current: Option<usize>,
    len: usize,
    step: Step,
    wrap: bool,
    enabled: impl Fn(usize) -> bool,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let current = current.filter(|&at| at < len);
    let forward = |from: usize, by: usize| {
        let mut index = from;
        let mut moved = 0;
        let mut last = None;
        for _ in 0..len {
            index = if index + 1 >= len {
                if !wrap {
                    break;
                }
                0
            } else {
                index + 1
            };
            if enabled(index) {
                last = Some(index);
                moved += 1;
                if moved >= by {
                    break;
                }
            }
        }
        last
    };
    let backward = |from: usize, by: usize| {
        let mut index = from;
        let mut moved = 0;
        let mut last = None;
        for _ in 0..len {
            index = if index == 0 {
                if !wrap {
                    break;
                }
                len - 1
            } else {
                index - 1
            };
            if enabled(index) {
                last = Some(index);
                moved += 1;
                if moved >= by {
                    break;
                }
            }
        }
        last
    };
    let first = || (0..len).find(|&i| enabled(i));
    let last = || (0..len).rev().find(|&i| enabled(i));
    match (step, current) {
        (Step::First, _) => first(),
        (Step::Last, _) => last(),
        (Step::Next, None) | (Step::PageDown, None) => first(),
        (Step::Prev, None) | (Step::PageUp, None) => last(),
        (Step::Next, Some(at)) => forward(at, 1).or(current),
        (Step::Prev, Some(at)) => backward(at, 1).or(current),
        (Step::PageDown, Some(at)) => forward_page(at, len, &enabled).or(current),
        (Step::PageUp, Some(at)) => backward_page(at, &enabled).or(current),
    }
}

const PAGE: usize = 10;

fn forward_page(at: usize, len: usize, enabled: &impl Fn(usize) -> bool) -> Option<usize> {
    let target = (at + PAGE).min(len - 1);
    (at + 1..=target).rev().find(|&i| enabled(i))
}

fn backward_page(at: usize, enabled: &impl Fn(usize) -> bool) -> Option<usize> {
    let target = at.saturating_sub(PAGE);
    (target..at).find(|&i| enabled(i))
}

pub fn typeahead(
    key: &str,
    labels: impl Iterator<Item = (usize, String)>,
    current: Option<usize>,
) -> Option<usize> {
    let mut chars = key.chars();
    let (Some(letter), None) = (chars.next(), chars.next()) else {
        return None;
    };
    if !letter.is_alphanumeric() {
        return None;
    }
    let letter = letter.to_lowercase().next()?;
    let matches: Vec<usize> = labels
        .filter(|(_, label)| label.to_lowercase().starts_with(letter))
        .map(|(index, _)| index)
        .collect();
    matches
        .iter()
        .find(|&&index| current.is_none_or(|at| index > at))
        .or(matches.first())
        .copied()
}

pub fn keyboard_focused(handle: &FocusHandle, window: &Window) -> bool {
    handle.is_focused(window) && window.last_input_was_keyboard()
}

pub fn focus_first(container: &FocusHandle, window: &mut Window, cx: &mut App) {
    window.focus(container, cx);
    window.focus_next(cx);
    if !container.contains_focused(window, cx) {
        window.focus(container, cx);
    }
}

pub fn trap_tab(container: &FocusHandle, backwards: bool, window: &mut Window, cx: &mut App) {
    for _ in 0..64 {
        if backwards {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        if container.contains_focused(window, cx) {
            return;
        }
    }
}

pub fn focus_handle(
    id: impl Into<gpui::ElementId>,
    window: &mut Window,
    cx: &mut App,
) -> FocusHandle {
    window
        .use_keyed_state(id, cx, |_, cx| cx.focus_handle())
        .read(cx)
        .clone()
}

#[cfg(test)]
mod tests {
    use super::{Step, step, typeahead};

    #[test]
    fn steps_skip_disabled_items_and_wrap_only_when_asked() {
        let enabled = |i: usize| i != 1;
        assert_eq!(step(Some(0), 4, Step::Next, false, enabled), Some(2));
        assert_eq!(step(Some(3), 4, Step::Next, false, enabled), Some(3));
        assert_eq!(step(Some(3), 4, Step::Next, true, enabled), Some(0));
        assert_eq!(step(Some(2), 4, Step::Prev, false, enabled), Some(0));
        assert_eq!(step(Some(0), 4, Step::Prev, true, enabled), Some(3));
        assert_eq!(step(Some(9), 4, Step::Prev, false, enabled), Some(3));
        assert_eq!(step(Some(9), 4, Step::Next, false, enabled), Some(0));
    }

    #[test]
    fn first_and_last_land_on_enabled_items() {
        let enabled = |i: usize| i != 0 && i != 4;
        assert_eq!(step(Some(2), 5, Step::First, false, enabled), Some(1));
        assert_eq!(step(Some(2), 5, Step::Last, false, enabled), Some(3));
        assert_eq!(step(None, 5, Step::Next, false, enabled), Some(1));
        assert_eq!(step(None, 0, Step::Next, true, |_| true), None);
    }

    #[test]
    fn pages_stop_at_the_ends() {
        assert_eq!(step(Some(3), 30, Step::PageDown, false, |_| true), Some(13));
        assert_eq!(
            step(Some(25), 30, Step::PageDown, false, |_| true),
            Some(29)
        );
        assert_eq!(step(Some(4), 30, Step::PageUp, false, |_| true), Some(0));
    }

    #[test]
    fn typeahead_cycles_through_matches_after_the_current_item() {
        let labels = || {
            ["Apple", "Grape", "Guava", "Tomato"]
                .into_iter()
                .enumerate()
                .map(|(i, label)| (i, label.to_string()))
        };
        assert_eq!(typeahead("g", labels(), None), Some(1));
        assert_eq!(typeahead("g", labels(), Some(1)), Some(2));
        assert_eq!(typeahead("G", labels(), Some(2)), Some(1));
        assert_eq!(typeahead("z", labels(), None), None);
        assert_eq!(typeahead("tab", labels(), None), None);
    }
}
