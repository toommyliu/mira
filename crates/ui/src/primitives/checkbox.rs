use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Toggled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::{
    Icon,
    icon::small_icon,
    label::{ToggleHandler, caption, labelled},
};
use crate::ActiveTheme as _;

const BOX: f32 = 14.0;

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
            disabled: false,
            on_toggle: None,
        }
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
        let filled = self.state != CheckedState::Unchecked;
        let mark = match self.state {
            CheckedState::Unchecked => None,
            CheckedState::Checked => Some(Icon::Check),
            CheckedState::Indeterminate => Some(Icon::Minus),
        };
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
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .mt(px(2.0))
                    .size(px(BOX))
                    .rounded(theme.radius.tag)
                    .map(|mark| {
                        if filled {
                            mark.bg(theme.inverse)
                        } else {
                            mark.border_1()
                                .border_color(theme.border)
                                .bg(theme.fill)
                                .when(!self.disabled, |mark| {
                                    mark.group_hover("checkbox", |style| {
                                        style.border_color(theme.border_strong)
                                    })
                                })
                        }
                    })
                    .when(ring, |control| control.child(theme.focus_ring(px(4.0))))
                    .when_some(mark, |container, mark| {
                        container.child(small_icon(mark, theme.on_inverse))
                    }),
            )
            .child(caption(self.label, self.description, cx))
            .when_some(
                self.on_toggle.filter(|_| !self.disabled),
                |row, on_toggle| row.on_click(move |_, window, cx| on_toggle(next, window, cx)),
            )
    }
}
