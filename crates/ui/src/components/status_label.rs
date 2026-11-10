use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use crate::{
    ActiveTheme as _, motion,
    theme::{LINE_BODY, TEXT_BODY},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Activity {
    Idle,
    Running,
    Waiting,
    Stopped,
    Failed,
}

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
        let theme = cx.theme().clone();
        let dot = div().flex_none().size(px(7.0)).rounded_full();
        let (dot, text) = match self.activity {
            Activity::Idle => (
                dot.border_1()
                    .border_color(theme.text_tertiary)
                    .into_any_element(),
                theme.text_secondary,
            ),
            Activity::Running => (
                motion::breathe((self.id.clone(), "breath"), dot.bg(theme.accent))
                    .into_any_element(),
                theme.text,
            ),
            Activity::Waiting => (
                dot.border_2()
                    .border_color(theme.warning)
                    .into_any_element(),
                theme.text,
            ),
            Activity::Stopped => (
                dot.bg(theme.text_tertiary).into_any_element(),
                theme.text_secondary,
            ),
            Activity::Failed => (dot.bg(theme.danger).into_any_element(), theme.danger_text),
        };
        div()
            .id(self.id)
            .role(Role::Status)
            .aria_label(self.label.clone())
            .flex()
            .flex_none()
            .items_center()
            .gap(px(7.0))
            .text_size(TEXT_BODY)
            .line_height(LINE_BODY)
            .text_color(text)
            .child(dot)
            .child(self.label)
    }
}
