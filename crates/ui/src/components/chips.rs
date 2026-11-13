use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, ElementId, Entity, FocusHandle, Focusable as _,
    Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    TextRun, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, IndexHandler, PointerCursor as _, StyledExt as _, h_flex,
    nav::{self, Axis, Step},
    primitives::{Flow, Icon, Input, InputEvent, Sortable, icon, small_icon},
    theme::{CONTROL_SM, DISABLED_OPACITY, TEXT_SMALL},
};

type ReorderHandler = Rc<dyn Fn(usize, usize, &mut Window, &mut App)>;
type MarkHandler = Rc<dyn Fn(usize, bool, &mut Window, &mut App)>;
type EditHandler = Rc<dyn Fn(usize, String, &mut Window, &mut App)>;

const SLOT: Pixels = px(16.0);
const EDIT_PADDING: Pixels = px(30.0);
const EDIT_MIN: Pixels = px(64.0);

#[derive(Clone)]
struct Marks {
    icon: Icon,
    label: SharedString,
    marked: Rc<[bool]>,
}

impl Marks {
    fn at(&self, index: usize) -> bool {
        self.marked.get(index).copied().unwrap_or(false)
    }
}

struct Editor {
    index: usize,
    original: SharedString,
    input: Entity<Input>,
    on_edit: EditHandler,
    _subscriptions: [Subscription; 2],
}

type Editing = Entity<Option<Editor>>;

#[derive(IntoElement)]
pub struct Chips {
    id: ElementId,
    label: SharedString,
    items: Rc<[SharedString]>,
    disabled: bool,
    quiet: bool,
    marks: Option<Marks>,
    end: Option<AnyElement>,
    on_remove: Option<IndexHandler>,
    on_reorder: Option<ReorderHandler>,
    on_mark: Option<MarkHandler>,
    on_edit: Option<EditHandler>,
}

