use std::{cell::RefCell, ops::Range, rc::Rc};

use gpui::{
    AnyElement, App, Div, ElementId, FollowMode, InteractiveElement as _, IntoElement, KeyBinding,
    KeyDownEvent, ListAlignment, ListState, MouseButton, ParentElement as _, Pixels, RenderOnce,
    Role, SharedString, StatefulInteractiveElement as _, Styled as _, Window, canvas, div, list,
    prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, Theme, Tone, h_flex,
    nav::{self, Step},
    primitives::{
        Button, ContextMenu, MenuItem, Scrollbar,
        button::{Size, Variant},
        text_field,
    },
    theme::tabular,
};

const CONTEXT: &str = "LogView";
const LINE: Pixels = px(17.0);
const ROW_INSET: Pixels = px(1.5);
const OPTICAL_LIFT: Pixels = px(0.5);
const SECTION_GAP: Pixels = px(6.0);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-c", text_field::Copy, Some(CONTEXT)),
        KeyBinding::new("cmd-a", text_field::SelectAll, Some(CONTEXT)),
    ]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

pub enum LogBody {
    Text(SharedString),
    Divider(SharedString),
}

pub struct LogRow {
    pub time: SharedString,
    pub level: LogLevel,
    pub body: LogBody,
    pub repeats: u32,
    pub opens_section: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Selection {
    anchor: usize,
    head: usize,
}

impl Selection {
    fn range(self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head) + 1
    }

    fn spliced(self, removed: Range<usize>, inserted: usize) -> Option<Self> {
        let moved = |ix: usize| {
            if ix < removed.start {
                Some(ix)
            } else if ix >= removed.end {
                Some(ix - removed.len() + inserted)
            } else {
                None
            }
        };
        match (moved(self.anchor), moved(self.head)) {
            (Some(anchor), Some(head)) => Some(Self { anchor, head }),
            (Some(anchor), None) => Some(Self {
                anchor,
                head: removed.start,
            }),
            (None, Some(head)) => Some(Self {
                anchor: removed.start,
                head,
            }),
            (None, None) => None,
        }
    }
}

struct Inner {
    list: ListState,
    selection: Option<Selection>,
}

#[derive(Clone)]
pub struct LogState(Rc<RefCell<Inner>>);

impl Default for LogState {
    fn default() -> Self {
        let list = ListState::new(0, ListAlignment::Top, px(400.0));
        list.set_follow_mode(FollowMode::Tail);
        Self(Rc::new(RefCell::new(Inner {
            list,
            selection: None,
        })))
    }
}

impl LogState {
    pub fn reset(&self, count: usize) {
        let mut inner = self.0.borrow_mut();
        inner.selection = None;
        inner.list.reset(count);
        inner.list.set_follow_mode(FollowMode::Tail);
    }

    pub fn splice(&self, removed: Range<usize>, inserted: usize) {
        let mut inner = self.0.borrow_mut();
        inner.list.splice(removed.clone(), inserted);
        if removed.len() != inserted {
            inner.selection = inner
                .selection
                .and_then(|selection| selection.spliced(removed, inserted));
        }
    }

