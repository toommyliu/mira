use gpui::{
    AnyElement, App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _,
    theme::{LINE_BODY, LINE_TITLE, TEXT_BODY, TEXT_TITLE},
};

#[derive(IntoElement)]
pub struct Empty {
    title: SharedString,
    body: SharedString,
    action: Option<AnyElement>,
}

impl Empty {
    pub fn new(title: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            action: None,
        }
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
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
            .py(px(32.0))
            .px(px(24.0))
            .child(
                div()
                    .text_size(TEXT_TITLE)
                    .line_height(LINE_TITLE)
                    .font_weight(FontWeight::SEMIBOLD)
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
            .when_some(self.action, |empty, action| {
                empty.child(div().pt(px(12.0)).child(action))
            })
    }
}
