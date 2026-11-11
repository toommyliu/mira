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

use super::{Button, Icon, button::Size};
use crate::{
    ActiveTheme as _,
    theme::{CONTROL_MD, LINE_BODY, TEXT_BODY},
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
    ]);
}

pub enum InputEvent {
    Changed,
    Submitted,
}

pub struct Input {
    focus: FocusHandle,
    label: SharedString,
    content: SharedString,
    placeholder: SharedString,
    search: bool,
    password: bool,
    max_length: Option<usize>,
    invalid: bool,
    selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    layout: Option<ShapedLine>,
    bounds: Option<Bounds<Pixels>>,
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
            max_length: None,
            invalid: false,
            selected: 0..0,
            reversed: false,
            marked: None,
            layout: None,
            bounds: None,
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

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
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

    fn display_text(&self) -> SharedString {
        if self.password {
            BULLET
                .to_string()
                .repeat(self.content.graphemes(true).count())
                .into()
        } else {
            self.content.clone()
        }
    }

    fn to_display(&self, offset: usize) -> usize {
        if self.password {
            self.content[..offset].graphemes(true).count() * BULLET.len_utf8()
        } else {
            offset
        }
    }

    fn from_display(&self, offset: usize) -> usize {
        if self.password {
            self.content
                .grapheme_indices(true)
                .nth(offset / BULLET.len_utf8())
                .map_or(self.content.len(), |(index, _)| index)
        } else {
            offset
        }
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
        let text = self.fit(&range, text);
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
        let focused = self.focus.is_focused(window);
        let clearable = self.search && !self.content.is_empty();
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
            .track_focus(&self.focus)
            .cursor(CursorStyle::IBeam)
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
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .flex()
            .items_center()
            .gap(px(4.0))
            .w_full()
            .h(CONTROL_MD)
            .pl(px(8.0))
            .pr(px(if clearable { 2.0 } else { 8.0 }))
            .rounded(theme.radius.control)
            .bg(theme.fill)
            .border_1()
            .border_color(if self.invalid {
                theme.danger
            } else {
                theme.border
            })
            .when(focused, |field| {
                field.child(theme.ring_around(
                    if self.invalid {
                        theme.danger
                    } else {
                        theme.focus
                    },
                    theme.radius.control,
                    px(1.0),
                ))
            })
            .text_size(TEXT_BODY)
            .line_height(LINE_BODY)
            .text_color(theme.text)
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(TextElement { field: cx.entity() }),
            )
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

struct TextElement {
    field: Entity<Input>,
}

struct Prepainted {
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
            (field.placeholder.clone(), theme.text_tertiary)
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
        let caret_x = if field.content.is_empty() {
            px(0.0)
        } else {
            line.x_for_index(field.to_display(field.cursor()))
        };
        let (selection, cursor) = if field.selected.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + caret_x, bounds.top() + px(1.0)),
                        size(px(1.5), bounds.size.height - px(2.0)),
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
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepainted: &mut Prepainted,
        window: &mut Window,
        cx: &mut App,
    ) {
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
        });
    }
}
