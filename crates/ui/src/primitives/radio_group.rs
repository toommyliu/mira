use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Toggled, Window, div, prelude::FluentBuilder as _, px,
};

use super::{label::caption_with, tooltip};
use crate::{
    ActiveTheme as _, IndexHandler, PointerCursor as _,
    nav::{self, Axis, Step},
    theme::DISABLED_OPACITY,
};

pub struct Radio {
    label: SharedString,
    description: Option<SharedString>,
    accessory: Option<AnyElement>,
    preview: Option<AnyElement>,
    invalid: bool,
    disabled: bool,
}

impl Radio {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            description: None,
            accessory: None,
            preview: None,
            invalid: false,
            disabled: false,
        }
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn accessory(mut self, accessory: impl IntoElement) -> Self {
        self.accessory = Some(accessory.into_any_element());
        self
    }

    pub fn preview(mut self, preview: impl IntoElement) -> Self {
        self.preview = Some(preview.into_any_element());
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

#[derive(IntoElement)]
pub struct RadioGroup {
    id: ElementId,
    label: SharedString,
    choices: Vec<Radio>,
    selected: Option<usize>,
    on_select: Option<IndexHandler>,
}

impl RadioGroup {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        choices: Vec<Radio>,
        selected: Option<usize>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            choices,
            selected,
            on_select: None,
        }
    }

    pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for RadioGroup {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let layout = cx.theme().layout;
        let typography = cx.theme().typography;
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let ring = nav::keyboard_focused(&focus, window, cx);
        let theme = cx.theme().clone();
        let enabled: Rc<[bool]> = self.choices.iter().map(|c| !c.disabled).collect();
        let count = self.choices.len();
        let previews = self.choices.iter().any(|choice| choice.preview.is_some());
        let id = self.id.clone();
        let selected = self.selected;
        let on_select = self.on_select.clone();
        div()
            .id(self.id.clone())
            .role(Role::RadioGroup)
            .aria_label(self.label)
            .track_focus(&focus.clone().tab_stop(true))
            .flex()
            .when(!previews, |group| group.flex_col())
            .gap(theme.space(if previews { 2.0 } else { 3.0 }))
            .rounded(theme.radius.control)
            .when(ring && selected.is_none(), |group| {
                group.child(theme.focus_ring(px(6.0)))
            })
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let step = Step::from_key(event, Axis::Vertical)
                    .or_else(|| Step::from_key(event, Axis::Horizontal));
                let Some(step) = step else {
                    if event.keystroke.key == "space" && selected.is_none() {
                        if let (Some(first), Some(on_select)) = (
                            nav::step(None, count, Step::First, false, |i| enabled[i]),
                            &on_select,
                        ) {
                            on_select(first, window, cx);
                        }
                        cx.stop_propagation();
                    }
                    return;
                };
                if let (Some(next), Some(on_select)) = (
                    nav::step(selected, count, step, true, |i| enabled[i]),
                    &on_select,
                ) {
                    on_select(next, window, cx);
                }
                cx.stop_propagation();
            })
            .children(self.choices.into_iter().enumerate().map(|(index, choice)| {
                let checked = selected == Some(index);
                let on_select = self.on_select.clone();
                let focus = focus.clone();
                let disabled = choice.disabled;
                let tip = choice.label.clone();
                let mark = div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .mt(layout.space(0.5))
                    .size(layout.check)
                    .rounded_full()
                    .id("mark")
                    .border_1()
                    .map(|dot| {
                        if checked {
                            dot.bg(theme.primary).border_color(theme.primary)
                        } else {
                            dot.border_color(theme.input).bg(theme.outline_fill())
                        }
                    })
                    .when(!disabled, |dot| {
                        dot.group_hover("radio", |style| style.border_color(theme.ring))
                            .group_active("radio", |style| {
                                style.bg(theme.pressed(
                                    theme.background,
                                    if checked {
                                        theme.primary
                                    } else {
                                        theme.input_fill()
                                    },
                                    if checked {
                                        theme.primary_foreground
                                    } else {
                                        theme.foreground
                                    },
                                ))
                            })
                    })
                    .when(checked && ring, |dot| {
                        dot.border_color(theme.ring).child(theme.halo(
                            theme.ring.opacity(0.5),
                            px(8.0),
                            px(1.0),
                            px(3.0),
                        ))
                    })
                    .when(checked, |dot| {
                        dot.child(
                            div()
                                .size(layout.check / 2.0)
                                .rounded_full()
                                .bg(theme.primary_foreground),
                        )
                    });
                let caption = caption_with(
                    choice.label.clone(),
                    choice.description,
                    choice.invalid,
                    choice.accessory,
                    cx,
                );
                div()
                    .id(index)
                    .role(Role::RadioButton)
                    .group("radio")
                    .aria_label(choice.label.clone())
                    .aria_toggled(if checked {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .aria_position_in_set(index + 1)
                    .aria_size_of_set(count)
                    .when(checked, |radio| radio.aria_active_descendant())
                    .flex()
                    .when(!previews, |radio| radio.items_start().py(px(3.0)))
                    .when(previews, |radio| {
                        radio.flex_col().flex_none().items_center()
                    })
                    .gap(layout.space(if previews { 2.5 } else { 2.0 }))
                    .text_size(typography.body)
                    .line_height(typography.body_line)
                    .text_color(theme.foreground)
                    .when(disabled, |radio| radio.opacity(DISABLED_OPACITY))
                    .when(!disabled, |radio| radio.pointer_cursor(&theme))
                    .map(|radio| match choice.preview {
                        Some(preview) => {
                            let radius = theme.radius.small + px(1.0);
                            let gap = px(2.0);
                            let width = px(2.0);
                            radio.p(gap + width).child(
                                div()
                                    .relative()
                                    .rounded(radius)
                                    .border_1()
                                    .border_color(theme.border)
                                    .when(!checked && !disabled, |frame| {
                                        frame.group_hover("radio", |style| {
                                            style.border_color(theme.ring)
                                        })
                                    })
                                    .when(checked, |frame| {
                                        frame.child(theme.outline(
                                            if ring { theme.ring } else { theme.primary },
                                            radius + gap,
                                            px(1.0) + gap,
                                            width,
                                        ))
                                    })
                                    .when(checked && ring, |frame| {
                                        frame.child(theme.halo(
                                            theme.ring.opacity(0.5),
                                            radius + gap + width,
                                            px(1.0) + gap + width,
                                            px(3.0),
                                        ))
                                    })
                                    .child(preview),
                            )
                        }
                        None => radio.child(mark).child(caption),
                    })
                    .when_some(on_select.filter(|_| !disabled), |radio, on_select| {
                        radio.on_click(move |_, window, cx| {
                            window.focus(&focus, cx);
                            on_select(index, window, cx);
                        })
                    })
                    .map(|radio| {
                        if previews {
                            tooltip::attach(
                                radio,
                                (id.clone(), SharedString::from(format!("tip-{index}"))),
                                tip,
                                checked && ring,
                                window,
                                cx,
                            )
                        } else {
                            radio
                        }
                    })
            }))
    }
}
