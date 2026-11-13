use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use crate::{
    ActiveTheme as _, StyledExt as _, h_flex,
    primitives::{Activity, StatusDot},
};

#[derive(IntoElement)]
pub struct StatusLabel {
    id: ElementId,
    activity: Activity,
    label: SharedString,
}

impl StatusLabel {
    pub fn new(
        id: impl Into<ElementId>,
        activity: Activity,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            activity,
            label: label.into(),
        }
    }
}

impl RenderOnce for StatusLabel {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let text = match self.activity {
            Activity::Idle | Activity::Stopped => theme.text_secondary,
            Activity::Running | Activity::Waiting => theme.text,
            Activity::Failed => theme.danger_text,
        };
        h_flex()
            .id(self.id.clone())
            .role(Role::Status)
            .aria_label(self.label.clone())
            .flex_none()
            .gap(px(7.0))
            .text_body()
            .text_color(text)
            .child(StatusDot::new((self.id.clone(), "dot"), self.activity))
            .child(div().min_w_0().child(self.label))
    }
}
