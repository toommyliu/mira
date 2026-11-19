use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Toggled, Window,
    prelude::FluentBuilder as _, px,
};

use super::{
    Icon, icon,
    label::{ToggleHandler, caption, labelled},
};
use crate::ActiveTheme as _;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckedState {
    Unchecked,
    Checked,
    Indeterminate,
}

#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    state: CheckedState,
    label: SharedString,
    description: Option<SharedString>,
    show_label: bool,
    disabled: bool,
    on_toggle: Option<ToggleHandler>,
}

impl Checkbox {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        state: CheckedState,
    ) -> Self {
        Self {
            id: id.into(),
            state,
            label: label.into(),
            description: None,
            show_label: true,
            disabled: false,
            on_toggle: None,
        }
    }

    pub fn hide_label(mut self) -> Self {
        self.show_label = false;
        self
    }

    pub fn indicator(value: CheckedState, theme: &crate::Theme) -> gpui::Div {
        let checked = value != CheckedState::Unchecked;
        let glyph = match value {
            CheckedState::Unchecked => None,
            CheckedState::Checked => Some(Icon::Check),
            CheckedState::Indeterminate => Some(Icon::Minus),
        };
        crate::h_flex()
            .relative()
            .flex_none()
            .justify_center()
            .size(theme.layout.check)
            .rounded(theme.radius.tag)
            .border_1()
            .when(!checked, |mark| mark.border_color(theme.input))
            .bg(if checked {
                theme.primary
            } else {
                theme.outline_fill()
            })
            .when_some(glyph, |mark, glyph| {
                mark.child(icon(glyph, theme.primary_foreground).size(theme.layout.check_mark))
            })
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_toggle(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let layout = theme.layout;
        let filled = self.state != CheckedState::Unchecked;
        let next = self.state != CheckedState::Checked;
        let (row, ring) = labelled(
            self.id.clone(),
            self.label.clone(),
            self.description.clone(),
            self.disabled,
            window,
            cx,
        );
        row.role(Role::CheckBox)
            .group("checkbox")
            .aria_toggled(match self.state {
                CheckedState::Unchecked => Toggled::False,
                CheckedState::Checked => Toggled::True,
                CheckedState::Indeterminate => Toggled::Mixed,
            })
            .child(
                Self::indicator(self.state, &theme)
                    .id("box")
                    .mt((theme.typography.body_line - layout.check) / 2.0)
                    .when(!self.disabled && !filled, |mark| {
                        mark.group_hover("checkbox", |style| style.border_color(theme.ring))
                    })
                    .when(!self.disabled, |mark| {
                        mark.group_active("checkbox", |style| {
                            style.bg(theme.pressed(
                                theme.background,
                                if filled {
                                    theme.primary
                                } else {
                                    theme.input_fill()
                                },
                                if filled {
                                    theme.primary_foreground
                                } else {
                                    theme.foreground
                                },
                            ))
                        })
                    })
                    .when(ring, |control| {
                        control.border_color(theme.ring).child(theme.ring_around(
                            theme.ring,
                            theme.radius.tag,
                            px(1.0),
                        ))
                    }),
            )
            .when(self.show_label, |row| {
                row.child(caption(self.label, self.description, cx))
            })
            .when_some(
                self.on_toggle.filter(|_| !self.disabled),
                |row, on_toggle| row.on_click(move |_, window, cx| on_toggle(next, window, cx)),
            )
    }
}
