use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId,
    InteractiveElement as _, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, ParentElement as _, Pixels, Point, Render, Role,
    ShapedLine, SharedString, StatefulInteractiveElement as _, Style, Styled as _, TextRun,
    UTF16Selection, UnderlineStyle, Window, actions, div, fill, point, prelude::FluentBuilder as _,
    px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation as _;

use super::{Button, Icon, button::Size, icon};
use crate::{
    ActiveTheme as _,
    theme::{CONTROL_MD, CONTROL_SM, LINE_BODY, TEXT_BODY, TEXT_SMALL, tabular},
};

actions!(
    text_field,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        SelectHome,
        SelectEnd,
        Paste,
        Cut,
        Copy,
        Submit,
        Cancel,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        DeleteWordLeft,
        DeleteToStart,
        Undo,
        Redo,
        Increment,
        Decrement,
        IncrementMore,
        DecrementMore,
    ]
);

const CONTEXT: &str = "Input";
const BULLET: char = '\u{2022}';

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some(CONTEXT)),
        KeyBinding::new("delete", Delete, Some(CONTEXT)),
        KeyBinding::new("left", Left, Some(CONTEXT)),
        KeyBinding::new("right", Right, Some(CONTEXT)),
        KeyBinding::new("shift-left", SelectLeft, Some(CONTEXT)),
        KeyBinding::new("shift-right", SelectRight, Some(CONTEXT)),
        KeyBinding::new("cmd-a", SelectAll, Some(CONTEXT)),
        KeyBinding::new("cmd-v", Paste, Some(CONTEXT)),
        KeyBinding::new("cmd-c", Copy, Some(CONTEXT)),
        KeyBinding::new("cmd-x", Cut, Some(CONTEXT)),
        KeyBinding::new("home", Home, Some(CONTEXT)),
        KeyBinding::new("cmd-left", Home, Some(CONTEXT)),
        KeyBinding::new("end", End, Some(CONTEXT)),
        KeyBinding::new("cmd-right", End, Some(CONTEXT)),
        KeyBinding::new("shift-home", SelectHome, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-left", SelectHome, Some(CONTEXT)),
        KeyBinding::new("shift-end", SelectEnd, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-right", SelectEnd, Some(CONTEXT)),
        KeyBinding::new("enter", Submit, Some(CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
        KeyBinding::new("alt-left", WordLeft, Some(CONTEXT)),
        KeyBinding::new("alt-right", WordRight, Some(CONTEXT)),
        KeyBinding::new("alt-shift-left", SelectWordLeft, Some(CONTEXT)),
        KeyBinding::new("alt-shift-right", SelectWordRight, Some(CONTEXT)),
        KeyBinding::new("alt-backspace", DeleteWordLeft, Some(CONTEXT)),
        KeyBinding::new("cmd-backspace", DeleteToStart, Some(CONTEXT)),
        KeyBinding::new("cmd-z", Undo, Some(CONTEXT)),
        KeyBinding::new("cmd-shift-z", Redo, Some(CONTEXT)),
        KeyBinding::new("up", Increment, Some(CONTEXT)),
        KeyBinding::new("down", Decrement, Some(CONTEXT)),
        KeyBinding::new("shift-up", IncrementMore, Some(CONTEXT)),
        KeyBinding::new("shift-down", DecrementMore, Some(CONTEXT)),
    ]);
}

pub enum InputEvent {
    Changed,
    Submitted,
}

#[derive(Clone, Copy)]
pub struct Numeric {
    pub min: i64,
    pub max: i64,
    pub step: i64,
}

const HISTORY: usize = 100;

struct Snapshot {
    content: SharedString,
    selected: Range<usize>,
}

pub struct Input {
    focus: FocusHandle,
    label: SharedString,
    content: SharedString,
    placeholder: SharedString,
    search: bool,
    password: bool,
    revealed: bool,
    max_length: Option<usize>,
    numeric: Option<Numeric>,
    suffix: Option<SharedString>,
    small: bool,
    pill: bool,
    bare: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    typing: bool,
    was_focused: bool,
    invalid: bool,
    disabled: bool,
    selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    layout: Option<ShapedLine>,
    bounds: Option<Bounds<Pixels>>,
    scroll: Pixels,
    selecting: bool,
}

impl EventEmitter<InputEvent> for Input {}

impl Input {
    pub fn new(label: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle().tab_stop(true),
            label: label.into(),
            content: SharedString::default(),
            placeholder: SharedString::default(),
            search: false,
            password: false,
            revealed: false,
            max_length: None,
            numeric: None,
            suffix: None,
            small: false,
            pill: false,
            bare: false,
            undo: Vec::new(),
            redo: Vec::new(),
            typing: false,
            was_focused: false,
            invalid: false,
            disabled: false,
            selected: 0..0,
            reversed: false,
            marked: None,
            layout: None,
            bounds: None,
            scroll: Pixels::ZERO,
            selecting: false,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn search(mut self) -> Self {
        self.search = true;
        self
    }

    pub fn password(mut self) -> Self {
        self.password = true;
        self
    }

    pub fn max_length(mut self, max_length: usize) -> Self {
        self.max_length = Some(max_length);
        self
    }

    pub fn numeric(mut self, numeric: Numeric) -> Self {
        self.numeric = Some(numeric);
        self
    }

    pub fn suffix(mut self, suffix: impl Into<SharedString>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    pub fn pill(mut self) -> Self {
        self.pill = true;
        self
    }

    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }

    pub fn reveal(&mut self, revealed: bool, cx: &mut Context<Self>) {
        if self.revealed != revealed {
            self.revealed = revealed;
            cx.notify();
        }
    }

    fn masked(&self) -> bool {
        self.password && !self.revealed
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.typing = false;
        self.redo.clear();
        self.content = text.into();
        self.selected = self.content.len()..self.content.len();
        self.marked = None;
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    pub fn set_invalid(&mut self, invalid: bool, cx: &mut Context<Self>) {
        self.invalid = invalid;
        cx.notify();
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            cx.notify();
        }
    }

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let placeholder = placeholder.into();
        if self.placeholder != placeholder {
            self.placeholder = placeholder;
            cx.notify();
        }
    }

    fn display_text(&self) -> SharedString {
        if self.masked() {
            BULLET
                .to_string()
                .repeat(self.content.graphemes(true).count())
                .into()
        } else {
            self.content.clone()
        }
    }

    fn to_display(&self, offset: usize) -> usize {
        if self.masked() {
            self.content[..offset].graphemes(true).count() * BULLET.len_utf8()
        } else {
            offset
        }
    }

    fn from_display(&self, offset: usize) -> usize {
        if self.masked() {
            self.content
                .grapheme_indices(true)
                .nth(offset / BULLET.len_utf8())
                .map_or(self.content.len(), |(index, _)| index)
        } else {
            offset
        }
    }

    fn accepts(&self, text: &str) -> bool {
        let Some(numeric) = self.numeric else {
            return true;
        };
        text.chars()
            .all(|ch| ch.is_ascii_digit() || (ch == '-' && numeric.min < 0))
    }

    fn remember(&mut self, typing: bool) {
        if typing && self.typing {
            return;
        }
        self.typing = typing;
        self.redo.clear();
        self.undo.push(Snapshot {
            content: self.content.clone(),
            selected: self.selected.clone(),
        });
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
    }

    fn restore(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) -> Snapshot {
        let current = Snapshot {
            content: std::mem::replace(&mut self.content, snapshot.content),
            selected: std::mem::replace(&mut self.selected, snapshot.selected),
        };
        self.reversed = false;
        self.marked = None;
        self.typing = false;
        cx.emit(InputEvent::Changed);
        cx.notify();
        current
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.undo.pop() {
            let current = self.restore(snapshot, cx);
            self.redo.push(current);
        }
    }

    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.redo.pop() {
            let current = self.restore(snapshot, cx);
            self.undo.push(current);
        }
    }

    fn step_by(&mut self, steps: i64, cx: &mut Context<Self>) {
        let Some(numeric) = self.numeric else {
            cx.propagate();
            return;
        };
        let value = self.content.trim().parse::<i64>().unwrap_or(numeric.min);
        let next = value
            .saturating_add(steps.saturating_mul(numeric.step))
            .clamp(numeric.min, numeric.max);
        self.remember(false);
        self.typing = false;
        self.content = next.to_string().into();
        self.selected = 0..self.content.len();
        self.reversed = false;
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    fn increment(&mut self, _: &Increment, _: &mut Window, cx: &mut Context<Self>) {
        self.step_by(1, cx);
    }

    fn decrement(&mut self, _: &Decrement, _: &mut Window, cx: &mut Context<Self>) {
        self.step_by(-1, cx);
    }

    fn increment_more(&mut self, _: &IncrementMore, _: &mut Window, cx: &mut Context<Self>) {
        self.step_by(10, cx);
    }

    fn decrement_more(&mut self, _: &DecrementMore, _: &mut Window, cx: &mut Context<Self>) {
        self.step_by(-10, cx);
    }

    fn previous_word(&self, offset: usize) -> usize {
        if self.password {
            return 0;
        }
        self.content[..offset]
            .unicode_word_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next_word(&self, offset: usize) -> usize {
        if self.password {
            return self.content.len();
        }
        self.content[offset..]
            .unicode_word_indices()
            .next()
            .map_or(self.content.len(), |(index, word)| {
                offset + index + word.len()
            })
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.typing = false;
        self.move_to(self.previous_word(self.cursor()), cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.typing = false;
        self.move_to(self.next_word(self.cursor()), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_word(self.cursor()), cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_word(self.cursor()), cx);
    }

    fn delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected.is_empty() {
            self.select_to(self.previous_word(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_to_start(&mut self, _: &DeleteToStart, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.select_to(0, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn fit<'a>(&self, range: &Range<usize>, text: &'a str) -> &'a str {
        let Some(max_length) = self.max_length else {
            return text;
        };
        let kept = self.content.chars().count() - self.content[range.clone()].chars().count();
        text.char_indices()
            .nth(max_length.saturating_sub(kept))
            .map_or(text, |(index, _)| &text[..index])
    }

    fn cursor(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.typing = false;
        self.selected = offset..offset;
        self.reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.reversed {
            self.selected.start = offset;
        } else {
            self.selected.end = offset;
        }
        if self.selected.end < self.selected.start {
            self.reversed = !self.reversed;
            self.selected = self.selected.end..self.selected.start;
        }
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.move_to(self.previous_boundary(self.cursor()), cx);
        } else {
            self.move_to(self.selected.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.move_to(self.next_boundary(self.selected.end), cx);
        } else {
            self.move_to(self.selected.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.select_to(self.previous_boundary(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
            self.select_to(self.next_boundary(self.cursor()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.password && !self.selected.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.password && !self.selected.is_empty() {
            self.copy(&Copy, window, cx);
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputEvent::Submitted);
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        if self.search && !self.content.is_empty() {
            self.set_text("", cx);
            return;
        }
        cx.propagate();
    }

    fn index_for_mouse(&self, position: Point<Pixels>) -> usize {
        let (Some(bounds), Some(line)) = (self.bounds.as_ref(), self.layout.as_ref()) else {
            return 0;
        };
        if self.content.is_empty() || position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() || line.text != self.display_text() {
            return self.content.len();
        }
        self.from_display(line.closest_index_for_x(position.x - bounds.left()))
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.selecting = true;
        let index = self.index_for_mouse(event.position);
        if event.modifiers.shift {
            self.select_to(index, cx);
        } else if event.click_count >= 2 {
            self.move_to(0, cx);
            self.select_to(self.content.len(), cx);
        } else {
            self.move_to(index, cx);
        }
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.selecting {
            self.select_to(self.index_for_mouse(event.position), cx);
        }
    }

    fn utf16_to_utf8(&self, offset: usize) -> usize {
        utf8_offset(&self.content, offset)
    }

    fn utf8_to_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        let mut utf8 = 0;
        for ch in self.content.chars() {
            if utf8 >= offset {
                break;
            }
            utf8 += ch.len_utf8();
            utf16 += ch.len_utf16();
        }
        utf16
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.utf8_to_utf16(range.start)..self.utf8_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.utf16_to_utf8(range.start)..self.utf16_to_utf8(range.end)
    }
}

fn utf8_offset(text: &str, utf16_offset: usize) -> usize {
    let mut utf8 = 0;
    let mut utf16 = 0;
    for ch in text.chars() {
        if utf16 >= utf16_offset {
            break;
        }
        utf16 += ch.len_utf16();
        utf8 += ch.len_utf8();
    }
    utf8
}

impl EntityInputHandler for Input {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range);
        actual.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected),
            reversed: self.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        if !self.accepts(text) {
            return;
        }
        let text = self.fit(&range, text);
        if range.is_empty() && text.is_empty() {
            return;
        }
        self.remember(range.is_empty() && !text.is_empty());
        self.content =
            (self.content[..range.start].to_owned() + text + &self.content[range.end..]).into();
        self.selected = range.start + text.len()..range.start + text.len();
        self.reversed = false;
        self.marked = None;
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        let text = self.fit(&range, text);
        self.content =
            (self.content[..range.start].to_owned() + text + &self.content[range.end..]).into();
        self.marked = (!text.is_empty()).then(|| range.start..range.start + text.len());
        self.selected = selected
            .as_ref()
            .map(|new| {
                range.start + utf8_offset(text, new.start)..range.start + utf8_offset(text, new.end)
            })
            .unwrap_or_else(|| range.start + text.len()..range.start + text.len());
        cx.emit(InputEvent::Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let range = self.range_from_utf16(&range);
        Some(Bounds::from_corners(
            point(
                bounds.left() + layout.x_for_index(self.to_display(range.start)),
                bounds.top(),
            ),
            point(
                bounds.left() + layout.x_for_index(self.to_display(range.end)),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        at: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let local = self.bounds?.localize(&at)?;
        let index = self.layout.as_ref()?.index_for_x(local.x)?;
        Some(self.utf8_to_utf16(self.from_display(index)))
    }
}

impl Focusable for Input {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Input {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let focused = !self.disabled && self.focus.is_focused(window);
        if focused && !self.was_focused && crate::nav::keyboard_input(window, cx) {
            self.selected = 0..self.content.len();
            self.reversed = false;
            self.typing = false;
        }
        if !focused && self.was_focused {
            self.selected = self.selected.end..self.selected.end;
            self.reversed = false;
        }
        self.was_focused = focused;
        let clearable = self.search && !self.content.is_empty();
        let height = match (self.bare, self.small) {
            (true, _) => LINE_BODY,
            (false, true) => CONTROL_SM,
            (false, false) => CONTROL_MD,
        };
        let radius = if self.pill {
            height / 2.0
        } else {
            theme.radius.control
        };
        div()
            .id("field")
            .role(if self.search {
                Role::SearchInput
            } else if self.password {
                Role::PasswordInput
            } else {
                Role::TextInput
            })
            .aria_label(self.label.clone())
            .aria_value(self.display_text())
            .when(!self.placeholder.is_empty(), |field| {
                field.aria_placeholder(self.placeholder.clone())
            })
            .when(self.invalid, |field| {
                field.aria_description("Invalid value")
            })
            .key_context(CONTEXT)
            .when(!self.disabled, |field| {
                field.track_focus(&self.focus).cursor(CursorStyle::IBeam)
            })
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_to_start))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::increment))
            .on_action(cx.listener(Self::decrement))
            .on_action(cx.listener(Self::increment_more))
            .on_action(cx.listener(Self::decrement_more))
            .when(!self.disabled, |field| {
                field
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
                    .on_mouse_move(cx.listener(Self::mouse_move))
            })
            .flex()
            .items_center()
            .gap(px(4.0))
            .w_full()
            .h(height)
            .when(!self.bare, |field| {
                field
                    .pl(px(8.0))
                    .pr(px(if clearable || self.password { 2.0 } else { 8.0 }))
                    .rounded(radius)
                    .bg(theme.fill)
                    .border_1()
                    .border_color(match (self.invalid, self.pill) {
                        (true, _) => theme.danger,
                        (false, true) => theme.fill,
                        (false, false) => theme.border,
                    })
            })
            .when(focused, |field| {
                let color = if self.invalid {
                    theme.danger
                } else {
                    theme.focus
                };
                if self.bare {
                    field.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom(px(-3.0))
                            .h(px(1.0))
                            .bg(color),
                    )
                } else {
                    field.child(theme.ring_around(color, radius, px(1.0)))
                }
            })
            .text_size(if self.small { TEXT_SMALL } else { TEXT_BODY })
            .line_height(LINE_BODY)
            .text_color(if self.disabled {
                theme.text_disabled
            } else {
                theme.text
            })
            .when(self.numeric.is_some(), |field| {
                field.font_features(tabular())
            })
            .when(self.search, |field| {
                field.child(icon(Icon::Search, theme.text_tertiary).mr(px(2.0)))
            })
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(TextElement { field: cx.entity() }),
            )
            .when_some(self.suffix.clone(), |field, suffix| {
                field.child(
                    div()
                        .flex_none()
                        .text_color(theme.text_tertiary)
                        .child(suffix),
                )
            })
            .when(self.password, |field| {
                let entity = cx.entity();
                let revealed = self.revealed;
                field.child(
                    Button::icon(
                        "reveal",
                        if revealed { Icon::EyeOff } else { Icon::Eye },
                        if revealed {
                            "Hide password"
                        } else {
                            "Show password"
                        },
                    )
                    .size(Size::Small)
                    .on_click(move |_, window, cx| {
                        entity.update(cx, |field, cx| {
                            field.reveal(!revealed, cx);
                            window.focus(&field.focus, cx);
                        })
                    }),
                )
            })
            .when(clearable, |field| {
                let entity = cx.entity();
                field.child(
                    Button::icon("clear", Icon::X, "Clear search")
                        .size(Size::Small)
                        .on_click(move |_, window, cx| {
                            entity.update(cx, |field, cx| {
                                field.set_text("", cx);
                                window.focus(&field.focus, cx);
                            })
                        }),
                )
            })
    }
}

const CARET_WIDTH: Pixels = px(1.5);

struct TextElement {
    field: Entity<Input>,
}

struct Prepainted {
    bounds: Bounds<Pixels>,
    scroll: Pixels,
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl gpui::Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepainted;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepainted {
        let theme = cx.theme().clone();
        let field = self.field.read(cx);
        let style = window.text_style();
        let (text, color) = if field.content.is_empty() {
            let color = if field.disabled {
                theme.text_disabled
            } else {
                theme.text_tertiary
            };
            (field.placeholder.clone(), color)
        } else {
            (field.display_text(), style.color)
        };
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let marked = field
            .marked
            .as_ref()
            .filter(|_| !field.content.is_empty())
            .map(|marked| field.to_display(marked.start)..field.to_display(marked.end));
        let runs = match marked {
            Some(marked) => [
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: text.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            None => vec![run],
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(text, font_size, &runs, None);
        let bounds = if field.pill {
            let inset = ((bounds.size.width - line.width) / 2.0).max(Pixels::ZERO);
            Bounds::new(
                point(bounds.left() + inset, bounds.top()),
                size(bounds.size.width - inset, bounds.size.height),
            )
        } else {
            bounds
        };
        let caret_x = if field.content.is_empty() {
            px(0.0)
        } else {
            line.x_for_index(field.to_display(field.cursor()))
        };
        let visible = bounds.size.width - CARET_WIDTH;
        let mut scroll = field.scroll;
        if field.focus.is_focused(window) {
            scroll = scroll.min(caret_x).max(caret_x - visible);
        }
        let scroll = scroll.min(line.width - visible).max(Pixels::ZERO);
        let bounds = Bounds::new(
            point(bounds.left() - scroll, bounds.top()),
            size(bounds.size.width + scroll, bounds.size.height),
        );
        let (selection, cursor) = if field.selected.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + caret_x, bounds.top() + px(1.0)),
                        size(CARET_WIDTH, bounds.size.height - px(2.0)),
                    ),
                    theme.text,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left()
                                + line.x_for_index(field.to_display(field.selected.start)),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(field.to_display(field.selected.end)),
                            bounds.bottom(),
                        ),
                    ),
                    theme.focus.opacity(0.35),
                )),
                None,
            )
        };
        Prepainted {
            bounds,
            scroll,
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        prepainted: &mut Prepainted,
        window: &mut Window,
        cx: &mut App,
    ) {
        let bounds = prepainted.bounds;
        let focus = self.field.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.field.clone()),
            cx,
        );
        if let Some(selection) = prepainted.selection.take() {
            window.paint_quad(selection);
        }
        let Some(line) = prepainted.line.take() else {
            return;
        };
        line.paint(
            bounds.origin,
            window.line_height(),
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        )
        .ok();
        if focus.is_focused(window)
            && let Some(cursor) = prepainted.cursor.take()
        {
            window.paint_quad(cursor);
        }
        self.field.update(cx, |field, _| {
            field.layout = Some(line);
            field.bounds = Some(bounds);
            field.scroll = prepainted.scroll;
        });
    }
}
