use gpui::{
    AnyElement, App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, h_flex,
    theme::{LINE_SMALL, TEXT_SMALL},
};

#[derive(IntoElement)]
pub struct Field {
    label: SharedString,
    description: Option<SharedString>,
    error: Option<SharedString>,
    end: Option<AnyElement>,
    control: AnyElement,
    inline: bool,
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
}

impl RenderOnce for Field {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let note = |text: SharedString, color| {
            div()
                .text_size(TEXT_SMALL)
                .line_height(LINE_SMALL)
                .text_color(color)
                .child(text)
        };
        let label = h_flex()
            .justify_between()
            .gap(px(8.0))
            .text_size(TEXT_SMALL)
            .line_height(LINE_SMALL)
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.text)
            .child(self.label)
            .children(self.end);
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .map(|field| {
                if self.inline {
                    field.child(
                        h_flex()
                            .gap(px(8.0))
                            .child(label.flex_1().min_w_0())
                            .child(div().flex_none().child(self.control)),
                    )
                } else {
                    field.child(label).child(self.control)
                }
            })
            .map(|field| match (self.error, self.description) {
                (Some(error), _) => field.child(note(error, theme.danger_text)),
                (None, Some(description)) => field.child(note(description, theme.text_tertiary)),
                (None, None) => field,
            })
    }
}
