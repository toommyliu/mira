use gpui::{
    App, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
    svg,
};

use crate::{ActiveTheme as _, assets::SPINNER, motion};

#[derive(IntoElement)]
pub struct Spinner {
    id: ElementId,
    label: SharedString,
    color: Option<Hsla>,
    size: Pixels,
}

impl Spinner {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: "Loading".into(),
            color: None,
            size: px(14.0),
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

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or(cx.theme().text);
        let ring = svg()
            .path(SPINNER)
            .flex_none()
            .size(self.size)
            .text_color(color);
        div()
            .id(self.id.clone())
            .role(Role::ProgressIndicator)
            .aria_label(self.label)
            .flex_none()
            .child(motion::spin(self.id, ring, cx))
    }
}
