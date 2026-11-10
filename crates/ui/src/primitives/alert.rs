use gpui::{
    AnyElement, App, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, Tone,
    theme::{LINE_BODY, LINE_SMALL, TEXT_BODY, TEXT_SMALL},
};

#[derive(IntoElement)]
pub struct Alert {
    id: ElementId,
    tone: Tone,
    title: SharedString,
    body: Option<SharedString>,
    action: Option<AnyElement>,
}

impl Alert {
    pub fn new(id: impl Into<ElementId>, tone: Tone, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            tone,
            title: title.into(),
            body: None,
            action: None,
        }
    }

    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }
}

impl RenderOnce for Alert {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (solid, soft, text) = theme.tone(self.tone);
        div()
            .id(self.id)
            .role(if self.tone == Tone::Danger {
                Role::Alert
            } else {
                Role::Note
            })
            .aria_label(self.title.clone())
            .when_some(self.body.clone(), |alert, body| {
                alert.aria_description(body)
            })
            .flex()
            .items_center()
            .gap(px(16.0))
            .py(px(10.0))
            .px(px(14.0))
            .rounded(theme.radius.panel)
            .bg(if self.tone == Tone::Neutral {
                theme.fill
            } else {
                soft
            })
            .border_1()
            .border_color(if self.tone == Tone::Neutral {
                theme.border
            } else {
                solid.opacity(0.35)
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(TEXT_BODY)
                            .line_height(LINE_BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if self.tone == Tone::Neutral {
                                theme.text
                            } else {
                                text
                            })
                            .child(self.title),
                    )
                    .when_some(self.body, |column, body| {
                        column.child(
                            div()
                                .text_size(TEXT_SMALL)
                                .line_height(LINE_SMALL)
                                .text_color(theme.text_secondary)
                                .child(body),
                        )
                    }),
            )
            .when_some(self.action, |alert, action| alert.child(action))
    }
}
