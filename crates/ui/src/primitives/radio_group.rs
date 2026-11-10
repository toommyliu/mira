use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Toggled, Window,
    div, prelude::FluentBuilder as _, px,
};

use super::label::caption;
use crate::{
    ActiveTheme as _, IndexHandler, PointerCursor as _,
    nav::{self, Axis, Step},
    theme::{DISABLED_OPACITY, LINE_BODY, TEXT_BODY},
};

const BOX: f32 = 14.0;

pub struct Radio {
    label: SharedString,
    description: Option<SharedString>,
    disabled: bool,
}

impl Radio {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            description: None,
            disabled: false,
        }
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
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let ring = nav::keyboard_focused(&focus, window);
        let theme = cx.theme().clone();
        let enabled: Rc<[bool]> = self.choices.iter().map(|c| !c.disabled).collect();
        let count = self.choices.len();
        let selected = self.selected;
        let on_select = self.on_select.clone();
        div()
            .id(self.id.clone())
            .role(Role::RadioGroup)
            .aria_label(self.label)
            .track_focus(&focus.clone().tab_stop(true))
            .flex()
            .flex_col()
            .gap(px(2.0))
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
                    .items_start()
                    .gap(px(8.0))
                    .py(px(3.0))
                    .text_size(TEXT_BODY)
                    .line_height(LINE_BODY)
                    .text_color(theme.text)
                    .when(disabled, |radio| radio.opacity(DISABLED_OPACITY))
                    .when(!disabled, |radio| radio.pointer_cursor(&theme))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .mt(px(2.0))
                            .size(px(BOX))
                            .rounded_full()
                            .map(|dot| {
                                if checked {
                                    dot.bg(theme.inverse)
                                } else {
                                    dot.border_1()
                                        .border_color(theme.border)
                                        .bg(theme.fill)
                                        .when(!disabled, |dot| {
                                            dot.group_hover("radio", |style| {
                                                style.border_color(theme.border_strong)
                                            })
                                        })
                                }
                            })
                            .when(checked && ring, |dot| dot.child(theme.focus_ring(px(7.0))))
                            .when(checked, |dot| {
                                dot.child(div().size(px(6.0)).rounded_full().bg(theme.on_inverse))
                            }),
                    )
                    .child(caption(choice.label, choice.description, cx))
                    .when_some(on_select.filter(|_| !disabled), |radio, on_select| {
                        radio.on_click(move |_, window, cx| {
                            window.focus(&focus, cx);
                            on_select(index, window, cx);
                        })
                    })
            }))
    }
}
