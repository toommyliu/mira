use gpui::{
    AnyElement, App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _,
};

use crate::{ActiveTheme as _, h_flex};

#[derive(IntoElement)]
pub struct Field {
    label: SharedString,
    description: Option<SharedString>,
    error: Option<SharedString>,
    end: Option<AnyElement>,
    control: AnyElement,
    inline: bool,
    disabled: bool,
}

impl Field {
    pub fn new(label: impl Into<SharedString>, control: impl IntoElement) -> Self {
        Self {
            label: label.into(),
            description: None,
            error: None,
            end: None,
            control: control.into_any_element(),
            inline: false,
            disabled: false,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn end_slot(mut self, element: impl IntoElement) -> Self {
        self.end = Some(element.into_any_element());
        self
    }

    pub fn error(mut self, error: Option<SharedString>) -> Self {
        self.error = error;
        self
    }

    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for Field {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let layout = theme.layout;
        let typography = theme.typography;
        let note = |text: SharedString, color| {
            div()
                .text_size(typography.secondary)
                .line_height(typography.secondary_line)
                .text_color(color)
                .child(text)
        };
        let label = h_flex()
            .justify_between()
            .gap(layout.space(2.0))
            .text_size(typography.body)
            .line_height(typography.body_line)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.foreground)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .when(self.disabled, |label| {
                        label.opacity(crate::theme::DISABLED_OPACITY)
                    })
                    .child(self.label),
            )
            .children(self.end);
        div()
            .flex()
            .flex_col()
            .gap(layout.space(0.5))
            .child(div().flex().flex_col().gap(layout.space(2.0)).map(|field| {
                if self.inline {
                    field.child(
                        h_flex()
                            .gap(layout.space(2.0))
                            .flex_wrap()
                            .child(label.flex_1().min_w(gpui::px(96.0)))
                            .child(div().flex_none().child(self.control)),
                    )
                } else {
                    field.child(label).child(self.control)
                }
            }))
            .map(|field| match (self.error, self.description) {
                (Some(error), _) => field.child(note(error, theme.destructive)),
                (None, Some(description)) => field.child(note(description, theme.muted_foreground)),
                (None, None) => field,
            })
    }
}
