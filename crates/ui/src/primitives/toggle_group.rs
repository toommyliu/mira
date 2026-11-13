use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    AnyElement, App, Bounds, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement as _, Pixels, RenderOnce, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Toggled, Window, canvas, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, IndexHandler, PointerCursor as _,
    nav::{self, Axis, Step},
    theme::{CONTROL_MD, LINE_BODY, Radii, TEXT_SMALL},
};

const INSET: Pixels = px(2.0);

#[derive(Default)]
struct Layout {
    root: Cell<Bounds<Pixels>>,
    segments: RefCell<Vec<Bounds<Pixels>>>,
}

fn probe(layout: Rc<Layout>, index: Option<usize>) -> impl IntoElement {
    canvas(
        move |area, window, _| {
            let changed = match index {
                None => layout.root.replace(area) != area,
                Some(index) => {
                    let mut segments = layout.segments.borrow_mut();
                    if segments.len() <= index {
                        segments.resize(index + 1, Bounds::default());
                    }
                    std::mem::replace(&mut segments[index], area) != area
                }
            };
            if changed {
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

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
            on_select: None,
        }
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
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let ring = nav::keyboard_focused(&focus, window, cx);
        let count = self.options.len().max(1);
        let layout = window
            .use_keyed_state((self.id.clone(), "layout"), cx, |_, _| {
                Rc::new(Layout::default())
            })
            .read(cx)
            .clone();
        let indicator = layout
            .segments
            .borrow()
            .get(self.selected)
            .copied()
            .filter(|segment| segment.size.width > Pixels::ZERO)
            .map(|segment| {
                (
                    f32::from(segment.left() - layout.root.get().left()),
                    f32::from(segment.size.width),
                )
            });
        let theme = cx.theme().clone();
        let selected = self.selected;
        let on_key = self.on_select.clone();
        let full_width = self.full_width;
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
            .h(CONTROL_MD)
            .p(INSET)
            .rounded(theme.radius.control)
            .bg(theme.fill)
            .when(self.full_width, |group| group.w_full())
            .child(probe(layout.clone(), None))
            .when_some(indicator, |group, (left, width)| {
                group.child(
                    div()
                        .absolute()
                        .top(INSET)
                        .bottom(INSET)
                        .left(px(left))
                        .w(px(width))
                        .rounded(Radii::inside(theme.radius.control, INSET))
                        .bg(theme.thumb)
                        .shadow(theme.raised())
                        .when(ring, |indicator| {
                            indicator
                                .child(theme.focus_ring(Radii::inside(theme.radius.control, INSET)))
                        }),
                )
            })
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
                            segment.flex_1().flex_basis(px(0.0)).min_w_0().px(px(6.0))
                        } else {
                            segment.flex_none().px(px(12.0))
                        }
                    })
                    .items_center()
                    .justify_center()
                    .text_size(TEXT_SMALL)
                    .line_height(LINE_BODY)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if checked {
                        theme.text
                    } else {
                        theme.text_secondary
                    })
                    .pointer_cursor(&theme)
                    .when(!checked, |segment| {
                        segment.hover(|style| style.text_color(theme.text))
                    })
                    .when_some(on_select, |segment, on_select| {
                        segment.on_click(move |_, window, cx| {
                            window.focus(&focus, cx);
                            on_select(index, window, cx);
                        })
                    })
                    .gap(px(4.0))
                    .child(probe(layout.clone(), Some(index)))
                    .children(option.start)
                    .child(div().truncate().child(option.label))
                    .children(option.end)
            }));
        div()
            .flex()
            .when(full_width, |row| row.w_full())
            .child(group)
    }
}
