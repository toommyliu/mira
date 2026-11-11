use gpui::{
    App, Div, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    Role, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use crate::{
    ActiveTheme as _, Theme, motion,
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
        let dot = status_dot(&theme, self.activity);
        let dot = match self.activity {
            Activity::Running => {
                motion::breathe((self.id.clone(), "breath"), dot).into_any_element()
            }
            _ => dot.into_any_element(),
        };
        let text = match self.activity {
            Activity::Idle | Activity::Stopped => theme.text_secondary,
            Activity::Running | Activity::Waiting => theme.text,
            Activity::Failed => theme.danger_text,
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
            .child(div().min_w_0().child(self.label))
    }
}

pub fn status_dot(theme: &Theme, activity: Activity) -> Div {
    let dot = div().flex_none().size(px(7.0)).rounded_full();
    match activity {
        Activity::Idle => dot.border_1().border_color(theme.text_tertiary),
        Activity::Running => dot.bg(theme.accent),
        Activity::Waiting => dot.border_2().border_color(theme.warning),
        Activity::Stopped => dot.bg(theme.text_tertiary),
        Activity::Failed => dot.bg(theme.danger),
    }
}
