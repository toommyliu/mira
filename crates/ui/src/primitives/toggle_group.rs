use gpui::{
    AnyElement, App, ElementId, FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Toggled, Window, div, prelude::FluentBuilder as _, px,
};
use std::rc::Rc;

use crate::{
    ActiveTheme as _, IndexHandler, PointerCursor as _,
    nav::{self, Axis, Step},
};

pub struct ToggleItem {
    label: SharedString,
    start: Option<AnyElement>,
    end: Option<AnyElement>,
}

impl ToggleItem {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            start: None,
            end: None,
        }
    }

    pub fn start_slot(mut self, element: impl IntoElement) -> Self {
        self.start = Some(element.into_any_element());
        self
    }

    pub fn end_slot(mut self, element: impl IntoElement) -> Self {
        self.end = Some(element.into_any_element());
        self
    }
}

impl From<SharedString> for ToggleItem {
    fn from(label: SharedString) -> Self {
        Self::new(label)
    }
}

impl From<&'static str> for ToggleItem {
    fn from(label: &'static str) -> Self {
        Self::new(label)
    }
}

impl From<String> for ToggleItem {
    fn from(label: String) -> Self {
        Self::new(label)
    }
}

#[derive(IntoElement)]
pub struct ToggleGroup {
    id: ElementId,
    label: SharedString,
    options: Vec<ToggleItem>,
    selected: usize,
    full_width: bool,
    small: bool,
    on_select: Option<IndexHandler>,
}

impl ToggleGroup {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        options: impl IntoIterator<Item = impl Into<ToggleItem>>,
        selected: usize,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            options: options.into_iter().map(Into::into).collect(),
            selected,
            full_width: false,
            small: false,
            on_select: None,
        }
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }

    pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for ToggleGroup {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let layout = cx.theme().layout;
        let typography = cx.theme().typography;
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let ring = nav::keyboard_focused(&focus, window, cx);
        let count = self.options.len().max(1);
        let theme = cx.theme().clone();
        let selected = self.selected;
        let on_key = self.on_select.clone();
        let full_width = self.full_width;
        let small = self.small;
        let group = div()
            .id(self.id.clone())
            .role(Role::RadioGroup)
            .aria_label(self.label)
            .track_focus(&focus.clone().tab_stop(true))
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let Some(step) = Step::from_key(event, Axis::Horizontal) else {
                    return;
                };
                if let (Some(next), Some(on_select)) = (
                    nav::step(Some(selected), count, step, true, |_| true),
                    &on_key,
                ) {
                    on_select(next, window, cx);
                }
                cx.stop_propagation();
            })
            .relative()
            .flex()
            .h(if small {
                layout.control_sm
            } else {
                layout.control
            })
            .when(self.full_width, |group| group.w_full())
            .children(self.options.into_iter().enumerate().map(|(index, option)| {
                let checked = index == selected;
                let on_select = self.on_select.clone();
                let focus = focus.clone();
                div()
                    .id(index)
                    .role(Role::RadioButton)
                    .aria_label(option.label.clone())
                    .aria_toggled(if checked {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .aria_position_in_set(index + 1)
                    .aria_size_of_set(count)
                    .when(checked, |segment| segment.aria_active_descendant())
                    .relative()
                    .flex()
                    .map(|segment| {
                        if self.full_width {
                            segment
                                .flex_1()
                                .flex_basis(px(0.0))
                                .min_w_0()
                                .px(layout.space(1.5))
                        } else {
                            segment.flex_none().px(theme.space(2.0))
                        }
                    })
                    .items_center()
                    .justify_center()
                    .h_full()
                    .rounded(theme.radius.control)
                    .when(index > 0, |segment| segment.rounded_l(px(0.0)).ml(px(-1.0)))
                    .when(index + 1 < count, |segment| segment.rounded_r(px(0.0)))
                    .border_1()
                    .border_color(if checked && ring {
                        theme.ring
                    } else {
                        theme.border
                    })
                    .when(checked, |segment| segment.bg(theme.muted))
                    .hover(|style| style.bg(theme.ghost_hover()).text_color(theme.foreground))
                    .active(|style| {
                        style.bg(theme.pressed(
                            theme.background,
                            theme.ghost_hover(),
                            theme.foreground,
                        ))
                    })
                    .when(checked && ring, |segment| {
                        segment.child(theme.halo(
                            theme.ring.opacity(0.5),
                            theme.radius.control,
                            px(1.0),
                            px(3.0),
                        ))
                    })
                    .text_size(if small {
                        typography.secondary
                    } else {
                        typography.body
                    })
                    .line_height(if small {
                        typography.secondary_line
                    } else {
                        typography.body_line
                    })
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if checked {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .pointer_cursor(&theme)
                    .when_some(on_select, |segment, on_select| {
                        segment.on_click(move |_, window, cx| {
                            window.focus(&focus, cx);
                            on_select(index, window, cx);
                        })
                    })
                    .gap(layout.space(1.0))
                    .children(option.start)
                    .child(div().truncate().child(option.label))
                    .children(option.end)
                    .child(div().flex_none().w(px(10.0)).when(checked, |slot| {
                        slot.child(super::icon::small_icon(
                            super::Icon::Check,
                            theme.foreground,
                        ))
                    }))
            }));
        div()
            .flex()
            .when(full_width, |row| row.w_full())
            .child(group)
    }
}
