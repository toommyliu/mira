use gpui::{
    AnyElement, App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme as _, StyledExt as _, h_flex};

#[derive(IntoElement)]
pub struct Empty {
    title: SharedString,
    body: Option<SharedString>,
    actions: Vec<AnyElement>,
    compact: bool,
}

impl Empty {
    pub fn new(title: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        Self {
            body: Some(body.into()),
            ..Self::title(title)
        }
    }

    pub fn title(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            body: None,
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
        let layout = theme.layout;
        let typography = theme.typography;
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(layout.space(1.0))
            .py(theme.space(if self.compact { 4.0 } else { 6.0 }))
            .px(layout.space(6.0))
            .child(
                div()
                    .map(|title| {
                        if self.compact {
                            title.text_body(typography).font_weight(FontWeight::MEDIUM)
                        } else {
                            title.text_title(typography)
                        }
                    })
                    .text_color(theme.foreground)
                    .child(self.title),
            )
            .children(self.body.map(|body| {
                div()
                    .max_w(px(320.0))
                    .text_center()
                    .text_size(typography.body)
                    .line_height(typography.body_line)
                    .text_color(theme.muted_foreground)
                    .child(body)
            }))
            .when(!self.actions.is_empty(), |empty| {
                empty.child(
                    h_flex()
                        .gap(layout.space(1.5))
                        .pt(theme.space(3.0))
                        .children(self.actions),
                )
            })
    }
}