impl Chips {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        items: impl IntoIterator<Item = SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            items: items.into_iter().collect(),
            disabled: false,
            quiet: false,
            marks: None,
            end: None,
            on_remove: None,
            on_reorder: None,
            on_mark: None,
            on_edit: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }

    pub fn end_slot(mut self, element: impl IntoElement) -> Self {
        self.end = Some(element.into_any_element());
        self
    }

    pub fn on_remove(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_remove = Some(Rc::new(f));
        self
    }

    pub fn on_reorder(mut self, f: impl Fn(usize, usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_reorder = Some(Rc::new(f));
        self
    }

    pub fn marked(
        mut self,
        icon: Icon,
        label: impl Into<SharedString>,
        marked: impl IntoIterator<Item = bool>,
    ) -> Self {
        self.marks = Some(Marks {
            icon,
            label: label.into(),
            marked: marked.into_iter().collect(),
        });
        self
    }

    pub fn on_mark(mut self, f: impl Fn(usize, bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_mark = Some(Rc::new(f));
        self
    }

    pub fn on_edit(mut self, f: impl Fn(usize, String, &mut Window, &mut App) + 'static) -> Self {
        self.on_edit = Some(Rc::new(f));
        self
    }
}

#[derive(Clone)]
struct Chip {
    items: Rc<[SharedString]>,
    current: Option<usize>,
    ring: bool,
    dimmed: bool,
    quiet: bool,
    removable: bool,
    focus: FocusHandle,
    selection: Entity<usize>,
    editing: Editing,
    marks: Option<Marks>,
    on_remove: Option<IndexHandler>,
    on_mark: Option<MarkHandler>,
    on_edit: Option<EditHandler>,
}

impl Chip {
    fn render(&self, index: usize, window: &mut Window, cx: &mut App) -> AnyElement {
        let editor = self
            .editing
            .read(cx)
            .as_ref()
            .filter(|editor| editor.index == index)
            .map(|editor| editor.input.clone());
        match editor {
            Some(input) => self.render_editor(input, window, cx),
            None => self.render_chip(index, cx),
        }
    }

    fn render_editor(&self, input: Entity<Input>, window: &mut Window, cx: &mut App) -> AnyElement {
        let text: SharedString = input.read(cx).text().to_owned().into();
        let style = window.text_style();
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let width = window
            .text_system()
            .shape_line(text, TEXT_SMALL, &[run], None)
            .width;
        let (editing, focus) = (self.editing.downgrade(), self.focus.clone());
        div()
            .w((width + EDIT_PADDING).max(EDIT_MIN))
            .child(input)
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    finish(&editing, false, Some(&focus), window, cx);
                    cx.stop_propagation();
                }
            })
            .into_any_element()
    }

    fn render_chip(&self, index: usize, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let label = self.items[index].clone();
        let current = self.current == Some(index);
        let marked = self.marks.as_ref().is_some_and(|marks| marks.at(index));
        let (fill, edge, text, quiet) = if marked {
            (
                theme.accent_soft,
                theme.accent.opacity(0.35),
                theme.accent_text,
                theme.accent_text,
            )
        } else if self.quiet {
            (
                Hsla::transparent_black(),
                theme.border,
                theme.text_secondary,
                theme.text_tertiary,
            )
        } else {
            (theme.fill, theme.hairline, theme.text, theme.text_tertiary)
        };
        let (focus, selection, editing) = (
            self.focus.clone(),
            self.selection.clone(),
            self.editing.clone(),
        );
        let on_edit = self.on_edit.clone();
        let editable_label = label.clone();
        h_flex()
            .id(index)
            .role(Role::ListBoxOption)
            .aria_label(label.clone())
            .aria_selected(current)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(self.items.len())
            .when(current, |chip| chip.aria_active_descendant())
            .when_some(self.marks.as_ref().filter(|_| marked), |chip, marks| {
                chip.aria_description(marks.label.clone())
            })
            .relative()
            .when(self.dimmed, |chip| chip.opacity(DISABLED_OPACITY))
            .h(CONTROL_SM)
            .pl(px(if self.marks.is_some() { 5.0 } else { 9.0 }))
            .pr(px(if self.removable { 3.0 } else { 9.0 }))
            .gap(px(4.0))
            .rounded_full()
            .bg(fill)
            .border_1()
            .border_color(edge)
            .text_small()
            .text_color(text)
            .on_click(move |event: &ClickEvent, window, cx| {
                select(&selection, index, cx);
                window.focus(&focus, cx);
                if event.click_count() >= 2
                    && let Some(on_edit) = &on_edit
                {
                    edit(
                        &editing,
                        index,
                        editable_label.clone(),
                        &focus,
                        on_edit.clone(),
                        window,
                        cx,
                    );
                }
            })
            .when_some(self.marks.clone(), |chip, marks| {
                let toggle = self.on_mark.clone();
                chip.child(
                    div()
                        .id("mark")
                        .role(Role::Button)
                        .aria_label(marks.label.clone())
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(SLOT)
                        .rounded_full()
                        .child(icon(marks.icon, if marked { text } else { quiet }).size(px(12.0)))
                        .when_some(toggle, |bell, toggle| {
                            bell.pointer_cursor(theme)
                                .hover(|style| style.bg(theme.fill_hover))
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    toggle(index, !marked, window, cx);
                                })
                        }),
                )
            })
            .child(label.clone())
            .when(self.removable, |chip| {
                chip.child(
                    div()
                        .id("remove")
                        .role(Role::Button)
                        .aria_label(SharedString::from(format!("Remove {label}")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(SLOT)
                        .rounded_full()
                        .child(small_icon(Icon::X, quiet))
                        .when_some(self.on_remove.clone(), |button, remove| {
                            button
                                .pointer_cursor(theme)
                                .hover(|style| style.bg(theme.fill_hover))
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    remove(index, window, cx);
                                })
                        }),
                )
            })
            .when(current && self.ring, |chip| {
                chip.child(theme.ring_around(theme.focus, CONTROL_SM / 2.0, px(1.0)))
            })
            .into_any_element()
    }
}

fn select(selection: &Entity<usize>, index: usize, cx: &mut App) {
    selection.update(cx, |selected, cx| {
        *selected = index;
        cx.notify();
    });
}

fn edit(
    editing: &Editing,
    index: usize,
    label: SharedString,
    list: &FocusHandle,
    on_edit: EditHandler,
    window: &mut Window,
    cx: &mut App,
) {
    let input = cx.new(|cx| {
        Input::new(SharedString::from(format!("Edit {label}")), cx)
            .small()
            .pill()
    });
    input.update(cx, |input, cx| input.set_text(label.clone(), cx));
    let (submitted, blurred) = (editing.downgrade(), editing.downgrade());
    let list = list.clone();
    let events = window.subscribe(
        &input,
        cx,
        move |_, event: &InputEvent, window, cx| match event {
            InputEvent::Submitted => finish(&submitted, true, Some(&list), window, cx),
            InputEvent::Changed => window.refresh(),
        },
    );
    let blur = window.on_focus_out(&input.focus_handle(cx), cx, move |_, window, cx| {
        finish(&blurred, true, None, window, cx)
    });
    let focus = input.focus_handle(cx);
    editing.update(cx, |editing, cx| {
        *editing = Some(Editor {
            index,
            original: label,
            input,
            on_edit,
            _subscriptions: [events, blur],
        });
        cx.notify();
    });
    window.focus(&focus, cx);
}