    pub fn len(&self) -> usize {
        self.0.borrow().list.item_count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selection(&self) -> Option<Range<usize>> {
        self.0.borrow().selection.map(Selection::range)
    }

    pub fn following(&self) -> bool {
        let inner = self.0.borrow();
        inner.list.is_following_tail() || inner.list.max_offset_for_scrollbar().y <= px(0.0)
    }

    pub fn jump_to_latest(&self) {
        self.0.borrow().list.set_follow_mode(FollowMode::Tail);
    }

    fn list(&self) -> ListState {
        self.0.borrow().list.clone()
    }

    fn selected(&self, ix: usize) -> bool {
        self.selection().is_some_and(|range| range.contains(&ix))
    }

    fn head(&self) -> Option<usize> {
        self.0.borrow().selection.map(|selection| selection.head)
    }

    fn select(&self, ix: usize, extend: bool, reveal: bool) {
        let mut inner = self.0.borrow_mut();
        let anchor = match inner.selection {
            Some(selection) if extend => selection.anchor,
            _ => ix,
        };
        inner.selection = Some(Selection { anchor, head: ix });
        let viewport = inner.list.viewport_bounds();
        let shown = inner
            .list
            .bounds_for_item(ix)
            .is_some_and(|row| row.top() >= viewport.top() && row.bottom() <= viewport.bottom());
        if reveal && !shown {
            inner.list.pause_following_tail();
            inner.list.scroll_to_reveal_item(ix);
        }
    }

    fn select_all(&self) {
        let mut inner = self.0.borrow_mut();
        let count = inner.list.item_count();
        inner.selection = count.checked_sub(1).map(|last| Selection {
            anchor: 0,
            head: last,
        });
    }
}

type RowBuilder = Rc<dyn Fn(usize, &App) -> LogRow>;
type SectionOf = Rc<dyn Fn(usize) -> Option<usize>>;
type CopyHandler = Rc<dyn Fn(Range<usize>, &mut App)>;

#[derive(IntoElement)]
pub struct LogView {
    id: ElementId,
    label: SharedString,
    state: LogState,
    row: RowBuilder,
    section_of: SectionOf,
    on_copy: CopyHandler,
}

impl LogView {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        state: LogState,
        row: impl Fn(usize, &App) -> LogRow + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            state,
            row: Rc::new(row),
            section_of: Rc::new(|_| None),
            on_copy: Rc::new(|_, _| {}),
        }
    }

    pub fn section_of(mut self, f: impl Fn(usize) -> Option<usize> + 'static) -> Self {
        self.section_of = Rc::new(f);
        self
    }

    pub fn on_copy(mut self, f: impl Fn(Range<usize>, &mut App) + 'static) -> Self {
        self.on_copy = Rc::new(f);
        self
    }
}

fn step_for(event: &KeyDownEvent) -> Option<(Step, bool)> {
    let modifiers = event.keystroke.modifiers;
    if modifiers.control || modifiers.alt {
        return None;
    }
    let step = match (event.keystroke.key.as_str(), modifiers.platform) {
        ("down", true) | ("end", _) => Step::Last,
        ("up", true) | ("home", _) => Step::First,
        ("down", false) => Step::Next,
        ("up", false) => Step::Prev,
        ("pagedown", _) => Step::PageDown,
        ("pageup", _) => Step::PageUp,
        _ => return None,
    };
    Some((step, modifiers.shift))
}

fn ink(theme: &Theme, level: LogLevel) -> gpui::Hsla {
    match level {
        LogLevel::Debug => theme.muted_foreground,
        LogLevel::Info => theme.foreground,
        LogLevel::Warn => theme.tone(Tone::Attention).2,
        LogLevel::Error => theme.tone(Tone::Danger).2,
    }
}

fn mono(theme: &Theme) -> Div {
    div()
        .font_family(theme.mono_family.clone())
        .text_size(theme.typography.mono)
        .line_height(LINE)
}

fn row_frame(theme: &Theme, level: LogLevel, selected: bool) -> Div {
    let fill = match level {
        LogLevel::Warn => Some(theme.warning_row()),
        LogLevel::Error => Some(theme.destructive_row()),
        LogLevel::Debug | LogLevel::Info => None,
    };
    h_flex()
        .w_full()
        .items_start()
        .gap(theme.space(2.5))
        .px(theme.space(3.0))
        .map(|row| match (selected, fill) {
            (true, _) => row.bg(theme.muted),
            (false, Some(fill)) => row.bg(fill),
            (false, None) => row,
        })
}

fn time(theme: &Theme, time: SharedString) -> Div {
    mono(theme)
        .flex_none()
        .font_features(tabular())
        .text_color(theme.muted_foreground)
        .child(time)
}

