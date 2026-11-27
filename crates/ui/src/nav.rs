use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use gpui::{
    App, Bounds, DispatchPhase, FocusHandle, Global, IntoElement, KeyDownEvent, Keystroke,
    MouseDownEvent, Pixels, ScrollHandle, Styled as _, Window, WindowId, canvas, px,
};

const REVEAL_MARGIN: Pixels = px(24.0);

#[derive(Default)]
struct Reveal {
    pending: bool,
    roots: HashMap<WindowId, ScrollHandle>,
    panes: Vec<(ScrollHandle, Pixels)>,
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

#[derive(Default)]
struct KeyboardWindows(HashSet<WindowId>);

impl Global for KeyboardWindows {}

pub(crate) fn track_input(cx: &mut App) {
    cx.intercept_keystrokes(|_, window, cx| set_keyboard(true, window, cx))
        .detach();
    cx.on_window_closed(|cx, id| {
        cx.default_global::<KeyboardWindows>().0.remove(&id);
    })
    .detach();
}

pub fn pointer_probe() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |_, _, window, _| {
            window.on_mouse_event(|_: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    set_keyboard(false, window, cx);
                }
            });
        },
    )
    .absolute()
}

fn set_keyboard(keyboard: bool, window: &mut Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let windows = &mut cx.default_global::<KeyboardWindows>().0;
    let changed = if keyboard {
        windows.insert(id)
    } else {
        windows.remove(&id)
    };
    if changed {
        window.refresh();
    }
}

pub fn keyboard_input(window: &Window, cx: &App) -> bool {
    cx.try_global::<KeyboardWindows>()
        .is_some_and(|windows| windows.0.contains(&window.window_handle().window_id()))
}

pub(crate) fn in_modal(window: &Window) -> bool {
    window
        .context_stack()
        .iter()
        .any(|ctx| ctx.contains("Modal"))
}

pub fn pane_start(handle: ScrollHandle) -> impl IntoElement {
    pane_start_above(handle, Pixels::ZERO)
}

pub fn pane_start_above(handle: ScrollHandle, covered: Pixels) -> impl IntoElement {
    canvas(
        move |_, _, cx| {
            cx.default_global::<Reveal>()
                .panes
                .push((handle.clone(), covered))
        },
        |_, _, _, _| {},
    )
    .absolute()
}

