use gpui::{
    AnyElement, App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, StyledExt as _, h_flex,
    theme::{LINE_BODY, TEXT_BODY},
};

#[derive(IntoElement)]
pub struct Empty {
    title: SharedString,
    body: SharedString,
    actions: Vec<AnyElement>,
    compact: bool,
}

impl Empty {
    pub fn new(title: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            actions: Vec::new(),
            compact: false,
        }
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }
}

impl RenderOnce for Empty {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .py(px(if self.compact { 16.0 } else { 32.0 }))
            .px(px(24.0))
            .child(
                div()
                    .map(|title| {
                        if self.compact {
                            title.text_body().font_weight(FontWeight::MEDIUM)
                        } else {
                            title.text_title()
                        }
                    })
                    .text_color(theme.text)
                    .child(self.title),
            )
            .child(
                div()
                    .max_w(px(320.0))
                    .text_center()
                    .text_size(TEXT_BODY)
                    .line_height(LINE_BODY)
                    .text_color(theme.text_secondary)
                    .child(self.body),
            )
            .when(!self.actions.is_empty(), |empty| {
                empty.child(
                    h_flex()
                        .gap(px(6.0))
                        .pt(px(if self.compact { 8.0 } else { 12.0 }))
                        .children(self.actions),
                )
            })
    }
}