fn divider(theme: &Theme, label: SharedString, selected: bool) -> Div {
    row_frame(theme, LogLevel::Info, selected)
        .h(LINE + ROW_INSET * 2.0)
        .items_center()
        .child(
            mono(theme)
                .flex_none()
                .text_color(theme.muted_foreground)
                .child(label),
        )
        .child(div().flex_1().h(px(1.0)).bg(theme.border))
}

fn line(theme: &Theme, row: LogRow, selected: bool, first: bool) -> Div {
    let level = row.level;
    let text = match row.body {
        LogBody::Text(text) => text,
        LogBody::Divider(label) => return divider(theme, label, selected),
    };
    let separated = row.opens_section && !first;
    let repeats = (row.repeats > 1).then(|| format!("×{}", row.repeats));
    let line = row_frame(theme, level, selected)
        .when(separated, |row| row.border_t_1().border_color(theme.border))
        .pt(ROW_INSET - OPTICAL_LIFT + if separated { px(2.0) } else { px(0.0) })
        .pb(ROW_INSET + OPTICAL_LIFT)
        .child(time(theme, row.time))
        .child(
            mono(theme)
                .flex_1()
                .min_w_0()
                .text_color(ink(theme, level))
                .child(text),
        )
        .children(repeats.map(|count| {
            mono(theme)
                .flex_none()
                .font_features(tabular())
                .text_color(theme.muted_foreground)
                .child(count)
        }));
    if separated {
        div().w_full().pt(SECTION_GAP).child(line)
    } else {
        line
    }
}

fn pinned(theme: &Theme, row: LogRow, shift: Pixels) -> AnyElement {
    let content = match row.body {
        LogBody::Text(text) => {
            let first = text.lines().next().unwrap_or_default().to_owned();
            row_frame(theme, row.level, false)
                .pt(ROW_INSET - OPTICAL_LIFT)
                .pb(ROW_INSET + OPTICAL_LIFT)
                .child(time(theme, row.time))
                .child(
                    mono(theme)
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(ink(theme, row.level))
                        .child(first),
                )
        }
        LogBody::Divider(label) => divider(theme, label, false),
    };
    div()
        .debug_selector(|| "log-pinned".into())
        .absolute()
        .left_0()
        .right_0()
        .top(shift)
        .bg(theme.background)
        .border_b_1()
        .border_color(theme.border)
        .child(content)
        .into_any_element()
}

fn pinned_at(list: &ListState, section_of: &SectionOf) -> Option<(usize, Pixels)> {
    let top = list.logical_scroll_top();
    let opener = section_of(top.item_ix)
        .filter(|&opener| opener < top.item_ix || top.offset_in_item > px(0.0))?;
    let viewport = list.viewport_bounds();
    let height = pinned_height();
    let next = (top.item_ix + 1..list.item_count())
        .map_while(|ix| Some((ix, list.bounds_for_item(ix)?)))
        .take_while(|(_, bounds)| bounds.top() < viewport.top() + height)
        .find(|&(ix, _)| section_of(ix) == Some(ix));
    let shift = next.map_or(px(0.0), |(_, bounds)| {
        (bounds.top() - viewport.top() - height).min(px(0.0))
    });
    Some((opener, shift))
}

fn pinned_height() -> Pixels {
    LINE + ROW_INSET * 2.0 + px(1.0)
}