fn finish(
    editing: &WeakEntity<Option<Editor>>,
    commit: bool,
    refocus: Option<&FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(closed) = editing
        .update(cx, |editing, cx| {
            cx.notify();
            editing.take()
        })
        .ok()
        .flatten()
    else {
        return;
    };
    let text = closed.input.read(cx).text().trim().to_owned();
    if commit && !text.is_empty() && text != closed.original.as_ref() {
        (closed.on_edit)(closed.index, text, window, cx);
    }
    if let Some(focus) = refocus {
        window.focus(focus, cx);
    }
}

impl RenderOnce for Chips {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let selection = window.use_keyed_state((self.id.clone(), "selected"), cx, |_, _| 0);
        let editing: Editing =
            window.use_keyed_state((self.id.clone(), "editing"), cx, |_, _| None);
        let count = self.items.len();
        let current = count
            .checked_sub(1)
            .map(|last| (*selection.read(cx)).min(last));
        let interactive = !self.disabled;
        let removable = self.on_remove.is_some();
        let (on_remove, on_reorder, on_mark, on_edit) = if interactive {
            (self.on_remove, self.on_reorder, self.on_mark, self.on_edit)
        } else {
            (None, None, None, None)
        };
        let chip = Chip {
            items: self.items.clone(),
            current,
            ring: interactive && nav::keyboard_focused(&focus, window, cx),
            dimmed: !interactive,
            quiet: self.quiet,
            removable,
            focus: focus.clone(),
            selection: selection.clone(),
            editing: editing.clone(),
            marks: self.marks.clone(),
            on_remove: on_remove.clone(),
            on_mark: on_mark.clone(),
            on_edit: on_edit.clone(),
        };
        let keys = {
            let (selection, focus, items) = (selection.clone(), focus.clone(), self.items.clone());
            let marks = self.marks.clone();
            let on_reorder = on_reorder.clone();
            move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                let Some(at) = current.filter(|_| focus.is_focused(window)) else {
                    return;
                };
                let keystroke = &event.keystroke;
                let unmodified = !keystroke.modifiers.modified();
                let handled = match keystroke.key.as_str() {
                    "space" if unmodified => on_mark.as_ref().map(|mark| {
                        let on = !marks.as_ref().is_some_and(|marks| marks.at(at));
                        mark(at, on, window, cx)
                    }),
                    "enter" if unmodified => on_edit.as_ref().map(|on_edit| {
                        edit(
                            &editing,
                            at,
                            items[at].clone(),
                            &focus,
                            on_edit.clone(),
                            window,
                            cx,
                        )
                    }),
                    "backspace" | "delete" if unmodified => {
                        on_remove.as_ref().map(|remove| remove(at, window, cx))
                    }
                    "left" | "right" if keystroke.modifiers.alt => {
                        let to = if keystroke.key == "left" {
                            at.checked_sub(1)
                        } else {
                            Some(at + 1).filter(|&to| to < count)
                        };
                        on_reorder.as_ref().zip(to).map(|(reorder, to)| {
                            reorder(at, to, window, cx);
                            select(&selection, to, cx);
                        })
                    }
                    _ => Step::from_key(event, Axis::Horizontal)
                        .or_else(|| Step::from_key(event, Axis::Vertical))
                        .and_then(|step| nav::step(current, count, step, false, |_| true))
                        .map(|next| select(&selection, next, cx)),
                };
                if handled.is_some() {
                    cx.stop_propagation();
                }
            }
        };
        let list = h_flex()
            .id(self.id.clone())
            .role(Role::ListBox)
            .aria_label(self.label)
            .flex_wrap()
            .gap(px(6.0))
            .when(interactive, |list| {
                list.track_focus(&focus.tab_stop(true)).on_key_down(keys)
            });
        match on_reorder {
            Some(reorder) => h_flex()
                .flex_wrap()
                .gap(px(6.0))
                .child(Sortable::new(
                    list,
                    Flow::Wrap,
                    self.items
                        .iter()
                        .map(|item| ElementId::Name(item.clone()))
                        .collect::<Vec<_>>(),
                    move |(index, _), window, cx| chip.render(index, window, cx),
                    move |from, to, window, cx| {
                        reorder(from, to, window, cx);
                        select(&selection, to, cx);
                    },
                ))
                .children(self.end)
                .into_any_element(),
            None => {
                let chips: Vec<AnyElement> = (0..count)
                    .map(|index| chip.render(index, window, cx))
                    .collect();
                list.children(chips).children(self.end).into_any_element()
            }
        }
    }
}
