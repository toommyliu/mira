use gpui::{
    App, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    Role, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px, svg,
};

use crate::{ActiveTheme as _, assets::SPINNER, motion};

#[derive(IntoElement)]
pub struct Spinner {
    id: ElementId,
    label: SharedString,
    color: Option<Hsla>,
}

impl Spinner {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: "Loading".into(),
            color: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(cx.theme().text_secondary);
        let ring = svg()
            .path(SPINNER)
            .flex_none()
            .size(px(14.0))
            .text_color(color);
        div()
            .id(self.id.clone())
            .role(Role::ProgressIndicator)
            .aria_label(self.label)
            .flex_none()
            .child(motion::spin(self.id, ring, cx))
    }
}