impl RenderOnce for LogView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let list_state = self.state.list();
        let count = list_state.item_count();

        let rows = {
            let state = self.state.clone();
            let row = self.row.clone();
            let theme = theme.clone();
            let focus = focus.clone();
            list(list_state.clone(), move |ix, _, cx| {
                let selected = state.selected(ix);
                let pick = state.clone();
                let keep = state.clone();
                let focus = focus.clone();
                line(&theme, row(ix, cx), selected, ix == 0)
                    .id(ix)
                    .role(Role::ListBoxOption)
                    .aria_selected(selected)
                    .aria_position_in_set(ix + 1)
                    .aria_size_of_set(count)
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        window.focus(&focus, cx);
                        pick.select(ix, event.modifiers.shift, false);
                        window.refresh();
                    })
                    .on_mouse_down(MouseButton::Right, move |_, window, _| {
                        if !keep.selected(ix) {
                            keep.select(ix, false, false);
                            window.refresh();
                        }
                    })
                    .into_any_element()
            })
            .size_full()
        };

        let pinned_at = pinned_at(&list_state, &self.section_of);
        let sticky = pinned_at.map(|(opener, shift)| pinned(&theme, (self.row)(opener, cx), shift));

        let following = self.state.following();
        let settled = {
            let state = self.state.clone();
            let list = list_state.clone();
            let section_of = self.section_of.clone();
            canvas(
                move |_, window, _| {
                    if state.following() != following
                        || self::pinned_at(&list, &section_of) != pinned_at
                    {
                        window.request_animation_frame();
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_0()
        };
        let jump = (!following).then(|| {
            let state = self.state.clone();
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(theme.space(3.0))
                .flex()
                .justify_center()
                .child(
                    theme
                        .floating_bar()
                        .debug_selector(|| "log-jump".into())
                        .pl(theme.space(1.0))
                        .child(
                            Button::new((self.id.clone(), "jump"), "Jump to latest")
                                .variant(Variant::Ghost)
                                .size(Size::Small)
                                .on_click(move |_, window, _| {
                                    state.jump_to_latest();
                                    window.refresh();
                                }),
                        ),
                )
        });

        let keys = self.state.clone();
        let all = self.state.clone();
        let copying = self.state.clone();
        let copy = self.on_copy.clone();
        let menu_state = self.state.clone();
        let menu_copy = self.on_copy.clone();
        let trace = div()
            .id(self.id.clone())
            .key_context(CONTEXT)
            .role(Role::ListBox)
            .aria_label(self.label)
            .track_focus(&focus.tab_stop(true))
            .relative()
            .size_full()
            .overflow_hidden()
            .on_key_down(move |event, window, cx| {
                let Some((step, extend)) = step_for(event) else {
                    return;
                };
                let next = match keys.head() {
                    None if matches!(step, Step::Next | Step::Prev) => keys.len().checked_sub(1),
                    head => nav::step(head, keys.len(), step, false, |_| true),
                };
                if let Some(next) = next {
                    keys.select(next, extend, true);
                    window.refresh();
                    cx.stop_propagation();
                }
            })
            .on_action(move |_: &text_field::SelectAll, window, _| {
                all.select_all();
                window.refresh();
            })
            .on_action(move |_: &text_field::Copy, _, cx| {
                if let Some(range) = copying.selection() {
                    copy(range, cx);
                }
            })
            .child(rows)
            .children(sticky)
            .child(Scrollbar::list((self.id.clone(), "scrollbar"), list_state))
            .children(jump)
            .child(settled);
        ContextMenu::new((self.id, "menu"), "Log actions")
            .fill()
            .items(move |_, _| {
                let state = menu_state.clone();
                let copy = menu_copy.clone();
                vec![MenuItem::action("Copy", move |_, cx| {
                    if let Some(range) = state.selection() {
                        copy(range, cx);
                    }
                })]
            })
            .child(trace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_follows_its_rows_as_old_rows_drop_and_new_ones_arrive() {
        let selection = Selection { anchor: 4, head: 6 };
        assert_eq!(
            selection.spliced(0..2, 0),
            Some(Selection { anchor: 2, head: 4 })
        );
        assert_eq!(selection.spliced(10..10, 3), Some(selection));
        assert_eq!(
            selection.spliced(0..5, 0),
            Some(Selection { anchor: 0, head: 1 })
        );
        assert_eq!(selection.spliced(0..7, 0), None);
        assert_eq!(Selection { anchor: 6, head: 4 }.range(), 4..7);
    }
}
