use std::rc::Rc;

use gpui::{
    App, ElementId, Entity, FocusHandle, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Pixels, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, IndexHandler, PointerCursor as _, StyledExt as _, h_flex,
    nav::{self, Axis, Step},
    primitives::{Flow, Icon, Sortable, small_icon},
    theme::{CONTROL_SM, DISABLED_OPACITY, FOCUS_RING},
};

type ReorderHandler = Rc<dyn Fn(usize, usize, &mut Window, &mut App)>;

const REMOVE: Pixels = px(16.0);

#[derive(IntoElement)]
pub struct Chips {
    id: ElementId,
    label: SharedString,
    items: Rc<[SharedString]>,
    disabled: bool,
    on_remove: Option<IndexHandler>,
    on_reorder: Option<ReorderHandler>,
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
            on_remove: None,
            on_reorder: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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
}

#[derive(Clone)]
struct Chip {
    items: Rc<[SharedString]>,
    current: Option<usize>,
    ring: bool,
    focus: FocusHandle,
    selection: Entity<usize>,
    on_remove: Option<IndexHandler>,
}

impl Chip {
    fn render(&self, index: usize, cx: &App) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let label = self.items[index].clone();
        let current = self.current == Some(index);
        let (focus, selection) = (self.focus.clone(), self.selection.clone());
        h_flex()
            .id(index)
            .role(Role::ListBoxOption)
            .aria_label(label.clone())
            .aria_selected(current)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(self.items.len())
            .when(current, |chip| chip.aria_active_descendant())
            .h(CONTROL_SM)
            .pl(px(9.0))
            .pr(px(3.0))
            .gap(px(4.0))
            .rounded_full()
            .bg(theme.fill)
            .border_1()
            .border_color(theme.hairline)
            .text_small()
            .text_color(theme.text)
            .when(self.on_remove.is_some(), |chip| {
                chip.on_click(move |_, window, cx| {
                    select(&selection, index, cx);
                    window.focus(&focus, cx);
                })
            })
            .child(label.clone())
            .child(
                div()
                    .id("remove")
                    .role(Role::Button)
                    .aria_label(SharedString::from(format!("Remove {label}")))
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(REMOVE)
                    .rounded_full()
                    .child(small_icon(Icon::X, theme.text_tertiary))
                    .when(current && self.ring, |button| {
                        button.child(
                            div()
                                .absolute()
                                .inset(-FOCUS_RING)
                                .rounded_full()
                                .border(FOCUS_RING)
                                .border_color(theme.focus),
                        )
                    })
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
    }
}

fn select(selection: &Entity<usize>, index: usize, cx: &mut App) {
    selection.update(cx, |selected, cx| {
        *selected = index;
        cx.notify();
    });
}

impl RenderOnce for Chips {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let selection = window.use_keyed_state((self.id.clone(), "selected"), cx, |_, _| 0);
        let count = self.items.len();
        let current = count
            .checked_sub(1)
            .map(|last| (*selection.read(cx)).min(last));
        let interactive = !self.disabled;
        let (on_remove, on_reorder) = if interactive {
            (self.on_remove, self.on_reorder)
        } else {
            (None, None)
        };
        let chip = Chip {
            items: self.items.clone(),
            current,
            ring: interactive && nav::keyboard_focused(&focus, window, cx),
            focus: focus.clone(),
            selection: selection.clone(),
            on_remove: on_remove.clone(),
        };
        let keys = {
            let (selection, on_remove, on_reorder) =
                (selection.clone(), on_remove.clone(), on_reorder.clone());
            move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                let Some(at) = current else {
                    return;
                };
                let keystroke = &event.keystroke;
                let handled = match keystroke.key.as_str() {
                    "backspace" | "delete" | "enter" | "space"
                        if !keystroke.modifiers.modified() =>
                    {
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
            })
            .when(!interactive, |list| list.opacity(DISABLED_OPACITY));
        match on_reorder {
            Some(reorder) => Sortable::new(
                list,
                Flow::Wrap,
                self.items
                    .iter()
                    .map(|item| ElementId::Name(item.clone()))
                    .collect::<Vec<_>>(),
                move |(index, _), _, cx| chip.render(index, cx).into_any_element(),
                move |from, to, window, cx| {
                    reorder(from, to, window, cx);
                    select(&selection, to, cx);
                },
            )
            .into_any_element(),
            None => list
                .children((0..count).map(|index| chip.render(index, cx)))
                .into_any_element(),
        }
    }
}
