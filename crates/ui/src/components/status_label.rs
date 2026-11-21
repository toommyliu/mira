use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, StyledExt as _, h_flex,
    primitives::{Activity, HoverCard, StatusDot},
};

#[derive(IntoElement)]
pub struct StatusLabel {
    id: ElementId,
    activity: Activity,
    label: SharedString,
    single_line: bool,
    details: Option<HoverCard>,
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
            single_line: false,
            details: None,
        }
    }

    pub fn single_line(mut self) -> Self {
        self.single_line = true;
        self
    }

    pub fn details(mut self, card: HoverCard) -> Self {
        self.details = Some(card);
        self
    }
}

impl RenderOnce for StatusLabel {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let typography = theme.typography;
        let text = match self.activity {
            Activity::Idle | Activity::Stopped => theme.muted_foreground,
            Activity::Running | Activity::Waiting => theme.foreground,
            Activity::NeedsYou => theme.warning,
            Activity::Failed => theme.destructive,
        };
        h_flex()
            .id(self.id.clone())
            .role(Role::Status)
            .aria_label(self.label.clone())
            .map(|label| {
                if self.single_line {
                    label.min_w_0()
                } else {
                    label.flex_none()
                }
            })
            .gap(px(7.0))
            .text_body(typography)
            .text_color(text)
            .child(StatusDot::new((self.id.clone(), "dot"), self.activity))
            .child({
                let shown = if self.single_line {
                    self.label.lines().collect::<Vec<_>>().join(" ").into()
                } else {
                    self.label
                };
                let label = div()
                    .min_w_0()
                    .when(self.single_line, |label| label.truncate())
                    .child(shown);
                match self.details {
                    Some(card) => card
                        .child(
                            label
                                .border_b_1()
                                .border_dashed()
                                .border_color(text.opacity(0.5))
                                .hover(move |style| style.border_color(text)),
                        )
                        .into_any_element(),
                    None => label.into_any_element(),
                }
            })
    }
}