pub fn pane_end() -> impl IntoElement {
    canvas(
        |_, _, cx| {
            cx.default_global::<Reveal>().panes.pop();
        },
        |_, _, _, _| {},
    )
    .absolute()
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
    let Some((root, covered)) = state.panes.last().cloned().or_else(|| {
        state
            .roots
            .get(&window.window_handle().window_id())
            .map(|root| (root.clone(), Pixels::ZERO))
    }) else {
        return;
    };
    cx.global_mut::<Reveal>().pending = false;
    let viewport = root.bounds();
    if target.right() < viewport.left() || target.left() > viewport.right() {
        return;
    }
    let mut offset = root.offset();
    let visible = viewport.size.height - covered;
    let margin = REVEAL_MARGIN.min(((visible - target.size.height) / 2.0).max(px(0.0)));
    let top = viewport.top() + margin;
    let bottom = viewport.bottom() - covered.max(margin);
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

const TYPEAHEAD_RESET: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct Typeahead {
    query: String,
    typed_at: Option<Instant>,
}

impl Typeahead {
    pub fn end_unless_typed(&mut self, keystroke: &Keystroke) {
        if typed(keystroke).is_none() {
            self.query.clear();
        }
    }

    pub fn typing(&self, now: Instant) -> bool {
        !self.query.is_empty()
            && self
                .typed_at
                .is_some_and(|at| now.duration_since(at) < TYPEAHEAD_RESET)
    }

    pub fn find(
        &mut self,
        keystroke: &Keystroke,
        now: Instant,
        labels: impl Iterator<Item = (usize, impl AsRef<str>)>,
        current: Option<usize>,
    ) -> Option<usize> {
        let typed = typed(keystroke)?;
        if !self.typing(now) {
            if typed == ' ' {
                return None;
            }
            self.query.clear();
        }
        self.query.extend(typed.to_lowercase());
        self.typed_at = Some(now);
        let first = self.query.chars().next()?;
        let search = if self.query.chars().all(|c| c == first) {
            &self.query[..first.len_utf8()]
        } else {
            &self.query
        };
        let start = match (current, search.len() == first.len_utf8()) {
            (Some(at), true) => at + 1,
            (Some(at), false) => at,
            (None, _) => 0,
        };
        let mut wrapped = None;
        for (index, label) in labels {
            if !starts_with(label.as_ref(), search) {
                continue;
            }
            if index >= start {
                return Some(index);
            }
            wrapped.get_or_insert(index);
        }
        wrapped
    }
}

fn typed(keystroke: &Keystroke) -> Option<char> {
    let modifiers = keystroke.modifiers;
    if modifiers.control || modifiers.platform || modifiers.function || modifiers.alt {
        return None;
    }
    let mut chars = keystroke.key_char.as_deref()?.chars();
    let (Some(typed), None) = (chars.next(), chars.next()) else {
        return None;
    };
    (!typed.is_control()).then_some(typed)
}

fn starts_with(label: &str, lowercase_prefix: &str) -> bool {
    let mut label = label.chars().flat_map(char::to_lowercase);
    lowercase_prefix.chars().all(|c| label.next() == Some(c))
}

pub fn keyboard_focused(handle: &FocusHandle, window: &Window, cx: &App) -> bool {
    handle.is_focused(window) && keyboard_input(window, cx)
}

pub fn focus_first(container: &FocusHandle, window: &mut Window, cx: &mut App) {
    window.focus(container, cx);
    window.focus_next(cx);
    if !container.contains_focused(window, cx) {
        window.focus(container, cx);
    }
}

pub fn trap_tab(container: &FocusHandle, backwards: bool, window: &mut Window, cx: &mut App) {
    request_reveal(cx);
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
    use std::time::{Duration, Instant};

    use gpui::Keystroke;

    use super::{Step, Typeahead, step};

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
    fn typeahead_matches_the_typed_prefix_until_a_pause() {
        let labels = || {
            [
                "Artix",
                "Sepulchure",
                "Safiria",
                "Sir Ver",
                "Swordhaven",
                "Swordhaven (EU)",
            ]
            .into_iter()
            .enumerate()
        };
        let key = |key: &'static str| Keystroke::parse(key).unwrap().with_simulated_ime();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut typeahead = Typeahead::default();
        let mut typing = |keys: &'static str, ms, current| {
            keys.split(' ')
                .map(|k| typeahead.find(&key(k), at(ms), labels(), current))
                .last()
                .flatten()
        };
        assert_eq!(typing("s a", 0, None), Some(2));
        assert_eq!(typing("s i r space", 2000, None), Some(3));
        assert_eq!(typing("s w o r d h a v e n space", 4000, None), Some(5));
        assert_eq!(typing("shift-s", 6000, Some(1)), Some(2));
        assert_eq!(typing("s", 6100, Some(2)), Some(3));
        assert_eq!(typing("a", 8000, Some(4)), Some(0));
        assert_eq!(typing("z", 10000, Some(4)), None);
        assert_eq!(typing("space", 12000, Some(4)), None);
        assert_eq!(typing("cmd-a", 14000, None), None);
        assert_eq!(typing("tab", 16000, None), None);
        let mut typeahead = Typeahead::default();
        typeahead.find(&key("s"), at(0), labels(), None);
        typeahead.end_unless_typed(&key("right"));
        assert_eq!(typeahead.find(&key("a"), at(100), labels(), None), Some(0));
    }
}
