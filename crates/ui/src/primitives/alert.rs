use gpui::{
    AnyElement, App, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _,
};

use super::{Icon, icon};
use crate::{ActiveTheme as _, Tone};

#[derive(IntoElement)]
pub struct Alert {
    id: ElementId,
    tone: Tone,
    icon: Icon,
    title: SharedString,
    body: Option<SharedString>,
    action: Option<AnyElement>,
}

impl Alert {
    pub fn new(id: impl Into<ElementId>, tone: Tone, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            tone,
            icon: match tone {
                Tone::Neutral | Tone::Accent => Icon::Info,
                Tone::Success => Icon::CircleCheck,
                Tone::Warning => Icon::TriangleAlert,
                Tone::Danger => Icon::OctagonX,
            },
            title: title.into(),
            body: None,
            action: None,
        }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = icon;
        self
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
        let layout = theme.layout;
        let typography = theme.typography;
        let (solid, _, text) = theme.tone(self.tone);
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
            .gap(layout.space(4.0))
            .py(theme.space(1.5))
            .px(theme.space(2.0))
            .rounded(theme.radius.panel)
            .bg(theme.card)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_start()
                    .gap(theme.space(2.0))
                    .child(
                        div()
                            .flex_none()
                            .mt(layout.space(0.5))
                            .child(icon(self.icon, solid)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(layout.space(0.5))
                            .child(
                                div()
                                    .text_size(typography.body)
                                    .line_height(typography.body_line)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if self.tone == Tone::Neutral {
                                        theme.foreground
                                    } else {
                                        text
                                    })
                                    .child(self.title),
                            )
                            .when_some(self.body, |column, body| {
                                column.child(
                                    div()
                                        .text_size(typography.secondary)
                                        .line_height(typography.secondary_line)
                                        .text_color(theme.muted_foreground)
                                        .child(body),
                                )
                            }),
                    ),
            )
            .when_some(self.action, |alert, action| alert.child(action))
    }
}
